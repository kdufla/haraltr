use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use futures_lite::StreamExt;
use tracing::{error, info, warn};
use zbus::{Proxy, Result, proxy::SignalStream};

use super::SessionController;

pub const NO_ACTIVE_UID: u32 = u32::MAX;

pub async fn spawn_session_watcher(controller: SessionController, active_uid: Arc<AtomicU32>) {
    match controller.find_active_session().await {
        Ok((_, uid)) => {
            info!(uid, "initial active session found");
            active_uid.store(uid, Ordering::Relaxed);
        }
        Err(_) => {
            info!("no active session at startup");
        }
    }

    tokio::spawn(async move {
        if let Err(e) = watch_seat0(controller, active_uid).await {
            error!("session watcher failed: {e}");
        }
    });
}

async fn watch_seat0(controller: SessionController, active_uid: Arc<AtomicU32>) -> Result<()> {
    let mut stream = listen_to_properties_changed(&controller).await?;
    info!("session watcher listening on seat0");

    while let Some(_signal) = stream.next().await {
        match controller.find_active_session().await {
            Ok((_, uid)) => {
                let prev = active_uid.swap(uid, Ordering::Relaxed);
                if prev != uid {
                    info!(prev_uid = prev, new_uid = uid, "active session changed");
                }
            }
            Err(_) => {
                let prev = active_uid.swap(NO_ACTIVE_UID, Ordering::Relaxed);
                if prev != NO_ACTIVE_UID {
                    info!(prev_uid = prev, "no active session on seat0");
                }
            }
        }
    }

    warn!("session watcher stream ended");
    Ok(())
}

async fn listen_to_properties_changed(controller: &SessionController) -> Result<SignalStream<'_>> {
    Proxy::new(
        &controller.connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/seat/seat0",
        "org.freedesktop.DBus.Properties",
    )
    .await?
    .receive_signal("PropertiesChanged")
    .await
}
