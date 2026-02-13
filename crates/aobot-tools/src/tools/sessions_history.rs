//! `sessions_history` tool — get chat history for a session.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct SessionsHistoryTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl SessionsHistoryTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "sessions_history".to_string(),
            description: "Get the chat history for a specific session.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_key": {
                        "type": "string",
                        "description": "The session key to retrieve history for."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of messages to return."
                    }
                },
                "required": ["session_key"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for SessionsHistoryTool {
    fn name(&self) -> &str {
        "sessions_history"
    }

    fn label(&self) -> &str {
        "Sessions History"
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

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ctx.ops_tx.send(GatewayOp::GetHistory {
            session_key,
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
