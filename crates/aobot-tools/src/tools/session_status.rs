//! `session_status` tool — get status of the current session.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::GatewayToolContext;

pub struct SessionStatusTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl SessionStatusTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "session_status".to_string(),
            description:
                "Get status information about the current session (model, agent, session key)."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for SessionStatusTool {
    fn name(&self) -> &str {
        "session_status"
    }

    fn label(&self) -> &str {
        "Session Status"
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
        let status = json!({
            "session_key": self.ctx.current_session_key,
            "agent_id": self.ctx.current_agent_id,
        });

        let text = serde_json::to_string_pretty(&status)?;

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text,
                text_signature: None,
            })],
            details: None,
        })
    }
}
