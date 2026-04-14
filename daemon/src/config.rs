use std::{
    collections::HashSet,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing::info;
use validator::{Validate, ValidationError};
use xdg::BaseDirectories;

use crate::mac::Mac;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
#[validate(schema(function = "validate_config_schema"))]
pub struct Config {
    #[validate(nested)]
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub adapter_index: u16,
    #[validate(nested)]
    #[serde(default)]
    pub br_edr: BrEdrDefaults,
    #[validate(nested)]
    #[serde(default)]
    pub le: LeDefaults,
    #[validate(nested)]
    #[serde(default)]
    pub wake: WakeConfig,
    #[validate(nested)]
    #[serde(default)]
    pub web: WebConfig,
    #[validate(nested)]
    #[serde(default)]
    pub devices: Vec<DeviceEntry>,
}

impl Config {
    pub fn bluetooth_for_device(&self, device: &DeviceEntry) -> BluetoothConfig {
        let (poll, dis_poll) = match device.address_type {
            AddressTypeConfig::BrEdr => (
                self.br_edr.poll_interval_ms,
                self.br_edr.disconnect_poll_interval_ms,
            ),
            AddressTypeConfig::LePublic | AddressTypeConfig::LeRandom => (
                self.le.poll_interval_ms,
                self.le.disconnect_poll_interval_ms,
            ),
        };
        BluetoothConfig {
            adapter_index: device.bluetooth.adapter_index.unwrap_or(self.adapter_index),
            address_type: device.address_type,
            poll_interval_ms: device.bluetooth.poll_interval_ms.unwrap_or(poll),
            disconnect_poll_interval_ms: device
                .bluetooth
                .disconnect_poll_interval_ms
                .unwrap_or(dis_poll),
        }
    }

    pub fn proximity_for_device(&self, device: &DeviceEntry) -> ProximityConfig {
        match device.address_type {
            AddressTypeConfig::BrEdr => {
                let classic_defaults = &self.br_edr;
                ProximityConfig {
                    rpl_threshold: device
                        .proximity
                        .rpl_threshold
                        .unwrap_or(classic_defaults.rpl_threshold),
                    lock_count: device
                        .proximity
                        .lock_count
                        .unwrap_or(classic_defaults.lock_count),
                    unlock_count: device
                        .proximity
                        .unlock_count
                        .unwrap_or(classic_defaults.unlock_count),
                    kalman_q: device
                        .proximity
                        .kalman_q
                        .unwrap_or(classic_defaults.kalman_q),
                    kalman_r: device
                        .proximity
                        .kalman_r
                        .unwrap_or(classic_defaults.kalman_r),
                    kalman_initial: device
                        .proximity
                        .kalman_initial
                        .unwrap_or(classic_defaults.kalman_initial),
                    disconnect_action: device
                        .proximity
                        .disconnect_action
                        .unwrap_or(classic_defaults.disconnect_action),
                    fallback_tx_power: device.proximity.fallback_tx_power.unwrap_or(0),
                }
            }
            AddressTypeConfig::LePublic | AddressTypeConfig::LeRandom => {
                let le_defaults = &self.le;
                ProximityConfig {
                    rpl_threshold: device
                        .proximity
                        .rpl_threshold
                        .unwrap_or(le_defaults.rpl_threshold),
                    lock_count: device
                        .proximity
                        .lock_count
                        .unwrap_or(le_defaults.lock_count),
                    unlock_count: device
                        .proximity
                        .unlock_count
                        .unwrap_or(le_defaults.unlock_count),
                    kalman_q: device.proximity.kalman_q.unwrap_or(le_defaults.kalman_q),
                    kalman_r: device.proximity.kalman_r.unwrap_or(le_defaults.kalman_r),
                    kalman_initial: device
                        .proximity
                        .kalman_initial
                        .unwrap_or(le_defaults.kalman_initial),
                    disconnect_action: device
                        .proximity
                        .disconnect_action
                        .unwrap_or(le_defaults.disconnect_action),
                    fallback_tx_power: device
                        .proximity
                        .fallback_tx_power
                        .unwrap_or(le_defaults.fallback_tx_power),
                }
            }
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

#[derive(Debug, Clone)]
pub struct BluetoothConfig {
    pub adapter_index: u16,
    pub address_type: AddressTypeConfig,
    pub poll_interval_ms: u64,
    pub disconnect_poll_interval_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ProximityConfig {
    pub rpl_threshold: f64,
    pub lock_count: u32,
    pub unlock_count: u32,
    pub kalman_q: f64,
    pub kalman_r: f64,
    pub kalman_initial: f64,
    pub disconnect_action: DisconnectActionConfig,
    pub fallback_tx_power: i8,
}

impl Default for ProximityConfig {
    fn default() -> Self {
        let defaults = BrEdrDefaults::default();
        Self {
            rpl_threshold: defaults.rpl_threshold,
            lock_count: defaults.lock_count,
            unlock_count: defaults.unlock_count,
            kalman_q: defaults.kalman_q,
            kalman_r: defaults.kalman_r,
            kalman_initial: defaults.kalman_initial,
            disconnect_action: defaults.disconnect_action,
            fallback_tx_power: 0,
        }
    }
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct DaemonConfig {
    #[serde(default)]
    pub mode: DaemonMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonMode {
    #[default]
    Both,
    PamOnly,
    LockOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct BrEdrDefaults {
    #[validate(range(min = 1))]
    pub poll_interval_ms: u64,
    #[validate(range(min = 1))]
    pub disconnect_poll_interval_ms: u64,
    pub rpl_threshold: f64,
    #[validate(range(min = 1))]
    pub lock_count: u32,
    #[validate(range(min = 1))]
    pub unlock_count: u32,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    pub kalman_q: f64,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    pub kalman_r: f64,
    pub kalman_initial: f64,
    pub disconnect_action: DisconnectActionConfig,
}

impl Default for BrEdrDefaults {
    fn default() -> Self {
        Self {
            poll_interval_ms: 1000,
            disconnect_poll_interval_ms: 5000,
            rpl_threshold: 15.0,
            lock_count: 4,
            unlock_count: 4,
            kalman_q: 0.1,
            kalman_r: 3.0,
            kalman_initial: 5.0,
            disconnect_action: DisconnectActionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct LeDefaults {
    #[validate(range(min = 1))]
    pub poll_interval_ms: u64,
    #[validate(range(min = 1))]
    pub disconnect_poll_interval_ms: u64,
    pub rpl_threshold: f64,
    #[validate(range(min = 1))]
    pub lock_count: u32,
    #[validate(range(min = 1))]
    pub unlock_count: u32,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    pub kalman_q: f64,
    #[validate(range(exclusive_min = 0.0, message = "must be positive"))]
    pub kalman_r: f64,
    pub kalman_initial: f64,
    pub disconnect_action: DisconnectActionConfig,
    // if LE hci does not support HCI_OP_READ_TX_POWER
    pub fallback_tx_power: i8,
}

impl Default for LeDefaults {
    fn default() -> Self {
        Self {
            poll_interval_ms: 1000,
            disconnect_poll_interval_ms: 5000,
            rpl_threshold: 60.0,
            lock_count: 8,
            unlock_count: 8,
            kalman_q: 0.1,
            kalman_r: 8.0,
            kalman_initial: 40.0,
            disconnect_action: DisconnectActionConfig::default(),
            fallback_tx_power: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct WakeConfig {
    #[validate(range(min = 1))]
    pub duration_secs: u64,
    #[validate(range(min = 1))]
    pub mouse_interval_ms: u64,
    #[validate(range(min = 1))]
    pub enter_interval_ms: u64,
}

impl Default for WakeConfig {
    fn default() -> Self {
        Self {
            duration_secs: 3,
            mouse_interval_ms: 250,
            enter_interval_ms: 3000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct WebConfig {
    pub enabled: bool,
    #[validate(range(min = 1))]
    pub port: u16,
    #[serde(default)]
    pub password_hash: Option<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 15999,
            password_hash: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct DeviceEntry {
    pub uid: u32,
    #[validate(custom(function = "validate_mac"))]
    pub target_mac: String,
    #[serde(default)]
    pub address_type: AddressTypeConfig,
    #[serde(default)]
    pub name: Option<String>,
    #[validate(nested)]
    #[serde(default)]
    pub bluetooth: BluetoothOverrides,
    #[validate(nested)]
    #[serde(default)]
    pub proximity: ProximityOverrides,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AddressTypeConfig {
    #[default]
    BrEdr,
    LePublic,
    LeRandom,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct BluetoothOverrides {
    #[serde(default)]
    pub adapter_index: Option<u16>,
    #[validate(range(min = 1))]
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
    #[validate(range(min = 1))]
    #[serde(default)]
    pub disconnect_poll_interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct ProximityOverrides {
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
    #[serde(default)]
    pub fallback_tx_power: Option<i8>,
}

pub fn validate_mac(mac: &str) -> Result<(), ValidationError> {
    mac.parse::<Mac>()
        .map(|_| ())
        .map_err(|_| ValidationError::new("invalid mac"))
}

fn validate_config_schema(config: &Config) -> Result<(), ValidationError> {
    let mut seen_macs = HashSet::new();
    for dev in &config.devices {
        let parsed = dev
            .target_mac
            .parse::<Mac>()
            .map_err(|_| ValidationError::new("invalid mac"))?;
        if !seen_macs.insert(parsed) {
            return Err(ValidationError::new("duplicate mac"));
        }
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_toml_gives_all_defaults() {
        let config: Config = toml::from_str("").unwrap();
        let br_edr = BrEdrDefaults::default();
        let le = LeDefaults::default();
        assert_eq!(config.adapter_index, 0);
        assert_eq!(config.br_edr.poll_interval_ms, br_edr.poll_interval_ms);
        assert_eq!(config.br_edr.rpl_threshold, br_edr.rpl_threshold);
        assert_eq!(config.br_edr.lock_count, br_edr.lock_count);
        assert_eq!(config.br_edr.kalman_q, br_edr.kalman_q);
        assert_eq!(config.le.poll_interval_ms, le.poll_interval_ms);
        assert_eq!(config.le.rpl_threshold, le.rpl_threshold);
        assert_eq!(config.le.fallback_tx_power, le.fallback_tx_power);
        assert_eq!(
            config.wake.duration_secs,
            WakeConfig::default().duration_secs
        );
        assert!(config.devices.is_empty());
    }

    #[test]
    fn partial_br_edr_fills_from_br_edr_defaults() {
        let toml_str = r#"
[br_edr]
rpl_threshold = 20.0
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.br_edr.rpl_threshold, 20.0);
        assert_eq!(config.br_edr.poll_interval_ms, 1000);
        assert_eq!(config.br_edr.kalman_r, 3.0);
    }

    #[test]
    fn partial_le_fills_from_le_defaults() {
        let toml_str = r#"
[le]
rpl_threshold = 65.0
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.le.rpl_threshold, 65.0);
        assert_eq!(config.le.poll_interval_ms, 1000);
        assert_eq!(config.le.kalman_r, 8.0);
        assert_eq!(config.le.fallback_tx_power, 0);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let toml_str = r#"
[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"

[br_edr]
rpl_threshold = 20.0
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.devices[0].target_mac, "AA:BB:CC:DD:EE:FF");
        assert_eq!(config.br_edr.poll_interval_ms, 1000);
        assert_eq!(config.br_edr.rpl_threshold, 20.0);
        assert_eq!(config.br_edr.lock_count, 4);
        assert_eq!(config.wake.duration_secs, 3);
    }

    #[test]
    fn round_trip_serialize_deserialize() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(
            config.br_edr.rpl_threshold,
            deserialized.br_edr.rpl_threshold
        );
        assert_eq!(config.le.rpl_threshold, deserialized.le.rpl_threshold);
        assert_eq!(
            config.le.fallback_tx_power,
            deserialized.le.fallback_tx_power
        );
        assert_eq!(config.wake.duration_secs, deserialized.wake.duration_secs);
        assert_eq!(config.devices.len(), deserialized.devices.len());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("haraltr_test_config");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test_config.toml");
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            uid: 1000,
            target_mac: "11:22:33:44:55:66".into(),
            address_type: AddressTypeConfig::BrEdr,
            name: None,
            bluetooth: BluetoothOverrides::default(),
            proximity: ProximityOverrides::default(),
        });
        config.br_edr.rpl_threshold = 25.0;

        config.save_to_file(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&contents).unwrap();

        assert_eq!(loaded.devices[0].target_mac, "11:22:33:44:55:66");
        assert_eq!(loaded.br_edr.rpl_threshold, 25.0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn address_type_enum_serialization() {
        let toml_str = r#"
[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
address_type = "le_public"

[[devices]]
uid = 1001
target_mac = "11:22:33:44:55:66"
address_type = "le_random"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.devices[0].address_type, AddressTypeConfig::LePublic);
        assert_eq!(config.devices[1].address_type, AddressTypeConfig::LeRandom);
    }

    #[test]
    fn device_address_type_defaults_to_br_edr() {
        let toml_str = r#"
[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.devices[0].address_type, AddressTypeConfig::BrEdr);
    }

    #[test]
    fn web_config_defaults_when_missing() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.web.enabled);
        assert_eq!(config.web.port, 15999);
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
    fn br_edr_device_resolves_against_br_edr_defaults() {
        let config: Config = toml::from_str(
            r#"
[br_edr]
poll_interval_ms = 500
rpl_threshold = 20.0

[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
address_type = "br_edr"
"#,
        )
        .unwrap();

        let device = &config.devices[0];
        let bt = config.bluetooth_for_device(device);
        assert_eq!(bt.poll_interval_ms, 500);
        assert_eq!(bt.address_type, AddressTypeConfig::BrEdr);

        let prox = config.proximity_for_device(device);
        assert_eq!(prox.rpl_threshold, 20.0);
        assert_eq!(prox.fallback_tx_power, 0);
    }

    #[test]
    fn le_device_resolves_against_le_defaults() {
        let config: Config = toml::from_str(
            r#"
[le]
poll_interval_ms = 4000
rpl_threshold = 65.0

[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
address_type = "le_public"
"#,
        )
        .unwrap();

        let device = &config.devices[0];
        let bt = config.bluetooth_for_device(device);
        assert_eq!(bt.poll_interval_ms, 4000);
        assert_eq!(bt.address_type, AddressTypeConfig::LePublic);

        let prox = config.proximity_for_device(device);
        assert_eq!(prox.rpl_threshold, 65.0);
        assert_eq!(prox.fallback_tx_power, 0);
    }

    #[test]
    fn le_random_device_resolves_against_le_defaults() {
        let config: Config = toml::from_str(
            r#"
[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
address_type = "le_random"
"#,
        )
        .unwrap();

        let device = &config.devices[0];
        let bt = config.bluetooth_for_device(device);
        assert_eq!(bt.address_type, AddressTypeConfig::LeRandom);
        assert_eq!(bt.poll_interval_ms, LeDefaults::default().poll_interval_ms);

        let prox = config.proximity_for_device(device);
        assert_eq!(prox.fallback_tx_power, 0);
    }

    #[test]
    fn device_overrides_win_over_transport_defaults() {
        let config: Config = toml::from_str(
            r#"
[br_edr]
rpl_threshold = 15.0
kalman_q = 0.1

[[devices]]
uid = 1000
target_mac = "11:22:33:44:55:66"
address_type = "br_edr"
name = "Phone"

[devices.bluetooth]
adapter_index = 1
poll_interval_ms = 1000

[devices.proximity]
rpl_threshold = 13.0
"#,
        )
        .unwrap();

        let device = &config.devices[0];
        let bt = config.bluetooth_for_device(device);
        assert_eq!(bt.adapter_index, 1);
        assert_eq!(bt.poll_interval_ms, 1000);
        assert_eq!(bt.disconnect_poll_interval_ms, 5000);

        let prox = config.proximity_for_device(device);
        assert_eq!(prox.rpl_threshold, 13.0);
        assert_eq!(prox.kalman_q, 0.1);
    }

    #[test]
    fn fallback_tx_power_device_override_wins() {
        let config: Config = toml::from_str(
            r#"
[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
address_type = "le_public"

[devices.proximity]
fallback_tx_power = 4
"#,
        )
        .unwrap();

        let prox = config.proximity_for_device(&config.devices[0]);
        assert_eq!(prox.fallback_tx_power, 4);
    }

    #[test]
    fn fallback_tx_power_zero_for_br_edr_without_override() {
        let config: Config = toml::from_str(
            r#"
[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
address_type = "br_edr"
"#,
        )
        .unwrap();

        let prox = config.proximity_for_device(&config.devices[0]);
        assert_eq!(prox.fallback_tx_power, 0);
    }

    #[test]
    fn device_no_overrides_uses_transport_defaults() {
        let config: Config = toml::from_str(
            r#"
[br_edr]
poll_interval_ms = 2000
rpl_threshold = 15.0

[[devices]]
uid = 1000
target_mac = "AA:BB:CC:DD:EE:FF"
"#,
        )
        .unwrap();

        let device = &config.devices[0];
        let bt = config.bluetooth_for_device(device);
        assert_eq!(bt.adapter_index, 0);
        assert_eq!(bt.poll_interval_ms, 2000);
        let prox = config.proximity_for_device(device);
        assert_eq!(prox.rpl_threshold, 15.0);
    }

    #[test]
    fn devices_round_trip() {
        let config = Config {
            devices: vec![
                DeviceEntry {
                    uid: 1000,
                    target_mac: "11:22:33:44:55:66".into(),
                    address_type: AddressTypeConfig::BrEdr,
                    name: Some("Phone".into()),
                    bluetooth: BluetoothOverrides {
                        adapter_index: Some(1),
                        ..Default::default()
                    },
                    proximity: ProximityOverrides {
                        rpl_threshold: Some(13.0),
                        ..Default::default()
                    },
                },
                DeviceEntry {
                    uid: 1001,
                    target_mac: "AA:BB:CC:DD:EE:FF".into(),
                    address_type: AddressTypeConfig::LePublic,
                    name: None,
                    bluetooth: BluetoothOverrides::default(),
                    proximity: ProximityOverrides::default(),
                },
            ],
            ..Default::default()
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.devices.len(), 2);
        assert_eq!(deserialized.devices[0].target_mac, "11:22:33:44:55:66");
        assert_eq!(deserialized.devices[0].name.as_deref(), Some("Phone"));
        assert_eq!(
            deserialized.devices[0].address_type,
            AddressTypeConfig::BrEdr
        );
        assert_eq!(deserialized.devices[0].bluetooth.adapter_index, Some(1));
        assert_eq!(deserialized.devices[0].proximity.rpl_threshold, Some(13.0));
        assert_eq!(deserialized.devices[1].target_mac, "AA:BB:CC:DD:EE:FF");
        assert_eq!(
            deserialized.devices[1].address_type,
            AddressTypeConfig::LePublic
        );
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

    #[test]
    fn config_error_display() {
        let io_err = ConfigError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "not found",
        ));
        assert!(format!("{}", io_err).contains("config I/O error"));

        let validation_err = ConfigError::Validation(validator::ValidationErrors::new());
        assert!(format!("{}", validation_err).contains("config validation error"));
    }

    #[test]
    fn config_error_from_conversions() {
        let io_err: ConfigError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "not found").into();
        assert!(matches!(io_err, ConfigError::Io(_)));

        let toml_err = toml::from_str::<Config>("invalid = [[[").unwrap_err();
        let parse_err: ConfigError = toml_err.into();
        assert!(matches!(parse_err, ConfigError::Parse(_)));

        let validation_errors = validator::ValidationErrors::new();
        let val_err: ConfigError = validation_errors.into();
        assert!(matches!(val_err, ConfigError::Validation(_)));
    }

    #[test]
    fn br_edr_is_default() {
        let br_edr_defaults = BrEdrDefaults::default();
        let proximity_defaults = ProximityConfig::default();

        assert_eq!(
            proximity_defaults.rpl_threshold,
            br_edr_defaults.rpl_threshold
        );
        assert_eq!(proximity_defaults.lock_count, br_edr_defaults.lock_count);
        assert_eq!(
            proximity_defaults.unlock_count,
            br_edr_defaults.unlock_count
        );
        assert_eq!(proximity_defaults.kalman_q, br_edr_defaults.kalman_q);
        assert_eq!(proximity_defaults.kalman_r, br_edr_defaults.kalman_r);
        assert_eq!(
            proximity_defaults.kalman_initial,
            br_edr_defaults.kalman_initial
        );
        assert_eq!(
            proximity_defaults.disconnect_action,
            br_edr_defaults.disconnect_action
        );
    }
}
