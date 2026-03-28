pub mod api;
pub mod auth;
pub mod bluez;
pub mod state;

pub use state::AppState;
pub use state::{DaemonStatus, ProximityPhase};

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rust_embed::RustEmbed;
use tracing::info;

use api::{
    add_device_handler, bt_devices_handler, get_config_handler, get_devices_handler,
    put_config_handler, remove_device_handler, status_handler,
};
use auth::{AuthUser, login_handler, logout_handler};

#[derive(RustEmbed)]
#[folder = "static/"]
struct Assets;

async fn static_handler(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');

    let path = match path {
        "" => "index.html",
        "login" => "login.html",
        other => other,
    };

    serve_asset(path).unwrap_or_else(|| {
        serve_asset("index.html").unwrap_or(StatusCode::NOT_FOUND.into_response())
    })
}

fn serve_asset(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(([(header::CONTENT_TYPE, mime.as_ref())], file.data).into_response())
}

pub async fn serve(state: Arc<AppState>) {
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
        .route("/api/bt-devices", get(bt_devices_handler))
        .route("/api/logout", post(logout_handler))
        .route_layer(middleware::from_extractor_with_state::<AuthUser, _>(
            state.clone(),
        ));

    let router = public
        .merge(protected)
        .fallback(get(static_handler))
        .with_state(state.clone());

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
