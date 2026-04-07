use std::{collections::HashMap, sync::Arc};

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use crate::{
    config::{
        BluetoothOverrides, Config, ConfigError, DeviceEntry, ProximityOverrides, validate_mac,
    },
    logind::watcher::NO_ACTIVE_UID,
    state::AppState,
    web::bt_devices::list_bt_devices,
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

pub(super) async fn status_handler(State(state): State<Arc<AppState>>) -> Json<Value> {
    let status = state.daemon_status.lock().unwrap();
    let uptime = status.started_at.elapsed().as_secs();

    let devices: Vec<Value> = status
        .devices
        .iter()
        .map(|(mac, device)| {
            json!({
                "target_mac": mac,
                "rpl": device.rpl,
                "raw_rpl": device.raw_rpl,
                "state": device.phase,
                "connected": device.connected,
            })
        })
        .collect();

    Json(json!({
        "any_near": status.any_near,
        "devices": devices,
        "uptime_secs": uptime,
    }))
}

pub(super) async fn get_config_handler(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.read().unwrap();
    Json(config_response(&config))
}

pub(super) async fn put_config_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let current = state.config.read().unwrap();
    let original_hash = current.web.password_hash.clone();

    let mut base = serde_json::to_value(&*current).unwrap();
    deep_merge(&mut base, &body);

    let mut new_config: Config = match serde_json::from_value(base) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("invalid config format: {e}")})),
            )
                .into_response();
        }
    };

    if let Err(e) = new_config.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("invalid config: {e}")})),
        )
            .into_response();
    }

    new_config.web.password_hash = original_hash;

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("failed to save config: {e}")})),
        )
            .into_response();
    }

    let old_devices: Vec<String> = current
        .devices
        .iter()
        .map(|device| device.target_mac.clone())
        .collect();
    drop(current);

    let new_devices: Vec<String> = new_config
        .devices
        .iter()
        .map(|device| device.target_mac.clone())
        .collect();

    let response = config_response(&new_config);
    *state.config.write().unwrap() = new_config;

    if old_devices != new_devices {
        state.config_notify.notify_one();
    }

    Json(response).into_response()
}

pub(super) async fn get_devices_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let bt_devices = match list_bt_devices().await {
        Ok(btd) => btd,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("{e}"), "devices": []})),
            )
                .into_response();
        }
    };

    let config = state.config.read().unwrap();
    let monitored_macs: Vec<_> = config
        .devices
        .iter()
        .map(|d| d.target_mac.as_str())
        .collect();

    let device_entries: HashMap<&str, &DeviceEntry> = config
        .devices
        .iter()
        .map(|d| (d.target_mac.as_str(), d))
        .collect();

    let mut devices: Vec<_> = bt_devices
        .into_iter()
        .map(|mut dev| {
            let mac = dev["mac"].as_str().unwrap_or("").to_owned();
            let monitored = monitored_macs.contains(&mac.as_str());
            dev["monitored"] = json!(monitored);
            if let Some(entry) = device_entries.get(mac.as_str()) {
                dev["bluetooth"] = serde_json::to_value(&entry.bluetooth).unwrap();
                dev["proximity"] = serde_json::to_value(&entry.proximity).unwrap();
            }
            dev
        })
        .collect();

    devices.sort_by(|a, b| {
        let a_name = a["name"].as_str().unwrap_or("\u{FFFF}");
        let b_name = b["name"].as_str().unwrap_or("\u{FFFF}");
        a_name.to_lowercase().cmp(&b_name.to_lowercase())
    });

    Json(json!({ "devices": devices })).into_response()
}

#[derive(Deserialize, Validate)]
pub(super) struct DeviceRequest {
    #[validate(custom(function = "validate_mac"))]
    target_mac: String,
    #[serde(default)]
    name: Option<String>,
    #[validate(nested)]
    #[serde(default)]
    bluetooth: BluetoothOverrides,
    #[validate(nested)]
    #[serde(default)]
    proximity: ProximityOverrides,
}

pub(super) async fn add_device_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DeviceRequest>,
) -> impl IntoResponse {
    if let Err(e) = body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("invalid device request: {e}")})),
        )
            .into_response();
    }

    let mut new_config = (*state.config.read().unwrap()).clone();

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

    new_config.devices.push(DeviceEntry {
        uid: NO_ACTIVE_UID, // TODO placeholder
        target_mac: body.target_mac.clone(),
        name: body.name,
        bluetooth: body.bluetooth,
        proximity: body.proximity,
    });

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        let (status, msg) = match e {
            ConfigError::Validation(ve) => {
                (StatusCode::BAD_REQUEST, format!("invalid config: {ve}"))
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to save config: {e}"),
            ),
        };
        return (status, Json(json!({"error": msg}))).into_response();
    }

    *state.config.write().unwrap() = new_config;
    state.config_notify.notify_one();

    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
pub(super) struct RemoveDeviceRequest {
    target_mac: String,
}

pub(super) async fn remove_device_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RemoveDeviceRequest>,
) -> impl IntoResponse {
    let mut new_config = (*state.config.read().unwrap()).clone();

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

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        let (status, msg) = match e {
            ConfigError::Validation(ve) => {
                (StatusCode::BAD_REQUEST, format!("invalid config: {ve}"))
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to save config: {e}"),
            ),
        };
        return (status, Json(json!({"error": msg}))).into_response();
    }

    *state.config.write().unwrap() = new_config;
    state.config_notify.notify_one();

    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize, Validate)]
pub(super) struct UpdateDeviceRequest {
    #[validate(custom(function = "validate_mac"))]
    target_mac: String,
    #[validate(nested)]
    #[serde(default)]
    bluetooth: BluetoothOverrides,
    #[validate(nested)]
    #[serde(default)]
    proximity: ProximityOverrides,
}

pub(super) async fn update_device_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpdateDeviceRequest>,
) -> impl IntoResponse {
    if let Err(e) = body.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("invalid request: {e}")})),
        )
            .into_response();
    }

    let mut new_config = (*state.config.read().unwrap()).clone();

    let device = new_config
        .devices
        .iter_mut()
        .find(|d| d.target_mac == body.target_mac);

    let Some(device) = device else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "device not found"})),
        )
            .into_response();
    };

    device.bluetooth = body.bluetooth;
    device.proximity = body.proximity;

    if let Err(e) = new_config.save_to_file(&state.config_path) {
        let (status, msg) = match e {
            ConfigError::Validation(ve) => {
                (StatusCode::BAD_REQUEST, format!("invalid config: {ve}"))
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to save config: {e}"),
            ),
        };
        return (status, Json(json!({"error": msg}))).into_response();
    }

    *state.config.write().unwrap() = new_config;
    state.config_notify.notify_one();

    Json(json!({"ok": true})).into_response()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf, time::Instant};

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
        state::{AppState, DaemonStatus, ProximityPhase},
        web::auth::{AuthUser, login_handler, logout_handler},
    };

    fn test_state_with_config_path(config: Config, path: PathBuf) -> Arc<AppState> {
        use crate::state::DeviceStatus;
        let mut devices = HashMap::new();
        devices.insert(
            "24:29:34:8E:0A:58".into(),
            DeviceStatus {
                rpl: Some(12.3),
                raw_rpl: Some(14.1),
                phase: ProximityPhase::Near,
                connected: true,
            },
        );
        Arc::new(AppState {
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path: path,
            web_sessions: std::sync::Mutex::new(HashMap::new()),
            daemon_status: std::sync::Mutex::new(DaemonStatus {
                devices,
                any_near: true,
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
                    .patch(update_device_handler)
                    .delete(remove_device_handler),
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

        // TODO update assertions when web UI is redesigned for multi-device
        let json = body_json(resp).await;
        assert!(json["any_near"].as_bool().unwrap());
        assert!(json["devices"].is_array());
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

        // verify config was updated
        assert_eq!(state.config.read().unwrap().proximity.rpl_threshold, 20.0);

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
            state.config.read().unwrap().web.password_hash.as_deref(),
            Some("$argon2id$secret")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn get_devices_returns_devices_array() {
        let state = test_state();
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(authed_get("/api/devices", &token))
            .await
            .unwrap();

        let json = body_json(resp).await;
        assert!(json["devices"].is_array());
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

        let cfg = state.config.read().unwrap();
        assert_eq!(cfg.devices.len(), 1);
        assert_eq!(cfg.devices[0].target_mac, "AA:BB:CC:DD:EE:FF");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn add_device_duplicate_mac_returns_409() {
        let dir = std::env::temp_dir().join("haraltr_test_add_device_dup");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            uid: NO_ACTIVE_UID,
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
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            uid: NO_ACTIVE_UID,
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

        let cfg = state.config.read().unwrap();
        assert!(cfg.devices.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn update_device_overrides() {
        let dir = std::env::temp_dir().join("haraltr_test_update_device");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.devices.push(DeviceEntry {
            uid: NO_ACTIVE_UID,
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
                    .method("PATCH")
                    .uri("/api/devices")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(
                        r#"{"target_mac":"AA:BB:CC:DD:EE:FF","proximity":{"rpl_threshold":20.0}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let cfg = state.config.read().unwrap();
        assert_eq!(cfg.devices[0].proximity.rpl_threshold, Some(20.0));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn update_device_not_found_returns_404() {
        let state = test_state();
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/devices")
                    .header("content-type", "application/json")
                    .header("cookie", format!("session={token}"))
                    .body(Body::from(
                        r#"{"target_mac":"AA:BB:CC:DD:EE:FF","proximity":{"rpl_threshold":20.0}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
