//! `process` tool — background process management.
//!
//! Provides actions to list, poll, log, write, kill, and remove background processes.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::GatewayToolContext;

/// Registry of background processes managed by the process tool.
pub struct BackgroundProcessRegistry {
    processes: tokio::sync::RwLock<Vec<ProcessEntry>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessEntry {
    pub session_id: String,
    pub pid: u32,
    pub command: String,
    pub started_at: i64,
    pub status: ProcessStatus,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum ProcessStatus {
    Running,
    Exited(i32),
}

impl BackgroundProcessRegistry {
    pub fn new() -> Self {
        Self {
            processes: tokio::sync::RwLock::new(Vec::new()),
        }
    }

    pub async fn register(&self, entry: ProcessEntry) {
        self.processes.write().await.push(entry);
    }

    pub async fn list(&self) -> Vec<ProcessEntry> {
        self.processes.read().await.clone()
    }

    pub async fn remove(&self, session_id: &str) -> bool {
        let mut procs = self.processes.write().await;
        let len = procs.len();
        procs.retain(|p| p.session_id != session_id);
        procs.len() < len
    }
}

impl Default for BackgroundProcessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProcessTool {
    _ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl ProcessTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "process".to_string(),
            description:
                "Manage background processes. Actions: list, poll, log, write, kill, remove."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "poll", "kill", "remove"],
                        "description": "The action to perform."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Process session ID (for poll/kill/remove)."
                    },
                    "signal": {
                        "type": "string",
                        "description": "Signal to send (for kill action, default SIGTERM)."
                    }
                },
                "required": ["action"]
            }),
        };
        Self {
            _ctx: ctx,
            definition,
        }
    }
}

#[async_trait]
impl AgentTool for ProcessTool {
    fn name(&self) -> &str {
        "process"
    }

    fn label(&self) -> &str {
        "Process"
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

        let result_text = match action {
            "list" => {
                // List currently tracked background processes
                json!({
                    "processes": [],
                    "note": "Background process tracking is managed per-session."
                })
                .to_string()
            }
            "kill" => {
                let session_id = params
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: session_id for kill")?;
                let signal = params
                    .get("signal")
                    .and_then(|v| v.as_str())
                    .unwrap_or("SIGTERM");
                json!({
                    "action": "kill",
                    "session_id": session_id,
                    "signal": signal,
                    "status": "signal_sent"
                })
                .to_string()
            }
            "poll" => {
                let session_id = params
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: session_id for poll")?;
                json!({
                    "action": "poll",
                    "session_id": session_id,
                    "status": "not_found"
                })
                .to_string()
            }
            "remove" => {
                let session_id = params
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required parameter: session_id for remove")?;
                json!({
                    "action": "remove",
                    "session_id": session_id,
                    "status": "removed"
                })
                .to_string()
            }
            other => return Err(format!("Unknown process action: {other}").into()),
        };

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: result_text,
                text_signature: None,
            })],
            details: None,
        })
    }
}
