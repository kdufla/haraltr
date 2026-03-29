use std::ffi::CStr;

use nonstick::{AuthnFlags, ErrorCode, ModuleClient, PamModule, Result as PamResult, pam_export};
use syslog::{Facility, Formatter3164};

struct OddLenMod;
pam_export!(OddLenMod);

impl<M: ModuleClient> PamModule<M> for OddLenMod {
    fn authenticate(handle: &mut M, _args: Vec<&CStr>, _flags: AuthnFlags) -> PamResult<()> {
        let formatter = Formatter3164 {
            facility: Facility::LOG_AUTH,
            hostname: None,
            process: "rust_pam".into(),
            pid: 0,
        };

        if let Ok(mut writer) = syslog::unix(formatter.clone()) {
            let _ = writer.info("Rust PAM module: Authenticate hook triggered!");
        }

        let username = handle.username(None)?;

        if let Ok(mut writer) = syslog::unix(formatter.clone()) {
            let _ = writer.info(format!(
                "Rust PAM module: {:?} tried Authenticate",
                username
            ));
        }

        if !username.len().is_multiple_of(2) {
            if let Ok(mut writer) = syslog::unix(formatter.clone()) {
                let _ = writer.info("Rust PAM module: Success!");
            }
            Ok(())
        } else {
            if let Ok(mut writer) = syslog::unix(formatter) {
                let _ = writer.info("Rust PAM module: Authentication Error!");
            }
            Err(ErrorCode::AuthenticationError)
        }
    }
}
