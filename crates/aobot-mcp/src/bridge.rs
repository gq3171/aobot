//! Bridge between MCP Tool types and pi-coding-agent Extension ToolDefinition.

use pi_coding_agent::extensions::types::ToolDefinition;
use rmcp::model::RawContent;
use serde_json::{Value, json};

/// Convert an MCP Tool to a pi-coding-agent ToolDefinition.
pub fn mcp_tool_to_extension_tool(tool: &rmcp::model::Tool) -> ToolDefinition {
    // Build parameters JSON schema from MCP input_schema
    let parameters: Value = serde_json::to_value(&*tool.input_schema)
        .unwrap_or_else(|_| json!({"type": "object", "properties": {}}));

    ToolDefinition {
        name: tool.name.to_string(),
        label: tool
            .title
            .as_deref()
            .unwrap_or(tool.name.as_ref())
            .to_string(),
        description: tool
            .description
            .as_deref()
            .unwrap_or("MCP tool")
            .to_string(),
        parameters,
    }
}

/// Convert an MCP CallToolResult content to a serde_json::Value.
pub fn mcp_result_to_value(result: &rmcp::model::CallToolResult) -> Value {
    let is_error = result.is_error.unwrap_or(false);

    // Prefer structured content if available
    if let Some(structured) = &result.structured_content {
        return json!({
            "content": structured,
            "isError": is_error,
        });
    }

    // Convert content blocks to text using the raw inner enum
    let text_parts: Vec<String> = result
        .content
        .iter()
        .filter_map(|content| match &content.raw {
            RawContent::Text(text_content) => Some(text_content.text.clone()),
            RawContent::Image(img) => Some(format!("[Image: {}]", img.mime_type)),
            RawContent::Resource(res) => {
                let uri = match &res.resource {
                    rmcp::model::ResourceContents::TextResourceContents { uri, text, .. } => {
                        return Some(format!("[Resource: {uri}]\n{text}"));
                    }
                    rmcp::model::ResourceContents::BlobResourceContents { uri, .. } => uri,
                };
                Some(format!("[Resource: {uri}]"))
            }
            RawContent::Audio(audio) => Some(format!("[Audio: {}]", audio.mime_type)),
            RawContent::ResourceLink(link) => Some(format!("[ResourceLink: {}]", link.uri)),
        })
        .collect();

    let text = text_parts.join("\n");

    json!({
        "content": text,
        "isError": is_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool as McpTool;
    use std::sync::Arc;

    #[test]
    fn test_mcp_tool_to_extension_tool() {
        let input_schema = serde_json::from_value::<serde_json::Map<String, Value>>(json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"}
            },
            "required": ["path"]
        }))
        .unwrap();

        let mcp_tool = McpTool {
            name: "read_file".into(),
            title: Some("Read File".into()),
            description: Some("Read a file from disk".into()),
            input_schema: Arc::new(input_schema),
            output_schema: None,
            annotations: None,
            execution: None,
            icons: None,
            meta: None,
        };

        let def = mcp_tool_to_extension_tool(&mcp_tool);
        assert_eq!(def.name, "read_file");
        assert_eq!(def.label, "Read File");
        assert_eq!(def.description, "Read a file from disk");
        assert!(def.parameters["properties"]["path"].is_object());
    }
}
