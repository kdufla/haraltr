pub mod api;
pub mod auth;
pub mod sse;
pub mod state;

pub use state::AppState;
pub use state::{DaemonStatus, ProximityPhase, RplReading, RplUpdate};

use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use tracing::info;

use api::{
    add_device_handler, get_config_handler, get_devices_handler, history_handler,
    put_config_handler, remove_device_handler, status_handler,
};
use auth::{AuthUser, login_handler, logout_handler};
use sse::sse_handler;

pub async fn serve(state: Arc<AppState>) {
    let public = Router::new().route("/api/login", post(login_handler));

    let protected = Router::new()
        .route("/api/status", get(status_handler))
        .route(
            "/api/config",
            get(get_config_handler).put(put_config_handler),
        )
        .route("/api/history", get(history_handler))
        .route(
            "/api/devices",
            get(get_devices_handler)
                .post(add_device_handler)
                .delete(remove_device_handler),
        )
        .route("/api/events", get(sse_handler))
        .route("/api/logout", post(logout_handler))
        .route_layer(middleware::from_extractor_with_state::<AuthUser, _>(
            state.clone(),
        ));

    let router = public.merge(protected).with_state(state.clone());

    let port = state.config.load().web.port;
    let addr = format!("127.0.0.1:{port}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind web server");

    info!(addr, "web server listening");

    axum::serve(listener, router)
        .await
        .expect("web server error");
}
