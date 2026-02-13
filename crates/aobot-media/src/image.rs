//! Image description providers.

use async_trait::async_trait;

use crate::types::{
    AudioRequest, AudioResult, ImageRequest, ImageResult, MediaCapability, MediaProvider,
};

/// OpenAI vision model image description provider.
pub struct OpenAiVisionProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiVisionProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "gpt-4o-mini".to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl MediaProvider for OpenAiVisionProvider {
    fn id(&self) -> &str {
        "openai-vision"
    }

    fn capabilities(&self) -> &[MediaCapability] {
        &[MediaCapability::Image]
    }

    async fn transcribe_audio(&self, _req: AudioRequest) -> anyhow::Result<AudioResult> {
        Err(anyhow::anyhow!(
            "OpenAiVisionProvider does not support audio transcription"
        ))
    }

    async fn describe_image(&self, req: ImageRequest) -> anyhow::Result<ImageResult> {
        let base64_data =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &req.data);

        let body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": req.prompt
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", req.mime_type, base64_data)
                        }
                    }
                ]
            }],
            "max_tokens": 1024
        });

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let msg = json
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("Vision API error: {msg}"));
        }

        let description = json
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        Ok(ImageResult { description })
    }
}
