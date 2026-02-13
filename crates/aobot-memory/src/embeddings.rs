//! Embedding providers for vector storage.

use anyhow::Result;
use async_trait::async_trait;

/// Trait for embedding text into vectors.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Provider identifier.
    fn id(&self) -> &str;
    /// Model name.
    fn model(&self) -> &str;
    /// Vector dimensions.
    fn dimensions(&self) -> usize;
    /// Embed a single query.
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>>;
    /// Embed a batch of texts.
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// OpenAI embedding provider.
pub struct OpenAiEmbedding {
    api_key: String,
    model: String,
    dimensions: usize,
    client: reqwest::Client,
}

impl OpenAiEmbedding {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_model(api_key: String, model: String, dimensions: usize) -> Self {
        Self {
            api_key,
            model,
            dimensions,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbedding {
    fn id(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        let batch = self.embed_batch(&[text.to_string()]).await?;
        batch
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });

        let resp = self
            .client
            .post("https://api.openai.com/v1/embeddings")
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
            return Err(anyhow::anyhow!("OpenAI embedding error: {msg}"));
        }

        let data = json
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response format"))?;

        let mut embeddings = Vec::with_capacity(texts.len());
        for item in data {
            let embedding: Vec<f32> = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing embedding array"))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            embeddings.push(embedding);
        }

        Ok(embeddings)
    }
}

/// Auto-select an embedding provider based on available API keys.
pub fn auto_select_provider() -> Option<Box<dyn EmbeddingProvider>> {
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        return Some(Box::new(OpenAiEmbedding::new(key)));
    }
    // Add more providers here as they are implemented
    None
}
