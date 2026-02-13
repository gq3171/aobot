//! `message` tool — send a message through a channel.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct MessageTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl MessageTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "message".to_string(),
            description: "Send a message to a user through a channel (Telegram, Discord, etc.)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "channel": {
                        "type": "string",
                        "description": "Channel ID to send through."
                    },
                    "target": {
                        "type": "string",
                        "description": "Recipient ID on the channel."
                    },
                    "message": {
                        "type": "string",
                        "description": "The message text to send."
                    },
                    "reply_to": {
                        "type": "string",
                        "description": "Optional message ID to reply to."
                    }
                },
                "required": ["channel", "target", "message"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }

    fn label(&self) -> &str {
        "Message"
    }

    fn definition(&self) -> &Tool {
        &self.definition
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: Value,
        _cancel: CancellationToken,
        _on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
    ) -> Result<AgentToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let channel_id = params
            .get("channel")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: channel")?
            .to_string();
        let recipient_id = params
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: target")?
            .to_string();
        let text = params
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: message")?
            .to_string();

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ctx.ops_tx.send(GatewayOp::ChannelSend {
            channel_id,
            recipient_id,
            text,
            reply: tx,
        })?;

        let result = rx.await?;
        let response = match result {
            crate::context::GatewayOpResult::Json(v) => serde_json::to_string_pretty(&v)?,
            crate::context::GatewayOpResult::Text(t) => t,
            crate::context::GatewayOpResult::Error(e) => return Err(e.into()),
        };

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: response,
                text_signature: None,
            })],
            details: None,
        })
    }
}
