//! aobot Plugin SDK — helpers for writing external channel plugins.
//!
//! An external plugin is a standalone process that communicates with the aobot
//! host over stdin/stdout using NDJSON JSON-RPC 2.0.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use aobot_plugin_sdk::{PluginChannel, run_plugin, PluginContext};
//! use aobot_types::{ChannelConfig, ChannelStatus, OutboundMessage};
//!
//! struct MyChannel { /* ... */ }
//!
//! #[async_trait::async_trait]
//! impl PluginChannel for MyChannel {
//!     fn channel_type(&self) -> &str { "my-channel" }
//!
//!     async fn initialize(&mut self, channel_id: &str, config: &ChannelConfig) -> anyhow::Result<()> {
//!         Ok(())
//!     }
//!
//!     async fn start(&self, ctx: PluginContext) -> anyhow::Result<()> {
//!         // Start listening, call ctx.emit_inbound() for incoming messages
//!         Ok(())
//!     }
//!
//!     async fn stop(&self) -> anyhow::Result<()> { Ok(()) }
//!     async fn send(&self, message: OutboundMessage) -> anyhow::Result<()> { Ok(()) }
//!     fn status(&self) -> ChannelStatus { ChannelStatus::Running }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     run_plugin(MyChannel { /* ... */ }).await.unwrap();
//! }
//! ```

use std::collections::HashMap;
use std::io::Write;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};

pub use aobot_types::{
    Attachment, ChannelConfig, ChannelStatus, InboundMessage, OutboundMessage,
};

// ──────────────────── JSON-RPC types (mirrored from plugin_protocol) ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcMessage {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InitializeParams {
    channel_id: String,
    config: ChannelConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InitializeResult {
    channel_type: String,
    #[serde(default)]
    supports_streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SendParams {
    message: OutboundMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotifyProcessingParams {
    recipient_id: String,
    #[serde(default)]
    metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InboundMessageNotification {
    message: InboundMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatusChangeNotification {
    status: ChannelStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogNotification {
    level: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatusResult {
    status: ChannelStatus,
}

// ──────────────────── Plugin trait ────────────────────

/// Trait for external channel plugin implementations.
///
/// Implement this trait for your channel and pass it to [`run_plugin`].
#[async_trait::async_trait]
pub trait PluginChannel: Send + Sync + 'static {
    /// Returns the channel type identifier (e.g. "slack", "whatsapp").
    fn channel_type(&self) -> &str;

    /// Whether this channel supports streaming responses.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Initialize the plugin with the given channel ID and config.
    async fn initialize(
        &mut self,
        channel_id: &str,
        config: &ChannelConfig,
    ) -> anyhow::Result<()>;

    /// Start the channel listener.
    ///
    /// Use `ctx.emit_inbound()` to forward incoming messages to the host.
    async fn start(&self, ctx: PluginContext) -> anyhow::Result<()>;

    /// Stop the channel listener.
    async fn stop(&self) -> anyhow::Result<()>;

    /// Send a message to the external platform.
    async fn send(&self, message: OutboundMessage) -> anyhow::Result<()>;

    /// Get the current channel status.
    fn status(&self) -> ChannelStatus;

    /// Notify the external platform that a message is being processed.
    async fn notify_processing(
        &self,
        _recipient_id: &str,
        _metadata: &HashMap<String, Value>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

// ──────────────────── Plugin context ────────────────────

/// Context passed to the plugin's `start()` method.
///
/// Provides methods for the plugin to communicate back to the host.
#[derive(Clone)]
pub struct PluginContext {
    _private: (),
}

impl PluginContext {
    /// Emit an inbound message to the host.
    ///
    /// This sends a JSON-RPC notification on stdout.
    pub fn emit_inbound(&self, message: InboundMessage) {
        let notif = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: None,
            method: "inbound_message".into(),
            params: Some(serde_json::to_value(InboundMessageNotification { message }).unwrap()),
        };
        write_stdout(&notif);
    }

    /// Emit a status change notification to the host.
    pub fn emit_status_change(&self, status: ChannelStatus) {
        let notif = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: None,
            method: "status_change".into(),
            params: Some(serde_json::to_value(StatusChangeNotification { status }).unwrap()),
        };
        write_stdout(&notif);
    }

    /// Emit a log message to the host.
    pub fn emit_log(&self, level: &str, message: &str) {
        let notif = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: None,
            method: "log".into(),
            params: Some(
                serde_json::to_value(LogNotification {
                    level: level.into(),
                    message: message.into(),
                })
                .unwrap(),
            ),
        };
        write_stdout(&notif);
    }
}

/// Write a JSON-RPC message to stdout as a single NDJSON line.
fn write_stdout(msg: &impl Serialize) {
    let mut line = serde_json::to_string(msg).expect("serialize JSON-RPC message");
    line.push('\n');
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = lock.write_all(line.as_bytes());
    let _ = lock.flush();
}

/// Write a JSON-RPC response to stdout.
fn write_response(resp: &JsonRpcResponse) {
    write_stdout(resp);
}

// ──────────────────── Main loop ────────────────────

/// Run the plugin main loop, reading JSON-RPC requests from stdin and
/// dispatching them to the [`PluginChannel`] implementation.
///
/// This function blocks until stdin is closed or a `shutdown` request is received.
pub async fn run_plugin(mut channel: impl PluginChannel) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    let ctx = PluginContext { _private: () };

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        let msg: JsonRpcMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {e}"),
                        data: None,
                    }),
                };
                write_response(&resp);
                continue;
            }
        };

        let id = msg.id;
        let result = handle_request(&mut channel, &ctx, &msg).await;

        if let Some(req_id) = id {
            let resp = match result {
                Ok(value) => JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(req_id),
                    result: Some(value),
                    error: None,
                },
                Err(e) => JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(req_id),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32603,
                        message: e.to_string(),
                        data: None,
                    }),
                },
            };
            write_response(&resp);
        }

        // Check for shutdown
        if msg.method == "shutdown" {
            break;
        }
    }

    Ok(())
}

async fn handle_request(
    channel: &mut impl PluginChannel,
    ctx: &PluginContext,
    msg: &JsonRpcMessage,
) -> anyhow::Result<Value> {
    match msg.method.as_str() {
        "initialize" => {
            let params: InitializeParams = msg
                .params
                .as_ref()
                .map(|p| serde_json::from_value(p.clone()))
                .transpose()?
                .ok_or_else(|| anyhow::anyhow!("Missing params for initialize"))?;

            channel
                .initialize(&params.channel_id, &params.config)
                .await?;

            Ok(serde_json::to_value(InitializeResult {
                channel_type: channel.channel_type().to_string(),
                supports_streaming: channel.supports_streaming(),
            })?)
        }
        "start" => {
            channel.start(ctx.clone()).await?;
            Ok(Value::Null)
        }
        "stop" => {
            channel.stop().await?;
            Ok(Value::Null)
        }
        "send" => {
            let params: SendParams = msg
                .params
                .as_ref()
                .map(|p| serde_json::from_value(p.clone()))
                .transpose()?
                .ok_or_else(|| anyhow::anyhow!("Missing params for send"))?;

            channel.send(params.message).await?;
            Ok(Value::Null)
        }
        "notify_processing" => {
            let params: NotifyProcessingParams = msg
                .params
                .as_ref()
                .map(|p| serde_json::from_value(p.clone()))
                .transpose()?
                .ok_or_else(|| anyhow::anyhow!("Missing params for notify_processing"))?;

            channel
                .notify_processing(&params.recipient_id, &params.metadata)
                .await?;
            Ok(Value::Null)
        }
        "status" => Ok(serde_json::to_value(StatusResult {
            status: channel.status(),
        })?),
        "shutdown" => {
            channel.stop().await?;
            Ok(Value::Null)
        }
        other => anyhow::bail!("Unknown method: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPlugin {
        initialized: std::sync::atomic::AtomicBool,
    }

    impl MockPlugin {
        fn new() -> Self {
            Self {
                initialized: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl PluginChannel for MockPlugin {
        fn channel_type(&self) -> &str {
            "mock"
        }

        async fn initialize(
            &mut self,
            _channel_id: &str,
            _config: &ChannelConfig,
        ) -> anyhow::Result<()> {
            self.initialized
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        async fn start(&self, _ctx: PluginContext) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send(&self, _message: OutboundMessage) -> anyhow::Result<()> {
            Ok(())
        }

        fn status(&self) -> ChannelStatus {
            ChannelStatus::Running
        }
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let mut plugin = MockPlugin::new();
        let ctx = PluginContext { _private: () };

        let msg = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: Some(1),
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "channel_id": "test-ch",
                "config": {
                    "channel_type": "external",
                    "enabled": true,
                    "settings": {}
                }
            })),
        };

        let result = handle_request(&mut plugin, &ctx, &msg).await.unwrap();
        let init_result: InitializeResult = serde_json::from_value(result).unwrap();
        assert_eq!(init_result.channel_type, "mock");
        assert!(!init_result.supports_streaming);
        assert!(plugin
            .initialized
            .load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_handle_status() {
        let mut plugin = MockPlugin::new();
        let ctx = PluginContext { _private: () };

        let msg = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: Some(2),
            method: "status".into(),
            params: None,
        };

        let result = handle_request(&mut plugin, &ctx, &msg).await.unwrap();
        let status: StatusResult = serde_json::from_value(result).unwrap();
        assert_eq!(status.status, ChannelStatus::Running);
    }

    #[tokio::test]
    async fn test_handle_unknown_method() {
        let mut plugin = MockPlugin::new();
        let ctx = PluginContext { _private: () };

        let msg = JsonRpcMessage {
            jsonrpc: "2.0".into(),
            id: Some(3),
            method: "nonexistent".into(),
            params: None,
        };

        let result = handle_request(&mut plugin, &ctx, &msg).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown method"));
    }

    #[test]
    fn test_plugin_context_emit_inbound() {
        // Just verify it doesn't panic — actual stdout is hard to capture in tests
        let ctx = PluginContext { _private: () };
        let msg = InboundMessage {
            channel_type: "mock".into(),
            channel_id: "ch-1".into(),
            sender_id: "user-1".into(),
            sender_name: None,
            text: "hello".into(),
            agent: None,
            session_key: None,
            metadata: HashMap::new(),
            attachments: vec![],
            timestamp: 0,
        };
        ctx.emit_inbound(msg);
    }

    #[test]
    fn test_plugin_context_emit_status_change() {
        let ctx = PluginContext { _private: () };
        ctx.emit_status_change(ChannelStatus::Running);
    }

    #[test]
    fn test_plugin_context_emit_log() {
        let ctx = PluginContext { _private: () };
        ctx.emit_log("info", "test message");
    }
}
