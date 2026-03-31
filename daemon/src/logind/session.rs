use zbus::{Connection, Proxy, Result, zvariant::OwnedObjectPath};

pub(super) struct LogindSession(Proxy<'static>);

impl LogindSession {
    pub async fn new(connection: Connection, object_path: OwnedObjectPath) -> Result<Self> {
        let session_proxy = Proxy::new_owned(
            connection,
            "org.freedesktop.login1",
            object_path,
            "org.freedesktop.login1.Session",
        )
        .await?;

        Ok(Self(session_proxy))
    }

    pub async fn is_active(&self) -> Result<bool> {
        self.0.get_property("Active").await
    }

    pub async fn lock(&self) -> Result<()> {
        self.0.call::<&str, (), ()>("Lock", &()).await?;
        Ok(())
    }

    pub async fn unlock(&self) -> Result<()> {
        self.0.call::<&str, (), ()>("Unlock", &()).await?;
        Ok(())
    }
}
