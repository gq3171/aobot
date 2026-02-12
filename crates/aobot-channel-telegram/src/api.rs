//! Telegram Bot API HTTP client.

use std::time::Duration;

use anyhow::{bail, Context};
use reqwest::Client;

use crate::types::{
    ApiResponse, BotInfo, EditMessageTextParams, GetUpdatesParams, SendChatActionParams,
    SendMessageParams, SetChatMenuButtonParams, SetMyCommandsParams, TgMessage, Update,
};

/// HTTP client for the Telegram Bot API.
pub struct TelegramApi {
    client: Client,
    base_url: String,
}

impl TelegramApi {
    /// Create a new API client with the given bot token.
    pub fn new(bot_token: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: format!("https://api.telegram.org/bot{bot_token}"),
        }
    }

    /// Verify the bot token by calling `getMe`.
    pub async fn get_me(&self) -> anyhow::Result<BotInfo> {
        let resp: ApiResponse<BotInfo> = self
            .client
            .get(format!("{}/getMe", self.base_url))
            .send()
            .await
            .context("getMe request failed")?
            .json()
            .await
            .context("getMe response parse failed")?;

        if !resp.ok {
            bail!(
                "getMe failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        resp.result.context("getMe returned no result")
    }

    /// Long-poll for updates.
    pub async fn get_updates(&self, params: &GetUpdatesParams) -> anyhow::Result<Vec<Update>> {
        let resp: ApiResponse<Vec<Update>> = self
            .client
            .post(format!("{}/getUpdates", self.base_url))
            .json(params)
            .send()
            .await
            .context("getUpdates request failed")?
            .json()
            .await
            .context("getUpdates response parse failed")?;

        if !resp.ok {
            bail!(
                "getUpdates failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(resp.result.unwrap_or_default())
    }

    /// Send a chat action (e.g. "typing").
    pub async fn send_chat_action(&self, params: &SendChatActionParams) -> anyhow::Result<()> {
        let resp: ApiResponse<bool> = self
            .client
            .post(format!("{}/sendChatAction", self.base_url))
            .json(params)
            .send()
            .await
            .context("sendChatAction request failed")?
            .json()
            .await
            .context("sendChatAction response parse failed")?;

        if !resp.ok {
            bail!(
                "sendChatAction failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(())
    }

    /// Set the bot's menu button (shown left of the input field).
    pub async fn set_chat_menu_button(
        &self,
        params: &SetChatMenuButtonParams,
    ) -> anyhow::Result<()> {
        let resp: ApiResponse<bool> = self
            .client
            .post(format!("{}/setChatMenuButton", self.base_url))
            .json(params)
            .send()
            .await
            .context("setChatMenuButton request failed")?
            .json()
            .await
            .context("setChatMenuButton response parse failed")?;

        if !resp.ok {
            bail!(
                "setChatMenuButton failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(())
    }

    /// Register bot commands in the menu.
    pub async fn set_my_commands(&self, params: &SetMyCommandsParams) -> anyhow::Result<()> {
        let resp: ApiResponse<bool> = self
            .client
            .post(format!("{}/setMyCommands", self.base_url))
            .json(params)
            .send()
            .await
            .context("setMyCommands request failed")?
            .json()
            .await
            .context("setMyCommands response parse failed")?;

        if !resp.ok {
            bail!(
                "setMyCommands failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        Ok(())
    }

    /// Edit an existing message's text.
    pub async fn edit_message_text(
        &self,
        params: &EditMessageTextParams,
    ) -> anyhow::Result<TgMessage> {
        let resp: ApiResponse<TgMessage> = self
            .client
            .post(format!("{}/editMessageText", self.base_url))
            .json(params)
            .send()
            .await
            .context("editMessageText request failed")?
            .json()
            .await
            .context("editMessageText response parse failed")?;

        if !resp.ok {
            bail!(
                "editMessageText failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        resp.result.context("editMessageText returned no result")
    }

    /// Send a text message.
    pub async fn send_message(&self, params: &SendMessageParams) -> anyhow::Result<TgMessage> {
        let resp: ApiResponse<TgMessage> = self
            .client
            .post(format!("{}/sendMessage", self.base_url))
            .json(params)
            .send()
            .await
            .context("sendMessage request failed")?
            .json()
            .await
            .context("sendMessage response parse failed")?;

        if !resp.ok {
            bail!(
                "sendMessage failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        resp.result.context("sendMessage returned no result")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_url() {
        let api = TelegramApi::new("123:ABC");
        assert_eq!(api.base_url, "https://api.telegram.org/bot123:ABC");
    }
}
