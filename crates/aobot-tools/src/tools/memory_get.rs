//! `memory_get` tool — read specific content from memory files.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct MemoryGetTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl MemoryGetTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "memory_get".to_string(),
            description:
                "Read content from a memory file, optionally within a specific line range."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path of the memory file to read."
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Start line (1-based, inclusive). Omit to start from beginning."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "End line (1-based, inclusive). Omit to read to end."
                    }
                },
                "required": ["path"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn label(&self) -> &str {
        "Memory Get"
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
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: path")?
            .to_string();
        let start_line = params
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let end_line = params
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ctx.ops_tx.send(GatewayOp::MemoryGet {
            path,
            start_line,
            end_line,
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
