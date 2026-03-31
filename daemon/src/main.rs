mod bt_mgmt;
mod config;
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
    time,
};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{
    bt_mgmt::BtMgmt,
    config::{Config, DaemonMode},
    ipc::spawn_ipc_listener,
    proximity::{Action, Reading, State},
    state::{AppState, DaemonStatus, ProximityPhase},
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

    let target_mac = config.resolved_target_mac().map(String::from);
    let bt = config.resolved_bluetooth();
    let prox = config.resolved_proximity();

    if target_mac.is_none() {
        warn!("no active device set — waiting for config reload via SIGHUP");
    }

    info!(
        target_mac = target_mac.as_deref().unwrap_or("<not set>"),
        poll_ms = bt.poll_interval_ms,
        rpl_threshold = prox.rpl_threshold,
        "config loaded"
    );

    let config = Arc::new(ArcSwap::from_pointee(config));
    let daemon_start = Instant::now();

    let app_state = Arc::new(AppState {
        config: config.clone(),
        config_path: config_path.clone(),
        web_sessions: std::sync::Mutex::new(HashMap::new()),
        daemon_status: ArcSwap::from_pointee(DaemonStatus {
            rpl: None,
            raw_rpl: None,
            state: ProximityPhase::Disconnected,
            connected: false,
            target_mac: config.load().resolved_target_mac().map(String::from),
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
        while config.load().resolved_target_mac().is_none() {
            info!("no active device set, waiting for config reload");
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
        }

        let cfg = config.load();
        let target_mac = cfg.resolved_target_mac().unwrap().to_string();
        let bt_cfg = cfg.resolved_bluetooth();
        let prox_cfg = cfg.resolved_proximity();
        info!("monitoring {target_mac}");

        let mut bt = BtMgmt::new(&target_mac, &bt_cfg, &prox_cfg)?;
        let mut state = State::new(&prox_cfg);
        let mut interval = time::interval(Duration::from_millis(bt_cfg.poll_interval_ms));
        let mut prev_kalman_q = prox_cfg.kalman_q;
        let mut prev_kalman_r = prox_cfg.kalman_r;

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                _ = sigterm.recv() => break 'daemon,
                _ = sigint.recv() => break 'daemon,
                _ = sighup.recv() => {
                    info!("received SIGHUP, reloading config");
                    match Config::load() {
                        Ok(new_cfg) => config.store(Arc::new(new_cfg)),
                        Err(e) => error!("failed to reload config: {e}"),
                    }
                    break;
                }
                _ = app_state.config_notify.notified() => {
                    info!("config changed via web UI, restarting monitor loop");
                    break;
                }
            }

            let cfg = config.load();
            let prox_cfg = cfg.resolved_proximity();
            if prox_cfg.kalman_q != prev_kalman_q || prox_cfg.kalman_r != prev_kalman_r {
                info!(
                    kalman_q = prox_cfg.kalman_q,
                    kalman_r = prox_cfg.kalman_r,
                    "kalman parameters updated"
                );
                bt.update_kalman_params(prox_cfg.kalman_q, prox_cfg.kalman_r);
                prev_kalman_q = prox_cfg.kalman_q;
                prev_kalman_r = prox_cfg.kalman_r;
            }

            let reading = match bt.relative_path_loss().await {
                Ok((filtered_rpl, raw_rpl)) => {
                    // update daemon status
                    app_state.daemon_status.store(Arc::new(DaemonStatus {
                        rpl: Some(filtered_rpl),
                        raw_rpl: Some(raw_rpl),
                        state: state.proximity_phase(),
                        connected: true,
                        target_mac: Some(target_mac.clone()),
                        started_at: daemon_start,
                    }));

                    Reading::Rpl(filtered_rpl)
                }
                Err(e) => {
                    warn!("BT poll failed: {e}");

                    // update daemon status to disconnected
                    app_state.daemon_status.store(Arc::new(DaemonStatus {
                        rpl: None,
                        raw_rpl: None,
                        state: ProximityPhase::Disconnected,
                        connected: false,
                        target_mac: Some(target_mac.clone()),
                        started_at: daemon_start,
                    }));

                    Reading::ConnectionLost
                }
            };

            let was_disconnected = state.is_disconnected();
            let action = state.transition(reading);
            let is_disconnected = state.is_disconnected();

            if was_disconnected != is_disconnected {
                let bt_cfg = config.load().resolved_bluetooth();
                let target_ms = if is_disconnected {
                    bt_cfg.disconnect_poll_interval_ms
                } else {
                    bt_cfg.poll_interval_ms
                };
                interval = time::interval(Duration::from_millis(target_ms));
            }

            if matches!(mode, DaemonMode::Both | DaemonMode::LockOnly) {
                match action {
                    Action::Lock => {
                        if let Err(e) = logind_session.lock().await {
                            error!("lock failed: {e}");
                        }
                    }
                    Action::Unlock => {
                        let cfg = config.load();
                        let wake_duration = Duration::from_secs(cfg.wake.duration_secs);
                        let mouse_interval = Duration::from_millis(cfg.wake.mouse_interval_ms);
                        let enter_interval = Duration::from_millis(cfg.wake.enter_interval_ms);
                        if let Err(e) =
                            wake_screen(wake_duration, mouse_interval, enter_interval).await
                        {
                            error!("wake failed: {e}");
                        }
                        if let Err(e) = logind_session.unlock().await {
                            error!("unlock failed: {e}");
                        }
                    }
                    Action::None => {}
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
