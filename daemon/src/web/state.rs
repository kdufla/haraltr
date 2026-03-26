use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use rand::Rng;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::config::Config;
use crate::web::auth::SESSION_DURATION;

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

#[derive(Serialize)]
pub struct RplReading {
    pub timestamp: f64,
    pub rpl: f64,
    pub raw_rpl: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RplUpdate {
    pub rpl: f64,
    pub raw_rpl: f64,
    pub state: String,
    pub connected: bool,
    pub timestamp: f64,
}

pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub config_path: PathBuf,
    pub sessions: std::sync::Mutex<HashMap<String, Instant>>,
    pub rpl_broadcast: broadcast::Sender<RplUpdate>,
    pub daemon_status: ArcSwap<DaemonStatus>,
    pub history: std::sync::Mutex<VecDeque<RplReading>>,
}

impl AppState {
    pub fn create_session(&self) -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);

        let expiry = Instant::now() + SESSION_DURATION;
        self.sessions.lock().unwrap().insert(token.clone(), expiry);

        token
    }

    pub fn validate_session(&self, token: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap();
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
        self.sessions.lock().unwrap().remove(token);
    }
}
