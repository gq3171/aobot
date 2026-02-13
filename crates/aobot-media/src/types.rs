//! Media types and provider traits.

use async_trait::async_trait;

/// Media capability categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaCapability {
    Audio,
    Image,
    Video,
}

/// Audio transcription request.
#[derive(Debug, Clone)]
pub struct AudioRequest {
    /// Audio data (raw bytes).
    pub data: Vec<u8>,
    /// MIME type (e.g. "audio/ogg", "audio/wav").
    pub mime_type: String,
    /// Optional language hint.
    pub language: Option<String>,
}

/// Audio transcription result.
#[derive(Debug, Clone)]
pub struct AudioResult {
    /// Transcribed text.
    pub text: String,
    /// Language detected.
    pub language: Option<String>,
    /// Duration in seconds.
    pub duration: Option<f64>,
}

/// Image description request.
#[derive(Debug, Clone)]
pub struct ImageRequest {
    /// Image data (raw bytes).
    pub data: Vec<u8>,
    /// MIME type.
    pub mime_type: String,
    /// Prompt for description.
    pub prompt: String,
}

/// Image description result.
#[derive(Debug, Clone)]
pub struct ImageResult {
    /// Generated description.
    pub description: String,
}

/// Trait for media processing providers.
#[async_trait]
pub trait MediaProvider: Send + Sync {
    /// Provider identifier.
    fn id(&self) -> &str;
    /// Supported capabilities.
    fn capabilities(&self) -> &[MediaCapability];
    /// Transcribe audio to text.
    async fn transcribe_audio(&self, req: AudioRequest) -> anyhow::Result<AudioResult>;
    /// Describe an image.
    async fn describe_image(&self, req: ImageRequest) -> anyhow::Result<ImageResult>;
}
