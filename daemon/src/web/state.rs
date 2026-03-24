use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::config::Config;

pub struct AppState {
    pub config: Arc<ArcSwap<Config>>,
    pub config_path: PathBuf,
    pub sessions: std::sync::Mutex<HashMap<String, Instant>>,
    pub rpl_broadcast: broadcast::Sender<RplUpdate>,
    pub daemon_status: ArcSwap<DaemonStatus>,
    pub history: std::sync::Mutex<VecDeque<RplReading>>,
}

pub struct DaemonStatus {
    pub rpl: Option<f64>,
    pub raw_rpl: Option<f64>,
    pub state: ProximityPhase,
    pub connected: bool,
    pub target_mac: Option<String>,
    pub started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProximityPhase {
    Near,
    Far,
    Disconnected,
}

pub struct RplReading {
    pub timestamp: f64,
    pub rpl: f64,
    pub raw_rpl: f64,
}

#[derive(Clone, Serialize)]
pub struct RplUpdate {
    pub rpl: f64,
    pub raw_rpl: f64,
    pub state: String,
    pub connected: bool,
    pub timestamp: f64,
}
