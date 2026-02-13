//! Audio transcription providers.

use async_trait::async_trait;
use reqwest::multipart;

use crate::types::{
    AudioRequest, AudioResult, ImageRequest, ImageResult, MediaCapability, MediaProvider,
};

/// OpenAI Whisper audio transcription provider.
pub struct WhisperProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl WhisperProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "whisper-1".to_string(),
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
impl MediaProvider for WhisperProvider {
    fn id(&self) -> &str {
        "openai-whisper"
    }

    fn capabilities(&self) -> &[MediaCapability] {
        &[MediaCapability::Audio]
    }

    async fn transcribe_audio(&self, req: AudioRequest) -> anyhow::Result<AudioResult> {
        let ext = match req.mime_type.as_str() {
            "audio/ogg" => "ogg",
            "audio/wav" | "audio/x-wav" => "wav",
            "audio/mpeg" | "audio/mp3" => "mp3",
            "audio/mp4" | "audio/m4a" => "m4a",
            "audio/webm" => "webm",
            "audio/flac" => "flac",
            _ => "ogg",
        };

        let part = multipart::Part::bytes(req.data)
            .file_name(format!("audio.{ext}"))
            .mime_str(&req.mime_type)?;

        let mut form = multipart::Form::new()
            .part("file", part)
            .text("model", self.model.clone());

        if let Some(lang) = req.language {
            form = form.text("language", lang);
        }

        let resp = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
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
            return Err(anyhow::anyhow!("Whisper transcription error: {msg}"));
        }

        let text = json
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        Ok(AudioResult {
            text,
            language: json
                .get("language")
                .and_then(|l| l.as_str())
                .map(String::from),
            duration: json.get("duration").and_then(|d| d.as_f64()),
        })
    }

    async fn describe_image(&self, _req: ImageRequest) -> anyhow::Result<ImageResult> {
        Err(anyhow::anyhow!(
            "WhisperProvider does not support image description"
        ))
    }
}
