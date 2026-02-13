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
pub mod external_channel;
pub mod handlers;
pub mod jsonrpc;
pub mod plugin_protocol;
pub mod session_manager;
pub mod ws;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Deserialize;
use tracing::info;

use aobot_config::AoBotConfig;
use aobot_storage::AoBotStorage;
use aobot_types::ChannelConfig;
use channel::ChannelManager;
use session_manager::GatewaySessionManager;

/// Factory function type for creating channel plugins from config.
pub type ChannelFactory = Box<
    dyn Fn(String, &ChannelConfig) -> anyhow::Result<Arc<dyn channel::ChannelPlugin>> + Send + Sync,
>;

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
    channel_factories: HashMap<String, ChannelFactory>,
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

    // Create the gateway operations channel
    let (ops_tx, ops_rx) =
        tokio::sync::mpsc::unbounded_channel::<aobot_tools::context::GatewayOp>();

    let mut session_manager = match &storage {
        Some(s) => GatewaySessionManager::with_storage(config, working_dir, s.clone()),
        None => GatewaySessionManager::new(config, working_dir),
    };
    session_manager.set_ops_tx(ops_tx);
    let manager = Arc::new(session_manager);

    // Restore sessions from persistent storage
    match manager.restore_sessions().await {
        Ok(count) if count > 0 => info!("Restored {count} sessions from storage"),
        Ok(_) => {}
        Err(e) => tracing::warn!("Session restoration failed: {e}"),
    }

    let channel_mgr = Arc::new(ChannelManager::new(256));

    // Register channel plugins from config
    for (ch_id, ch_config) in &manager.get_config().await.channels {
        if !ch_config.enabled {
            info!(channel_id = %ch_id, "Channel disabled, skipping");
            continue;
        }

        if ch_config.channel_type == "external" {
            // External plugin — spawn subprocess with NDJSON JSON-RPC
            match external_channel::ExternalChannelPlugin::new(ch_id.clone(), ch_config) {
                Ok(plugin) => {
                    channel_mgr.register(Arc::new(plugin)).await;
                }
                Err(e) => {
                    tracing::warn!(
                        channel_id = %ch_id,
                        "Failed to create external plugin: {e}"
                    );
                }
            }
        } else if let Some(factory) = channel_factories.get(&ch_config.channel_type) {
            // Built-in channel — use factory
            match factory(ch_id.clone(), ch_config) {
                Ok(plugin) => {
                    channel_mgr.register(plugin).await;
                }
                Err(e) => {
                    tracing::warn!(channel_id = %ch_id, "Failed to create channel plugin: {e}");
                }
            }
        } else {
            tracing::warn!(
                channel_id = %ch_id,
                channel_type = %ch_config.channel_type,
                "No factory registered for channel type"
            );
        }
    }

    // Start all registered channels
    channel_mgr.start_all().await;

    // Start config file watcher for hot-reload
    let _watcher_handle = config_watcher::start_config_watcher(manager.clone());

    // Create hook registry
    let hook_registry = Arc::new(aobot_hooks::registry::HookRegistry::new());

    // Emit GatewayStartup hook event
    hook_registry
        .emit(aobot_hooks::events::HookEvent::GatewayStartup)
        .await;

    // Start GatewayOp handler loop
    let ops_manager = manager.clone();
    let ops_channel_mgr = channel_mgr.clone();
    tokio::spawn(async move {
        run_gateway_ops_loop(ops_rx, ops_manager, ops_channel_mgr).await;
    });

    // Load skills
    let config_for_skills = manager.get_config().await;
    let skill_dirs = {
        let mut dirs: Vec<(std::path::PathBuf, aobot_skills::SkillSource)> = Vec::new();
        if let Some(skills_config) = &config_for_skills.skills {
            for dir in &skills_config.dirs {
                let expanded = if dir.starts_with("~/") {
                    if let Ok(home) = std::env::var("HOME") {
                        std::path::PathBuf::from(home).join(&dir[2..])
                    } else {
                        std::path::PathBuf::from(dir)
                    }
                } else {
                    std::path::PathBuf::from(dir)
                };
                dirs.push((expanded, aobot_skills::SkillSource::Managed));
            }
        }
        // Always include default locations
        if let Ok(config_dir) = aobot_config::ensure_config_dir() {
            let skills_dir = config_dir.join("skills");
            if skills_dir.exists() {
                dirs.push((skills_dir, aobot_skills::SkillSource::Managed));
            }
        }
        dirs
    };
    let skills = aobot_skills::loader::load_skills(&skill_dirs);
    let skill_commands = aobot_skills::commands::build_skill_commands(&skills);
    let skills = Arc::new(skills);
    let skill_commands = Arc::new(skill_commands);
    info!(
        "Loaded {} skills ({} user-invocable)",
        skills.len(),
        skill_commands.len()
    );

    // Start channel message processing loop
    let channel_mgr_loop = channel_mgr.clone();
    let manager_loop = manager.clone();
    let hooks_loop = hook_registry.clone();
    let skills_loop = skills.clone();
    tokio::spawn(async move {
        channel_mgr_loop
            .run_message_loop(manager_loop, hooks_loop, skills_loop)
            .await;
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

/// Process GatewayOp messages from gateway tools.
///
/// This loop receives operation requests from gateway tools and executes them
/// against the real SessionManager and ChannelManager.
async fn run_gateway_ops_loop(
    mut ops_rx: tokio::sync::mpsc::UnboundedReceiver<aobot_tools::context::GatewayOp>,
    manager: Arc<GatewaySessionManager>,
    channel_mgr: Arc<ChannelManager>,
) {
    use aobot_tools::context::{GatewayOp, GatewayOpResult};

    info!("Gateway ops handler loop started");

    while let Some(op) = ops_rx.recv().await {
        match op {
            GatewayOp::ListSessions { reply } => {
                let sessions = manager.list_sessions().await;
                let json = serde_json::to_value(&sessions).unwrap_or_default();
                let _ = reply.send(GatewayOpResult::Json(json));
            }
            GatewayOp::GetHistory { session_key, reply } => {
                match manager.get_history(&session_key).await {
                    Ok(history) => {
                        let json = serde_json::to_value(&history).unwrap_or_default();
                        let _ = reply.send(GatewayOpResult::Json(json));
                    }
                    Err(e) => {
                        let _ = reply.send(GatewayOpResult::Error(e));
                    }
                }
            }
            GatewayOp::SendMessage {
                session_key,
                message,
                agent,
                reply,
            } => match manager
                .send_message(&session_key, &message, agent.as_deref())
                .await
            {
                Ok(response) => {
                    let _ = reply.send(GatewayOpResult::Text(response));
                }
                Err(e) => {
                    let _ = reply.send(GatewayOpResult::Error(e));
                }
            },
            GatewayOp::SpawnSession {
                task,
                agent_id,
                label,
                reply,
            } => {
                let session_key = format!(
                    "subagent:{}:{}",
                    agent_id.as_deref().unwrap_or("default"),
                    uuid::Uuid::new_v4()
                );
                match manager
                    .create_session(&session_key, agent_id.as_deref())
                    .await
                {
                    Ok(()) => {
                        match manager
                            .send_message(&session_key, &task, agent_id.as_deref())
                            .await
                        {
                            Ok(response) => {
                                let json = serde_json::json!({
                                    "session_key": session_key,
                                    "label": label,
                                    "response": response,
                                });
                                let _ = reply.send(GatewayOpResult::Json(json));
                            }
                            Err(e) => {
                                let _ = reply.send(GatewayOpResult::Error(e));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = reply.send(GatewayOpResult::Error(e));
                    }
                }
            }
            GatewayOp::ChannelSend {
                channel_id,
                recipient_id,
                text,
                reply,
            } => {
                let outbound = aobot_types::OutboundMessage {
                    channel_type: String::new(), // will be resolved by channel
                    channel_id: channel_id.clone(),
                    recipient_id,
                    text,
                    session_key: None,
                    attachments: vec![],
                    metadata: std::collections::HashMap::new(),
                };
                match channel_mgr.send_message(outbound).await {
                    Ok(()) => {
                        let _ = reply.send(GatewayOpResult::Json(
                            serde_json::json!({"status": "sent", "channel_id": channel_id}),
                        ));
                    }
                    Err(e) => {
                        let _ = reply.send(GatewayOpResult::Error(e.to_string()));
                    }
                }
            }
            GatewayOp::ListAgents { reply } => {
                let agents = manager.list_agents().await;
                let json = serde_json::to_value(&agents).unwrap_or_default();
                let _ = reply.send(GatewayOpResult::Json(json));
            }
            GatewayOp::GetConfig { reply } => {
                let config = manager.get_config().await;
                let json = serde_json::to_value(&config).unwrap_or_default();
                let _ = reply.send(GatewayOpResult::Json(json));
            }
            GatewayOp::PatchConfig { patch, reply } => {
                // Read current config, merge patch, and apply
                let mut config = manager.get_config().await;
                let mut config_json = serde_json::to_value(&config).unwrap_or_default();
                if let (Some(base), Some(patch_obj)) =
                    (config_json.as_object_mut(), patch.as_object())
                {
                    for (k, v) in patch_obj {
                        base.insert(k.clone(), v.clone());
                    }
                }
                match serde_json::from_value::<AoBotConfig>(config_json) {
                    Ok(new_config) => {
                        manager.apply_config(new_config).await;
                        config = manager.get_config().await;
                        let json = serde_json::to_value(&config).unwrap_or_default();
                        let _ = reply.send(GatewayOpResult::Json(json));
                    }
                    Err(e) => {
                        let _ = reply.send(GatewayOpResult::Error(format!("Invalid config: {e}")));
                    }
                }
            }
            GatewayOp::MemorySearch {
                query,
                max_results,
                reply,
            } => {
                // Memory search — placeholder until memory system is fully wired
                let _ = reply.send(GatewayOpResult::Json(serde_json::json!({
                    "query": query,
                    "max_results": max_results,
                    "results": [],
                    "note": "Memory system not yet initialized. Configure [memory] in config.toml."
                })));
            }
            GatewayOp::MemoryGet {
                path,
                start_line,
                end_line,
                reply,
            } => {
                // Read memory file directly
                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let start = start_line.unwrap_or(1).saturating_sub(1);
                        let end = end_line.unwrap_or(lines.len()).min(lines.len());
                        let selected: Vec<&str> =
                            lines.get(start..end).unwrap_or_default().to_vec();
                        let _ = reply.send(GatewayOpResult::Text(selected.join("\n")));
                    }
                    Err(e) => {
                        let _ = reply.send(GatewayOpResult::Error(format!(
                            "Failed to read {path}: {e}"
                        )));
                    }
                }
            }
            GatewayOp::CronList { reply } => {
                let _ = reply.send(GatewayOpResult::Json(serde_json::json!({
                    "jobs": [],
                    "note": "Cron system not yet initialized. Configure [cron] in config.toml."
                })));
            }
            GatewayOp::CronAdd {
                schedule,
                task,
                agent_id,
                reply,
            } => {
                let _ = reply.send(GatewayOpResult::Json(serde_json::json!({
                    "status": "not_available",
                    "schedule": schedule,
                    "task": task,
                    "agent_id": agent_id,
                    "note": "Cron system not yet initialized."
                })));
            }
            GatewayOp::CronRemove { job_id, reply } => {
                let _ = reply.send(GatewayOpResult::Json(serde_json::json!({
                    "status": "not_available",
                    "job_id": job_id,
                })));
            }
            GatewayOp::CronUpdate {
                job_id,
                enabled,
                reply,
            } => {
                let _ = reply.send(GatewayOpResult::Json(serde_json::json!({
                    "status": "not_available",
                    "job_id": job_id,
                    "enabled": enabled,
                })));
            }
            GatewayOp::CronRun { job_id, reply } => {
                let _ = reply.send(GatewayOpResult::Json(serde_json::json!({
                    "status": "not_available",
                    "job_id": job_id,
                })));
            }
        }
    }

    info!("Gateway ops handler loop stopped");
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
