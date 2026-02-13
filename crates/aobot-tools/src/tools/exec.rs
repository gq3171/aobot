//! `exec` tool — enhanced command execution with background mode and safety features.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::GatewayToolContext;

/// Maximum output size in characters before truncation.
const MAX_OUTPUT_CHARS: usize = 200_000;

/// Default timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

pub struct ExecTool {
    _ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl ExecTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "exec".to_string(),
            description: "Execute a shell command with enhanced features: configurable timeout, output truncation, and background execution mode.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 120)."
                    },
                    "background": {
                        "type": "boolean",
                        "description": "Run in background mode (default: false)."
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Working directory for the command."
                    }
                },
                "required": ["command"]
            }),
        };
        Self {
            _ctx: ctx,
            definition,
        }
    }
}

#[async_trait]
impl AgentTool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn label(&self) -> &str {
        "Exec"
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
        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: command")?;
        let timeout_secs = params
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS);
        let background = params
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let working_dir = params
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(String::from);

        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(command);

        if let Some(dir) = &working_dir {
            cmd.current_dir(dir);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        if background {
            // Background mode: spawn and return immediately
            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id().unwrap_or(0);
                    let text = json!({
                        "mode": "background",
                        "pid": pid,
                        "command": command,
                        "status": "started"
                    })
                    .to_string();
                    return Ok(AgentToolResult {
                        content: vec![ContentBlock::Text(TextContent {
                            text,
                            text_signature: None,
                        })],
                        details: None,
                    });
                }
                Err(e) => return Err(format!("Failed to spawn command: {e}").into()),
            }
        }

        // Foreground mode: execute with timeout
        let timeout = tokio::time::Duration::from_secs(timeout_secs);
        let output = match tokio::time::timeout(timeout, cmd.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(format!("Command execution failed: {e}").into()),
            Err(_) => return Err(format!("Command timed out after {timeout_secs}s").into()),
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();

        let mut truncated = false;
        if stdout.len() > MAX_OUTPUT_CHARS {
            stdout.truncate(MAX_OUTPUT_CHARS);
            stdout.push_str("\n... [output truncated]");
            truncated = true;
        }
        if stderr.len() > MAX_OUTPUT_CHARS {
            stderr.truncate(MAX_OUTPUT_CHARS);
            stderr.push_str("\n... [output truncated]");
            truncated = true;
        }

        let mut result = format!("Exit code: {exit_code}\n");
        if !stdout.is_empty() {
            result.push_str(&format!("\n--- stdout ---\n{stdout}\n"));
        }
        if !stderr.is_empty() {
            result.push_str(&format!("\n--- stderr ---\n{stderr}\n"));
        }
        if truncated {
            result.push_str("\n[Output was truncated due to size limits]\n");
        }

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: result,
                text_signature: None,
            })],
            details: None,
        })
    }
}
