pub mod auth;
pub mod state;

pub use state::{AppState, DaemonStatus, ProximityPhase, RplReading, RplUpdate};

use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::post;
use tracing::info;

use auth::{AuthUser, login_handler, logout_handler};

pub async fn serve(state: Arc<AppState>) {
    let public = Router::new().route("/api/login", post(login_handler));

    let protected = Router::new()
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
