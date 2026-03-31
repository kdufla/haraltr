use std::{collections::HashSet, sync::Arc};

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::{Value, json};

use super::AppState;
use crate::{
    config::{BluetoothOverrides, Config, DeviceEntry, ProximityOverrides},
    web::bt_devices::list_devices,
};

fn config_response(config: &Config) -> Value {
    let mut val = serde_json::to_value(config).unwrap();
    if let Some(web_cfg) = val.get_mut("web").and_then(|wc| wc.as_object_mut()) {
        web_cfg.remove("password_hash");
    }
    val
}

fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                deep_merge(base_map.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (base, patch) => {
            *base = patch.clone();
        }
    }
}

fn validate_config(config: &Config) -> Result<(), String> {
    if config.web.port == 0 {
        return Err("web.port must be > 0".into());
    }
    if config.proximity.rpl_threshold <= 0.0 {
        return Err("proximity.rpl_threshold must be positive".into());
    }
    if config.proximity.kalman_q <= 0.0 {
        return Err("proximity.kalman_q must be positive".into());
    }
    if config.proximity.kalman_r <= 0.0 {
        return Err("proximity.kalman_r must be positive".into());
    }
    if config.bluetooth.poll_interval_ms == 0 {
        return Err("bluetooth.poll_interval_ms must be > 0".into());
    }
    if config.bluetooth.disconnect_poll_interval_ms == 0 {
        return Err("bluetooth.disconnect_poll_interval_ms must be > 0".into());
    }
    if config.proximity.lock_count == 0 {
        return Err("proximity.lock_count must be > 0".into());
    }
    if config.proximity.unlock_count == 0 {
        return Err("proximity.unlock_count must be > 0".into());
    }

    let mut seen_macs = HashSet::new();
    for (i, dev) in config.devices.iter().enumerate() {
        if !is_valid_mac(&dev.target_mac) {
            return Err(format!(
                "devices[{i}]: invalid MAC address '{}'",
                dev.target_mac
            ));
        }
        if !seen_macs.insert(&dev.target_mac) {
            return Err(format!(
                "devices[{i}]: duplicate MAC address '{}'",
                dev.target_mac
            ));
        }
        if let Some(v) = dev.proximity.rpl_threshold
            && v <= 0.0
        {
            return Err(format!("devices[{i}]: rpl_threshold must be positive"));
        }
        if let Some(v) = dev.proximity.kalman_q
            && v <= 0.0
        {
            return Err(format!("devices[{i}]: kalman_q must be positive"));
        }
        if let Some(v) = dev.proximity.kalman_r
            && v <= 0.0
        {
            return Err(format!("devices[{i}]: kalman_r must be positive"));
        }
        if let Some(v) = dev.bluetooth.poll_interval_ms
            && v == 0
        {
            return Err(format!("devices[{i}]: poll_interval_ms must be > 0"));
        }
        if let Some(v) = dev.bluetooth.disconnect_poll_interval_ms
            && v == 0
        {
            return Err(format!(
                "devices[{i}]: disconnect_poll_interval_ms must be > 0"
            ));
        }
        if let Some(v) = dev.proximity.lock_count
            && v == 0
        {
            return Err(format!("devices[{i}]: lock_count must be > 0"));
        }
        if let Some(v) = dev.proximity.unlock_count
            && v == 0
        {
            return Err(format!("devices[{i}]: unlock_count must be > 0"));
        }
    }

    if let Some(ref active_mac) = config.active_device
        && !config.devices.iter().any(|d| d.target_mac == *active_mac)
    {
        return Err(format!(
            "active_device '{}' not found in devices list",
            active_mac
        ));
    }

    Ok(())
}

fn is_valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

pub async fn status_handler(State(state): State<Arc<AppState>>) -> Json<Value> {
    let status = state.daemon_status.load();
    let uptime = status.started_at.elapsed().as_secs();
    Json(json!({
        "rpl": status.rpl,
        "raw_rpl": status.raw_rpl,
        "state": status.state,
        "connected": status.connected,
        "target_mac": status.target_mac,
        "uptime_secs": uptime,
    }))
}

pub async fn get_config_handler(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.load();
    Json(config_response(&config))
}

pub async fn put_config_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let current = state.config.load();
    let original_hash = current.web.password_hash.clone();

    let mut base = serde_json::to_value(current.as_ref()).unwrap();
    deep_merge(&mut base, &body);

    let mut new_config: Config = match serde_json::from_value(base) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid config: {e}")})),
            )
                .into_response();
        }
    };

    if let Err(msg) = validate_config(&new_config) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response();
    }

    new_config.web.password_hash = original_hash;

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("failed to save config: {e}")})),
        )
            .into_response();
    }

    let old_mac = current.resolved_target_mac().map(str::to_string);
    let mac_changed = old_mac != new_config.resolved_target_mac().map(str::to_string);

    let response = config_response(&new_config);
    state.config.store(Arc::new(new_config));

    if mac_changed {
        state.config_notify.notify_one();
    }

    Json(response).into_response()
}

pub async fn get_devices_handler(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.load();
    Json(json!({
        "active_device": config.active_device,
        "devices": config.devices.iter().map(|d| json!({
            "target_mac": d.target_mac,
            "name": d.name,
            "bluetooth": d.bluetooth,
            "proximity": d.proximity,
        })).collect::<Vec<_>>(),
    }))
}

pub async fn bt_devices_handler() -> impl IntoResponse {
    match list_devices().await {
        Ok(devices) => Json(json!({ "devices": devices })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("{e}"), "devices": []})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct DeviceRequest {
    target_mac: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    bluetooth: BluetoothOverrides,
    #[serde(default)]
    proximity: ProximityOverrides,
}

pub async fn add_device_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DeviceRequest>,
) -> impl IntoResponse {
    if !is_valid_mac(&body.target_mac) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid MAC address format, expected AA:BB:CC:DD:EE:FF"})),
        )
            .into_response();
    }

    let current = state.config.load();
    let mut new_config = current.as_ref().clone();

    if new_config
        .devices
        .iter()
        .any(|d| d.target_mac == body.target_mac)
    {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "device with this MAC already exists"})),
        )
            .into_response();
    }

    let is_first = new_config.devices.is_empty();
    new_config.devices.push(DeviceEntry {
        target_mac: body.target_mac.clone(),
        name: body.name,
        bluetooth: body.bluetooth,
        proximity: body.proximity,
    });

    if is_first {
        new_config.active_device = Some(body.target_mac);
    }

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("failed to save config: {e}")})),
        )
            .into_response();
    }

    state.config.store(Arc::new(new_config));
    state.config_notify.notify_one();

    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub struct RemoveDeviceRequest {
    target_mac: String,
}

pub async fn remove_device_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RemoveDeviceRequest>,
) -> impl IntoResponse {
    let current = state.config.load();
    let mut new_config = current.as_ref().clone();

    let before_len = new_config.devices.len();
    new_config
        .devices
        .retain(|d| d.target_mac != body.target_mac);

    if new_config.devices.len() == before_len {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "device not found"})),
        )
            .into_response();
    }

    if new_config.active_device.as_deref() == Some(&body.target_mac) {
        new_config.active_device = None;
    }

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("failed to save config: {e}")})),
        )
            .into_response();
    }

    state.config.store(Arc::new(new_config));
    state.config_notify.notify_one();

    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub struct SetActiveDeviceRequest {
    target_mac: String,
}

pub async fn set_active_device_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetActiveDeviceRequest>,
) -> impl IntoResponse {
    let current = state.config.load();
    let mut new_config = current.as_ref().clone();

    if !new_config
        .devices
        .iter()
        .any(|d| d.target_mac == body.target_mac)
    {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "device not found in devices list"})),
        )
            .into_response();
    }

    let old_mac = current.resolved_target_mac().map(str::to_string);
    new_config.active_device = Some(body.target_mac);
    let new_mac = new_config.resolved_target_mac().map(str::to_string);
    let mac_changed = old_mac != new_mac;

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("failed to save config: {e}")})),
        )
            .into_response();
    }

    state.config.store(Arc::new(new_config));
    if mac_changed {
        state.config_notify.notify_one();
    }

    Json(json!({"ok": true})).into_response()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf, time::Instant};

    use arc_swap::ArcSwap;
    use axum::{
        Router,
        body::Body,
        http::Request,
        middleware,
        routing::{get, post},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        config::Config,
        web::{
            auth::{AuthUser, login_handler, logout_handler},
            state::{AppState, DaemonStatus, ProximityPhase},
        },
    };

    fn test_state_with_config_path(config: Config, path: PathBuf) -> Arc<AppState> {
        Arc::new(AppState {
            config: Arc::new(ArcSwap::from_pointee(config)),
            config_path: path,
            sessions: std::sync::Mutex::new(HashMap::new()),
            daemon_status: ArcSwap::from_pointee(DaemonStatus {
                rpl: Some(12.3),
                raw_rpl: Some(14.1),
                state: ProximityPhase::Near,
                connected: true,
                target_mac: Some("24:29:34:8E:0A:58".into()),
                started_at: Instant::now(),
            }),
            config_notify: tokio::sync::Notify::new(),
        })
    }

    fn test_state() -> Arc<AppState> {
        test_state_with_config_path(Config::default(), PathBuf::from("/tmp/test-config.toml"))
    }

    fn test_router(state: Arc<AppState>) -> Router {
        let public = Router::new().route("/api/login", post(login_handler));

        let protected = Router::new()
            .route("/api/status", get(status_handler))
            .route(
                "/api/config",
                get(get_config_handler).put(put_config_handler),
            )
            .route(
                "/api/devices",
                get(get_devices_handler)
                    .post(add_device_handler)
                    .delete(remove_device_handler),
            )
            .route(
                "/api/devices/active",
                axum::routing::put(set_active_device_handler),
            )
            .route("/api/logout", post(logout_handler))
            .route_layer(middleware::from_extractor_with_state::<AuthUser, _>(
                state.clone(),
            ));

        public.merge(protected).with_state(state)
    }

    fn authed_get(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("cookie", format!("session={token}"))
            .body(Body::empty())
            .unwrap()
    }

    async fn body_json(resp: axum::response::Response) -> Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn status_returns_expected_fields() {
        let state = test_state();
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(authed_get("/api/status", &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["rpl"], 12.3);
        assert_eq!(json["raw_rpl"], 14.1);
        assert_eq!(json["state"], "near");
        assert!(json["connected"].as_bool().unwrap());
        assert_eq!(json["target_mac"], "24:29:34:8E:0A:58");
        assert!(json["uptime_secs"].is_number());
    }

    #[tokio::test]
    async fn status_without_auth_returns_401() {
        let state = test_state();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_config_omits_password_hash() {
        let mut config = Config::default();
        config.web.password_hash = Some("$argon2id$secret".into());
        let state = test_state_with_config_path(config, PathBuf::from("/tmp/test-config.toml"));
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(authed_get("/api/config", &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert!(json["web"].is_object());
        assert!(json["web"].get("password_hash").is_none());
        assert!(json["bluetooth"].is_object());
        assert!(json["proximity"].is_object());
    }

    #[tokio::test]
    async fn put_config_updates_and_persists() {
        let dir = std::env::temp_dir().join("haraltr_test_put_config");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let config = Config::default();
        config.save_to_file(&path).unwrap();

        let state = test_state_with_config_path(config, path.clone());
        let token = state.create_session();
        let app = test_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/config")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"proximity":{"rpl_threshold":20.0}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["proximity"]["rpl_threshold"], 20.0);

        // verify ArcSwap was updated
        assert_eq!(state.config.load().proximity.rpl_threshold, 20.0);

        // verify file was written
        let contents = std::fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&contents).unwrap();
        assert_eq!(loaded.proximity.rpl_threshold, 20.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn put_config_invalid_port_returns_400() {
        let state = test_state();
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/config")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"web":{"port":0}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn put_config_preserves_password_hash() {
        let dir = std::env::temp_dir().join("haraltr_test_put_preserve_hash");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.web.password_hash = Some("$argon2id$secret".into());
        config.save_to_file(&path).unwrap();

        let state = test_state_with_config_path(config, path.clone());
        let token = state.create_session();
        let app = test_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/config")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"proximity":{"rpl_threshold":25.0}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // password_hash must be preserved in stored config
        assert_eq!(
            state.config.load().web.password_hash.as_deref(),
            Some("$argon2id$secret")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn get_devices_returns_list() {
        let mut config = Config {
            active_device: Some("AA:BB:CC:DD:EE:FF".into()),
            ..Default::default()
        };
        config.devices.push(DeviceEntry {
            target_mac: "AA:BB:CC:DD:EE:FF".into(),
            name: Some("Phone".into()),
            bluetooth: BluetoothOverrides::default(),
            proximity: ProximityOverrides::default(),
        });
        let state = test_state_with_config_path(config, PathBuf::from("/tmp/test-config.toml"));
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(authed_get("/api/devices", &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["active_device"], "AA:BB:CC:DD:EE:FF");
        assert_eq!(json["devices"].as_array().unwrap().len(), 1);
        assert_eq!(json["devices"][0]["target_mac"], "AA:BB:CC:DD:EE:FF");
        assert_eq!(json["devices"][0]["name"], "Phone");
    }

    #[tokio::test]
    async fn add_device_valid_mac() {
        let dir = std::env::temp_dir().join("haraltr_test_add_device");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let config = Config::default();
        config.save_to_file(&path).unwrap();

        let state = test_state_with_config_path(config, path.clone());
        let token = state.create_session();
        let app = test_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/devices")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"target_mac":"AA:BB:CC:DD:EE:FF"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let cfg = state.config.load();
        assert_eq!(cfg.devices.len(), 1);
        assert_eq!(cfg.devices[0].target_mac, "AA:BB:CC:DD:EE:FF");
        // First device becomes active
        assert_eq!(cfg.active_device.as_deref(), Some("AA:BB:CC:DD:EE:FF"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_device_duplicate_mac_returns_409() {
        let dir = std::env::temp_dir().join("haraltr_test_add_device_dup");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            target_mac: "AA:BB:CC:DD:EE:FF".into(),
            name: None,
            bluetooth: BluetoothOverrides::default(),
            proximity: ProximityOverrides::default(),
        });
        config.save_to_file(&path).unwrap();

        let state = test_state_with_config_path(config, path.clone());
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/devices")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"target_mac":"AA:BB:CC:DD:EE:FF"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_device_invalid_mac_returns_400() {
        let state = test_state();
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/devices")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"target_mac":"not-a-mac"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn remove_device_removes_from_list() {
        let dir = std::env::temp_dir().join("haraltr_test_remove_device");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut config = Config {
            active_device: Some("AA:BB:CC:DD:EE:FF".into()),
            ..Default::default()
        };
        config.devices.push(DeviceEntry {
            target_mac: "AA:BB:CC:DD:EE:FF".into(),
            name: None,
            bluetooth: BluetoothOverrides::default(),
            proximity: ProximityOverrides::default(),
        });
        config.save_to_file(&path).unwrap();

        let state = test_state_with_config_path(config, path.clone());
        let token = state.create_session();
        let app = test_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/devices")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"target_mac":"AA:BB:CC:DD:EE:FF"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let cfg = state.config.load();
        assert!(cfg.devices.is_empty());
        assert!(cfg.active_device.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn set_active_device() {
        let dir = std::env::temp_dir().join("haraltr_test_set_active");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            target_mac: "AA:BB:CC:DD:EE:FF".into(),
            name: Some("Phone".into()),
            bluetooth: BluetoothOverrides::default(),
            proximity: ProximityOverrides::default(),
        });
        config.devices.push(DeviceEntry {
            target_mac: "11:22:33:44:55:66".into(),
            name: Some("Watch".into()),
            bluetooth: BluetoothOverrides::default(),
            proximity: ProximityOverrides::default(),
        });
        config.save_to_file(&path).unwrap();

        let state = test_state_with_config_path(config, path.clone());
        let token = state.create_session();
        let app = test_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/devices/active")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(r#"{"target_mac":"11:22:33:44:55:66"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        assert_eq!(
            state.config.load().active_device.as_deref(),
            Some("11:22:33:44:55:66")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_mac_addresses() {
        assert!(is_valid_mac("AA:BB:CC:DD:EE:FF"));
        assert!(is_valid_mac("00:11:22:33:44:55"));
        assert!(is_valid_mac("aa:bb:cc:dd:ee:ff"));
    }

    #[test]
    fn invalid_mac_addresses() {
        assert!(!is_valid_mac("not-a-mac"));
        assert!(!is_valid_mac("AA:BB:CC:DD:EE"));
        assert!(!is_valid_mac("AA:BB:CC:DD:EE:GG"));
        assert!(!is_valid_mac("AABB.CCDD.EEFF"));
        assert!(!is_valid_mac(""));
    }
}
