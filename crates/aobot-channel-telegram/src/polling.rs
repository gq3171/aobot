//! Telegram long-polling loop.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use base64::Engine;

use aobot_types::{Attachment, InboundMessage};

use crate::api::TelegramApi;
use crate::types::GetUpdatesParams;

/// Run the long-polling loop, converting Telegram updates to `InboundMessage`.
///
/// Exits when `cancel` is cancelled or the `sender` is closed.
pub async fn run_polling_loop(
    api: &TelegramApi,
    channel_id: String,
    agent: Option<String>,
    sender: mpsc::Sender<InboundMessage>,
    cancel: CancellationToken,
) {
    let mut offset: Option<i64> = None;
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    info!(channel_id, "Telegram polling loop started");

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let params = GetUpdatesParams {
            offset,
            timeout: Some(30),
            allowed_updates: Some(vec!["message".into()]),
        };

        let updates = tokio::select! {
            _ = cancel.cancelled() => break,
            result = api.get_updates(&params) => result,
        };

        match updates {
            Ok(updates) => {
                backoff = Duration::from_secs(1);

                for update in updates {
                    offset = Some(update.update_id + 1);

                    let Some(msg) = update.message else {
                        continue;
                    };

                    // Determine text content: prefer text, fall back to caption for media messages
                    let text = msg.text.clone().or_else(|| msg.caption.clone());

                    // Build attachments from photo/document/voice
                    let mut attachments: Vec<Attachment> = Vec::new();

                    // Handle photo messages (pick largest resolution)
                    if let Some(ref photos) = msg.photo {
                        if let Some(largest) = photos.iter().max_by_key(|p| p.width * p.height) {
                            match download_as_attachment(api, &largest.file_id, "image/jpeg").await {
                                Ok(att) => attachments.push(att),
                                Err(e) => warn!(channel_id, "Failed to download photo: {e}"),
                            }
                        }
                    }

                    // Handle document messages
                    if let Some(ref doc) = msg.document {
                        let mime = doc.mime_type.as_deref().unwrap_or("application/octet-stream");
                        match download_as_attachment(api, &doc.file_id, mime).await {
                            Ok(att) => {
                                // Convert to Document variant with file_name
                                if let Attachment::Image { base64, mime_type } = att {
                                    attachments.push(Attachment::Document {
                                        base64,
                                        mime_type,
                                        file_name: doc.file_name.clone(),
                                    });
                                }
                            }
                            Err(e) => warn!(channel_id, "Failed to download document: {e}"),
                        }
                    }

                    // Handle voice messages
                    if let Some(ref voice) = msg.voice {
                        let mime = voice.mime_type.as_deref().unwrap_or("audio/ogg");
                        match download_as_attachment(api, &voice.file_id, mime).await {
                            Ok(att) => {
                                if let Attachment::Image { base64, mime_type } = att {
                                    attachments.push(Attachment::Audio { base64, mime_type });
                                }
                            }
                            Err(e) => warn!(channel_id, "Failed to download voice: {e}"),
                        }
                    }

                    // Skip messages with no text and no attachments
                    let text = match text {
                        Some(t) => t,
                        None if !attachments.is_empty() => String::new(),
                        None => continue,
                    };

                    let sender_id = msg
                        .from
                        .as_ref()
                        .map(|u| u.id.to_string())
                        .unwrap_or_else(|| msg.chat.id.to_string());

                    let sender_name = msg.from.as_ref().map(|u| u.display_name());

                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "chat_id".into(),
                        serde_json::Value::Number(msg.chat.id.into()),
                    );
                    metadata.insert(
                        "message_id".into(),
                        serde_json::Value::Number(msg.message_id.into()),
                    );

                    // Detect bot commands (entity type "bot_command" at offset 0)
                    let is_command = msg.entities.iter().any(|e| {
                        e.entity_type == "bot_command" && e.offset == 0
                    });
                    if is_command {
                        // Extract command name (e.g. "/new" → "new", "/help@botname" → "help")
                        let cmd = text
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .trim_start_matches('/')
                            .split('@')
                            .next()
                            .unwrap_or("");
                        metadata.insert(
                            "command".into(),
                            serde_json::Value::String(cmd.to_string()),
                        );
                    }

                    let inbound = InboundMessage {
                        channel_type: "telegram".into(),
                        channel_id: channel_id.clone(),
                        sender_id,
                        sender_name,
                        text,
                        agent: agent.clone(),
                        session_key: None,
                        metadata,
                        attachments,
                        timestamp: msg.date * 1000,
                    };

                    debug!(
                        channel_id,
                        update_id = update.update_id,
                        "Forwarding Telegram message"
                    );

                    if sender.send(inbound).await.is_err() {
                        info!(channel_id, "Inbound channel closed, stopping polling");
                        return;
                    }
                }
            }
            Err(e) => {
                warn!(
                    channel_id,
                    backoff_secs = backoff.as_secs(),
                    "getUpdates error: {e}"
                );

                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(backoff) => {},
                }

                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }

    info!(channel_id, "Telegram polling loop stopped");
}

/// Download a Telegram file by file_id and return it as an Attachment::Image.
///
/// The caller is responsible for converting to the appropriate variant
/// (Document, Audio) based on the message type.
async fn download_as_attachment(
    api: &TelegramApi,
    file_id: &str,
    mime_type: &str,
) -> anyhow::Result<Attachment> {
    let file = api.get_file(file_id).await?;
    let file_path = file
        .file_path
        .ok_or_else(|| anyhow::anyhow!("No file_path in getFile response"))?;

    let bytes = api.download_file(&file_path).await?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    Ok(Attachment::Image {
        base64: b64,
        mime_type: mime_type.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_polling_loop_cancellation() {
        // Verify that the polling loop exits promptly when cancelled.
        // We use a fake API URL so the request will fail, but the cancel should win.
        let api = TelegramApi::new("fake_token");
        let (tx, _rx) = mpsc::channel(16);
        let cancel = CancellationToken::new();

        cancel.cancel();

        // Should return immediately since cancel is already set
        tokio::time::timeout(
            Duration::from_secs(2),
            run_polling_loop(&api, "test".into(), None, tx, cancel),
        )
        .await
        .expect("polling loop should exit promptly on cancel");
    }
}
