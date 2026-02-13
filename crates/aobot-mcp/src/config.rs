use serde::{Deserialize, Serialize};

/// Transport configuration for connecting to an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpTransport {
    /// Stdio transport: spawn a child process.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: std::collections::HashMap<String, String>,
    },
    /// SSE transport: connect to an HTTP SSE endpoint.
    Sse {
        url: String,
    },
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Display name for this MCP server.
    pub name: String,
    /// Transport configuration.
    pub transport: McpTransport,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_config_serde() {
        let toml_str = r#"
name = "filesystem"
[transport]
type = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
"#;
        let config: McpServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "filesystem");
        match &config.transport {
            McpTransport::Stdio { command, args, .. } => {
                assert_eq!(command, "npx");
                assert_eq!(args.len(), 3);
            }
            _ => panic!("expected stdio transport"),
        }
    }

    #[test]
    fn test_sse_config_serde() {
        let toml_str = r#"
name = "browser"
[transport]
type = "sse"
url = "http://localhost:3001/sse"
"#;
        let config: McpServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name, "browser");
        match &config.transport {
            McpTransport::Sse { url } => {
                assert_eq!(url, "http://localhost:3001/sse");
            }
            _ => panic!("expected sse transport"),
        }
    }
}
