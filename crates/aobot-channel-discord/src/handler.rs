//! Serenity EventHandler that converts Discord events to InboundMessage.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use aobot_types::{Attachment, InboundMessage};

/// Serenity event handler that bridges Discord events into the aobot channel system.
pub struct DiscordHandler {
    pub channel_id: String,
    pub agent: Option<String>,
    pub sender: mpsc::Sender<InboundMessage>,
    pub http_client: Arc<reqwest::Client>,
}

#[async_trait]
impl EventHandler for DiscordHandler {
    async fn message(&self, _ctx: Context, msg: Message) {
        // Skip messages from bots
        if msg.author.bot {
            return;
        }

        let text = msg.content.clone();

        // Download attachments
        let mut attachments = Vec::new();
        for att in &msg.attachments {
            match download_discord_attachment(&self.http_client, att).await {
                Ok(a) => attachments.push(a),
                Err(e) => warn!(
                    channel_id = self.channel_id,
                    filename = att.filename,
                    "Failed to download Discord attachment: {e}"
                ),
            }
        }

        // Skip messages with no text and no attachments
        let text = if text.is_empty() && !attachments.is_empty() {
            String::new()
        } else if text.is_empty() {
            return;
        } else {
            text
        };

        let sender_id = msg.author.id.to_string();
        let sender_name = Some(msg.author.name.clone());

        let mut metadata = HashMap::new();
        metadata.insert(
            "discord_channel_id".into(),
            serde_json::Value::String(msg.channel_id.to_string()),
        );
        metadata.insert(
            "message_id".into(),
            serde_json::Value::String(msg.id.to_string()),
        );

        // Detect bot commands (messages starting with !)
        let (command, clean_text) = parse_command(&text);
        if let Some(cmd) = command {
            metadata.insert("command".into(), serde_json::Value::String(cmd));
        }

        let inbound = InboundMessage {
            channel_type: "discord".into(),
            channel_id: self.channel_id.clone(),
            sender_id,
            sender_name,
            text: clean_text,
            agent: self.agent.clone(),
            session_key: None,
            metadata,
            attachments,
            timestamp: msg.timestamp.unix_timestamp() * 1000,
        };

        debug!(
            channel_id = self.channel_id,
            message_id = %msg.id,
            "Forwarding Discord message"
        );

        if self.sender.send(inbound).await.is_err() {
            info!(
                channel_id = self.channel_id,
                "Inbound channel closed, handler will stop processing"
            );
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!(
            channel_id = self.channel_id,
            bot_name = ready.user.name,
            "Discord bot connected and ready"
        );
    }
}

/// Parse a `!command` prefix from the message text.
/// Returns `(Some(command_name), remaining_text)` if a command was found,
/// or `(None, original_text)` otherwise.
fn parse_command(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim();
    if !trimmed.starts_with('!') {
        return (None, text.to_string());
    }

    let cmd_text = &trimmed[1..];
    let cmd = cmd_text.split_whitespace().next().unwrap_or("");
    if cmd.is_empty() {
        return (None, text.to_string());
    }

    // Map Discord commands to the same names used by the gateway
    let command = match cmd {
        "new" | "reset" => "new",
        "help" | "start" => "help",
        _ => return (None, text.to_string()),
    };

    (Some(command.to_string()), text.to_string())
}

/// Download a Discord attachment and convert it to an aobot Attachment.
async fn download_discord_attachment(
    http_client: &reqwest::Client,
    att: &serenity::model::channel::Attachment,
) -> anyhow::Result<Attachment> {
    let bytes = http_client.get(&att.url).send().await?.bytes().await?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let content_type = att
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    // Classify by content type
    if content_type.starts_with("image/") {
        Ok(Attachment::Image {
            base64: b64,
            mime_type: content_type.to_string(),
        })
    } else if content_type.starts_with("audio/") {
        Ok(Attachment::Audio {
            base64: b64,
            mime_type: content_type.to_string(),
        })
    } else {
        Ok(Attachment::Document {
            base64: b64,
            mime_type: content_type.to_string(),
            file_name: Some(att.filename.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command_new() {
        let (cmd, _text) = parse_command("!new");
        assert_eq!(cmd, Some("new".to_string()));
    }

    #[test]
    fn test_parse_command_help() {
        let (cmd, _text) = parse_command("!help");
        assert_eq!(cmd, Some("help".to_string()));
    }

    #[test]
    fn test_parse_command_reset() {
        let (cmd, _text) = parse_command("!reset");
        assert_eq!(cmd, Some("new".to_string()));
    }

    #[test]
    fn test_parse_command_unknown() {
        let (cmd, text) = parse_command("!unknown");
        assert_eq!(cmd, None);
        assert_eq!(text, "!unknown");
    }

    #[test]
    fn test_parse_command_no_prefix() {
        let (cmd, text) = parse_command("hello world");
        assert_eq!(cmd, None);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_parse_command_just_exclamation() {
        let (cmd, text) = parse_command("!");
        assert_eq!(cmd, None);
        assert_eq!(text, "!");
    }

    #[test]
    fn test_parse_command_with_extra_text() {
        let (cmd, _text) = parse_command("!new some extra text");
        assert_eq!(cmd, Some("new".to_string()));
    }
}
