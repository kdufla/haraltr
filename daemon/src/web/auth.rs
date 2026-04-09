use std::{sync::Arc, time::Duration};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordVerifier},
};
use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    response::IntoResponse,
};
use axum_extra::extract::CookieJar;
use cookie::Cookie;
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::state::AppState;

const SESSION_COOKIE: &str = "session";
const SESSION_DURATION_HOURS: u64 = 24;
pub const AUTH_SESSION_DURATION: Duration = Duration::from_hours(SESSION_DURATION_HOURS);

pub(super) fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub(super) struct AuthUser;

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if state.validate_session(token) {
            Ok(AuthUser)
        } else {
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

#[derive(Deserialize)]
pub(super) struct LoginRequest {
    password: String,
}

pub(super) async fn login_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> impl IntoResponse {
    let config = state.config.read().unwrap();
    let Some(ref hash) = config.web.password_hash else {
        error!("web server is running without password hash in config");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "config corruption"})),
        )
            .into_response();
    };

    if !verify_password(&body.password, hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid password"})),
        )
            .into_response();
    }

    let token = state.create_session();

    let cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .same_site(cookie::SameSite::Strict)
        .path("/")
        .max_age(cookie::time::Duration::hours(SESSION_DURATION_HOURS as i64));

    let jar = CookieJar::new().add(cookie);

    (jar, Json(json!({"ok": true}))).into_response()
}

pub(super) async fn logout_handler(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> impl IntoResponse {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        state.remove_session(cookie.value());
    }

    let removal = Cookie::build(SESSION_COOKIE)
        .path("/")
        .max_age(cookie::time::Duration::ZERO);

    let jar = CookieJar::new().add(removal);

    (jar, Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        path::PathBuf,
        time::{Duration, Instant},
    };

    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
    use axum::{Router, body::Body, http::Request, middleware, routing::post};
    use tower::ServiceExt;

    use super::*;
    use crate::{
        config::Config,
        state::{AppState, DaemonStatus},
    };

    fn test_state(password_hash: Option<String>) -> Arc<AppState> {
        let mut config = Config::default();
        config.web.password_hash = password_hash;
        Arc::new(AppState {
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path: PathBuf::from("/tmp/test-config.toml"),
            web_sessions: std::sync::Mutex::new(HashMap::new()),
            daemon_status: std::sync::Mutex::new(DaemonStatus {
                devices: HashMap::new(),
                started_at: Instant::now(),
            }),
            config_notify: tokio::sync::Notify::new(),
            active_uid: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(1000)),
        })
    }

    fn hash_password(password: &str) -> String {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    #[test]
    fn verify_password_correct() {
        let hash = hash_password("test123");
        assert!(verify_password("test123", &hash));
    }

    #[test]
    fn verify_password_wrong() {
        let hash = hash_password("test123");
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn create_session_returns_64_hex() {
        let state = test_state(None);
        let token = state.create_session();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(state.web_sessions.lock().unwrap().contains_key(&token));
    }

    #[test]
    fn validate_session_valid() {
        let state = test_state(None);
        let token = state.create_session();
        assert!(state.validate_session(&token));
    }

    #[test]
    fn validate_session_unknown() {
        let state = test_state(None);
        assert!(!state.validate_session("nonexistent"));
    }

    #[test]
    fn validate_session_expired() {
        let state = test_state(None);
        let token = "expired_token".to_string();
        let past_expiry = Instant::now() - Duration::from_secs(1);
        state
            .web_sessions
            .lock()
            .unwrap()
            .insert(token.clone(), past_expiry);
        assert!(!state.validate_session(&token));
        assert!(!state.web_sessions.lock().unwrap().contains_key(&token));
    }

    fn test_router(state: Arc<AppState>) -> Router {
        let public = Router::new().route("/api/login", post(login_handler));

        let protected = Router::new()
            .route("/api/logout", post(logout_handler))
            .route_layer(middleware::from_extractor_with_state::<AuthUser, _>(
                state.clone(),
            ));

        public.merge(protected).with_state(state)
    }

    #[tokio::test]
    async fn login_success() {
        let hash = hash_password("secret");
        let state = test_state(Some(hash));
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"password":"secret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(set_cookie.contains("session="));
        assert!(set_cookie.contains("HttpOnly"));
    }

    #[tokio::test]
    async fn login_wrong_password() {
        let hash = hash_password("secret");
        let state = test_state(Some(hash));
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"password":"wrong"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn logout_with_valid_session() {
        let hash = hash_password("secret");
        let state = test_state(Some(hash));
        let token = state.create_session();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logout")
                    .header("cookie", format!("session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(set_cookie.contains("Max-Age=0"));
    }

    #[tokio::test]
    async fn login_no_password_hash() {
        let state = test_state(None);
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"password":"anything"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn logout_invalid_session() {
        let state = test_state(None);
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logout")
                    .header("cookie", "session=bogus_token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn logout_removes_session() {
        let hash = hash_password("secret");
        let state = test_state(Some(hash));
        let token = state.create_session();
        let state_ref = state.clone();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logout")
                    .header("cookie", format!("session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!state_ref.web_sessions.lock().unwrap().contains_key(&token));
    }

    #[tokio::test]
    async fn protected_route_without_cookie() {
        let state = test_state(None);
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
