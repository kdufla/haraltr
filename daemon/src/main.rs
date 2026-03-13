mod bt_mgmt;
mod input;
mod kalman;
mod proximity;
mod session;
mod wake_up;

use bdaddr::Address;
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tokio::time;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::bt_mgmt::BtMgmt;
use crate::proximity::{Action, Reading, State};
use crate::session::SessionController;
use crate::wake_up::wake_screen;

const PHONE_MAC: &str = "24:29:34:8E:0A:58";
const POLL_INTERVAL_MS: u64 = 2000;
const DISCONNECT_POLL_INTERVAL_MS: u64 = 5000;
const WAKE_DURATION: Duration = Duration::from_secs(3);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let mut bt = BtMgmt::new(PHONE_MAC.parse::<Address>().unwrap().into())?;
    let session = SessionController::new().await?;
    let mut state = State::new(Action::Lock);
    let mut interval = time::interval(Duration::from_millis(POLL_INTERVAL_MS));

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    info!("daemon started, monitoring {PHONE_MAC}");

    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = sigterm.recv() => {
                info!("received SIGTERM, shutting down");
                break;
            }
            _ = sigint.recv() => {
                info!("received SIGINT, shutting down");
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

        // Adjust poll rate when connection state changes
        if was_disconnected != is_disconnected {
            let target_ms = if is_disconnected {
                DISCONNECT_POLL_INTERVAL_MS
            } else {
                POLL_INTERVAL_MS
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
                if let Err(e) = wake_screen(WAKE_DURATION).await {
                    error!("wake failed: {e}");
                }
                if let Err(e) = session.unlock().await {
                    error!("unlock failed: {e}");
                }
            }
            Action::None => {}
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
