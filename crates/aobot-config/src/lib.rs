use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use aobot_types::{AgentConfig, ChannelConfig};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON5 parse error: {0}")]
    Json5(#[from] json5::Error),
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
        }
    }
}

/// Resolve the aobot config directory (~/.aobot/).
pub fn config_dir() -> Result<PathBuf, ConfigError> {
    dirs::home_dir()
        .map(|h| h.join(".aobot"))
        .ok_or(ConfigError::NoDirFound)
}

/// Resolve the config file path (~/.aobot/config.json5).
pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("config.json5"))
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
    let config: AoBotConfig = json5::from_str(&content)?;
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
    let path = dir.join("config.json5");
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| ConfigError::Io(std::io::Error::other(e)))?;
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
    fn test_json5_parse() {
        let json5_str = r#"{
            gateway: { port: 8080 },
            agents: {
                "coder": {
                    name: "coder",
                    model: "anthropic/claude-sonnet-4",
                    tools: ["bash", "read"],
                }
            },
            default_agent: "coder",
        }"#;
        let config: AoBotConfig = json5::from_str(json5_str).unwrap();
        assert_eq!(config.gateway.port, 8080);
        assert!(config.agents.contains_key("coder"));
        assert!(config.channels.is_empty());
    }

    #[test]
    fn test_json5_parse_with_channels() {
        let json5_str = r#"{
            channels: {
                "my-tg-bot": {
                    channel_type: "telegram",
                    enabled: true,
                    agent: "coder",
                    settings: { bot_token: "123:ABC" },
                }
            },
        }"#;
        let config: AoBotConfig = json5::from_str(json5_str).unwrap();
        assert!(config.channels.contains_key("my-tg-bot"));
        let ch = &config.channels["my-tg-bot"];
        assert_eq!(ch.channel_type, "telegram");
        assert!(ch.enabled);
        assert_eq!(ch.agent, Some("coder".into()));
    }
}
