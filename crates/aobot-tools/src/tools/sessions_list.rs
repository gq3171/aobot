//! `sessions_list` tool — list active sessions.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct SessionsListTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl SessionsListTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "sessions_list".to_string(),
            description: "List active agent sessions with their metadata (session key, agent name, model, message count).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of sessions to return."
                    }
                },
                "required": []
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for SessionsListTool {
    fn name(&self) -> &str {
        "sessions_list"
    }

    fn label(&self) -> &str {
        "Sessions List"
    }

    fn definition(&self) -> &Tool {
        &self.definition
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        _params: Value,
        _cancel: CancellationToken,
        _on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
    ) -> Result<AgentToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ctx
            .ops_tx
            .send(GatewayOp::ListSessions { reply: tx })?;

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
