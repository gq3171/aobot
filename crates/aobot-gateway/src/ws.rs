//! WebSocket connection handler.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::StreamExt;
use tracing::{info, warn};

use crate::channel::ChannelManager;
use crate::handlers::handle_rpc;
use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, INVALID_PARAMS, PARSE_ERROR};
use crate::session_manager::{GatewaySessionManager, StreamEvent};

/// Handle a WebSocket connection.
pub async fn handle_ws_connection(
    mut socket: WebSocket,
    manager: Arc<GatewaySessionManager>,
    channel_mgr: Arc<ChannelManager>,
) {
    info!("WebSocket client connected");

    while let Some(msg) = socket.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                warn!("WebSocket receive error: {e}");
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                // Check if this is a streaming request
                if let Some(request) = try_parse_stream_request(&text) {
                    handle_stream_request(&mut socket, request, &manager).await;
                } else {
                    let response = process_rpc_message(&text, &manager, &channel_mgr).await;
                    let response_json = match serde_json::to_string(&response) {
                        Ok(json) => json,
                        Err(e) => {
                            warn!("Failed to serialize response: {e}");
                            continue;
                        }
                    };

                    if socket.send(Message::Text(response_json.into())).await.is_err() {
                        break;
                    }
                }
            }
            Message::Close(_) => {
                info!("WebSocket client disconnected");
                break;
            }
            Message::Ping(data) => {
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            _ => {}
        }
    }

    info!("WebSocket connection closed");
}

/// Try to parse a text message as a chat.stream request.
/// Returns the parsed request if it's a valid chat.stream call.
fn try_parse_stream_request(text: &str) -> Option<JsonRpcRequest> {
    let request: JsonRpcRequest = serde_json::from_str(text).ok()?;
    if request.jsonrpc == "2.0" && request.method == "chat.stream" {
        Some(request)
    } else {
        None
    }
}

/// Handle a chat.stream request by sending streaming events over the WebSocket.
async fn handle_stream_request(
    socket: &mut WebSocket,
    request: JsonRpcRequest,
    manager: &GatewaySessionManager,
) {
    let id = request.id.clone();

    // Extract params
    let message = match request.params.get("message").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => {
            let resp = JsonRpcResponse::error(id, INVALID_PARAMS, "Missing 'message' parameter");
            let _ = send_json(socket, &resp).await;
            return;
        }
    };

    let session_key = request
        .params
        .get("session_key")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let agent = request
        .params
        .get("agent")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Create channel for streaming events
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let agent_ref = agent.as_deref();

    let prompt_fut = manager.send_message_streaming(
        &session_key,
        &message,
        agent_ref,
        event_tx,
    );

    tokio::pin!(prompt_fut);

    let mut prompt_result = None;
    let mut send_error = false;

    loop {
        tokio::select! {
            // Forward streaming events to WebSocket
            event = event_rx.recv() => {
                match event {
                    Some(stream_event) => {
                        let notification = serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": "chat.event",
                            "params": {
                                "session_key": &session_key,
                                "event": stream_event,
                            }
                        });
                        if send_json_value(socket, &notification).await.is_err() {
                            send_error = true;
                            break;
                        }
                    }
                    None => {
                        // Channel closed, prompt task is done but we haven't got the result yet
                        // Continue to await the prompt future
                    }
                }
            }
            // Wait for prompt to complete
            result = &mut prompt_fut, if prompt_result.is_none() => {
                prompt_result = Some(result);
                // Don't break yet - drain remaining events
            }
        }

        // If prompt is done and channel is drained, exit
        if prompt_result.is_some() && event_rx.is_empty() {
            break;
        }
    }

    if send_error {
        return;
    }

    // Send final response
    let response = match prompt_result {
        Some(Ok(full_text)) => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "session_key": session_key,
                "response": full_text,
            }),
        ),
        Some(Err(e)) => JsonRpcResponse::error(id, INTERNAL_ERROR, e),
        None => JsonRpcResponse::error(id, INTERNAL_ERROR, "Prompt completed without result"),
    };

    let _ = send_json(socket, &response).await;
}

/// Send a serializable value as JSON text over WebSocket.
async fn send_json<T: serde::Serialize>(
    socket: &mut WebSocket,
    value: &T,
) -> Result<(), axum::Error> {
    let json = serde_json::to_string(value).map_err(|_| axum::Error::new("serialize error"))?;
    socket
        .send(Message::Text(json.into()))
        .await
        .map_err(axum::Error::new)
}

/// Send a serde_json::Value as JSON text over WebSocket.
async fn send_json_value(
    socket: &mut WebSocket,
    value: &serde_json::Value,
) -> Result<(), axum::Error> {
    let json = value.to_string();
    socket
        .send(Message::Text(json.into()))
        .await
        .map_err(axum::Error::new)
}

/// Parse and process a JSON-RPC message.
async fn process_rpc_message(
    text: &str,
    manager: &GatewaySessionManager,
    channel_mgr: &ChannelManager,
) -> JsonRpcResponse {
    let request: JsonRpcRequest = match serde_json::from_str(text) {
        Ok(req) => req,
        Err(e) => {
            return JsonRpcResponse::error(
                serde_json::Value::Null,
                PARSE_ERROR,
                format!("Parse error: {e}"),
            );
        }
    };

    // Validate jsonrpc version
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse::error(
            request.id,
            INTERNAL_ERROR,
            "Invalid JSON-RPC version, expected '2.0'",
        );
    }

    handle_rpc(&request.method, &request.params, request.id, manager, channel_mgr).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_manager() -> Arc<GatewaySessionManager> {
        let config = aobot_config::AoBotConfig::default();
        Arc::new(GatewaySessionManager::new(config, PathBuf::from("/tmp")))
    }

    fn create_test_channel_mgr() -> Arc<ChannelManager> {
        Arc::new(ChannelManager::new(16))
    }

    #[tokio::test]
    async fn test_process_valid_health() {
        let manager = create_test_manager();
        let channel_mgr = create_test_channel_mgr();
        let msg = r#"{"jsonrpc":"2.0","id":1,"method":"health","params":{}}"#;
        let resp = process_rpc_message(msg, &manager, &channel_mgr).await;
        assert!(resp.result.is_some());
        assert_eq!(resp.result.unwrap()["status"], "ok");
    }

    #[tokio::test]
    async fn test_process_invalid_json() {
        let manager = create_test_manager();
        let channel_mgr = create_test_channel_mgr();
        let resp = process_rpc_message("not json", &manager, &channel_mgr).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, PARSE_ERROR);
    }

    #[tokio::test]
    async fn test_process_unknown_method() {
        let manager = create_test_manager();
        let channel_mgr = create_test_channel_mgr();
        let msg = r#"{"jsonrpc":"2.0","id":2,"method":"unknown"}"#;
        let resp = process_rpc_message(msg, &manager, &channel_mgr).await;
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_process_sessions_list() {
        let manager = create_test_manager();
        let channel_mgr = create_test_channel_mgr();
        let msg = r#"{"jsonrpc":"2.0","id":3,"method":"sessions.list"}"#;
        let resp = process_rpc_message(msg, &manager, &channel_mgr).await;
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result["sessions"].is_array());
    }

    #[tokio::test]
    async fn test_try_parse_stream_request() {
        let msg = r#"{"jsonrpc":"2.0","id":1,"method":"chat.stream","params":{"message":"hi"}}"#;
        let req = try_parse_stream_request(msg);
        assert!(req.is_some());
        assert_eq!(req.unwrap().method, "chat.stream");
    }

    #[tokio::test]
    async fn test_try_parse_stream_request_non_stream() {
        let msg = r#"{"jsonrpc":"2.0","id":1,"method":"chat.send","params":{"message":"hi"}}"#;
        let req = try_parse_stream_request(msg);
        assert!(req.is_none());
    }

    #[tokio::test]
    async fn test_process_agents_list() {
        let manager = create_test_manager();
        let channel_mgr = create_test_channel_mgr();
        let msg = r#"{"jsonrpc":"2.0","id":4,"method":"agents.list"}"#;
        let resp = process_rpc_message(msg, &manager, &channel_mgr).await;
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert!(result["agents"].is_object());
        assert_eq!(result["default_agent"], "default");
    }

    #[tokio::test]
    async fn test_process_channels_list() {
        let manager = create_test_manager();
        let channel_mgr = create_test_channel_mgr();
        let msg = r#"{"jsonrpc":"2.0","id":5,"method":"channels.list"}"#;
        let resp = process_rpc_message(msg, &manager, &channel_mgr).await;
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert_eq!(result["channels"], serde_json::json!([]));
    }
}
