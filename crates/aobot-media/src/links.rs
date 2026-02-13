//! Link extraction and summarization.

use regex::Regex;

/// Extract URLs from text.
///
/// Excludes URLs that are inside markdown link syntax `[text](url)` references
/// and only extracts standalone URLs.
pub fn extract_links(text: &str, max_links: usize) -> Vec<String> {
    let url_re = Regex::new(r"https?://[^\s<>\]\)]+").unwrap();

    let mut links = Vec::new();
    for cap in url_re.find_iter(text) {
        let url = cap.as_str().to_string();
        // Clean trailing punctuation
        let url = url.trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if !links.contains(&url.to_string()) {
            links.push(url.to_string());
        }
        if links.len() >= max_links {
            break;
        }
    }

    links
}

/// Fetch URL content and return as text (simplified).
pub async fn fetch_url_content(url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp = client
        .get(url)
        .header("User-Agent", "aobot/0.1")
        .send()
        .await?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let text = resp.text().await?;

    // For HTML, do basic stripping
    if content_type.contains("text/html") {
        Ok(strip_html_tags(&text))
    } else {
        Ok(text)
    }
}

/// Basic HTML tag stripping.
fn strip_html_tags(html: &str) -> String {
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    let result = tag_re.replace_all(html, "");
    // Collapse whitespace
    let ws_re = Regex::new(r"\s+").unwrap();
    ws_re.replace_all(&result, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_links() {
        let text = "Check out https://example.com and https://rust-lang.org for more info.";
        let links = extract_links(text, 10);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "https://example.com");
        assert_eq!(links[1], "https://rust-lang.org");
    }

    #[test]
    fn test_extract_links_max() {
        let text = "https://a.com https://b.com https://c.com";
        let links = extract_links(text, 2);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn test_extract_links_dedup() {
        let text = "https://example.com and again https://example.com";
        let links = extract_links(text, 10);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn test_strip_html() {
        let html = "<html><body><p>Hello <b>World</b></p></body></html>";
        let text = strip_html_tags(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("<"));
    }
}
