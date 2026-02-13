//! aobot-mcp: MCP (Model Context Protocol) support for aobot.
//!
//! Wraps MCP Client connections as `Extension` instances that can be loaded
//! into the pi-coding-agent `ExtensionRunner`.

pub mod bridge;
pub mod config;

use std::borrow::Cow;

use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, info};

use pi_coding_agent::extensions::types::{Extension, ExtensionContext, ToolDefinition};

use crate::bridge::{mcp_result_to_value, mcp_tool_to_extension_tool};
use crate::config::{McpServerConfig, McpTransport};

/// Import required for `.serve()` method.
use rmcp::ServiceExt;

/// Type alias for the running MCP client service.
type McpRunningService =
    rmcp::service::RunningService<rmcp::RoleClient, ()>;

/// An MCP client wrapped as a pi-coding-agent Extension.
///
/// Each `McpExtension` manages a connection to a single MCP server
/// and exposes its tools through the Extension trait.
pub struct McpExtension {
    config: McpServerConfig,
    tools: Vec<ToolDefinition>,
    service: Option<McpRunningService>,
}

impl McpExtension {
    /// Create a new MCP extension from config (not yet connected).
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            tools: Vec::new(),
            service: None,
        }
    }
}

#[async_trait]
impl Extension for McpExtension {
    fn name(&self) -> &str {
        &self.config.name
    }

    async fn init(
        &mut self,
        _context: ExtensionContext,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(name = %self.config.name, "Initializing MCP extension");

        let running = match &self.config.transport {
            McpTransport::Stdio { command, args, env } => {
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(args);
                for (k, v) in env {
                    cmd.env(k, v);
                }
                let process = rmcp::transport::TokioChildProcess::new(cmd)?;
                ().serve(process).await?
            }
            McpTransport::Sse { url } => {
                use rmcp::transport::streamable_http_client::StreamableHttpClientWorker;
                let worker =
                    StreamableHttpClientWorker::<reqwest::Client>::new_simple(url.as_str());
                ().serve(worker).await?
            }
        };

        // List available tools
        let tools_result = running.list_tools(Default::default()).await?;

        self.tools = tools_result
            .tools
            .iter()
            .map(|t| mcp_tool_to_extension_tool(t))
            .collect();

        info!(
            name = %self.config.name,
            tool_count = self.tools.len(),
            "MCP extension initialized"
        );

        for tool in &self.tools {
            debug!(name = %self.config.name, tool = %tool.name, "Registered MCP tool");
        }

        self.service = Some(running);

        Ok(())
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.tools.clone()
    }

    async fn handle_tool_call(
        &self,
        tool_name: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let service = self.service.as_ref().ok_or("MCP extension not initialized")?;

        let arguments = if params.is_object() {
            Some(
                params
                    .as_object()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            )
        } else {
            None
        };

        let call_params = rmcp::model::CallToolRequestParams {
            name: Cow::Owned(tool_name.to_string()),
            arguments,
            meta: None,
            task: None,
        };

        debug!(name = %self.config.name, tool = %tool_name, "Calling MCP tool");

        let result = service.call_tool(call_params).await?;

        Ok(mcp_result_to_value(&result))
    }

    async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(service) = self.service.take() {
            info!(name = %self.config.name, "Shutting down MCP extension");
            drop(service);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_extension_new() {
        let config = McpServerConfig {
            name: "test".into(),
            transport: McpTransport::Stdio {
                command: "echo".into(),
                args: vec![],
                env: Default::default(),
            },
        };
        let ext = McpExtension::new(config);
        assert_eq!(ext.name(), "test");
        assert!(ext.tools().is_empty());
    }
}
