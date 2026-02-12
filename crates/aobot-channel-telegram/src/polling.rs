//! Telegram long-polling loop.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use aobot_types::InboundMessage;

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
                    let Some(text) = msg.text else {
                        continue;
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
