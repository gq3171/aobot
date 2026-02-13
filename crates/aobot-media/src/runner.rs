//! Media processing pipeline runner.

use crate::types::{
    AudioRequest, AudioResult, ImageRequest, ImageResult, MediaCapability, MediaProvider,
};

/// Media processing runner that routes requests to appropriate providers.
pub struct MediaRunner {
    providers: Vec<Box<dyn MediaProvider>>,
}

impl MediaRunner {
    /// Create a new runner with the given providers.
    pub fn new(providers: Vec<Box<dyn MediaProvider>>) -> Self {
        Self { providers }
    }

    /// Find a provider that supports audio transcription.
    fn audio_provider(&self) -> Option<&dyn MediaProvider> {
        self.providers
            .iter()
            .find(|p| p.capabilities().contains(&MediaCapability::Audio))
            .map(|p| p.as_ref())
    }

    /// Find a provider that supports image description.
    fn image_provider(&self) -> Option<&dyn MediaProvider> {
        self.providers
            .iter()
            .find(|p| p.capabilities().contains(&MediaCapability::Image))
            .map(|p| p.as_ref())
    }

    /// Transcribe audio using the first available audio provider.
    pub async fn transcribe_audio(&self, req: AudioRequest) -> anyhow::Result<AudioResult> {
        let provider = self
            .audio_provider()
            .ok_or_else(|| anyhow::anyhow!("No audio transcription provider available"))?;
        provider.transcribe_audio(req).await
    }

    /// Describe an image using the first available image provider.
    pub async fn describe_image(&self, req: ImageRequest) -> anyhow::Result<ImageResult> {
        let provider = self
            .image_provider()
            .ok_or_else(|| anyhow::anyhow!("No image description provider available"))?;
        provider.describe_image(req).await
    }

    /// Process an attachment based on its MIME type.
    pub async fn process_attachment(
        &self,
        data: Vec<u8>,
        mime_type: &str,
    ) -> anyhow::Result<String> {
        if mime_type.starts_with("audio/") {
            let result = self
                .transcribe_audio(AudioRequest {
                    data,
                    mime_type: mime_type.to_string(),
                    language: None,
                })
                .await?;
            Ok(format!("[Audio transcription]: {}", result.text))
        } else if mime_type.starts_with("image/") {
            let result = self
                .describe_image(ImageRequest {
                    data,
                    mime_type: mime_type.to_string(),
                    prompt: "Describe this image in detail.".to_string(),
                })
                .await?;
            Ok(format!("[Image description]: {}", result.description))
        } else {
            Err(anyhow::anyhow!("Unsupported media type: {mime_type}"))
        }
    }
}
