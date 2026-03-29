use tracing::{debug, error, info};
use zbus::{Connection, Proxy, Result, zvariant, zvariant::OwnedObjectPath};

use crate::session::logind_session::LogindSession;

#[derive(zvariant::Type, serde::Deserialize, Debug)]
struct SessionInfo {
    _id: String,
    _uid: u32,
    _user: String,
    seat: String,
    object_path: OwnedObjectPath,
}

#[derive(Debug)]
pub struct SessionController {
    connection: Connection,
    manager: Proxy<'static>,
}

impl SessionController {
    pub async fn new() -> Result<Self> {
        let connection = Connection::system().await?;
        let manager = Proxy::new_owned(
            connection.clone(),
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;

        Ok(Self {
            connection,
            manager,
        })
    }

    async fn find_active_session(&self) -> Result<LogindSession> {
        let sessions: Vec<SessionInfo> = self
            .manager
            .call::<&str, (), _>("ListSessions", &())
            .await?;

        for session_info in &sessions {
            debug!(seat = %session_info.seat, path = %session_info.object_path, "checking session");

            if session_info.seat != "seat0" {
                continue;
            }

            let session =
                LogindSession::new(self.connection.clone(), session_info.object_path.clone())
                    .await?;

            if session.is_active().await? {
                return Ok(session);
            }
        }

        Err(zbus::Error::Failure(format!(
            "no active session found on seat0 ({} sessions total)",
            sessions.len()
        )))
    }

    pub async fn lock(&self) -> Result<()> {
        let active_session = match self.find_active_session().await {
            Ok(session) => session,
            Err(e) => {
                error!("failed to find active session for lock: {e}");
                return Err(e);
            }
        };

        info!("locking session");
        active_session.lock().await?;
        info!("session locked");

        Ok(())
    }

    pub async fn unlock(&self) -> Result<()> {
        let active_session = match self.find_active_session().await {
            Ok(session) => session,
            Err(e) => {
                error!("failed to find active session for unlock: {e}");
                return Err(e);
            }
        };

        info!("unlocking session");
        active_session.unlock().await?;
        info!("session unlocked");

        Ok(())
    }
}
