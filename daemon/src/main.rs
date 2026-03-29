mod bt_mgmt;
mod config;
mod input;
mod kalman;
mod passwd;
mod proximity;
mod session;
mod wake_up;
mod web;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use tokio::{
    signal::unix::{SignalKind, signal},
    time,
};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{
    bt_mgmt::BtMgmt,
    config::Config,
    proximity::{Action, Reading, State},
    session::SessionController,
    wake_up::wake_screen,
    web::{AppState, DaemonStatus, ProximityPhase},
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

    if config.bluetooth.target_mac.is_none() {
        warn!("target_mac is not set — waiting for config reload via SIGHUP");
    }

    info!(
        target_mac = config
            .bluetooth
            .target_mac
            .as_deref()
            .unwrap_or("<not set>"),
        poll_ms = config.bluetooth.poll_interval_ms,
        rpl_threshold = config.proximity.rpl_threshold,
        "config loaded"
    );

    let config = Arc::new(ArcSwap::from_pointee(config));
    let daemon_start = Instant::now();

    let app_state = Arc::new(AppState {
        config: config.clone(),
        config_path: config_path.clone(),
        sessions: std::sync::Mutex::new(HashMap::new()),
        daemon_status: ArcSwap::from_pointee(DaemonStatus {
            rpl: None,
            raw_rpl: None,
            state: ProximityPhase::Disconnected,
            connected: false,
            target_mac: config.load().bluetooth.target_mac.clone(),
            started_at: daemon_start,
        }),
        config_notify: tokio::sync::Notify::new(),
    });

    spawn_web_server(&app_state);

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sighup = signal(SignalKind::hangup())?;

    let session = SessionController::new().await?;

    info!("daemon started");

    'daemon: loop {
        while config.load().bluetooth.target_mac.is_none() {
            info!("target_mac not set, waiting for config reload");
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
        let target_mac = cfg.bluetooth.target_mac.clone().unwrap();
        info!("monitoring {target_mac}");

        let mut bt = BtMgmt::new(&cfg.bluetooth, &cfg.proximity)?;
        let mut state = State::new(&cfg.proximity);
        let mut interval = time::interval(Duration::from_millis(cfg.bluetooth.poll_interval_ms));
        let mut prev_kalman_q = cfg.proximity.kalman_q;
        let mut prev_kalman_r = cfg.proximity.kalman_r;

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
            if cfg.proximity.kalman_q != prev_kalman_q || cfg.proximity.kalman_r != prev_kalman_r {
                info!(
                    kalman_q = cfg.proximity.kalman_q,
                    kalman_r = cfg.proximity.kalman_r,
                    "kalman parameters updated"
                );
                bt.update_kalman_params(cfg.proximity.kalman_q, cfg.proximity.kalman_r);
                prev_kalman_q = cfg.proximity.kalman_q;
                prev_kalman_r = cfg.proximity.kalman_r;
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
                let cfg = config.load();
                let target_ms = if is_disconnected {
                    cfg.bluetooth.disconnect_poll_interval_ms
                } else {
                    cfg.bluetooth.poll_interval_ms
                };
                interval = time::interval(Duration::from_millis(target_ms));
            }

            match action {
                Action::Lock => {
                    if let Err(e) = session.lock().await {
                        error!("lock failed: {e}");
                    }
                }
                Action::Unlock => {
                    let cfg = config.load();
                    let wake_duration = Duration::from_secs(cfg.wake.duration_secs);
                    let mouse_interval = Duration::from_millis(cfg.wake.mouse_interval_ms);
                    let enter_interval = Duration::from_millis(cfg.wake.enter_interval_ms);
                    if let Err(e) = wake_screen(wake_duration, mouse_interval, enter_interval).await
                    {
                        error!("wake failed: {e}");
                    }
                    if let Err(e) = session.unlock().await {
                        error!("unlock failed: {e}");
                    }
                }
                Action::None => {}
            }
        }
    }

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
