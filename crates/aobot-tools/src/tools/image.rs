//! `image` tool — load and describe images using vision models.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use pi_agent_core::agent_types::{AgentTool, AgentToolResult};
use pi_agent_core::types::{ContentBlock, TextContent, Tool};

use crate::context::GatewayToolContext;

pub struct ImageTool {
    _ctx: Arc<GatewayToolContext>,
    definition: Tool,
}

impl ImageTool {
    pub fn new(ctx: Arc<GatewayToolContext>) -> Self {
        let definition = Tool {
            name: "image".to_string(),
            description:
                "Load and describe an image from a local file path or URL using a vision model."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Local file path or URL of the image."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Analysis prompt. Defaults to 'Describe the image.'"
                    }
                },
                "required": ["path"]
            }),
        };
        Self {
            _ctx: ctx,
            definition,
        }
    }
}

#[async_trait]
impl AgentTool for ImageTool {
    fn name(&self) -> &str {
        "image"
    }

    fn label(&self) -> &str {
        "Image"
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
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: path")?;
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe the image.");

        // Load image data
        let image_data = if path.starts_with("http://") || path.starts_with("https://") {
            // Fetch from URL
            let response = reqwest::get(path).await?;
            let bytes = response.bytes().await?;
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
        } else {
            // Load from local file
            let bytes = tokio::fs::read(path).await?;
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
        };

        // Determine mime type from extension
        let mime_type = if path.ends_with(".png") {
            "image/png"
        } else if path.ends_with(".gif") {
            "image/gif"
        } else if path.ends_with(".webp") {
            "image/webp"
        } else {
            "image/jpeg"
        };

        let result_text = format!(
            "Image loaded ({} bytes, {mime_type}). Prompt: {prompt}\n\n\
             [Image data has been loaded as base64. The vision model should process this image \
             with the given prompt to provide a description.]",
            image_data.len()
        );

        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: result_text,
                text_signature: None,
            })],
            details: Some(json!({
                "image_base64": image_data,
                "mime_type": mime_type,
                "prompt": prompt,
            })),
        })
    }
}
