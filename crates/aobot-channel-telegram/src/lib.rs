//! Telegram Bot channel plugin for aobot.
//!
//! Uses Telegram Bot API with long-polling (no webhook required).
//!
//! # Configuration
//!
//! ```toml
//! [channels.my-tg-bot]
//! channel_type = "telegram"
//! enabled = true
//! agent = "default"
//!
//! [channels.my-tg-bot.settings]
//! bot_token = "123456:ABC-DEF..."
//! ```

pub mod api;
pub mod polling;
pub mod types;

use std::sync::Arc;

use anyhow::{Context, bail};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::info;

use std::collections::HashMap;

use aobot_gateway::session_manager::StreamEvent;
use aobot_types::{Attachment, ChannelConfig, ChannelStatus, InboundMessage, OutboundMessage};

use api::TelegramApi;
use types::{
    BotCommand, EditMessageTextParams, MenuButton, SendChatActionParams, SendMessageParams,
    SetChatMenuButtonParams, SetMyCommandsParams,
};

/// Maximum characters per Telegram message (API limit is 4096, leave margin).
const MAX_MESSAGE_LEN: usize = 4000;

/// Split a long message into chunks that fit within Telegram's limit.
///
/// Uses a two-pass strategy:
/// 1. Split naively by paragraph → line → space → hard cut (guaranteed progress).
/// 2. Fix fenced code blocks by closing/reopening ``` across chunk boundaries.
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    // Pass 1: naive split (no code-fence awareness)
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

        // Reopen the code block from the previous chunk
        if in_code_block {
            chunk.push_str(&code_fence);
            chunk.push('\n');
        }

        chunk.push_str(&raw);

        // Track fence state within the raw content
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

        // Close the code block if still open at the end of this chunk
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
            return pos + 1; // include one newline, next chunk starts after second
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

/// Telegram channel plugin implementing `ChannelPlugin`.
pub struct TelegramChannel {
    id: String,
    bot_token: String,
    agent: Option<String>,
    state: Mutex<TelegramState>,
}

struct TelegramState {
    status: ChannelStatus,
    cancel: Option<CancellationToken>,
    poll_handle: Option<JoinHandle<()>>,
}

impl TelegramChannel {
    /// Create a new Telegram channel with the given ID and bot token.
    pub fn new(id: String, bot_token: String, agent: Option<String>) -> Self {
        Self {
            id,
            bot_token,
            agent,
            state: Mutex::new(TelegramState {
                status: ChannelStatus::Stopped,
                cancel: None,
                poll_handle: None,
            }),
        }
    }
}

#[async_trait::async_trait]
impl aobot_gateway::channel::ChannelPlugin for TelegramChannel {
    fn channel_type(&self) -> &str {
        "telegram"
    }

    fn channel_id(&self) -> &str {
        &self.id
    }

    async fn start(&self, sender: mpsc::Sender<InboundMessage>) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        if state.status == ChannelStatus::Running {
            bail!("Telegram channel {} is already running", self.id);
        }

        state.status = ChannelStatus::Starting;

        let api = TelegramApi::new(&self.bot_token);

        // Verify bot token
        match api.get_me().await {
            Ok(bot) => {
                info!(
                    channel_id = self.id,
                    bot_username = bot.username.as_deref().unwrap_or("unknown"),
                    "Telegram bot authenticated"
                );
            }
            Err(e) => {
                state.status = ChannelStatus::Error(format!("Auth failed: {e}"));
                bail!("Failed to authenticate Telegram bot: {e}");
            }
        }

        // Register bot commands menu
        if let Err(e) = api
            .set_my_commands(&SetMyCommandsParams {
                commands: vec![
                    BotCommand {
                        command: "new".into(),
                        description: "Start a new conversation".into(),
                    },
                    BotCommand {
                        command: "help".into(),
                        description: "Show help information".into(),
                    },
                ],
            })
            .await
        {
            tracing::warn!(channel_id = self.id, "Failed to register bot commands: {e}");
        }

        // Show menu button (commands list) in the input field
        if let Err(e) = api
            .set_chat_menu_button(&SetChatMenuButtonParams {
                menu_button: MenuButton::Commands,
            })
            .await
        {
            tracing::warn!(channel_id = self.id, "Failed to set menu button: {e}");
        }

        let cancel = CancellationToken::new();
        let cancel_child = cancel.child_token();
        let channel_id = self.id.clone();
        let agent = self.agent.clone();

        let handle = tokio::spawn(async move {
            polling::run_polling_loop(&api, channel_id, agent, sender, cancel_child).await;
        });

        state.cancel = Some(cancel);
        state.poll_handle = Some(handle);
        state.status = ChannelStatus::Running;

        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;

        if let Some(cancel) = state.cancel.take() {
            cancel.cancel();
        }

        if let Some(handle) = state.poll_handle.take() {
            let _ = handle.await;
        }

        state.status = ChannelStatus::Stopped;
        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> anyhow::Result<()> {
        let chat_id = message
            .metadata
            .get("chat_id")
            .and_then(|v| v.as_i64())
            .or_else(|| message.recipient_id.parse::<i64>().ok())
            .context("missing chat_id in metadata and recipient_id is not a valid i64")?;

        let api = TelegramApi::new(&self.bot_token);

        // Send attachments first
        for attachment in &message.attachments {
            send_attachment(&api, chat_id, attachment).await?;
        }

        // Send text (skip if empty and we had attachments)
        if !message.text.is_empty() {
            let chunks = split_message(&message.text, MAX_MESSAGE_LEN);
            for chunk in chunks {
                send_with_markdown_fallback(&api, chat_id, &chunk).await?;
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
        let chat_id = metadata
            .get("chat_id")
            .and_then(|v| v.as_i64())
            .context("missing chat_id in metadata")?;

        let api = TelegramApi::new(&self.bot_token);
        api.send_chat_action(&SendChatActionParams {
            chat_id,
            action: "typing".into(),
        })
        .await
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    async fn send_streaming(
        &self,
        metadata: &HashMap<String, serde_json::Value>,
        mut stream_rx: mpsc::UnboundedReceiver<StreamEvent>,
    ) -> anyhow::Result<()> {
        let chat_id = metadata
            .get("chat_id")
            .and_then(|v| v.as_i64())
            .context("missing chat_id in metadata for streaming")?;

        let api = TelegramApi::new(&self.bot_token);

        // Show typing indicator until we have content to display
        let _ = api
            .send_chat_action(&SendChatActionParams {
                chat_id,
                action: "typing".into(),
            })
            .await;

        let mut full_text = String::new();
        let mut last_edited_text = String::new();
        let mut last_edit_time = tokio::time::Instant::now();
        let throttle_interval = std::time::Duration::from_millis(500);
        let mut streaming_stopped = false;
        let mut message_id: Option<i64> = None; // created lazily on first delta

        while let Some(event) = stream_rx.recv().await {
            match event {
                StreamEvent::TextDelta { delta } => {
                    full_text.push_str(&delta);

                    if streaming_stopped {
                        continue;
                    }

                    if full_text.len() > MAX_MESSAGE_LEN {
                        streaming_stopped = true;
                        continue;
                    }

                    if last_edit_time.elapsed() < throttle_interval {
                        continue;
                    }

                    let display_text = format!("{full_text}▍");
                    if display_text == last_edited_text {
                        continue;
                    }

                    if let Some(mid) = message_id {
                        // Edit existing message
                        let result = api
                            .edit_message_text(&EditMessageTextParams {
                                chat_id,
                                message_id: mid,
                                text: display_text.clone(),
                                parse_mode: Some("Markdown".into()),
                            })
                            .await;

                        if result.is_err() {
                            let _ = api
                                .edit_message_text(&EditMessageTextParams {
                                    chat_id,
                                    message_id: mid,
                                    text: display_text.clone(),
                                    parse_mode: None,
                                })
                                .await;
                        }
                    } else {
                        // First delta: create the message with initial content
                        match api
                            .send_message(&SendMessageParams {
                                chat_id,
                                text: display_text.clone(),
                                parse_mode: None,
                            })
                            .await
                        {
                            Ok(msg) => {
                                message_id = Some(msg.message_id);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to send initial streaming message: {e}");
                            }
                        }
                    }

                    last_edited_text = display_text;
                    last_edit_time = tokio::time::Instant::now();
                }
                StreamEvent::Done { .. } => {
                    break;
                }
                _ => {}
            }
        }

        if full_text.is_empty() {
            return Ok(());
        }

        if full_text.len() <= MAX_MESSAGE_LEN {
            if let Some(mid) = message_id {
                // Final edit: remove cursor
                let result = api
                    .edit_message_text(&EditMessageTextParams {
                        chat_id,
                        message_id: mid,
                        text: full_text.clone(),
                        parse_mode: Some("Markdown".into()),
                    })
                    .await;

                if result.is_err() {
                    let _ = api
                        .edit_message_text(&EditMessageTextParams {
                            chat_id,
                            message_id: mid,
                            text: full_text,
                            parse_mode: None,
                        })
                        .await;
                }
            } else {
                // Never got to create a message (throttle delay > total stream time)
                send_with_markdown_fallback(&api, chat_id, &full_text).await?;
            }
        } else {
            // Text exceeded limit — split and send
            let chunks = split_message(&full_text, MAX_MESSAGE_LEN);

            // Use first chunk to edit or send
            if let Some(first) = chunks.first() {
                if let Some(mid) = message_id {
                    let result = api
                        .edit_message_text(&EditMessageTextParams {
                            chat_id,
                            message_id: mid,
                            text: first.clone(),
                            parse_mode: Some("Markdown".into()),
                        })
                        .await;

                    if result.is_err() {
                        let _ = api
                            .edit_message_text(&EditMessageTextParams {
                                chat_id,
                                message_id: mid,
                                text: first.clone(),
                                parse_mode: None,
                            })
                            .await;
                    }
                } else {
                    send_with_markdown_fallback(&api, chat_id, first).await?;
                }
            }

            for chunk in chunks.iter().skip(1) {
                send_with_markdown_fallback(&api, chat_id, chunk).await?;
            }
        }

        Ok(())
    }
}

/// Decode base64 and send an attachment via the appropriate Telegram API method.
async fn send_attachment(
    api: &TelegramApi,
    chat_id: i64,
    attachment: &Attachment,
) -> anyhow::Result<()> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    match attachment {
        Attachment::Image { base64, mime_type } => {
            let bytes = engine
                .decode(base64)
                .context("failed to decode image base64")?;
            let ext = mime_extension(mime_type);
            api.send_photo(chat_id, bytes, &format!("image.{ext}"), mime_type, None)
                .await?;
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
            let name = file_name.as_deref().unwrap_or(&fallback);
            api.send_document(chat_id, bytes, name, mime_type, None)
                .await?;
        }
        Attachment::Audio { base64, mime_type } => {
            let bytes = engine
                .decode(base64)
                .context("failed to decode audio base64")?;
            api.send_voice(chat_id, bytes, mime_type, None).await?;
        }
    }
    Ok(())
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

/// Send a message with Markdown, falling back to plain text on error.
async fn send_with_markdown_fallback(
    api: &TelegramApi,
    chat_id: i64,
    text: &str,
) -> anyhow::Result<()> {
    let result = api
        .send_message(&SendMessageParams {
            chat_id,
            text: text.to_string(),
            parse_mode: Some("Markdown".into()),
        })
        .await;

    if result.is_err() {
        api.send_message(&SendMessageParams {
            chat_id,
            text: text.to_string(),
            parse_mode: None,
        })
        .await?;
    }
    Ok(())
}

/// Factory function: create a `TelegramChannel` from a channel config.
///
/// Expects `config.settings["bot_token"]` to be a string.
pub fn create_telegram_channel(
    id: String,
    config: &ChannelConfig,
) -> anyhow::Result<Arc<dyn aobot_gateway::channel::ChannelPlugin>> {
    let bot_token = config
        .settings
        .get("bot_token")
        .and_then(|v| v.as_str())
        .context("Telegram channel requires settings.bot_token (string)")?;

    let channel = TelegramChannel::new(id, bot_token.to_string(), config.agent.clone());
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
        // No spaces, newlines, or paragraph breaks — must hard cut
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
        // With a small limit, the code block should be closed/reopened across chunks
        let chunks = split_message(&text, 60);
        assert!(chunks.len() >= 2);
        // First chunk that starts a code block should close it if split
        // Verify no chunk has unmatched code fences
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
        // All content should be present (minus possible stripped newlines between chunks)
        assert!(joined.contains("Hello world"));
        assert!(joined.contains("Second paragraph"));
        assert!(joined.contains("Third line"));
    }
}
