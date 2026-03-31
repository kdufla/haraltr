use std::{
    collections::HashSet,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing::info;
use validator::{Validate, ValidationError};
use xdg::BaseDirectories;

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
    Validation(validator::ValidationErrors),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config I/O error: {e}"),
            ConfigError::Parse(e) => write!(f, "config parse error: {e}"),
            ConfigError::Serialize(e) => write!(f, "config serialize error: {e}"),
            ConfigError::Validation(e) => write!(f, "config validation error: {e}"),
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

impl From<validator::ValidationErrors> for ConfigError {
    fn from(e: validator::ValidationErrors) -> Self {
        ConfigError::Validation(e)
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

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct BluetoothConfig {
    #[serde(default)]
    pub adapter_index: u16,
    #[serde(default)]
    pub address_type: AddressTypeConfig,
    #[validate(range(min = 1))]
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[validate(range(min = 1))]
    #[serde(default = "default_disconnect_poll_interval_ms")]
    pub disconnect_poll_interval_ms: u64,
}

impl Default for BluetoothConfig {
    fn default() -> Self {
        Self {
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

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ProximityConfig {
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    #[serde(default = "default_rpl_threshold")]
    pub rpl_threshold: f64,
    #[validate(range(min = 1))]
    #[serde(default = "default_lock_count")]
    pub lock_count: u32,
    #[validate(range(min = 1))]
    #[serde(default = "default_unlock_count")]
    pub unlock_count: u32,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    #[serde(default = "default_kalman_q")]
    pub kalman_q: f64,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
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

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct WakeConfig {
    #[validate(range(min = 1))]
    #[serde(default = "default_duration_secs")]
    pub duration_secs: u64,
    #[validate(range(min = 1))]
    #[serde(default = "default_mouse_interval_ms")]
    pub mouse_interval_ms: u64,
    #[validate(range(min = 1))]
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

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct WebConfig {
    #[serde(default = "default_web_enabled")]
    pub enabled: bool,
    #[validate(range(min = 1))]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonMode {
    #[default]
    Both,
    PamOnly,
    LockOnly,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct DaemonConfig {
    #[serde(default)]
    pub mode: DaemonMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct BluetoothOverrides {
    #[serde(default)]
    pub adapter_index: Option<u16>,
    #[serde(default)]
    pub address_type: Option<AddressTypeConfig>,
    #[validate(range(min = 1))]
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
    #[validate(range(min = 1))]
    #[serde(default)]
    pub disconnect_poll_interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct ProximityOverrides {
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    #[serde(default)]
    pub rpl_threshold: Option<f64>,
    #[validate(range(min = 1))]
    #[serde(default)]
    pub lock_count: Option<u32>,
    #[validate(range(min = 1))]
    #[serde(default)]
    pub unlock_count: Option<u32>,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    #[serde(default)]
    pub kalman_q: Option<f64>,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    #[serde(default)]
    pub kalman_r: Option<f64>,
    #[serde(default)]
    pub kalman_initial: Option<f64>,
    #[serde(default)]
    pub disconnect_action: Option<DisconnectActionConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct DeviceEntry {
    #[validate(custom(function = "validate_mac"))]
    pub target_mac: String,
    #[serde(default)]
    pub name: Option<String>,
    #[validate(nested)]
    #[serde(default)]
    pub bluetooth: BluetoothOverrides,
    #[validate(nested)]
    #[serde(default)]
    pub proximity: ProximityOverrides,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
#[validate(schema(function = "validate_config_schema"))]
pub struct Config {
    #[validate(nested)]
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[validate(nested)]
    #[serde(default)]
    pub bluetooth: BluetoothConfig,
    #[validate(nested)]
    #[serde(default)]
    pub proximity: ProximityConfig,
    #[validate(nested)]
    #[serde(default)]
    pub wake: WakeConfig,
    #[validate(nested)]
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub active_device: Option<String>,
    #[validate(nested)]
    #[serde(default)]
    pub devices: Vec<DeviceEntry>,
}

pub fn validate_mac(mac: &str) -> Result<(), ValidationError> {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(ValidationError::new("invalid mac"))
    }
}

fn validate_config_schema(config: &Config) -> Result<(), ValidationError> {
    let mut seen_macs = HashSet::new();
    for dev in &config.devices {
        if !seen_macs.insert(&dev.target_mac) {
            return Err(ValidationError::new("duplicate mac"));
        }
    }

    if let Some(ref active_mac) = config.active_device
        && !config.devices.iter().any(|d| d.target_mac == *active_mac)
    {
        return Err(ValidationError::new("active device not found"));
    }

    Ok(())
}

impl Config {
    pub fn active_device_entry(&self) -> Option<&DeviceEntry> {
        let mac = self.active_device.as_deref()?;
        self.devices.iter().find(|d| d.target_mac == mac)
    }

    pub fn resolved_target_mac(&self) -> Option<&str> {
        self.active_device_entry().map(|d| d.target_mac.as_str())
    }

    pub fn resolved_bluetooth(&self) -> BluetoothConfig {
        if let Some(active_device) = self.active_device_entry() {
            let BluetoothOverrides {
                adapter_index,
                address_type,
                poll_interval_ms,
                disconnect_poll_interval_ms,
            } = &active_device.bluetooth;

            BluetoothConfig {
                adapter_index: adapter_index.unwrap_or(self.bluetooth.adapter_index),
                address_type: address_type.unwrap_or(self.bluetooth.address_type),
                poll_interval_ms: poll_interval_ms.unwrap_or(self.bluetooth.poll_interval_ms),
                disconnect_poll_interval_ms: disconnect_poll_interval_ms
                    .unwrap_or(self.bluetooth.disconnect_poll_interval_ms),
            }
        } else {
            self.bluetooth.clone()
        }
    }

    pub fn resolved_proximity(&self) -> ProximityConfig {
        if let Some(active_device) = self.active_device_entry() {
            let ProximityOverrides {
                rpl_threshold,
                lock_count,
                unlock_count,
                kalman_q,
                kalman_r,
                kalman_initial,
                disconnect_action,
            } = &active_device.proximity;

            ProximityConfig {
                rpl_threshold: rpl_threshold.unwrap_or(self.proximity.rpl_threshold),
                lock_count: lock_count.unwrap_or(self.proximity.lock_count),
                unlock_count: unlock_count.unwrap_or(self.proximity.unlock_count),
                kalman_q: kalman_q.unwrap_or(self.proximity.kalman_q),
                kalman_r: kalman_r.unwrap_or(self.proximity.kalman_r),
                kalman_initial: kalman_initial.unwrap_or(self.proximity.kalman_initial),
                disconnect_action: (*disconnect_action).unwrap_or(self.proximity.disconnect_action),
            }
        } else {
            self.proximity.clone()
        }
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = ensure_config_file()?;
        info!("loading config from {}", path.display());
        let contents = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = ensure_config_file()?;
        self.save_to_file(&path)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
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
        assert!(config.active_device.is_none());
        assert!(config.devices.is_empty());
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let toml_str = r#"
active_device = "AA:BB:CC:DD:EE:FF"

[[devices]]
target_mac = "AA:BB:CC:DD:EE:FF"

[proximity]
rpl_threshold = 20.0
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.resolved_target_mac(), Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(config.bluetooth.poll_interval_ms, 2000);
        assert_eq!(config.proximity.rpl_threshold, 20.0);
        assert_eq!(config.proximity.lock_count, 5);
        assert_eq!(config.wake.duration_secs, 3);
    }

    #[test]
    fn round_trip_serialize_deserialize() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(
            config.proximity.rpl_threshold,
            deserialized.proximity.rpl_threshold
        );
        assert_eq!(config.wake.duration_secs, deserialized.wake.duration_secs);
        assert_eq!(config.active_device, deserialized.active_device);
        assert_eq!(config.devices.len(), deserialized.devices.len());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("haraltr_test_config");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test_config.toml");
        let mut config = Config {
            active_device: Some("11:22:33:44:55:66".into()),
            ..Default::default()
        };
        config.devices.push(DeviceEntry {
            target_mac: "11:22:33:44:55:66".into(),
            name: None,
            bluetooth: BluetoothOverrides::default(),
            proximity: ProximityOverrides::default(),
        });
        config.proximity.rpl_threshold = 25.0;

        config.save_to_file(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&contents).unwrap();

        assert_eq!(loaded.resolved_target_mac(), Some("11:22:33:44:55:66"));
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

    #[test]
    fn no_devices_returns_global_config() {
        let config: Config = toml::from_str(
            r#"
[bluetooth]
poll_interval_ms = 500

[proximity]
rpl_threshold = 20.0
"#,
        )
        .unwrap();

        assert!(config.active_device_entry().is_none());
        assert!(config.resolved_target_mac().is_none());
        let bt = config.resolved_bluetooth();
        assert_eq!(bt.poll_interval_ms, 500);
        let prox = config.resolved_proximity();
        assert_eq!(prox.rpl_threshold, 20.0);
    }

    #[test]
    fn active_device_overrides_applied() {
        let config: Config = toml::from_str(
            r#"
active_device = "11:22:33:44:55:66"

[bluetooth]
adapter_index = 0
address_type = "br_edr"
poll_interval_ms = 2000

[proximity]
rpl_threshold = 15.0
kalman_q = 0.1

[[devices]]
target_mac = "11:22:33:44:55:66"
name = "Phone"

[devices.bluetooth]
adapter_index = 1
address_type = "le_public"

[devices.proximity]
rpl_threshold = 13.0
"#,
        )
        .unwrap();

        assert_eq!(config.resolved_target_mac(), Some("11:22:33:44:55:66"));
        let bt = config.resolved_bluetooth();
        assert_eq!(bt.adapter_index, 1);
        assert_eq!(bt.address_type, AddressTypeConfig::LePublic);
        assert_eq!(bt.poll_interval_ms, 2000);

        let prox = config.resolved_proximity();
        assert_eq!(prox.rpl_threshold, 13.0);
        assert_eq!(prox.kalman_q, 0.1);
    }

    #[test]
    fn active_device_no_overrides_uses_globals() {
        let config: Config = toml::from_str(
            r#"
active_device = "AA:BB:CC:DD:EE:FF"

[bluetooth]
adapter_index = 0
poll_interval_ms = 2000

[proximity]
rpl_threshold = 15.0

[[devices]]
target_mac = "AA:BB:CC:DD:EE:FF"
name = "Watch"
"#,
        )
        .unwrap();

        assert_eq!(config.resolved_target_mac(), Some("AA:BB:CC:DD:EE:FF"));
        let bt = config.resolved_bluetooth();
        assert_eq!(bt.adapter_index, 0);
        assert_eq!(bt.poll_interval_ms, 2000);
        let prox = config.resolved_proximity();
        assert_eq!(prox.rpl_threshold, 15.0);
    }

    #[test]
    fn active_device_nonexistent_mac_returns_none() {
        let config: Config = toml::from_str(
            r#"
active_device = "FF:FF:FF:FF:FF:FF"

[bluetooth]
poll_interval_ms = 2000

[[devices]]
target_mac = "11:22:33:44:55:66"
"#,
        )
        .unwrap();

        assert!(config.active_device_entry().is_none());
        assert!(config.resolved_target_mac().is_none());
    }

    #[test]
    fn devices_round_trip() {
        let mut config = Config {
            active_device: Some("11:22:33:44:55:66".into()),
            ..Default::default()
        };
        config.devices = vec![
            DeviceEntry {
                target_mac: "11:22:33:44:55:66".into(),
                name: Some("Phone".into()),
                bluetooth: BluetoothOverrides {
                    adapter_index: Some(1),
                    address_type: Some(AddressTypeConfig::LePublic),
                    ..Default::default()
                },
                proximity: ProximityOverrides {
                    rpl_threshold: Some(13.0),
                    ..Default::default()
                },
            },
            DeviceEntry {
                target_mac: "AA:BB:CC:DD:EE:FF".into(),
                name: None,
                bluetooth: BluetoothOverrides::default(),
                proximity: ProximityOverrides::default(),
            },
        ];

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(
            deserialized.active_device.as_deref(),
            Some("11:22:33:44:55:66")
        );
        assert_eq!(deserialized.devices.len(), 2);
        assert_eq!(deserialized.devices[0].target_mac, "11:22:33:44:55:66");
        assert_eq!(deserialized.devices[0].name.as_deref(), Some("Phone"));
        assert_eq!(deserialized.devices[0].bluetooth.adapter_index, Some(1));
        assert_eq!(deserialized.devices[0].proximity.rpl_threshold, Some(13.0));
        assert_eq!(deserialized.devices[1].target_mac, "AA:BB:CC:DD:EE:FF");
        assert!(deserialized.devices[1].name.is_none());
    }

    #[test]
    fn validation_invalid_mac() {
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            target_mac: "invalid".into(),
            ..Default::default()
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn validation_duplicate_mac() {
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            target_mac: "AA:BB:CC:DD:EE:FF".into(),
            ..Default::default()
        });
        config.devices.push(DeviceEntry {
            target_mac: "AA:BB:CC:DD:EE:FF".into(),
            ..Default::default()
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn validation_active_device_not_found() {
        let config = Config {
            active_device: Some("AA:BB:CC:DD:EE:FF".into()),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn validation_web_port_zero() {
        let mut config = Config::default();
        config.web.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn valid_mac_addresses() {
        assert!(validate_mac("AA:BB:CC:DD:EE:FF").is_ok());
        assert!(validate_mac("00:11:22:33:44:55").is_ok());
        assert!(validate_mac("aa:bb:cc:dd:ee:ff").is_ok());
    }

    #[test]
    fn invalid_mac_addresses() {
        assert!(validate_mac("not-a-mac").is_err());
        assert!(validate_mac("AA:BB:CC:DD:EE").is_err());
        assert!(validate_mac("AA:BB:CC:DD:EE:GG").is_err());
        assert!(validate_mac("AABB.CCDD.EEFF").is_err());
        assert!(validate_mac("").is_err());
    }
}
