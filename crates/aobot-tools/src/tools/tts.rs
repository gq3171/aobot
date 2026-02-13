//! `tts` tool — text-to-speech synthesis via API providers.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::GatewayToolContext;

pub struct TtsTool {
    _ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl TtsTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "tts".to_string(),
            description: "Convert text to speech audio using a TTS provider (e.g. OpenAI TTS)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to convert to speech."
                    },
                    "voice": {
                        "type": "string",
                        "description": "Voice ID (default: 'alloy'). Options: alloy, echo, fable, onyx, nova, shimmer."
                    },
                    "model": {
                        "type": "string",
                        "description": "TTS model (default: 'tts-1'). Options: tts-1, tts-1-hd."
                    }
                },
                "required": ["text"]
            }),
        };
        Self {
            _ctx: ctx,
            definition,
        }
    }
}

#[async_trait]
impl AgentTool for TtsTool {
    fn name(&self) -> &str {
        "tts"
    }

    fn label(&self) -> &str {
        "TTS"
    }

    fn definition(&self) -> &Tool {
        &self.definition
    }

    async fn execute(
        &self,
        _tool_call_id: &str,
        params: Value,
        _cancel: CancellationToken,
        _on_update: Option<Box<dyn Fn(AgentToolResult) + Send + Sync>>,
    ) -> Result<AgentToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: text")?;
        let voice = params
            .get("voice")
            .and_then(|v| v.as_str())
            .unwrap_or("alloy");
        let model = params
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("tts-1");

        // Get API key from environment
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY environment variable not set for TTS")?;

        let client = reqwest::Client::new();
        let response = client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&json!({
                "model": model,
                "input": text,
                "voice": voice,
                "response_format": "mp3"
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("TTS API error ({status}): {body}").into());
        }

        let audio_bytes = response.bytes().await?;
        let audio_base64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &audio_bytes);

        let text = format!(
            "Generated speech audio ({} bytes, mp3). Voice: {voice}, Model: {model}.",
            audio_bytes.len()
        );

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text,
                text_signature: None,
            })],
            details: Some(json!({
                "audio_base64": audio_base64,
                "mime_type": "audio/mpeg",
                "voice": voice,
                "model": model,
                "size_bytes": audio_bytes.len(),
            })),
        })
    }
}
