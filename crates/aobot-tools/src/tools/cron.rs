//! `cron` tool — manage scheduled tasks.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::{GatewayOp, GatewayToolContext};

pub struct CronTool {
    ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl CronTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "cron".to_string(),
            description: "Manage scheduled cron jobs. Actions: list, add, remove, update, run."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "add", "remove", "update", "run"],
                        "description": "The action to perform."
                    },
                    "schedule": {
                        "type": "string",
                        "description": "Cron expression (for add action, e.g. '0 * * * *')."
                    },
                    "task": {
                        "type": "string",
                        "description": "Task description (for add action)."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "Job ID (for remove/update/run actions)."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Enable/disable toggle (for update action)."
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "Agent to run the task (for add action)."
                    }
                },
                "required": ["action"]
            }),
        };
        Self { ctx, definition }
    }
}

#[async_trait]
impl AgentTool for CronTool {
    fn name(&self) -> &str {
        "cron"
    }

    fn label(&self) -> &str {
        "Cron"
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
            "list" => {
                self.ctx.ops_tx.send(GatewayOp::CronList { reply: tx })?;
            }
            "add" => {
                let schedule = params
                    .get("schedule")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: schedule")?
                    .to_string();
                let task = params
                    .get("task")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: task")?
                    .to_string();
                let agent_id = params
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                self.ctx.ops_tx.send(GatewayOp::CronAdd {
                    schedule,
                    task,
                    agent_id,
                    reply: tx,
                })?;
            }
            "remove" => {
                let job_id = params
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: job_id")?
                    .to_string();
                self.ctx
                    .ops_tx
                    .send(GatewayOp::CronRemove { job_id, reply: tx })?;
            }
            "update" => {
                let job_id = params
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: job_id")?
                    .to_string();
                let enabled = params.get("enabled").and_then(|v| v.as_bool());
                self.ctx.ops_tx.send(GatewayOp::CronUpdate {
                    job_id,
                    enabled,
                    reply: tx,
                })?;
            }
            "run" => {
                let job_id = params
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: job_id")?
                    .to_string();
                self.ctx
                    .ops_tx
                    .send(GatewayOp::CronRun { job_id, reply: tx })?;
            }
            other => {
                return Err(format!("Unknown cron action: {other}").into());
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
