use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use futures_lite::StreamExt;
use tracing::{error, info, warn};
use zbus::{Connection, Proxy};

use super::SessionController;

pub const NO_ACTIVE_UID: u32 = u32::MAX;

pub async fn spawn_session_watcher(controller: &SessionController, active_uid: Arc<AtomicU32>) {
    match controller.find_active_session().await {
        Ok((_session, uid)) => {
            info!(uid, "initial active session found");
            active_uid.store(uid, Ordering::Relaxed);
        }
        Err(_) => {
            info!("no active session at startup");
        }
    }

    let connection = controller.connection.clone();
    tokio::spawn(async move {
        if let Err(e) = watch_seat0(connection, active_uid).await {
            error!("session watcher failed: {e}");
        }
    });
}

async fn watch_seat0(connection: Connection, active_uid: Arc<AtomicU32>) -> zbus::Result<()> {
    let seat_proxy = Proxy::new_owned(
        connection.clone(),
        "org.freedesktop.login1",
        "/org/freedesktop/login1/seat/seat0",
        "org.freedesktop.DBus.Properties",
    )
    .await?;

    let mut stream = seat_proxy.receive_signal("PropertiesChanged").await?;

    info!("session watcher listening on seat0");

    while let Some(_signal) = stream.next().await {
        let manager = Proxy::new_owned(
            connection.clone(),
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;

        match resolve_active_uid(&connection, &manager).await {
            Ok(uid) => {
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

async fn resolve_active_uid(
    connection: &Connection,
    manager: &Proxy<'static>,
) -> zbus::Result<u32> {
    #[derive(zbus::zvariant::Type, serde::Deserialize)]
    struct SessionInfo {
        _id: String,
        uid: u32,
        _user: String,
        seat: String,
        object_path: zbus::zvariant::OwnedObjectPath,
    }

    let sessions: Vec<SessionInfo> = manager.call::<&str, (), _>("ListSessions", &()).await?;

    for session_info in &sessions {
        if session_info.seat != "seat0" {
            continue;
        }

        let session_proxy = Proxy::new_owned(
            connection.clone(),
            "org.freedesktop.login1",
            session_info.object_path.clone(),
            "org.freedesktop.login1.Session",
        )
        .await?;

        let active: bool = session_proxy.get_property("Active").await?;
        if active {
            return Ok(session_info.uid);
        }
    }

    Err(zbus::Error::Failure("no active session on seat0".into()))
}
