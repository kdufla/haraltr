mod bt_mgmt;
mod config;
mod input;
mod kalman;
mod proximity;
mod session;
mod wake_up;

use arc_swap::ArcSwap;
use crate::config::Config;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tokio::time;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::bt_mgmt::BtMgmt;
use crate::proximity::{Action, Reading, State};
use crate::session::SessionController;
use crate::wake_up::wake_screen;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let config = Config::load()?;

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
            }
        }

        let cfg = config.load();
        info!(
            "monitoring {}",
            cfg.bluetooth.target_mac.as_deref().unwrap()
        );

        let mut bt = BtMgmt::new(&cfg.bluetooth, &cfg.proximity)?;
        let mut state = State::new(&cfg.proximity);
        let mut interval = time::interval(Duration::from_millis(cfg.bluetooth.poll_interval_ms));

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
            }

            let reading = match bt.relative_path_loss().await {
                Ok(rpl) => Reading::Rpl(rpl),
                Err(e) => {
                    warn!("BT poll failed: {e}");
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


fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
}
