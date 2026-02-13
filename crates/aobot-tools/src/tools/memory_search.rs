//! `memory_search` tool — search memory using hybrid vector + keyword search.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct MemorySearchTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl MemorySearchTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "memory_search".to_string(),
            description: "Search memory files using hybrid vector + keyword search. Returns relevant passages with file paths, line numbers, and relevance scores.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query."
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 10)."
                    }
                },
                "required": ["query"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn label(&self) -> &str {
        "Memory Search"
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
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: query")?
            .to_string();
        let max_results = params
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ctx.ops_tx.send(GatewayOp::MemorySearch {
            query,
            max_results,
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
