mod bt_mgmt;
mod config;
mod device;
mod input;
mod ipc;
mod logind;
mod mac;
mod passwd;
mod proximity;
mod state;
mod wake_up;
mod web;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use clap::{Parser, Subcommand};
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
    logind::watcher::{NO_ACTIVE_UID, spawn_session_watcher},
    mac::Mac,
    proximity::Action,
    state::{AppState, DaemonStatus, DeviceAction},
    wake_up::wake_screen,
};

#[derive(Parser)]
#[command(
    name = "haraltr",
    bin_name = "haraltr",
    version,
    about = "Proximity-based authentication daemon",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the daemon
    Run,
    /// Set web UI password
    Passwd,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Passwd => return passwd::set_password(),
        Command::Run => {}
    }

    init_tracing();

    let config = Config::load()?;
    let config_path = config::config_path()?;

    info!(devices = config.devices.len(), "config loaded");

    let live_config = Arc::new(RwLock::new(config));
    let daemon_start = Instant::now();
    let active_uid = Arc::new(AtomicU32::new(NO_ACTIVE_UID));

    let app_state = Arc::new(AppState {
        config: live_config.clone(),
        config_path: config_path.clone(),
        web_sessions: std::sync::Mutex::new(HashMap::new()),
        daemon_status: Mutex::from(DaemonStatus {
            devices: HashMap::new(),
            started_at: daemon_start,
        }),
        config_notify: tokio::sync::Notify::new(),
        active_uid: active_uid.clone(),
    });

    let mode = live_config.read().unwrap().daemon.mode;

    spawn_web_server(&app_state);
    if matches!(mode, DaemonMode::Both | DaemonMode::PamOnly) {
        spawn_ipc_listener(&app_state);
    }

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sighup = signal(SignalKind::hangup())?;

    let logind_session = logind::SessionController::new().await?;
    spawn_session_watcher(logind_session.clone(), active_uid.clone()).await;

    info!("daemon started");

    'daemon: loop {
        let config = live_config.read().unwrap().clone();
        let devices = config.devices.clone();

        let current_uid = active_uid.load(Ordering::Relaxed);
        let user_devices: Vec<_> = devices.iter().filter(|d| d.uid == current_uid).collect();

        if user_devices.is_empty() {
            info!(uid = current_uid, "no devices for active user, waiting");
            tokio::select! {
                _ = sigterm.recv() => break 'daemon,
                _ = sigint.recv() => break 'daemon,
                _ = sighup.recv() => {
                    info!("received SIGHUP, reloading config");
                    match Config::load() {
                        Ok(new_config) => *live_config.write().unwrap() = new_config,
                        Err(e) => error!("failed to reload config: {e}"),
                    }
                }
                _ = app_state.config_notify.notified() => {
                    info!("config changed via web UI");
                }
            }
            continue;
        }

        let (action_tx, mut action_rx) = mpsc::channel::<DeviceAction>(user_devices.len() * 4);
        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let mut spawned_macs: HashSet<Mac> = HashSet::new();

        for device in &user_devices {
            let target_mac = match device.target_mac.parse() {
                Ok(m) => m,
                Err(e) => {
                    error!(mac = %device.target_mac, uid = device.uid, "skip device with invalid MAC: {e}");
                    continue;
                }
            };
            if spawned_macs.insert(target_mac) {
                let handle = spawn_device_task(
                    target_mac,
                    config.bluetooth_for_device(device),
                    config.proximity_for_device(device),
                    app_state.clone(),
                    action_tx.clone(),
                );
                handles.push(handle);
                info!(mac = %target_mac, uid = current_uid, "spawned device monitor");
            }
        }
        drop(action_tx);

        let mut device_wants: HashMap<Mac, Action> = spawned_macs
            .iter()
            .map(|&mac| (mac, Action::Unlock))
            .collect();
        let mut was_near = true;

        app_state.reset_daemon_state();

        loop {
            tokio::select! {
                msg = action_rx.recv() => {
                    let Some(device_action) = msg else {
                        warn!("all device tasks exited, waiting for config reload");
                        break;
                    };

                    if active_uid.load(Ordering::Relaxed) != current_uid {
                        info!("active user changed, restarting device monitors");
                        for h in &handles { h.abort(); }
                        break;
                    }

                    device_wants.insert(device_action.target_mac, device_action.action);

                    if !matches!(mode, DaemonMode::Both | DaemonMode::LockOnly) {
                        continue;
                    }

                    let is_near = device_wants.values().any(|a| *a == Action::Unlock);

                    if was_near && !is_near {
                        info!(uid = current_uid, "all user devices far/disconnected, locking");
                        if let Err(e) = logind_session.lock(current_uid).await {
                            error!("lock failed: {e}");
                        }
                    } else if !was_near && is_near {
                        info!(uid = current_uid, "user device near, unlocking");
                        let (wake_duration, mouse_interval, enter_interval) = {
                            let cfg = live_config.read().unwrap();
                            (
                                Duration::from_secs(cfg.wake.duration_secs),
                                Duration::from_millis(cfg.wake.mouse_interval_ms),
                                Duration::from_millis(cfg.wake.enter_interval_ms),
                            )
                        };
                        if let Err(e) = wake_screen(wake_duration, mouse_interval, enter_interval).await {
                            error!("wake failed: {e}");
                        }
                        if let Err(e) = logind_session.unlock(current_uid).await {
                            error!("unlock failed: {e}");
                        }
                    }
                    was_near = is_near;
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
                        Ok(new_cfg) => *live_config.write().unwrap() = new_cfg,
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
    let cfg = app_state.config.read().unwrap();

    if !cfg.web.enabled {
        info!("web server disabled in config");
        return;
    }

    if cfg.web.password_hash.is_none() {
        warn!("web UI enabled but no password set - run 'haraltr passwd'. web server disabled.");
        return;
    }

    #[cfg(not(debug_assertions))]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(&app_state.config_path) {
            Ok(meta) if meta.uid() != 0 => {
                warn!(
                    "config file not owned by root - web server disabled. \
                    fix with: sudo chown root:root {}",
                    app_state.config_path.display()
                );
                return;
            }
            Err(e) => {
                warn!("cannot stat config file: {e} - web server disabled.");
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
