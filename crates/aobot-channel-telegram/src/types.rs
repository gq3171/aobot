//! Telegram Bot API types (minimal subset).

use serde::{Deserialize, Serialize};

/// Generic Telegram API response wrapper.
#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct ApiResponse<T> {
    pub ok: bool,
    #[serde(default)]
    pub result: Option<T>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Bot identity returned by `getMe`.
#[derive(Debug, Deserialize)]
pub struct BotInfo {
    pub id: i64,
    pub is_bot: bool,
    pub first_name: String,
    #[serde(default)]
    pub username: Option<String>,
}

/// A Telegram Update object.
#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<TgMessage>,
}

/// A Telegram message.
#[derive(Debug, Deserialize)]
pub struct TgMessage {
    pub message_id: i64,
    pub date: i64,
    #[serde(default)]
    pub from: Option<User>,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub entities: Vec<MessageEntity>,
}

/// A message entity (bold, command, mention, etc.).
#[derive(Debug, Deserialize)]
pub struct MessageEntity {
    #[serde(rename = "type")]
    pub entity_type: String,
    pub offset: i64,
    pub length: i64,
}

/// A Telegram user.
#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    pub is_bot: bool,
    pub first_name: String,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
}

impl User {
    /// Build a display name from first + last name.
    pub fn display_name(&self) -> String {
        match &self.last_name {
            Some(last) => format!("{} {last}", self.first_name),
            None => self.first_name.clone(),
        }
    }
}

/// A Telegram chat.
#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
}

/// Parameters for `getUpdates`.
#[derive(Debug, Serialize)]
pub struct GetUpdatesParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_updates: Option<Vec<String>>,
}

/// Parameters for `sendChatAction`.
#[derive(Debug, Serialize)]
pub struct SendChatActionParams {
    pub chat_id: i64,
    pub action: String,
}

/// Parameters for `sendMessage`.
#[derive(Debug, Serialize)]
pub struct SendMessageParams {
    pub chat_id: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<String>,
}

/// Parameters for `editMessageText`.
#[derive(Debug, Serialize)]
pub struct EditMessageTextParams {
    pub chat_id: i64,
    pub message_id: i64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<String>,
}

/// A bot command for `setMyCommands`.
#[derive(Debug, Serialize)]
pub struct BotCommand {
    pub command: String,
    pub description: String,
}

/// Parameters for `setMyCommands`.
#[derive(Debug, Serialize)]
pub struct SetMyCommandsParams {
    pub commands: Vec<BotCommand>,
}

/// Menu button shown in the input field.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MenuButton {
    Commands,
    Default,
}

/// Parameters for `setChatMenuButton`.
#[derive(Debug, Serialize)]
pub struct SetChatMenuButtonParams {
    pub menu_button: MenuButton,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_response_ok() {
        let json = r#"{"ok":true,"result":{"id":123,"is_bot":true,"first_name":"TestBot"}}"#;
        let resp: ApiResponse<BotInfo> = serde_json::from_str(json).unwrap();
        assert!(resp.ok);
        let bot = resp.result.unwrap();
        assert_eq!(bot.id, 123);
        assert!(bot.is_bot);
    }

    #[test]
    fn test_api_response_error() {
        let json = r#"{"ok":false,"description":"Unauthorized"}"#;
        let resp: ApiResponse<BotInfo> = serde_json::from_str(json).unwrap();
        assert!(!resp.ok);
        assert!(resp.result.is_none());
        assert_eq!(resp.description.as_deref(), Some("Unauthorized"));
    }

    #[test]
    fn test_update_with_message() {
        let json = r#"{
            "update_id": 100,
            "message": {
                "message_id": 1,
                "date": 1700000000,
                "from": {"id": 42, "is_bot": false, "first_name": "Alice", "last_name": "Smith"},
                "chat": {"id": 42, "type": "private"},
                "text": "Hello bot"
            }
        }"#;
        let update: Update = serde_json::from_str(json).unwrap();
        assert_eq!(update.update_id, 100);
        let msg = update.message.unwrap();
        assert_eq!(msg.text.as_deref(), Some("Hello bot"));
        assert_eq!(msg.from.unwrap().display_name(), "Alice Smith");
    }

    #[test]
    fn test_update_without_message() {
        let json = r#"{"update_id": 200}"#;
        let update: Update = serde_json::from_str(json).unwrap();
        assert_eq!(update.update_id, 200);
        assert!(update.message.is_none());
    }

    #[test]
    fn test_send_message_params_serialize() {
        let params = SendMessageParams {
            chat_id: 42,
            text: "Hello".into(),
            parse_mode: Some("MarkdownV2".into()),
        };
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["chat_id"], 42);
        assert_eq!(json["parse_mode"], "MarkdownV2");
    }

    #[test]
    fn test_send_message_params_skip_none() {
        let params = SendMessageParams {
            chat_id: 42,
            text: "Hello".into(),
            parse_mode: None,
        };
        let json = serde_json::to_value(&params).unwrap();
        assert!(!json.as_object().unwrap().contains_key("parse_mode"));
    }

    #[test]
    fn test_user_display_name_no_last() {
        let user = User {
            id: 1,
            is_bot: false,
            first_name: "Bob".into(),
            last_name: None,
            username: None,
        };
        assert_eq!(user.display_name(), "Bob");
    }
}
