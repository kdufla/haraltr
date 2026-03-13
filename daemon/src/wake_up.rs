use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time;
use tracing::{debug, info, warn};

use crate::input::{VirtualKeyboard, VirtualMouse};

const MOUSE_INTERVAL: Duration = Duration::from_millis(250);
const ENTER_INTERVAL: Duration = Duration::from_millis(3000);

pub async fn wake_screen(duration: Duration) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        time::sleep(duration).await;
        let _ = tx.send(());
    });
    wake_up(rx).await
}

pub async fn wake_up(
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut keyboard = VirtualKeyboard::new()?;
    let mut mouse = VirtualMouse::new()?;

    let mut mouse_tick = time::interval(MOUSE_INTERVAL);
    let mut enter_tick = time::interval(ENTER_INTERVAL);
    let _skip_first_enter = enter_tick.tick().await;

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                info!("stopping wake-up inputs");
                break;
            }
            _ = mouse_tick.tick() => {
                if let Err(e) = mouse.move_mouse() {
                    warn!("mouse move failed: {e}");
                }
            }
            _ = enter_tick.tick() => {
                debug!("sending enter");
                if let Err(e) = keyboard.press_enter() {
                    warn!("enter press failed: {e}");
                }
            }
        }
    }

    Ok(())
}
