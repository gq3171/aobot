//! Telegram Bot channel plugin for aobot.
//!
//! Uses Telegram Bot API with long-polling (no webhook required).
//!
//! # Configuration
//!
//! ```json5
//! channels: {
//!     "my-tg-bot": {
//!         channel_type: "telegram",
//!         enabled: true,
//!         agent: "default",
//!         settings: {
//!             bot_token: "123456:ABC-DEF...",
//!         },
//!     },
//! }
//! ```

pub mod api;
pub mod polling;
pub mod types;

use std::sync::Arc;

use anyhow::{bail, Context};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::info;

use std::collections::HashMap;

use aobot_types::{ChannelConfig, ChannelStatus, InboundMessage, OutboundMessage};

use api::TelegramApi;
use types::{SendChatActionParams, SendMessageParams};

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

        // Try Markdown first, fallback to plain text
        let result = api
            .send_message(&SendMessageParams {
                chat_id,
                text: message.text.clone(),
                parse_mode: Some("Markdown".into()),
            })
            .await;

        if result.is_err() {
            api.send_message(&SendMessageParams {
                chat_id,
                text: message.text,
                parse_mode: None,
            })
            .await?;
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
