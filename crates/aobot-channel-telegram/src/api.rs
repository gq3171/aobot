//! Telegram Bot API HTTP client.

use std::time::Duration;

use anyhow::{bail, Context};
use reqwest::Client;

use crate::types::{
    ApiResponse, BotInfo, EditMessageTextParams, GetUpdatesParams, SendChatActionParams,
    SendMessageParams, SetChatMenuButtonParams, SetMyCommandsParams, TgFile, TgMessage, Update,
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

    /// Get file metadata by file_id (needed to download files).
    pub async fn get_file(&self, file_id: &str) -> anyhow::Result<TgFile> {
        let resp: ApiResponse<TgFile> = self
            .client
            .post(format!("{}/getFile", self.base_url))
            .json(&serde_json::json!({"file_id": file_id}))
            .send()
            .await
            .context("getFile request failed")?
            .json()
            .await
            .context("getFile response parse failed")?;

        if !resp.ok {
            bail!(
                "getFile failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        resp.result.context("getFile returned no result")
    }

    /// Download a file by its file_path (obtained from getFile).
    pub async fn download_file(&self, file_path: &str) -> anyhow::Result<Vec<u8>> {
        // Telegram file download URL format:
        // https://api.telegram.org/file/bot<token>/<file_path>
        let url = self
            .base_url
            .replace("/bot", "/file/bot");
        let download_url = format!("{url}/{file_path}");

        let bytes = self
            .client
            .get(&download_url)
            .send()
            .await
            .context("file download request failed")?
            .bytes()
            .await
            .context("file download body failed")?;

        Ok(bytes.to_vec())
    }

    /// Send a photo (binary data) with optional caption.
    pub async fn send_photo(
        &self,
        chat_id: i64,
        photo_bytes: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<TgMessage> {
        let photo_part = reqwest::multipart::Part::bytes(photo_bytes)
            .file_name(file_name.to_string())
            .mime_str(mime_type)
            .context("invalid mime type for photo")?;

        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", photo_part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp: ApiResponse<TgMessage> = self
            .client
            .post(format!("{}/sendPhoto", self.base_url))
            .multipart(form)
            .send()
            .await
            .context("sendPhoto request failed")?
            .json()
            .await
            .context("sendPhoto response parse failed")?;

        if !resp.ok {
            bail!(
                "sendPhoto failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        resp.result.context("sendPhoto returned no result")
    }

    /// Send a document (binary data) with optional caption.
    pub async fn send_document(
        &self,
        chat_id: i64,
        doc_bytes: Vec<u8>,
        file_name: &str,
        mime_type: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<TgMessage> {
        let doc_part = reqwest::multipart::Part::bytes(doc_bytes)
            .file_name(file_name.to_string())
            .mime_str(mime_type)
            .context("invalid mime type for document")?;

        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", doc_part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp: ApiResponse<TgMessage> = self
            .client
            .post(format!("{}/sendDocument", self.base_url))
            .multipart(form)
            .send()
            .await
            .context("sendDocument request failed")?
            .json()
            .await
            .context("sendDocument response parse failed")?;

        if !resp.ok {
            bail!(
                "sendDocument failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        resp.result.context("sendDocument returned no result")
    }

    /// Send an audio/voice file (binary data) with optional caption.
    pub async fn send_voice(
        &self,
        chat_id: i64,
        voice_bytes: Vec<u8>,
        mime_type: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<TgMessage> {
        let voice_part = reqwest::multipart::Part::bytes(voice_bytes)
            .file_name("voice.ogg".to_string())
            .mime_str(mime_type)
            .context("invalid mime type for voice")?;

        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", voice_part);

        if let Some(cap) = caption {
            form = form.text("caption", cap.to_string());
        }

        let resp: ApiResponse<TgMessage> = self
            .client
            .post(format!("{}/sendVoice", self.base_url))
            .multipart(form)
            .send()
            .await
            .context("sendVoice request failed")?
            .json()
            .await
            .context("sendVoice response parse failed")?;

        if !resp.ok {
            bail!(
                "sendVoice failed: {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }
        resp.result.context("sendVoice returned no result")
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
