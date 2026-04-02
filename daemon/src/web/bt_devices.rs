use std::collections::HashMap;

use serde_json::{Value, json};
use zbus::zvariant::OwnedValue;

fn prop_str(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    String::try_from(props.get(key)?.clone()).ok()
}

fn prop_bool(props: &HashMap<String, OwnedValue>, key: &str) -> bool {
    props
        .get(key)
        .and_then(|v| bool::try_from(v.clone()).ok())
        .unwrap_or(false)
}

fn prop_u16(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u16> {
    u16::try_from(props.get(key)?.clone()).ok()
}

fn prop_u32(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u32> {
    u32::try_from(props.get(key)?.clone()).ok()
}

pub(super) async fn list_bt_devices() -> Result<Vec<Value>, Box<dyn std::error::Error + Send + Sync>>
{
    let conn = zbus::Connection::system().await?;
    let proxy = zbus::fdo::ObjectManagerProxy::builder(&conn)
        .destination("org.bluez")?
        .path("/")?
        .build()
        .await?;
    let objects = proxy.get_managed_objects().await?;

    let mut devices = vec![];
    for (path, ifaces) in &objects {
        if !path.as_str().contains("/dev_") {
            continue;
        }
        if let Some(props) = ifaces.get("org.bluez.Device1") {
            let mac = match prop_str(props, "Address") {
                Some(m) => m,
                None => continue,
            };

            let has_class = prop_u32(props, "Class").is_some();
            let has_appearance = prop_u16(props, "Appearance").is_some();
            let address_type = match (has_class, has_appearance) {
                (_, false) if has_class => "br_edr",
                (false, true) => "le_public",
                (true, true) => "br_edr",
                _ => continue,
            };

            let name = prop_str(props, "Alias").unwrap_or_else(|| mac.clone());
            let connected = prop_bool(props, "Connected");
            let paired = prop_bool(props, "Paired");

            devices.push(json!({
                "mac": mac,
                "name": name,
                "connected": connected,
                "paired": paired,
                "address_type": address_type,
            }));
        }
    }
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use zbus::zvariant::Str;

    use super::*;

    #[test]
    fn prop_str_extracts_string_and_handles_missing() {
        let mut props = HashMap::new();
        props.insert(
            "Address".to_string(),
            OwnedValue::from(Str::from("AA:BB:CC:DD:EE:FF")),
        );

        assert_eq!(
            prop_str(&props, "Address"),
            Some("AA:BB:CC:DD:EE:FF".to_string())
        );
        assert_eq!(prop_str(&props, "11:22:33:44:55:66"), None);
    }

    #[test]
    fn prop_bool_extracts_bool_and_defaults_to_false() {
        let mut props = HashMap::new();
        props.insert("Connected".to_string(), OwnedValue::from(true));
        props.insert("Paired".to_string(), OwnedValue::from(false));

        assert!(prop_bool(&props, "Connected"));
        assert!(!prop_bool(&props, "Paired"));
        assert!(!prop_bool(&props, "Nonexistent"));
    }

    #[test]
    fn prop_u16_extracts_u16_and_handles_missing() {
        let mut props = HashMap::new();
        props.insert("Appearance".to_string(), OwnedValue::from(123u16));

        assert_eq!(prop_u16(&props, "Appearance"), Some(123));
        assert_eq!(prop_u16(&props, "Nonexistent"), None);
    }

    #[test]
    fn prop_u32_extracts_u32_and_handles_missing() {
        let mut props = HashMap::new();
        props.insert("Class".to_string(), OwnedValue::from(456u32));

        assert_eq!(prop_u32(&props, "Class"), Some(456));
        assert_eq!(prop_u32(&props, "Nonexistent"), None);
    }
}
