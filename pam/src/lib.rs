use std::{
    ffi::CStr,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::Duration,
};

use common::{IPC_SOCKET_PATH, ProximityStatus, QueryKind, QueryResponse};
use nonstick::{
    AuthnFlags, BaseFlags, ConversationAdapter, CredAction, ErrorCode, ModuleClient, PamModule,
    Result as PamResult, pam_export,
};
use syslog::{Facility, Formatter3164};

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(300);
const READ_TIMEOUT: Duration = Duration::from_millis(500);

struct HaraltrPam;
pam_export!(HaraltrPam);

impl<M: ModuleClient> PamModule<M> for HaraltrPam {
    fn authenticate(handle: &mut M, _args: Vec<&CStr>, _flags: AuthnFlags) -> PamResult<()> {
        let username = handle.username(None)?;
        log(format!("proximity auth requested for {username:?}"));

        handle.info_msg("haraltr: checking device proximity...");

        // TODO pass username to check only devices bound to the user
        for attempt in 0..MAX_RETRIES {
            match query_daemon() {
                Err(e) => return Err(e),
                Ok(response) => {
                    let status = response.status;
                    if status == ProximityStatus::Near as u8 {
                        log(format!(
                            "proximity auth succeeded for {username:?} (rpl={})",
                            { response.rpl }
                        ));
                        return Ok(());
                    }

                    log(format!(
                        "attempt {}/{}: device not near (status=0x{status:02x})",
                        attempt + 1,
                        MAX_RETRIES
                    ));

                    if attempt + 1 < MAX_RETRIES {
                        thread::sleep(RETRY_DELAY);
                    }
                }
            }
        }

        log(format!("proximity auth denied for {username:?}"));
        Err(ErrorCode::AuthenticationError)
    }

    fn set_credentials(
        _handle: &mut M,
        _args: Vec<&CStr>,
        _action: CredAction,
        _flags: BaseFlags,
    ) -> PamResult<()> {
        Ok(())
    }
}

fn log(msg: impl std::fmt::Display) {
    let syslog_formatter = Formatter3164 {
        facility: Facility::LOG_AUTH,
        hostname: None,
        process: "haraltr_pam".into(),
        pid: std::process::id(),
    };

    if let Ok(mut writer) = syslog::unix(syslog_formatter) {
        let _ = writer.info(msg);
    }
}

fn query_daemon() -> Result<QueryResponse, ErrorCode> {
    let mut stream = UnixStream::connect(IPC_SOCKET_PATH).map_err(|e| {
        log(format!("daemon unavailable: {e}"));
        ErrorCode::AuthInfoUnavailable
    })?;

    stream.set_read_timeout(Some(READ_TIMEOUT)).map_err(|e| {
        log(format!("failed to set read timeout: {e}"));
        ErrorCode::SystemError
    })?;

    stream
        .write_all(&[QueryKind::IsDeviceNear as u8])
        .map_err(|e| {
            log(format!("failed to send query: {e}"));
            ErrorCode::SystemError
        })?;

    let mut buf = [0u8; size_of::<QueryResponse>()];
    stream.read_exact(&mut buf).map_err(|e| {
        log(format!("failed to read response: {e}"));
        ErrorCode::SystemError
    })?;

    Ok(QueryResponse::from_bytes(&buf))
}
