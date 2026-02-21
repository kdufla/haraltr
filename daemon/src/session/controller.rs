use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};
use zbus::{Result, zvariant};

use crate::session::logind_session::LogindSession;

#[derive(zvariant::Type, serde::Deserialize)]
struct SessionInfo {
    _id: String,
    _uid: u32,
    _user: String,
    seat: String,
    object_path: OwnedObjectPath,
}

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

        for session_info in sessions {
            if session_info.seat != "seat0" {
                continue;
            }

            let session =
                LogindSession::new(self.connection.clone(), session_info.object_path).await?;

            if session.is_active().await? {
                return Ok(session);
            }
        }

        panic!("No graphical session found on seat0");
    }

    pub async fn lock(&self) -> Result<()> {
        let active_session = self.find_active_session().await?;

        println!("Locking");
        active_session.lock().await?;
        println!("Locked");

        Ok(())
    }

    pub async fn unlock(&self) -> Result<()> {
        let active_session = self.find_active_session().await?;

        println!("Unlocking");
        active_session.unlock().await?;
        println!("Unlocked");

        Ok(())
    }
}
