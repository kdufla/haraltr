use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};

use arc_swap::ArcSwap;
use rand::Rng;
use serde::Serialize;

use crate::{config::Config, web::auth::AUTH_SESSION_DURATION};

pub struct DaemonStatus {
    pub rpl: Option<f64>,
    pub raw_rpl: Option<f64>,
    pub state: ProximityPhase,
    pub connected: bool,
    pub target_mac: Option<String>,
    pub started_at: Instant,
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
    pub config: Arc<ArcSwap<Config>>,
    pub config_path: PathBuf,
    pub web_sessions: std::sync::Mutex<HashMap<String, Instant>>,
    pub daemon_status: ArcSwap<DaemonStatus>,
    pub config_notify: tokio::sync::Notify,
}

impl AppState {
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
