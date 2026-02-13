//! `sessions_send` tool — send a message to a specific session.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct SessionsSendTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl SessionsSendTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "sessions_send".to_string(),
            description: "Send a message to a specific agent session and get the response."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_key": {
                        "type": "string",
                        "description": "The session key to send the message to."
                    },
                    "message": {
                        "type": "string",
                        "description": "The message text to send."
                    },
                    "agent": {
                        "type": "string",
                        "description": "Optional agent name override."
                    }
                },
                "required": ["session_key", "message"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for SessionsSendTool {
    fn name(&self) -> &str {
        "sessions_send"
    }

    fn label(&self) -> &str {
        "Sessions Send"
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
        let session_key = params
            .get("session_key")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: session_key")?
            .to_string();
        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: message")?
            .to_string();
        let agent = params
            .get("agent")
            .and_then(|v| v.as_str())
            .map(String::from);

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ctx.ops_tx.send(GatewayOp::SendMessage {
            session_key,
            message,
            agent,
            reply: tx,
        })?;

        let result = rx.await?;
        let text = match result {
            crate::context::GatewayOpResult::Json(v) => serde_json::to_string_pretty(&v)?,
            crate::context::GatewayOpResult::Text(t) => t,
            crate::context::GatewayOpResult::Error(e) => return Err(e.into()),
        };

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text,
                text_signature: None,
            })],
            details: None,
        })
    }
}
