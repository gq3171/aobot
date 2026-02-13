//! `sessions_spawn` tool — spawn a sub-agent session with a task.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct SessionsSpawnTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl SessionsSpawnTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "sessions_spawn".to_string(),
            description: "Spawn a sub-agent session to handle a task autonomously. Returns the sub-agent's response when complete.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The task description for the sub-agent."
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "Optional target agent ID. Uses default agent if not specified."
                    },
                    "label": {
                        "type": "string",
                        "description": "Optional human-readable label for this sub-agent session."
                    }
                },
                "required": ["task"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for SessionsSpawnTool {
    fn name(&self) -> &str {
        "sessions_spawn"
    }

    fn label(&self) -> &str {
        "Sessions Spawn"
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
        let task = params
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: task")?
            .to_string();
        let agent_id = params
            .get("agent_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let label = params
            .get("label")
            .and_then(|v| v.as_str())
            .map(String::from);

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ctx.ops_tx.send(GatewayOp::SpawnSession {
            task,
            agent_id,
            label,
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
