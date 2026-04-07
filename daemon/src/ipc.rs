use std::{
    os::unix::fs::PermissionsExt,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use common::{IPC_SOCKET_PATH, ProximityStatus, QUERY_SIZE, QueryKind, QueryResponse};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use tracing::{info, warn};

use crate::state::AppState;

pub fn spawn_ipc_listener(app_state: &Arc<AppState>) {
    let state = app_state.clone();
    tokio::spawn(async move {
        if let Err(e) = run_listener(state).await {
            warn!("IPC listener failed: {e}");
        }
    });
}

async fn run_listener(app_state: Arc<AppState>) -> std::io::Result<()> {
    std::fs::create_dir_all("/run/haraltr")?;
    std::fs::set_permissions("/run/haraltr", std::fs::Permissions::from_mode(0o700))?;

    match std::fs::remove_file(IPC_SOCKET_PATH) {
        Ok(()) => info!("removed stale socket"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!("failed to remove stale socket: {e}"),
    }

    let listener = UnixListener::bind(IPC_SOCKET_PATH)?;
    std::fs::set_permissions(IPC_SOCKET_PATH, std::fs::Permissions::from_mode(0o600))?;

    info!("IPC listener started on {IPC_SOCKET_PATH}");

    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("IPC accept error: {e}");
                continue;
            }
        };

        let state = app_state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &state).await {
                warn!("IPC connection error: {e}");
            }
        });
    }
}

async fn handle_connection(mut stream: UnixStream, app_state: &AppState) -> std::io::Result<()> {
    let cred = stream.peer_cred()?;
    if cred.uid() != 0 {
        warn!(uid = cred.uid(), "rejected non-root IPC connection");
        return Ok(());
    }

    let mut query = [0u8; QUERY_SIZE];
    stream.read_exact(&mut query).await?;

    if query[0] != QueryKind::IsDeviceNear as u8 {
        warn!(byte = query[0], "unknown IPC query kind");
        return Ok(());
    }

    let uid = u32::from_le_bytes(query[1..5].try_into().unwrap());

    let user_macs: Vec<String> = {
        let config = app_state.config.read().unwrap();
        config
            .devices
            .iter()
            .filter(|dev| dev.uid == uid)
            .map(|dev| dev.target_mac.clone())
            .collect()
    };

    let proximity = {
        let status = app_state.daemon_status.lock().unwrap();
        if user_macs.is_empty() {
            ProximityStatus::Unknown
        } else {
            if status.is_any_near(&user_macs) {
                ProximityStatus::Near
            } else if status.is_any_far(&user_macs) {
                ProximityStatus::Far
            } else {
                ProximityStatus::Disconnected
            }
        }
    };

    let response = QueryResponse {
        status: proximity as u8,
        timestamp_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    stream.write_all(response.as_bytes()).await?;
    Ok(())
}
