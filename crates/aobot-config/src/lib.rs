use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use aobot_types::{AgentConfig, ChannelConfig};

/// MCP server transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
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

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("Config directory not found")]
    NoDirFound,
}

/// Gateway server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Port to listen on.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Host to bind to.
    #[serde(default = "default_host")]
    pub host: String,
    /// Bearer token for authentication (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

fn default_port() -> u16 {
    3000
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            auth_token: None,
        }
    }
}

/// Configuration for automatic context compaction.
///
/// Aligned with pi-mono's `CompactionSettings`:
/// - `reserve_tokens`: absolute number of tokens to keep free for new messages
/// - `keep_recent_tokens`: how many tokens of recent context to preserve after compaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Whether automatic compaction is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Tokens to reserve for new messages (subtracted from context_window to get trigger threshold).
    #[serde(default = "default_reserve_tokens")]
    pub reserve_tokens: u64,
    /// Approximate number of tokens of recent context to keep after compaction.
    #[serde(default = "default_keep_recent_tokens")]
    pub keep_recent_tokens: u64,
}

fn default_true() -> bool {
    true
}

fn default_reserve_tokens() -> u64 {
    16384
}

fn default_keep_recent_tokens() -> u64 {
    20000
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reserve_tokens: 16384,
            keep_recent_tokens: 20000,
        }
    }
}

/// Configuration for automatic retry of transient API errors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Whether automatic retry is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum number of retries before giving up.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Base delay in milliseconds before the first retry.
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,
    /// Maximum delay in milliseconds (caps exponential backoff).
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
}

fn default_max_retries() -> u32 {
    3
}

fn default_base_delay_ms() -> u64 {
    2000
}

fn default_max_delay_ms() -> u64 {
    60000
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            base_delay_ms: 2000,
            max_delay_ms: 60000,
        }
    }
}

/// Top-level aobot configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AoBotConfig {
    /// Gateway server config.
    #[serde(default)]
    pub gateway: GatewayConfig,
    /// Named agent configurations.
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,
    /// Default agent name.
    #[serde(default = "default_agent_name")]
    pub default_agent: String,
    /// Named channel configurations.
    #[serde(default)]
    pub channels: HashMap<String, ChannelConfig>,
    /// Automatic context compaction settings.
    #[serde(default)]
    pub compaction: CompactionConfig,
    /// Automatic retry settings for transient API errors.
    #[serde(default)]
    pub retry: RetryConfig,
    /// MCP server configurations.
    #[serde(default)]
    pub mcp: HashMap<String, McpServerConfig>,
}

fn default_agent_name() -> String {
    "default".to_string()
}

impl Default for AoBotConfig {
    fn default() -> Self {
        let mut agents = HashMap::new();
        agents.insert(
            "default".to_string(),
            AgentConfig {
                name: "default".to_string(),
                model: "anthropic/claude-sonnet-4".to_string(),
                system_prompt: Some("You are a helpful assistant.".to_string()),
                tools: vec![
                    "bash".to_string(),
                    "read".to_string(),
                    "write".to_string(),
                    "edit".to_string(),
                ],
            },
        );

        Self {
            gateway: GatewayConfig::default(),
            agents,
            default_agent: "default".to_string(),
            channels: HashMap::new(),
            compaction: CompactionConfig::default(),
            retry: RetryConfig::default(),
            mcp: HashMap::new(),
        }
    }
}

/// Resolve the aobot config directory (~/.aobot/).
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    dirs::home_dir()
        .map(|h| h.join(".aobot"))
        .ok_or(ConfigError::NoDirFound)
}

/// Resolve the config file path (~/.aobot/config.toml).
pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("config.toml"))
}

/// Load configuration from the default path, falling back to defaults.
pub fn load_config() -> Result<AoBotConfig, ConfigError> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    let path = config_file_path()?;
    load_config_from(&path)
}

/// Load configuration from a specific path, falling back to defaults if not found.
pub fn load_config_from(path: &Path) -> Result<AoBotConfig, ConfigError> {
    if !path.exists() {
        tracing::debug!("Config file not found at {}, using defaults", path.display());
        return Ok(AoBotConfig::default());
    }

    let content = std::fs::read_to_string(path)?;
    let config: AoBotConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Ensure the config directory exists.
pub fn ensure_config_dir() -> Result<PathBuf, ConfigError> {
    let dir = config_dir()?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// Save configuration to the default path.
pub fn save_config(config: &AoBotConfig) -> Result<(), ConfigError> {
    let dir = ensure_config_dir()?;
    let path = dir.join("config.toml");
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AoBotConfig::default();
        assert_eq!(config.default_agent, "default");
        assert!(config.agents.contains_key("default"));
        assert_eq!(config.gateway.port, 3000);
    }

    #[test]
    fn test_toml_parse() {
        let toml_str = r#"
default_agent = "coder"

[gateway]
port = 8080

[agents.coder]
name = "coder"
model = "anthropic/claude-sonnet-4"
tools = ["bash", "read"]
"#;
        let config: AoBotConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.gateway.port, 8080);
        assert!(config.agents.contains_key("coder"));
        assert!(config.channels.is_empty());
    }

    #[test]
    fn test_toml_parse_with_channels() {
        let toml_str = r#"
[channels.my-tg-bot]
channel_type = "telegram"
enabled = true
agent = "coder"

[channels.my-tg-bot.settings]
bot_token = "123:ABC"
"#;
        let config: AoBotConfig = toml::from_str(toml_str).unwrap();
        assert!(config.channels.contains_key("my-tg-bot"));
        let ch = &config.channels["my-tg-bot"];
        assert_eq!(ch.channel_type, "telegram");
        assert!(ch.enabled);
        assert_eq!(ch.agent, Some("coder".into()));
    }

    #[test]
    fn test_roundtrip() {
        let config = AoBotConfig::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: AoBotConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config.default_agent, deserialized.default_agent);
        assert_eq!(config.gateway.port, deserialized.gateway.port);
    }
}
