use std::{
    os::unix::fs::PermissionsExt,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use common::{IPC_SOCKET_PATH, ProximityStatus, QueryKind, QueryResponse};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};
use tracing::{info, warn};

use crate::state::{AppState, ProximityPhase};

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

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    app_state: &AppState,
) -> std::io::Result<()> {
    let cred = stream.peer_cred()?;
    if cred.uid() != 0 {
        warn!(uid = cred.uid(), "rejected non-root IPC connection");
        return Ok(());
    }

    let mut query = [0u8; 1];
    stream.read_exact(&mut query).await?;

    if query[0] != QueryKind::IsDeviceNear as u8 {
        warn!(byte = query[0], "unknown IPC query kind");
        return Ok(());
    }

    // TODO multi-device daemon status. This is a tmp fix just to make it run.
    let status = app_state.daemon_status.load();
    let proximity = if status.any_near {
        ProximityStatus::Near
    } else if status
        .devices
        .values()
        .any(|d| d.phase == ProximityPhase::Far)
    {
        ProximityStatus::Far
    } else {
        ProximityStatus::Disconnected
    };

    let rpl = status
        .devices
        .values()
        .filter_map(|d| d.rpl)
        .reduce(f64::min)
        .unwrap_or(0.0);

    let response = QueryResponse {
        status: proximity as u8,
        rpl: rpl as f32,
        timestamp_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };

    stream.write_all(response.as_bytes()).await?;
    Ok(())
}
