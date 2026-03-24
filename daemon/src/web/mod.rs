pub mod state;

use axum::Router;
pub use state::{AppState, DaemonStatus, ProximityPhase, RplReading, RplUpdate};
use std::sync::Arc;
use tracing::info;

pub async fn serve(state: Arc<AppState>) {
    let router = Router::new();

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
