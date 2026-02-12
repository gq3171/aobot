//! aobot-gateway: WebSocket Gateway + JSON-RPC server.
//!
//! Provides:
//! - WebSocket server with JSON-RPC 2.0 protocol
//! - Multi-session agent management
//! - Channel plugin framework for external platform integrations
//! - RPC methods: health, chat.send/stream/history,
//!   sessions.list/delete, agents.list/add/delete,
//!   channels.list/status, config.get/set
//! - Bearer token authentication
//! - HTTP health check endpoint
//! - Configuration hot-reload

pub mod channel;
pub mod config_watcher;
pub mod jsonrpc;
pub mod session_manager;
pub mod handlers;
pub mod ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use tracing::info;

use aobot_config::AoBotConfig;
use aobot_storage::AoBotStorage;
use channel::ChannelManager;
use session_manager::GatewaySessionManager;

/// Shared gateway state.
pub struct GatewayState {
    pub manager: Arc<GatewaySessionManager>,
    pub channel_mgr: Arc<ChannelManager>,
    pub auth_token: Option<String>,
}

/// Start the Gateway server.
///
/// This is the main entry point for the gateway. It creates the axum router,
/// binds to the configured address, and serves requests.
pub async fn start_gateway(
    config: AoBotConfig,
    working_dir: PathBuf,
    port_override: Option<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    let port = port_override.unwrap_or(config.gateway.port);
    let host = config.gateway.host.clone();
    let auth_token = config.gateway.auth_token.clone();

    // Initialize persistent storage
    let storage = match aobot_config::ensure_config_dir() {
        Ok(dir) => {
            let db_path = dir.join("aobot.db");
            match AoBotStorage::open(&db_path) {
                Ok(s) => {
                    info!("Storage initialized: {}", db_path.display());
                    Some(Arc::new(s))
                }
                Err(e) => {
                    tracing::warn!("Failed to open storage, running without persistence: {e}");
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to resolve config dir, running without persistence: {e}");
            None
        }
    };

    let manager = Arc::new(match &storage {
        Some(s) => GatewaySessionManager::with_storage(config, working_dir, s.clone()),
        None => GatewaySessionManager::new(config, working_dir),
    });

    // Restore sessions from persistent storage
    match manager.restore_sessions().await {
        Ok(count) if count > 0 => info!("Restored {count} sessions from storage"),
        Ok(_) => {}
        Err(e) => tracing::warn!("Session restoration failed: {e}"),
    }

    let channel_mgr = Arc::new(ChannelManager::new(256));

    // Start config file watcher for hot-reload
    let _watcher_handle = config_watcher::start_config_watcher(manager.clone());

    // Start channel message processing loop
    let channel_mgr_loop = channel_mgr.clone();
    let manager_loop = manager.clone();
    tokio::spawn(async move {
        channel_mgr_loop.run_message_loop(manager_loop).await;
    });

    let state = Arc::new(GatewayState {
        manager,
        channel_mgr,
        auth_token,
    });

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    info!("Gateway listening on {addr}");
    info!("  WebSocket: ws://{addr}/ws");
    info!("  Health:    http://{addr}/health");
    if _watcher_handle.is_some() {
        info!("  Config watcher: active");
    }
    info!("  Channel manager: active");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// GET /health — simple HTTP health check.
async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Query parameters for WebSocket connection (alternative auth).
#[derive(Deserialize, Default)]
struct WsQuery {
    token: Option<String>,
}

/// GET /ws — WebSocket upgrade with optional bearer token authentication.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    // Authenticate if auth_token is configured
    if let Some(expected_token) = &state.auth_token {
        let provided_token = extract_bearer_token(&headers).or(query.token.as_deref());

        match provided_token {
            Some(token) if token == expected_token => {}
            _ => {
                tracing::warn!("WebSocket authentication failed");
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
    }

    let manager = state.manager.clone();
    let channel_mgr = state.channel_mgr.clone();
    Ok(ws.on_upgrade(move |socket| ws::handle_ws_connection(socket, manager, channel_mgr)))
}

/// Extract bearer token from Authorization header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer my-secret-token".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), Some("my-secret-token"));
    }

    #[test]
    fn test_extract_bearer_token_missing() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn test_extract_bearer_token_wrong_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Basic abc123".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), None);
    }
}
