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

pub async fn list_devices() -> Result<Vec<Value>, Box<dyn std::error::Error + Send + Sync>> {
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
