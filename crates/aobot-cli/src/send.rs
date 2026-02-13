use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite;

/// Send a message to the Gateway via WebSocket JSON-RPC.
pub async fn run_send(
    message: String,
    url: String,
    session_key: Option<String>,
    agent: Option<String>,
    token: Option<String>,
) -> Result<()> {
    // Build WebSocket URL with optional token query param
    let ws_url = if let Some(ref token) = token {
        if url.contains('?') {
            format!("{url}&token={token}")
        } else {
            format!("{url}?token={token}")
        }
    } else {
        url.clone()
    };

    // Build request with optional auth header
    let mut request = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Host", extract_host(&ws_url).unwrap_or("localhost"));

    if let Some(ref token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let request = request
        .body(())
        .context("Failed to build WebSocket request")?;

    let (mut ws, _response) = tokio_tungstenite::connect_async(request)
        .await
        .context("Failed to connect to Gateway WebSocket")?;

    // Build JSON-RPC request
    let session_key = session_key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let mut params = json!({
        "message": message,
        "session_key": session_key,
    });
    if let Some(agent) = agent {
        params["agent"] = json!(agent);
    }

    let rpc_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "chat.send",
        "params": params,
    });

    // Send request
    ws.send(tungstenite::Message::Text(rpc_request.to_string().into()))
        .await
        .context("Failed to send message")?;

    // Wait for response
    while let Some(msg) = ws.next().await {
        match msg? {
            tungstenite::Message::Text(text) => {
                let response: serde_json::Value =
                    serde_json::from_str(&text).context("Failed to parse response")?;

                if let Some(error) = response.get("error") {
                    eprintln!(
                        "Error: {}",
                        error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error")
                    );
                    std::process::exit(1);
                }

                if let Some(result) = response.get("result") {
                    if let Some(reply) = result.get("response").and_then(|r| r.as_str()) {
                        println!("{reply}");
                    }
                    if let Some(key) = result.get("session_key").and_then(|k| k.as_str()) {
                        eprintln!("(session: {key})");
                    }
                }
                break;
            }
            tungstenite::Message::Close(_) => break,
            _ => {}
        }
    }

    // Close connection
    let _ = ws.close(None).await;

    Ok(())
}

/// Extract host from a URL string.
fn extract_host(url: &str) -> Option<&str> {
    let after_scheme = url
        .strip_prefix("ws://")
        .or_else(|| url.strip_prefix("wss://"))?;
    after_scheme.split('/').next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_host() {
        assert_eq!(
            extract_host("ws://127.0.0.1:3000/ws"),
            Some("127.0.0.1:3000")
        );
        assert_eq!(extract_host("wss://example.com/ws"), Some("example.com"));
        assert_eq!(extract_host("http://invalid"), None);
    }
}
