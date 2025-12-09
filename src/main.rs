mod config;
mod handlers;
mod services;
mod state;
mod traits;

use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::ServerConfig;
use crate::handlers::{ota, websocket};
use crate::state::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "xiaozhi_server=trace,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = match ServerConfig::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    // AppState::new is now async
    let app_state = AppState::new(config).await;

    // Build our application with routes
    // Configure TraceLayer to include headers and body (if printable)
    let trace_layer = TraceLayer::new_for_http()
        .on_request(|request: &axum::extract::Request, _span: &tracing::Span| {
            tracing::info!(
                "Started request: {} {} {:?}",
                request.method(),
                request.uri(),
                request.headers()
            );
        })
        .on_response(
            |response: &axum::response::Response,
             latency: std::time::Duration,
             _span: &tracing::Span| {
                tracing::info!(
                    "Finished request: {:?} in {:?}ms",
                    response.status(),
                    latency.as_millis()
                );
            },
        );

    let app = Router::new()
        .route("/xiaozhi/v1/", get(websocket::handle_websocket))
        .route("/xiaozhi/ota/", post(ota::handle_ota))
        .route("/xiaozhi/ota/activate", post(ota::handle_ota_activate))
        .layer(trace_layer)
        .with_state(app_state.clone());

    // Run it
    let port = app_state.config.server.port;
    let host = &app_state.config.server.host;
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .expect("Invalid host/port");

    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind to address");
    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        tracing::error!("Server error: {}", e);
        std::process::exit(1);
    }
}
