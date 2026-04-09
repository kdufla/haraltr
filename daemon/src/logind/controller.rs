use tracing::{debug, error, info};
use zbus::{Connection, Proxy, Result, zvariant, zvariant::OwnedObjectPath};

use crate::logind::session::LogindSession;

#[derive(zvariant::Type, serde::Deserialize, Debug)]
#[allow(dead_code)]
struct SessionInfo {
    id: String,
    uid: u32,
    user: String,
    seat: String,
    object_path: OwnedObjectPath,
}

#[derive(Debug, Clone)]
pub struct SessionController {
    pub(crate) connection: Connection,
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

    pub(super) async fn find_active_session(&self) -> Result<(LogindSession, u32)> {
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
                return Ok((session, session_info.uid));
            }
        }

        Err(zbus::Error::Failure(format!(
            "no active session found on seat0 ({} sessions total)",
            sessions.len()
        )))
    }

    pub async fn lock(&self, uid: u32) -> Result<()> {
        let (session, active_uid) = match self.find_active_session().await {
            Ok(result) => result,
            Err(e) => {
                error!("failed to find active session for lock: {e}");
                return Err(e);
            }
        };

        if active_uid != uid {
            info!(
                expected = uid,
                actual = active_uid,
                "skipping lock: not active user"
            );
            return Ok(());
        }

        info!("locking session");
        session.lock().await?;
        info!("session locked");

        Ok(())
    }

    pub async fn unlock(&self, uid: u32) -> Result<()> {
        let (session, active_uid) = match self.find_active_session().await {
            Ok(result) => result,
            Err(e) => {
                error!("failed to find active session for unlock: {e}");
                return Err(e);
            }
        };

        if active_uid != uid {
            info!(
                expected = uid,
                actual = active_uid,
                "skipping unlock: not active user"
            );
            return Ok(());
        }

        info!("unlocking session");
        session.unlock().await?;
        info!("session unlocked");

        Ok(())
    }
}
