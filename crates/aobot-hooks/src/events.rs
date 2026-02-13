//! Hook event types.

use serde::{Deserialize, Serialize};

use aobot_types::{InboundMessage, OutboundMessage};

/// Events that hooks can subscribe to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookEvent {
    /// Gateway has started.
    GatewayStartup,
    /// Gateway is shutting down.
    GatewayShutdown,
    /// A new session has been created.
    SessionStart {
        session_key: String,
        agent_id: String,
    },
    /// A session has ended.
    SessionEnd {
        session_key: String,
        agent_id: String,
    },
    /// The /new command was issued.
    CommandNew { session_key: String },
    /// The /help command was issued.
    CommandHelp { session_key: String },
    /// An inbound message has been received.
    MessageReceived { inbound: InboundMessage },
    /// An outbound message is about to be sent.
    MessageSending { outbound: OutboundMessage },
    /// A tool call is about to be executed.
    ToolCallBefore {
        tool_name: String,
        params: serde_json::Value,
    },
    /// A tool call has finished.
    ToolCallAfter {
        tool_name: String,
        result: serde_json::Value,
        is_error: bool,
    },
}
