use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};

use rand::Rng;
use serde::Serialize;

use crate::{config::Config, web::auth::AUTH_SESSION_DURATION};

#[derive(Debug, Clone)]
pub struct DeviceReport {
    pub target_mac: String,
    pub phase: ProximityPhase,
    pub rpl: Option<f64>,
    pub raw_rpl: Option<f64>,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub rpl: Option<f64>,
    pub raw_rpl: Option<f64>,
    pub phase: ProximityPhase,
    pub connected: bool,
}

// TODO this is stripped down kinda useless
// multi-device should be fixed for web and ipc
pub struct DaemonStatus {
    pub devices: HashMap<String, DeviceStatus>,
    pub any_near: bool,
    pub started_at: Instant,
}

impl DaemonStatus {
    pub fn is_any_near(&self, user_macs: &[String]) -> bool {
        self.devices.iter().any(|(mac, dev_status)| {
            user_macs.contains(mac) && dev_status.phase == ProximityPhase::Near
        })
    }

    pub fn is_any_far(&self, user_macs: &[String]) -> bool {
        self.devices.iter().any(|(mac, dev_status)| {
            user_macs.contains(mac) && dev_status.phase == ProximityPhase::Far
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProximityPhase {
    Near,
    Far,
    Disconnected,
}

impl std::fmt::Display for ProximityPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Near => write!(f, "near"),
            Self::Far => write!(f, "far"),
            Self::Disconnected => write!(f, "disconnected"),
        }
    }
}

pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub config_path: PathBuf,
    pub web_sessions: std::sync::Mutex<HashMap<String, Instant>>,
    pub daemon_status: Mutex<DaemonStatus>,
    pub config_notify: tokio::sync::Notify,
}

impl AppState {
    pub fn reset_daemon_state(&self) {
        let mut ds = self.daemon_status.lock().unwrap();
        ds.devices.clear();
        ds.any_near = true;
    }

    pub fn update_device(&self, mac: String, status: DeviceStatus, any_near: bool) {
        let mut ds = self.daemon_status.lock().unwrap();
        ds.devices.insert(mac, status);
        ds.any_near = any_near;
    }

    pub fn create_session(&self) -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);

        let expiry = Instant::now() + AUTH_SESSION_DURATION;
        self.web_sessions
            .lock()
            .unwrap()
            .insert(token.clone(), expiry);

        token
    }

    pub fn validate_session(&self, token: &str) -> bool {
        let mut sessions = self.web_sessions.lock().unwrap();
        match sessions.get(token) {
            Some(&expiry) if expiry > Instant::now() => true,
            Some(_) => {
                sessions.remove(token);
                false
            }
            None => false,
        }
    }

    pub fn remove_session(&self, token: &str) {
        self.web_sessions.lock().unwrap().remove(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app_state() -> AppState {
        AppState {
            config: Arc::new(RwLock::new(Config::default())),
            config_path: PathBuf::from("test_config.toml"),
            web_sessions: std::sync::Mutex::new(HashMap::new()),
            daemon_status: Mutex::from(DaemonStatus {
                devices: HashMap::new(),
                any_near: false,
                started_at: Instant::now(),
            }),
            config_notify: tokio::sync::Notify::new(),
        }
    }

    #[test]
    fn proximity_phase_display() {
        assert_eq!(format!("{}", ProximityPhase::Near), "near");
        assert_eq!(format!("{}", ProximityPhase::Far), "far");
        assert_eq!(format!("{}", ProximityPhase::Disconnected), "disconnected");
    }

    #[test]
    fn app_state_session_management() {
        let state = test_app_state();
        let token = state.create_session();

        assert_eq!(token.len(), 64); // hex encoded 32 bytes
        assert!(state.validate_session(&token));

        state.remove_session(&token);
        assert!(!state.validate_session(&token));
    }

    #[test]
    fn app_state_validate_missing_session() {
        let state = test_app_state();
        assert!(!state.validate_session("nonexistent"));
    }
}
