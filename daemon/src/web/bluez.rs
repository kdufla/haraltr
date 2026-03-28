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
            let mac = prop_str(props, "Address").unwrap_or_default();
            let name = prop_str(props, "Alias").unwrap_or_else(|| mac.clone());
            let connected = prop_bool(props, "Connected");
            let paired = prop_bool(props, "Paired");

            devices.push(json!({
                "mac": mac,
                "name": name,
                "connected": connected,
                "paired": paired,
            }));
        }
    }
    Ok(devices)
}
