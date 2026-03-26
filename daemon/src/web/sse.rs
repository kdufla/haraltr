use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use super::AppState;
use super::auth::AuthUser;

pub async fn sse_handler(
    _auth: AuthUser,
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.rpl_broadcast.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| async {
        result.ok().map(|update| {
            Ok(Event::default()
                .event("rpl")
                .data(serde_json::to_string(&update).unwrap()))
        })
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::web::auth::{login_handler, logout_handler};
    use crate::web::state::{DaemonStatus, ProximityPhase, RplUpdate};
    use arc_swap::ArcSwap;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::middleware;
    use axum::routing::{get, post};
    use futures::StreamExt;
    use std::collections::{HashMap, VecDeque};
    use std::path::PathBuf;
    use std::time::Instant;
    use tokio::sync::broadcast;
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        let (tx, _) = broadcast::channel(32);
        Arc::new(AppState {
            config: Arc::new(ArcSwap::from_pointee(Config::default())),
            config_path: PathBuf::from("/tmp/test-config.toml"),
            sessions: std::sync::Mutex::new(HashMap::new()),
            rpl_broadcast: tx,
            daemon_status: ArcSwap::from_pointee(DaemonStatus {
                rpl: None,
                raw_rpl: None,
                state: ProximityPhase::Disconnected,
                connected: false,
                target_mac: None,
                started_at: Instant::now(),
            }),
            history: std::sync::Mutex::new(VecDeque::new()),
            config_notify: tokio::sync::Notify::new(),
        })
    }

    fn test_router(state: Arc<AppState>) -> Router {
        let public = Router::new().route("/api/login", post(login_handler));
        let protected = Router::new()
            .route("/api/events", get(sse_handler))
            .route("/api/logout", post(logout_handler))
            .route_layer(middleware::from_extractor_with_state::<AuthUser, _>(
                state.clone(),
            ));
        public.merge(protected).with_state(state)
    }

    #[tokio::test]
    async fn sse_without_auth_returns_401() {
        let state = test_state();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sse_receives_broadcast_event() {
        let state = test_state();
        let token = state.create_session();
        let tx = state.rpl_broadcast.clone();
        let app = test_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/events")
                    .header("cookie", format!("session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            resp.headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap()
                .contains("text/event-stream")
        );

        // Send a broadcast event
        tx.send(RplUpdate {
            rpl: 12.3,
            raw_rpl: 14.1,
            state: "near".into(),
            connected: true,
            timestamp: 1711000000.0,
        })
        .unwrap();

        // Read the SSE body as a stream of frames
        let mut body = resp.into_body().into_data_stream();

        // Collect frames until we find our event (with a timeout)
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut collected = String::new();
            while let Some(Ok(chunk)) = body.next().await {
                collected.push_str(&String::from_utf8_lossy(&chunk));
                if collected.contains("event: rpl") && collected.contains("\"rpl\":12.3") {
                    return collected;
                }
            }
            collected
        })
        .await
        .expect("timed out waiting for SSE event");

        assert!(result.contains("event: rpl"));
        assert!(result.contains("\"rpl\":12.3"));
        assert!(result.contains("\"raw_rpl\":14.1"));
        assert!(result.contains("\"connected\":true"));
    }
}
