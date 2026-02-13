//! NDJSON JSON-RPC 2.0 protocol for external channel plugins.
//!
//! Defines the message types exchanged between the aobot host process and
//! external plugin subprocesses over stdin/stdout (one JSON object per line).
//!
//! # Host → Plugin (Requests)
//!
//! | Method             | Params                              | Description            |
//! |--------------------|-------------------------------------|------------------------|
//! | `initialize`       | `{ channel_id, config }`            | Initialize the plugin  |
//! | `start`            | `{}`                                | Start channel listener |
//! | `stop`             | `{}`                                | Stop channel           |
//! | `send`             | `{ message: OutboundMessage }`      | Send a message         |
//! | `notify_processing`| `{ recipient_id, metadata }`        | Typing indicator       |
//! | `status`           | `{}`                                | Query status           |
//! | `shutdown`         | `{}`                                | Terminate process      |
//!
//! # Plugin → Host (Notifications, no `id`)
//!
//! | Method             | Params                              | Description            |
//! |--------------------|-------------------------------------|------------------------|
//! | `inbound_message`  | `{ message: InboundMessage }`       | Received message       |
//! | `status_change`    | `{ status: ChannelStatus }`         | Status update          |
//! | `log`              | `{ level, message }`                | Log forwarding         |

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request / notification.
///
/// When `id` is `Some`, this is a request expecting a response.
/// When `id` is `None`, this is a one-way notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ──────────────────── Standard error codes ────────────────────

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

// ──────────────────── Initialize ────────────────────

/// Params for the `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    pub channel_id: String,
    pub config: aobot_types::ChannelConfig,
}

/// Result of the `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    pub channel_type: String,
    #[serde(default)]
    pub supports_streaming: bool,
}

// ──────────────────── Send ────────────────────

/// Params for the `send` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendParams {
    pub message: aobot_types::OutboundMessage,
}

// ──────────────────── Notify Processing ────────────────────

/// Params for the `notify_processing` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyProcessingParams {
    pub recipient_id: String,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, Value>,
}

// ──────────────────── Status ────────────────────

/// Result of the `status` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResult {
    pub status: aobot_types::ChannelStatus,
}

// ──────────────────── Plugin → Host notifications ────────────────────

/// Params for the `inbound_message` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessageNotification {
    pub message: aobot_types::InboundMessage,
}

/// Params for the `status_change` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusChangeNotification {
    pub status: aobot_types::ChannelStatus,
}

/// Params for the `log` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogNotification {
    pub level: String,
    pub message: String,
}

// ──────────────────── Helpers ────────────────────

impl JsonRpcMessage {
    /// Create a request (has an `id`, expects a response).
    pub fn request(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: method.into(),
            params,
        }
    }

    /// Create a notification (no `id`, one-way).
    pub fn notification(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: None,
            method: method.into(),
            params,
        }
    }
}

impl JsonRpcResponse {
    /// Create a success response.
    pub fn success(id: u64, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response.
    pub fn error(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcMessage::request(1, "initialize", Some(serde_json::json!({"channel_id": "test"})));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_notification_serialization() {
        let notif = JsonRpcMessage::notification("inbound_message", Some(serde_json::json!({"text": "hi"})));
        let json = serde_json::to_string(&notif).unwrap();
        assert!(!json.contains("\"id\""));
        assert!(json.contains("\"method\":\"inbound_message\""));
    }

    #[test]
    fn test_response_success() {
        let resp = JsonRpcResponse::success(1, serde_json::json!({"channel_type": "slack"}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_response_error() {
        let resp = JsonRpcResponse::error(1, METHOD_NOT_FOUND, "unknown method");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"error\""));
        assert!(!json.contains("\"result\""));
        assert!(json.contains("-32601"));
    }

    #[test]
    fn test_roundtrip_message() {
        let msg = JsonRpcMessage::request(42, "send", Some(serde_json::json!({"text": "hello"})));
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: JsonRpcMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, Some(42));
        assert_eq!(parsed.method, "send");
    }
}
