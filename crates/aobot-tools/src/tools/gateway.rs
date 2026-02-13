//! `gateway` tool — gateway configuration management.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct GatewayConfigTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl GatewayConfigTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "gateway".to_string(),
            description: "Manage gateway configuration. Actions: config.get (read current config), config.patch (merge partial config).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["config.get", "config.patch"],
                        "description": "The action to perform."
                    },
                    "patch": {
                        "type": "object",
                        "description": "Configuration patch to apply (for config.patch action)."
                    }
                },
                "required": ["action"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for GatewayConfigTool {
    fn name(&self) -> &str {
        "gateway"
    }

    fn label(&self) -> &str {
        "Gateway Config"
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
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: action")?;

        let (tx, rx) = tokio::sync::oneshot::channel();

        match action {
            "config.get" => {
                self.ctx.ops_tx.send(GatewayOp::GetConfig { reply: tx })?;
            }
            "config.patch" => {
                let patch = params
                    .get("patch")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                self.ctx
                    .ops_tx
                    .send(GatewayOp::PatchConfig { patch, reply: tx })?;
            }
            other => {
                return Err(format!("Unknown action: {other}").into());
            }
        }

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
