use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing::info;
use xdg::BaseDirectories;

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config I/O error: {e}"),
            ConfigError::Parse(e) => write!(f, "config parse error: {e}"),
            ConfigError::Serialize(e) => write!(f, "config serialize error: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        ConfigError::Parse(e)
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(e: toml::ser::Error) -> Self {
        ConfigError::Serialize(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AddressTypeConfig {
    #[default]
    BrEdr,
    LePublic,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DisconnectActionConfig {
    Lock,
    Unlock,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BluetoothConfig {
    #[serde(default)]
    pub target_mac: Option<String>,
    #[serde(default)]
    pub adapter_index: u16,
    #[serde(default)]
    pub address_type: AddressTypeConfig,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_disconnect_poll_interval_ms")]
    pub disconnect_poll_interval_ms: u64,
}

impl Default for BluetoothConfig {
    fn default() -> Self {
        Self {
            target_mac: None,
            adapter_index: 0,
            address_type: AddressTypeConfig::default(),
            poll_interval_ms: default_poll_interval_ms(),
            disconnect_poll_interval_ms: default_disconnect_poll_interval_ms(),
        }
    }
}

fn default_poll_interval_ms() -> u64 {
    2000
}

fn default_disconnect_poll_interval_ms() -> u64 {
    5000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProximityConfig {
    #[serde(default = "default_rpl_threshold")]
    pub rpl_threshold: f64,
    #[serde(default = "default_lock_count")]
    pub lock_count: u32,
    #[serde(default = "default_unlock_count")]
    pub unlock_count: u32,
    #[serde(default = "default_kalman_q")]
    pub kalman_q: f64,
    #[serde(default = "default_kalman_r")]
    pub kalman_r: f64,
    #[serde(default = "default_kalman_initial")]
    pub kalman_initial: f64,
    #[serde(default)]
    pub disconnect_action: DisconnectActionConfig,
}

impl Default for ProximityConfig {
    fn default() -> Self {
        Self {
            rpl_threshold: default_rpl_threshold(),
            lock_count: default_lock_count(),
            unlock_count: default_unlock_count(),
            kalman_q: default_kalman_q(),
            kalman_r: default_kalman_r(),
            kalman_initial: default_kalman_initial(),
            disconnect_action: DisconnectActionConfig::default(),
        }
    }
}

fn default_rpl_threshold() -> f64 {
    15.0
}

fn default_lock_count() -> u32 {
    5
}

fn default_unlock_count() -> u32 {
    5
}

fn default_kalman_q() -> f64 {
    0.1
}

fn default_kalman_r() -> f64 {
    3.0
}

fn default_kalman_initial() -> f64 {
    5.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeConfig {
    #[serde(default = "default_duration_secs")]
    pub duration_secs: u64,
    #[serde(default = "default_mouse_interval_ms")]
    pub mouse_interval_ms: u64,
    #[serde(default = "default_enter_interval_ms")]
    pub enter_interval_ms: u64,
}

impl Default for WakeConfig {
    fn default() -> Self {
        Self {
            duration_secs: default_duration_secs(),
            mouse_interval_ms: default_mouse_interval_ms(),
            enter_interval_ms: default_enter_interval_ms(),
        }
    }
}

fn default_duration_secs() -> u64 {
    3
}

fn default_mouse_interval_ms() -> u64 {
    250
}

fn default_enter_interval_ms() -> u64 {
    3000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(default = "default_web_enabled")]
    pub enabled: bool,
    #[serde(default = "default_web_port")]
    pub port: u16,
    #[serde(default)]
    pub password_hash: Option<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: default_web_enabled(),
            port: default_web_port(),
            password_hash: None,
        }
    }
}

fn default_web_enabled() -> bool {
    true
}

fn default_web_port() -> u16 {
    7878
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub bluetooth: BluetoothConfig,
    #[serde(default)]
    pub proximity: ProximityConfig,
    #[serde(default)]
    pub wake: WakeConfig,
    #[serde(default)]
    pub web: WebConfig,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let path = ensure_config_file()?;
        info!("loading config from {}", path.display());
        let contents = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = ensure_config_file()?;
        self.save_to_file(&path)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), ConfigError> {
        let contents = toml::to_string_pretty(self)?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let tmp_path = dir.join(".config.toml.tmp");
        fs::write(&tmp_path, &contents)?;
        fs::rename(&tmp_path, path)?;

        #[cfg(not(debug_assertions))]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    ensure_config_file()
}

fn ensure_config_file() -> Result<PathBuf, ConfigError> {
    #[cfg(debug_assertions)]
    {
        let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config.toml");
        if local.exists() {
            return Ok(local);
        }
    }

    let xdg = BaseDirectories::with_prefix("haraltr");

    if let Some(path) = xdg.find_config_file("config.toml") {
        return Ok(path);
    }

    let path = xdg.place_config_file("config.toml")?;
    let defaults = Config::default();
    defaults.save_to_file(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_toml_gives_all_defaults() {
        let config: Config = toml::from_str("").unwrap();
        let defaults = Config::default();
        assert_eq!(config.bluetooth.target_mac, defaults.bluetooth.target_mac);
        assert_eq!(
            config.bluetooth.adapter_index,
            defaults.bluetooth.adapter_index
        );
        assert_eq!(
            config.bluetooth.poll_interval_ms,
            defaults.bluetooth.poll_interval_ms
        );
        assert_eq!(
            config.proximity.rpl_threshold,
            defaults.proximity.rpl_threshold
        );
        assert_eq!(config.proximity.lock_count, defaults.proximity.lock_count);
        assert_eq!(config.proximity.kalman_q, defaults.proximity.kalman_q);
        assert_eq!(config.wake.duration_secs, defaults.wake.duration_secs);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let toml_str = r#"
[bluetooth]
target_mac = "AA:BB:CC:DD:EE:FF"

[proximity]
rpl_threshold = 20.0
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.bluetooth.target_mac.as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );
        assert_eq!(config.bluetooth.poll_interval_ms, 2000); // default
        assert_eq!(config.proximity.rpl_threshold, 20.0);
        assert_eq!(config.proximity.lock_count, 5); // default
        assert_eq!(config.wake.duration_secs, 3); // default
    }

    #[test]
    fn round_trip_serialize_deserialize() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(
            config.bluetooth.target_mac,
            deserialized.bluetooth.target_mac
        );
        assert_eq!(
            config.proximity.rpl_threshold,
            deserialized.proximity.rpl_threshold
        );
        assert_eq!(config.wake.duration_secs, deserialized.wake.duration_secs);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("haraltr_test_config");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test_config.toml");

        let mut config = Config::default();
        config.bluetooth.target_mac = Some("11:22:33:44:55:66".to_string());
        config.proximity.rpl_threshold = 25.0;

        config.save_to_file(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&contents).unwrap();

        assert_eq!(
            loaded.bluetooth.target_mac.as_deref(),
            Some("11:22:33:44:55:66")
        );
        assert_eq!(loaded.proximity.rpl_threshold, 25.0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn enum_serialization() {
        let toml_str = r#"
[bluetooth]
address_type = "le_public"

[proximity]
disconnect_action = "unlock"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.bluetooth.address_type, AddressTypeConfig::LePublic);
        assert_eq!(
            config.proximity.disconnect_action,
            DisconnectActionConfig::Unlock,
        );
    }

    #[test]
    fn web_config_defaults_when_missing() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.web.enabled);
        assert_eq!(config.web.port, 7878);
        assert!(config.web.password_hash.is_none());
    }

    #[test]
    fn web_config_round_trip() {
        let mut config = Config::default();
        config.web.enabled = false;
        config.web.port = 9090;
        config.web.password_hash = Some("$argon2id$v=19$test".to_string());

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert!(!deserialized.web.enabled);
        assert_eq!(deserialized.web.port, 9090);
        assert_eq!(
            deserialized.web.password_hash.as_deref(),
            Some("$argon2id$v=19$test"),
        );
    }

    #[test]
    fn web_config_password_hash_none_round_trip() {
        let config = Config::default();
        assert!(config.web.password_hash.is_none());

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert!(deserialized.web.password_hash.is_none());
    }
}
