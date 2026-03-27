use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::{AddressTypeConfig, Config};

use super::AppState;

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

    let old_mac = current.bluetooth.target_mac.clone();
    let mac_changed = old_mac != new_config.bluetooth.target_mac;

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
        "target_mac": config.bluetooth.target_mac,
        "address_type": config.bluetooth.address_type,
    }))
}

#[derive(Deserialize)]
pub struct DeviceRequest {
    target_mac: String,
    #[serde(default)]
    address_type: Option<AddressTypeConfig>,
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
    new_config.bluetooth.target_mac = Some(body.target_mac);
    if let Some(addr_type) = body.address_type {
        new_config.bluetooth.address_type = addr_type;
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

pub async fn remove_device_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let current = state.config.load();
    let mut new_config = current.as_ref().clone();
    new_config.bluetooth.target_mac = None;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::web::auth::{AuthUser, login_handler, logout_handler};
    use crate::web::state::{AppState, DaemonStatus, ProximityPhase};
    use arc_swap::ArcSwap;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::middleware;
    use axum::routing::{get, post};
    use http_body_util::BodyExt;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Instant;
    use tower::ServiceExt;

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
    async fn get_devices_returns_target() {
        let mut config = Config::default();
        config.bluetooth.target_mac = Some("AA:BB:CC:DD:EE:FF".into());
        let state = test_state_with_config_path(config, PathBuf::from("/tmp/test-config.toml"));
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(authed_get("/api/devices", &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let json = body_json(resp).await;
        assert_eq!(json["target_mac"], "AA:BB:CC:DD:EE:FF");
        assert_eq!(json["address_type"], "br_edr");
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

        assert_eq!(
            state.config.load().bluetooth.target_mac.as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );

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
    async fn remove_device_clears_target() {
        let dir = std::env::temp_dir().join("haraltr_test_remove_device");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.bluetooth.target_mac = Some("AA:BB:CC:DD:EE:FF".into());
        config.save_to_file(&path).unwrap();

        let state = test_state_with_config_path(config, path.clone());
        let token = state.create_session();
        let app = test_router(state.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/devices")
                    .header("cookie", format!("session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        assert!(state.config.load().bluetooth.target_mac.is_none());

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
