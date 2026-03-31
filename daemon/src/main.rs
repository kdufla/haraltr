mod bt_mgmt;
mod config;
mod device;
mod input;
mod ipc;
mod logind;
mod passwd;
mod proximity;
mod state;
mod wake_up;
mod web;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use common::IPC_SOCKET_PATH;
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::mpsc,
    task::JoinHandle,
};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{
    config::{Config, DaemonMode},
    device::spawn_device_task,
    ipc::spawn_ipc_listener,
    state::{AppState, DaemonStatus, DeviceReport, DeviceStatus, ProximityPhase},
    wake_up::wake_screen,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("passwd") {
        return passwd::set_password();
    }

    init_tracing();

    let config = Config::load()?;
    let config_path = config::config_path()?;

    info!(devices = config.devices.len(), "config loaded");

    let config = Arc::new(ArcSwap::from_pointee(config));
    let daemon_start = Instant::now();

    let app_state = Arc::new(AppState {
        config: config.clone(),
        config_path: config_path.clone(),
        web_sessions: std::sync::Mutex::new(HashMap::new()),
        daemon_status: ArcSwap::from_pointee(DaemonStatus {
            devices: HashMap::new(),
            any_near: true,
            started_at: daemon_start,
        }),
        config_notify: tokio::sync::Notify::new(),
    });

    let mode = config.load().daemon.mode;

    spawn_web_server(&app_state);
    if matches!(mode, DaemonMode::Both | DaemonMode::PamOnly) {
        spawn_ipc_listener(&app_state);
    }

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sighup = signal(SignalKind::hangup())?;

    let logind_session = logind::SessionController::new().await?;

    info!("daemon started");

    'daemon: loop {
        let cfg = config.load();
        let devices = cfg.devices.clone();

        if devices.is_empty() {
            info!("no devices configured, waiting for config reload");
            tokio::select! {
                _ = sigterm.recv() => break 'daemon,
                _ = sigint.recv() => break 'daemon,
                _ = sighup.recv() => {
                    info!("received SIGHUP, reloading config");
                    match Config::load() {
                        Ok(new_cfg) => config.store(Arc::new(new_cfg)),
                        Err(e) => error!("failed to reload config: {e}"),
                    }
                }
                _ = app_state.config_notify.notified() => {
                    info!("config changed via web UI");
                }
            }
            continue;
        }

        let (state_change_tx, mut state_change_rx) =
            mpsc::channel::<DeviceReport>(devices.len() * 4);
        let mut handles: Vec<JoinHandle<()>> = Vec::new();

        for device in &devices {
            let handle = spawn_device_task(
                device.target_mac.clone(),
                cfg.bluetooth_for_device(device),
                cfg.proximity_for_device(device),
                config.clone(),
                state_change_tx.clone(),
            );
            handles.push(handle);
            info!(mac = %device.target_mac, "spawned device monitor");
        }
        drop(state_change_tx);

        let mut device_phases: HashMap<String, DeviceReport> = HashMap::new();
        let mut was_any_near = true; // start true to avoid spurious unlock on daemon start

        app_state.daemon_status.store(Arc::new(DaemonStatus {
            devices: HashMap::new(),
            any_near: true,
            started_at: daemon_start,
        }));

        loop {
            tokio::select! {
                msg = state_change_rx.recv() => {
                    let Some(report) = msg else {
                        warn!("all device tasks exited, waiting for config reload");
                        break;
                    };

                    device_phases.insert(report.target_mac.clone(), report);

                    let is_any_near = device_phases.values().any(|r| r.phase == ProximityPhase::Near);

                    let device_map = device_phases.iter().map(|(mac, r)| {
                        (mac.clone(), DeviceStatus {
                            rpl: r.rpl,
                            raw_rpl: r.raw_rpl,
                            phase: r.phase,
                            connected: r.connected,
                        })
                    }).collect();

                    app_state.daemon_status.store(Arc::new(DaemonStatus {
                        devices: device_map,
                        any_near: is_any_near,
                        started_at: daemon_start,
                    }));

                    if matches!(mode, DaemonMode::Both | DaemonMode::LockOnly) {
                        if was_any_near && !is_any_near {
                            info!("all devices far/disconnected, locking");
                            if let Err(e) = logind_session.lock().await {
                                error!("lock failed: {e}");
                            }
                        } else if !was_any_near && is_any_near {
                            info!("device near, unlocking");
                            let cfg = config.load();
                            let wake_duration = Duration::from_secs(cfg.wake.duration_secs);
                            let mouse_interval = Duration::from_millis(cfg.wake.mouse_interval_ms);
                            let enter_interval = Duration::from_millis(cfg.wake.enter_interval_ms);
                            if let Err(e) = wake_screen(wake_duration, mouse_interval, enter_interval).await {
                                error!("wake failed: {e}");
                            }
                            if let Err(e) = logind_session.unlock().await {
                                error!("unlock failed: {e}");
                            }
                        }
                        was_any_near = is_any_near;
                    }
                }
                _ = sigterm.recv() => {
                    for h in &handles { h.abort(); }
                    break 'daemon;
                }
                _ = sigint.recv() => {
                    for h in &handles { h.abort(); }
                    break 'daemon;
                }
                _ = sighup.recv() => {
                    info!("received SIGHUP, reloading config");
                    match Config::load() {
                        Ok(new_cfg) => config.store(Arc::new(new_cfg)),
                        Err(e) => error!("failed to reload config: {e}"),
                    }
                    for h in &handles { h.abort(); }
                    break;
                }
                _ = app_state.config_notify.notified() => {
                    info!("config changed via web UI, restarting device monitors");
                    for h in &handles { h.abort(); }
                    break;
                }
            }
        }
    }

    let _ = std::fs::remove_file(IPC_SOCKET_PATH);
    info!("daemon stopped");
    Ok(())
}

fn spawn_web_server(app_state: &Arc<AppState>) {
    let cfg = app_state.config.load();

    if !cfg.web.enabled {
        info!("web server disabled in config");
        return;
    }

    if cfg.web.password_hash.is_none() {
        warn!("web UI enabled but no password set — run 'haraltr passwd'. web server disabled.");
        return;
    }

    #[cfg(not(debug_assertions))]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&app_state.config_path) {
            Ok(meta) if meta.uid() != 0 => {
                warn!(
                    "config file not owned by root — web server disabled. \
                    fix with: sudo chown root:root {}",
                    app_state.config_path.display()
                );
                return;
            }
            Err(e) => {
                warn!("cannot stat config file: {e} — web server disabled.");
                return;
            }
            _ => {}
        }
    }

    let state = app_state.clone();
    tokio::spawn(async move { web::serve(state).await });
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
}
