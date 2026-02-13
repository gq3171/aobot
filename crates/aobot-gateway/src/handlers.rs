//! JSON-RPC method handlers.

use serde_json::{Value, json};

use crate::channel::ChannelManager;
use crate::jsonrpc::{INTERNAL_ERROR, INVALID_PARAMS, JsonRpcResponse, METHOD_NOT_FOUND};
use crate::session_manager::GatewaySessionManager;

/// Route a JSON-RPC request to the appropriate handler.
pub async fn handle_rpc(
    method: &str,
    params: &Value,
    id: Value,
    manager: &GatewaySessionManager,
    channel_mgr: &ChannelManager,
) -> JsonRpcResponse {
    match method {
        "health" => handle_health(id).await,
        "chat.send" => handle_chat_send(params, id, manager).await,
        "chat.history" => handle_chat_history(params, id, manager).await,
        "sessions.list" => handle_sessions_list(id, manager).await,
        "sessions.delete" => handle_sessions_delete(params, id, manager).await,
        "agents.list" => handle_agents_list(id, manager).await,
        "agents.add" => handle_agents_add(params, id, manager).await,
        "agents.delete" => handle_agents_delete(params, id, manager).await,
        "channels.list" => handle_channels_list(id, channel_mgr).await,
        "channels.status" => handle_channels_status(params, id, channel_mgr).await,
        "config.get" => handle_config_get(id, manager).await,
        "config.set" => handle_config_set(params, id, manager).await,
        // chat.stream is handled specially in ws.rs, but we route it here as a fallback
        "chat.stream" => handle_chat_send(params, id, manager).await,
        _ => JsonRpcResponse::error(id, METHOD_NOT_FOUND, format!("Method not found: {method}")),
    }
}

/// health — returns system status.
async fn handle_health(id: Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
        }),
    )
}

/// chat.send — send a message to an agent session, return the response.
///
/// Params:
///   - message: string (required)
///   - session_key: string (optional, auto-generated if missing)
///   - agent: string (optional, uses default agent)
async fn handle_chat_send(
    params: &Value,
    id: Value,
    manager: &GatewaySessionManager,
) -> JsonRpcResponse {
    let message = match params.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'message' parameter"),
    };

    let session_key = params
        .get("session_key")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let agent = params.get("agent").and_then(|v| v.as_str());

    match manager.send_message(&session_key, message, agent).await {
        Ok(response) => JsonRpcResponse::success(
            id,
            json!({
                "session_key": session_key,
                "response": response,
            }),
        ),
        Err(e) => JsonRpcResponse::error(id, INTERNAL_ERROR, e),
    }
}

/// chat.history — get conversation history for a session.
///
/// Params:
///   - session_key: string (required)
async fn handle_chat_history(
    params: &Value,
    id: Value,
    manager: &GatewaySessionManager,
) -> JsonRpcResponse {
    let session_key = match params.get("session_key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'session_key' parameter");
        }
    };

    match manager.get_history(session_key).await {
        Ok(history) => JsonRpcResponse::success(
            id,
            json!({
                "session_key": session_key,
                "messages": history,
            }),
        ),
        Err(e) => JsonRpcResponse::error(id, INTERNAL_ERROR, e),
    }
}

/// sessions.list — list all active sessions.
async fn handle_sessions_list(id: Value, manager: &GatewaySessionManager) -> JsonRpcResponse {
    let sessions = manager.list_sessions().await;
    JsonRpcResponse::success(
        id,
        json!({
            "sessions": sessions,
        }),
    )
}

/// sessions.delete — delete a session.
///
/// Params:
///   - session_key: string (required)
async fn handle_sessions_delete(
    params: &Value,
    id: Value,
    manager: &GatewaySessionManager,
) -> JsonRpcResponse {
    let session_key = match params.get("session_key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'session_key' parameter");
        }
    };

    let deleted = manager.delete_session(session_key).await;
    JsonRpcResponse::success(
        id,
        json!({
            "deleted": deleted,
        }),
    )
}

/// agents.list — list all configured agents.
async fn handle_agents_list(id: Value, manager: &GatewaySessionManager) -> JsonRpcResponse {
    let agents = manager.list_agents().await;
    let config = manager.get_config().await;
    JsonRpcResponse::success(
        id,
        json!({
            "agents": agents,
            "default_agent": config.default_agent,
        }),
    )
}

/// agents.add — add or update an agent configuration.
///
/// Params:
///   - name: string (required)
///   - model: string (required)
///   - system_prompt: string (optional)
///   - tools: string[] (optional)
async fn handle_agents_add(
    params: &Value,
    id: Value,
    manager: &GatewaySessionManager,
) -> JsonRpcResponse {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'name' parameter"),
    };

    let model = match params.get("model").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'model' parameter"),
    };

    let system_prompt = params
        .get("system_prompt")
        .and_then(|v| v.as_str())
        .map(String::from);

    let tools = params
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| {
            vec![
                "bash".to_string(),
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
            ]
        });

    let agent_config = aobot_types::AgentConfig {
        name: name.clone(),
        model,
        system_prompt,
        tools: aobot_types::AgentToolsConfig {
            allow: tools,
            ..Default::default()
        },
        subagents: None,
        sandbox: None,
    };

    manager.add_agent(name.clone(), agent_config).await;

    JsonRpcResponse::success(
        id,
        json!({
            "added": name,
        }),
    )
}

/// agents.delete — delete an agent configuration.
///
/// Params:
///   - name: string (required)
async fn handle_agents_delete(
    params: &Value,
    id: Value,
    manager: &GatewaySessionManager,
) -> JsonRpcResponse {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'name' parameter"),
    };

    let deleted = manager.delete_agent(name).await;
    JsonRpcResponse::success(
        id,
        json!({
            "deleted": deleted,
        }),
    )
}

/// config.get — get current configuration.
async fn handle_config_get(id: Value, manager: &GatewaySessionManager) -> JsonRpcResponse {
    let config = manager.get_config().await;
    match serde_json::to_value(&config) {
        Ok(val) => JsonRpcResponse::success(id, val),
        Err(e) => JsonRpcResponse::error(id, INTERNAL_ERROR, format!("Serialization error: {e}")),
    }
}

/// config.set — update configuration.
///
/// Params: the full AoBotConfig object
async fn handle_config_set(
    params: &Value,
    id: Value,
    manager: &GatewaySessionManager,
) -> JsonRpcResponse {
    let config: aobot_config::AoBotConfig = match serde_json::from_value(params.clone()) {
        Ok(c) => c,
        Err(e) => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, format!("Invalid config: {e}"));
        }
    };

    manager.set_config(config).await;
    JsonRpcResponse::success(id, json!({"updated": true}))
}

/// channels.list — list all registered channels with status.
async fn handle_channels_list(id: Value, channel_mgr: &ChannelManager) -> JsonRpcResponse {
    let channels = channel_mgr.list_channels().await;
    JsonRpcResponse::success(
        id,
        json!({
            "channels": channels,
        }),
    )
}

/// channels.status — get the status of a specific channel.
///
/// Params:
///   - channel_id: string (required)
async fn handle_channels_status(
    params: &Value,
    id: Value,
    channel_mgr: &ChannelManager,
) -> JsonRpcResponse {
    let channel_id = match params.get("channel_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'channel_id' parameter");
        }
    };

    match channel_mgr.channel_status(channel_id).await {
        Some(status) => JsonRpcResponse::success(
            id,
            json!({
                "channel_id": channel_id,
                "status": status,
            }),
        ),
        None => JsonRpcResponse::error(
            id,
            INVALID_PARAMS,
            format!("Channel not found: {channel_id}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_manager() -> GatewaySessionManager {
        let config = aobot_config::AoBotConfig::default();
        GatewaySessionManager::new(config, PathBuf::from("/tmp"))
    }

    fn create_test_channel_mgr() -> ChannelManager {
        ChannelManager::new(16)
    }

    #[tokio::test]
    async fn test_handle_health() {
        let resp = handle_health(json!(1)).await;
        let result = resp.result.unwrap();
        assert_eq!(result["status"], "ok");
    }

    #[tokio::test]
    async fn test_handle_sessions_list_empty() {
        let manager = create_test_manager();
        let resp = handle_sessions_list(json!(1), &manager).await;
        let result = resp.result.unwrap();
        assert_eq!(result["sessions"], json!([]));
    }

    #[tokio::test]
    async fn test_handle_chat_send_missing_message() {
        let manager = create_test_manager();
        let resp = handle_chat_send(&json!({}), json!(1), &manager).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_handle_chat_history_missing_key() {
        let manager = create_test_manager();
        let resp = handle_chat_history(&json!({}), json!(1), &manager).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_handle_method_not_found() {
        let manager = create_test_manager();
        let channel_mgr = create_test_channel_mgr();
        let resp = handle_rpc(
            "nonexistent.method",
            &json!({}),
            json!(1),
            &manager,
            &channel_mgr,
        )
        .await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_handle_config_get() {
        let manager = create_test_manager();
        let resp = handle_config_get(json!(1), &manager).await;
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result.get("gateway").is_some());
        assert!(result.get("agents").is_some());
    }

    #[tokio::test]
    async fn test_handle_agents_list() {
        let manager = create_test_manager();
        let resp = handle_agents_list(json!(1), &manager).await;
        let result = resp.result.unwrap();
        assert!(result["agents"].is_object());
        assert_eq!(result["default_agent"], "default");
    }

    #[tokio::test]
    async fn test_handle_agents_add_and_delete() {
        let manager = create_test_manager();

        // Add agent
        let params = json!({
            "name": "coder",
            "model": "anthropic/claude-sonnet-4",
            "system_prompt": "You are a coding assistant.",
            "tools": ["bash", "read"]
        });
        let resp = handle_agents_add(&params, json!(1), &manager).await;
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["added"], "coder");

        // Verify it's in the list
        let agents = manager.list_agents().await;
        assert!(agents.contains_key("coder"));
        assert_eq!(agents["coder"].model, "anthropic/claude-sonnet-4");

        // Delete agent
        let params = json!({"name": "coder"});
        let resp = handle_agents_delete(&params, json!(2), &manager).await;
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["deleted"], true);

        // Verify it's gone
        let agents = manager.list_agents().await;
        assert!(!agents.contains_key("coder"));
    }

    #[tokio::test]
    async fn test_handle_agents_add_missing_name() {
        let manager = create_test_manager();
        let params = json!({"model": "test"});
        let resp = handle_agents_add(&params, json!(1), &manager).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_handle_channels_list_empty() {
        let channel_mgr = create_test_channel_mgr();
        let resp = handle_channels_list(json!(1), &channel_mgr).await;
        let result = resp.result.unwrap();
        assert_eq!(result["channels"], json!([]));
    }

    #[tokio::test]
    async fn test_handle_channels_status_not_found() {
        let channel_mgr = create_test_channel_mgr();
        let params = json!({"channel_id": "nonexistent"});
        let resp = handle_channels_status(&params, json!(1), &channel_mgr).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_handle_channels_status_missing_param() {
        let channel_mgr = create_test_channel_mgr();
        let resp = handle_channels_status(&json!({}), json!(1), &channel_mgr).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }
}
