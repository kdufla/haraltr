use std::{
    ffi::CStr,
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

use common::{IPC_SOCKET_PATH, ProximityStatus, QueryKind, QueryResponse};
use nonstick::{AuthnFlags, ErrorCode, ModuleClient, PamModule, Result as PamResult, pam_export};
use syslog::{Facility, Formatter3164};

struct HaraltrPam;
pam_export!(HaraltrPam);

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

impl<M: ModuleClient> PamModule<M> for HaraltrPam {
    fn authenticate(handle: &mut M, _args: Vec<&CStr>, _flags: AuthnFlags) -> PamResult<()> {
        let username = handle.username(None)?;
        log(format!("proximity auth requested for {username:?}"));

        let mut stream = match UnixStream::connect(IPC_SOCKET_PATH) {
            Ok(s) => s,
            Err(e) => {
                log(format!("daemon unavailable: {e}"));
                return Err(ErrorCode::AuthInfoUnavailable);
            }
        };

        if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
            log(format!("failed to set read timeout: {e}"));
            return Err(ErrorCode::AuthInfoUnavailable);
        }

        if let Err(e) = stream.write_all(&[QueryKind::IsDeviceNear as u8]) {
            log(format!("failed to send query: {e}"));
            return Err(ErrorCode::AuthInfoUnavailable);
        }

        let mut buf = [0u8; size_of::<QueryResponse>()];
        if let Err(e) = stream.read_exact(&mut buf) {
            log(format!("failed to read response: {e}"));
            return Err(ErrorCode::AuthInfoUnavailable);
        }

        let response = QueryResponse::from_bytes(&buf);
        let status = response.status;

        if status == ProximityStatus::Near as u8 {
            log(format!(
                "proximity auth succeeded for {username:?} (rpl={})",
                { response.rpl }
            ));
            Ok(())
        } else {
            log(format!(
                "proximity auth denied for {username:?} (status=0x{status:02x})"
            ));
            Err(ErrorCode::AuthenticationError)
        }
    }
}
