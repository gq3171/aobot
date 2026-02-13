use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ──────────────────── Tool Policy Types ────────────────────

/// Pre-defined tool profiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolProfile {
    /// Only `session_status`.
    Minimal,
    /// File system, runtime, sessions, memory, image.
    Coding,
    /// Messaging and limited session tools.
    Messaging,
    /// All tools allowed (default).
    #[default]
    Full,
}

/// Per-provider tool policy override.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolPolicyOverride {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Tool configuration for an agent.
///
/// Supports both the new structured format and backward-compatible `Vec<String>`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentToolsConfig {
    /// Base profile.
    #[serde(default)]
    pub profile: ToolProfile,
    /// Explicit allow list (replaces profile base when non-empty for non-Full profiles).
    #[serde(default)]
    pub allow: Vec<String>,
    /// Additional tools on top of profile/allow.
    #[serde(default)]
    pub also_allow: Vec<String>,
    /// Tools to deny (takes priority over everything).
    #[serde(default)]
    pub deny: Vec<String>,
    /// Per-provider overrides.
    #[serde(default)]
    pub by_provider: HashMap<String, ToolPolicyOverride>,
}

/// Sub-agent configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubagentConfig {
    /// Allowed sub-agent IDs. `["*"]` allows all.
    #[serde(default)]
    pub allow_agents: Vec<String>,
    /// Model override for sub-agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Thinking level override for sub-agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// Sandbox configuration for an agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Sandbox mode: "none", "read-only", "workspace".
    #[serde(default = "default_sandbox_mode")]
    pub mode: String,
    /// Allowed directories for file operations.
    #[serde(default)]
    pub allowed_dirs: Vec<String>,
}

fn default_sandbox_mode() -> String {
    "none".to_string()
}

// ──────────────────── Agent Types ────────────────────

/// Helper enum for deserializing `tools` field which can be
/// either `Vec<String>` (legacy) or `AgentToolsConfig` (new).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum ToolsField {
    Legacy(Vec<String>),
    Config(AgentToolsConfig),
}

/// Configuration for a single agent instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Display name for this agent.
    pub name: String,
    /// Model ID to use (e.g. "anthropic/claude-sonnet-4").
    pub model: String,
    /// Optional system prompt override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Tool configuration — supports both legacy `Vec<String>` and new `AgentToolsConfig`.
    #[serde(default, deserialize_with = "deserialize_tools")]
    pub tools: AgentToolsConfig,
    /// Sub-agent configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagents: Option<SubagentConfig>,
    /// Sandbox configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxConfig>,
}

fn deserialize_tools<'de, D>(deserializer: D) -> Result<AgentToolsConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let field = ToolsField::deserialize(deserializer)?;
    Ok(match field {
        ToolsField::Legacy(names) => AgentToolsConfig {
            profile: ToolProfile::Full,
            allow: names,
            ..Default::default()
        },
        ToolsField::Config(config) => config,
    })
}

/// Helper to get the legacy tool names from AgentToolsConfig for backward compatibility.
impl AgentToolsConfig {
    /// Returns the allow list, which for legacy configs contains the tool names.
    pub fn tool_names(&self) -> &[String] {
        &self.allow
    }

    /// Check if this is a legacy-style config (Full profile with just an allow list).
    pub fn is_legacy(&self) -> bool {
        self.profile == ToolProfile::Full
            && self.also_allow.is_empty()
            && self.deny.is_empty()
            && self.by_provider.is_empty()
    }
}

// ──────────────────── Attachment Types ────────────────────

/// An attachment (image, document, or audio) included with a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Attachment {
    Image {
        base64: String,
        mime_type: String,
    },
    Document {
        base64: String,
        mime_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_name: Option<String>,
    },
    Audio {
        base64: String,
        mime_type: String,
    },
}

// ──────────────────── Channel Types ────────────────────

/// Message from an external channel to the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    /// Channel type (e.g. "telegram", "discord").
    pub channel_type: String,
    /// Unique channel instance ID.
    pub channel_id: String,
    /// External user/sender identifier.
    pub sender_id: String,
    /// Display name of the sender.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,
    /// Message text content.
    pub text: String,
    /// Target agent name (uses default if None).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Session key for conversation continuity.
    /// If None, derived from channel_id + sender_id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    /// Platform-specific metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Attachments (images, documents, audio).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Message timestamp (unix millis).
    pub timestamp: i64,
}

/// Message from the gateway to an external channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    /// Channel type (e.g. "telegram", "discord").
    pub channel_type: String,
    /// Unique channel instance ID.
    pub channel_id: String,
    /// Recipient identifier on the external platform.
    pub recipient_id: String,
    /// Response text content.
    pub text: String,
    /// Session key for conversation continuity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    /// Attachments (images, documents, audio).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Platform-specific metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Status of a channel plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelStatus {
    /// Channel is not running.
    Stopped,
    /// Channel is initializing.
    Starting,
    /// Channel is running and accepting messages.
    Running,
    /// Channel encountered an error.
    Error(String),
}

/// Summary information about a registered channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Channel type (e.g. "telegram", "discord").
    pub channel_type: String,
    /// Unique channel instance ID.
    pub channel_id: String,
    /// Current status.
    pub status: ChannelStatus,
}

/// Configuration for a channel instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Channel type (e.g. "telegram", "discord").
    pub channel_type: String,
    /// Whether this channel is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Agent to route messages to (uses default if None).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Channel-specific settings (e.g. bot token, webhook URL).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub settings: HashMap<String, serde_json::Value>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inbound_message_serde() {
        let msg = InboundMessage {
            channel_type: "telegram".into(),
            channel_id: "tg-bot-1".into(),
            sender_id: "user123".into(),
            sender_name: Some("Alice".into()),
            text: "Hello!".into(),
            agent: None,
            session_key: None,
            metadata: HashMap::new(),
            attachments: vec![],
            timestamp: 1700000000000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: InboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.channel_type, "telegram");
        assert_eq!(parsed.sender_id, "user123");
    }

    #[test]
    fn test_outbound_message_serde() {
        let msg = OutboundMessage {
            channel_type: "discord".into(),
            channel_id: "dc-bot-1".into(),
            recipient_id: "user456".into(),
            text: "Hi there!".into(),
            session_key: Some("sess-1".into()),
            attachments: vec![],
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: OutboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.channel_type, "discord");
        assert_eq!(parsed.session_key, Some("sess-1".into()));
    }

    #[test]
    fn test_channel_status_serde() {
        let status = ChannelStatus::Running;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"running\"");

        let err = ChannelStatus::Error("connection lost".into());
        let json = serde_json::to_string(&err).unwrap();
        let parsed: ChannelStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ChannelStatus::Error("connection lost".into()));
    }

    #[test]
    fn test_attachment_serde() {
        let img = Attachment::Image {
            base64: "aGVsbG8=".into(),
            mime_type: "image/png".into(),
        };
        let json = serde_json::to_string(&img).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        let parsed: Attachment = serde_json::from_str(&json).unwrap();
        match parsed {
            Attachment::Image { base64, mime_type } => {
                assert_eq!(base64, "aGVsbG8=");
                assert_eq!(mime_type, "image/png");
            }
            _ => panic!("Expected Image variant"),
        }
    }

    #[test]
    fn test_inbound_message_with_attachments() {
        let msg = InboundMessage {
            channel_type: "telegram".into(),
            channel_id: "tg-1".into(),
            sender_id: "user1".into(),
            sender_name: None,
            text: "Look at this".into(),
            agent: None,
            session_key: None,
            metadata: HashMap::new(),
            attachments: vec![Attachment::Image {
                base64: "abc".into(),
                mime_type: "image/jpeg".into(),
            }],
            timestamp: 1700000000000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: InboundMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.attachments.len(), 1);
    }

    #[test]
    fn test_inbound_message_without_attachments_compat() {
        // Verify backward compatibility: no "attachments" field defaults to empty vec
        let json = r#"{"channel_type":"telegram","channel_id":"x","sender_id":"u","text":"hi","timestamp":0}"#;
        let parsed: InboundMessage = serde_json::from_str(json).unwrap();
        assert!(parsed.attachments.is_empty());
    }

    #[test]
    fn test_channel_config_defaults() {
        let json = r#"{"channel_type": "telegram"}"#;
        let config: ChannelConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert!(config.agent.is_none());
        assert!(config.settings.is_empty());
    }

    #[test]
    fn test_agent_config_legacy_tools() {
        let toml_str = r#"
name = "coder"
model = "anthropic/claude-sonnet-4"
tools = ["bash", "read"]
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tools.allow, vec!["bash", "read"]);
        assert_eq!(config.tools.profile, ToolProfile::Full);
        assert!(config.tools.is_legacy());
    }

    #[test]
    fn test_agent_config_new_tools() {
        let toml_str = r#"
name = "coder"
model = "anthropic/claude-sonnet-4"

[tools]
profile = "coding"
also_allow = ["web_search"]
deny = ["bash"]
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tools.profile, ToolProfile::Coding);
        assert_eq!(config.tools.also_allow, vec!["web_search"]);
        assert_eq!(config.tools.deny, vec!["bash"]);
        assert!(!config.tools.is_legacy());
    }

    #[test]
    fn test_agent_config_with_subagents() {
        let toml_str = r#"
name = "orchestrator"
model = "anthropic/claude-sonnet-4"

[subagents]
allow_agents = ["*"]
model = "anthropic/claude-haiku"
"#;
        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert!(config.subagents.is_some());
        let sub = config.subagents.unwrap();
        assert_eq!(sub.allow_agents, vec!["*"]);
        assert_eq!(sub.model, Some("anthropic/claude-haiku".to_string()));
    }
}
