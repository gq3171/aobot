//! Discord Bot channel plugin for aobot.
//!
//! Uses serenity to connect to the Discord Gateway and handle messages.
//!
//! # Configuration
//!
//! ```toml
//! [channels.my-dc-bot]
//! channel_type = "discord"
//! enabled = true
//! agent = "default"
//!
//! [channels.my-dc-bot.settings]
//! bot_token = "Bot MTIzNDU2Nzg5..."
//! ```

pub mod handler;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Context};
use serenity::all::{CreateAttachment, CreateMessage, GatewayIntents, Http};
use serenity::model::id::ChannelId;
use serenity::Client;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::info;

use aobot_types::{Attachment, ChannelConfig, ChannelStatus, InboundMessage, OutboundMessage};

/// Maximum characters per Discord message (API limit is 2000).
const MAX_MESSAGE_LEN: usize = 2000;

/// Split a long message into chunks that fit within Discord's limit.
///
/// Uses a two-pass strategy:
/// 1. Split naively by paragraph -> line -> space -> hard cut.
/// 2. Fix fenced code blocks by closing/reopening ``` across chunk boundaries.
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    // Pass 1: naive split
    let mut raw_chunks = Vec::new();
    let mut buf = text.to_string();

    while !buf.is_empty() {
        if buf.len() <= max_len {
            raw_chunks.push(buf);
            break;
        }

        let search_area = &buf[..max_len];
        let split_at = find_split_point(search_area);

        raw_chunks.push(buf[..split_at].to_string());
        buf = buf[split_at..].trim_start_matches('\n').to_string();
    }

    // Pass 2: inject code-fence close/reopen across chunk boundaries
    let mut chunks = Vec::new();
    let mut in_code_block = false;
    let mut code_fence = String::new();

    for raw in raw_chunks {
        let mut chunk = String::new();

        if in_code_block {
            chunk.push_str(&code_fence);
            chunk.push('\n');
        }

        chunk.push_str(&raw);

        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_code_block {
                    in_code_block = false;
                    code_fence.clear();
                } else {
                    in_code_block = true;
                    code_fence = trimmed.to_string();
                }
            }
        }

        if in_code_block {
            chunk.push_str("\n```");
        }

        chunks.push(chunk);
    }

    chunks
}

/// Find the best position to split text, searching backwards from the end.
fn find_split_point(text: &str) -> usize {
    // Priority 1: paragraph break (\n\n)
    if let Some(pos) = text.rfind("\n\n") {
        if pos > 0 {
            return pos + 1;
        }
    }

    // Priority 2: line break (\n)
    if let Some(pos) = text.rfind('\n') {
        if pos > 0 {
            return pos + 1;
        }
    }

    // Priority 3: space
    if let Some(pos) = text.rfind(' ') {
        if pos > 0 {
            return pos + 1;
        }
    }

    // Priority 4: hard cut at max_len
    text.len()
}

/// Map common MIME types to file extensions.
fn mime_extension(mime: &str) -> &str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "audio/ogg" => "ogg",
        "audio/mpeg" => "mp3",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

/// Discord channel plugin implementing `ChannelPlugin`.
pub struct DiscordChannel {
    id: String,
    bot_token: String,
    agent: Option<String>,
    state: Mutex<DiscordState>,
}

struct DiscordState {
    status: ChannelStatus,
    http: Option<Arc<Http>>,
    client_handle: Option<JoinHandle<()>>,
    /// Serenity shard manager for graceful shutdown.
    shard_manager: Option<Arc<serenity::gateway::ShardManager>>,
}

impl DiscordChannel {
    pub fn new(id: String, bot_token: String, agent: Option<String>) -> Self {
        Self {
            id,
            bot_token,
            agent,
            state: Mutex::new(DiscordState {
                status: ChannelStatus::Stopped,
                http: None,
                client_handle: None,
                shard_manager: None,
            }),
        }
    }
}

#[async_trait::async_trait]
impl aobot_gateway::channel::ChannelPlugin for DiscordChannel {
    fn channel_type(&self) -> &str {
        "discord"
    }

    fn channel_id(&self) -> &str {
        &self.id
    }

    async fn start(&self, sender: mpsc::Sender<InboundMessage>) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        if state.status == ChannelStatus::Running {
            bail!("Discord channel {} is already running", self.id);
        }

        state.status = ChannelStatus::Starting;

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let http_client = Arc::new(reqwest::Client::new());

        let event_handler = handler::DiscordHandler {
            channel_id: self.id.clone(),
            agent: self.agent.clone(),
            sender,
            http_client,
        };

        let mut client = Client::builder(&self.bot_token, intents)
            .event_handler(event_handler)
            .await
            .context("Failed to create Discord client")?;

        let http = client.http.clone();
        let shard_manager = client.shard_manager.clone();

        let channel_id = self.id.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = client.start().await {
                tracing::error!(
                    channel_id,
                    "Discord client error: {e}"
                );
            }
        });

        state.http = Some(http);
        state.shard_manager = Some(shard_manager);
        state.client_handle = Some(handle);
        state.status = ChannelStatus::Running;

        info!(channel_id = self.id, "Discord channel started");

        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;

        if let Some(shard_manager) = state.shard_manager.take() {
            shard_manager.shutdown_all().await;
        }

        if let Some(handle) = state.client_handle.take() {
            let _ = handle.await;
        }

        state.http = None;
        state.status = ChannelStatus::Stopped;

        info!(channel_id = self.id, "Discord channel stopped");

        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> anyhow::Result<()> {
        let state = self.state.lock().await;
        let http = state
            .http
            .as_ref()
            .context("Discord channel not started")?
            .clone();
        drop(state);

        let discord_channel_id = message
            .metadata
            .get("discord_channel_id")
            .and_then(|v| v.as_str())
            .or(Some(message.recipient_id.as_str()))
            .and_then(|s| s.parse::<u64>().ok())
            .context("missing discord_channel_id in metadata and recipient_id is not a valid u64")?;

        let channel = ChannelId::new(discord_channel_id);

        // Send attachments first
        for attachment in &message.attachments {
            send_attachment(&http, channel, attachment).await?;
        }

        // Send text (skip if empty and we had attachments)
        if !message.text.is_empty() {
            let chunks = split_message(&message.text, MAX_MESSAGE_LEN);
            for chunk in chunks {
                let builder = CreateMessage::new().content(&chunk);
                channel.send_message(&http, builder).await?;
            }
        }

        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        match self.state.try_lock() {
            Ok(state) => state.status.clone(),
            Err(_) => ChannelStatus::Starting,
        }
    }

    async fn notify_processing(
        &self,
        _recipient_id: &str,
        metadata: &HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<()> {
        let state = self.state.lock().await;
        let http = state
            .http
            .as_ref()
            .context("Discord channel not started")?
            .clone();
        drop(state);

        let discord_channel_id = metadata
            .get("discord_channel_id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .context("missing discord_channel_id in metadata")?;

        let channel = ChannelId::new(discord_channel_id);
        channel.broadcast_typing(&http).await?;

        Ok(())
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}

/// Decode base64 and send an attachment via Discord's file upload.
async fn send_attachment(
    http: &Http,
    channel: ChannelId,
    attachment: &Attachment,
) -> anyhow::Result<()> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    let (bytes, filename) = match attachment {
        Attachment::Image { base64, mime_type } => {
            let bytes = engine
                .decode(base64)
                .context("failed to decode image base64")?;
            let ext = mime_extension(mime_type);
            (bytes, format!("image.{ext}"))
        }
        Attachment::Document {
            base64,
            mime_type,
            file_name,
        } => {
            let bytes = engine
                .decode(base64)
                .context("failed to decode document base64")?;
            let fallback = format!("file.{}", mime_extension(mime_type));
            let name = file_name.as_deref().unwrap_or(&fallback).to_string();
            (bytes, name)
        }
        Attachment::Audio { base64, mime_type } => {
            let bytes = engine
                .decode(base64)
                .context("failed to decode audio base64")?;
            let ext = mime_extension(mime_type);
            (bytes, format!("audio.{ext}"))
        }
    };

    let discord_attachment = CreateAttachment::bytes(bytes, filename);
    let builder = CreateMessage::new().add_file(discord_attachment);
    channel.send_message(http, builder).await?;

    Ok(())
}

/// Factory function: create a `DiscordChannel` from a channel config.
///
/// Expects `config.settings["bot_token"]` to be a string.
pub fn create_discord_channel(
    id: String,
    config: &ChannelConfig,
) -> anyhow::Result<Arc<dyn aobot_gateway::channel::ChannelPlugin>> {
    let bot_token = config
        .settings
        .get("bot_token")
        .and_then(|v| v.as_str())
        .context("Discord channel requires settings.bot_token (string)")?;

    let channel = DiscordChannel::new(id, bot_token.to_string(), config.agent.clone());
    Ok(Arc::new(channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("hello", 100);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_split_message_exact_limit() {
        let text = "a".repeat(100);
        let chunks = split_message(&text, 100);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_split_message_paragraph_boundary() {
        let text = format!("{}\n\n{}", "a".repeat(50), "b".repeat(60));
        let chunks = split_message(&text, 80);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with("aaa"));
        assert!(chunks[1].starts_with("bbb"));
    }

    #[test]
    fn test_split_message_line_boundary() {
        let text = format!("{}\n{}", "a".repeat(50), "b".repeat(60));
        let chunks = split_message(&text, 80);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_split_message_space_boundary() {
        let text = format!("{} {}", "a".repeat(50), "b".repeat(60));
        let chunks = split_message(&text, 80);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_split_message_hard_cut() {
        let text = "a".repeat(200);
        let chunks = split_message(&text, 80);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 80);
        assert_eq!(chunks[1].len(), 80);
        assert_eq!(chunks[2].len(), 40);
    }

    #[test]
    fn test_split_message_code_block_awareness() {
        let text = format!("Before\n```rust\n{}\n```\nAfter", "x".repeat(100));
        let chunks = split_message(&text, 60);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            let fence_count = chunk.matches("```").count();
            assert_eq!(
                fence_count % 2,
                0,
                "Unmatched code fences in chunk: {chunk}"
            );
        }
    }

    #[test]
    fn test_split_message_preserves_all_content() {
        let text = "Hello world! This is a test message.\n\nSecond paragraph here.\nThird line.";
        let chunks = split_message(text, 40);
        let joined: String = chunks.join("");
        assert!(joined.contains("Hello world"));
        assert!(joined.contains("Second paragraph"));
        assert!(joined.contains("Third line"));
    }

    #[test]
    fn test_split_message_discord_limit() {
        // Verify with Discord's 2000 char limit
        let text = "x".repeat(5000);
        let chunks = split_message(&text, MAX_MESSAGE_LEN);
        for chunk in &chunks {
            assert!(chunk.len() <= MAX_MESSAGE_LEN);
        }
    }

    #[test]
    fn test_mime_extension() {
        assert_eq!(mime_extension("image/jpeg"), "jpg");
        assert_eq!(mime_extension("image/png"), "png");
        assert_eq!(mime_extension("image/gif"), "gif");
        assert_eq!(mime_extension("audio/ogg"), "ogg");
        assert_eq!(mime_extension("audio/mpeg"), "mp3");
        assert_eq!(mime_extension("application/pdf"), "pdf");
        assert_eq!(mime_extension("application/unknown"), "bin");
    }

    #[test]
    fn test_factory_missing_token() {
        let config = ChannelConfig {
            channel_type: "discord".into(),
            enabled: true,
            agent: None,
            settings: HashMap::new(),
        };
        let result = create_discord_channel("test".into(), &config);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("bot_token"));
    }

    #[test]
    fn test_factory_success() {
        let mut settings = HashMap::new();
        settings.insert(
            "bot_token".into(),
            serde_json::Value::String("test-token-123".into()),
        );
        let config = ChannelConfig {
            channel_type: "discord".into(),
            enabled: true,
            agent: Some("my-agent".into()),
            settings,
        };
        let result = create_discord_channel("dc-1".into(), &config);
        assert!(result.is_ok());
        let channel = result.unwrap();
        assert_eq!(channel.channel_type(), "discord");
        assert_eq!(channel.channel_id(), "dc-1");
    }
}
