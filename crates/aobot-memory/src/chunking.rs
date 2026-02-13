//! Markdown-aware text chunking for memory indexing.

use sha2::{Digest, Sha256};

/// A chunk of text from a source file.
#[derive(Debug, Clone)]
pub struct MemoryChunk {
    /// The chunk text.
    pub text: String,
    /// Start line number (1-indexed).
    pub start_line: usize,
    /// End line number (1-indexed, inclusive).
    pub end_line: usize,
    /// SHA-256 hash of the text.
    pub hash: String,
}

/// Split markdown content into chunks, respecting heading boundaries.
///
/// Chunks are split at heading boundaries (# lines) and when they exceed
/// `max_chunk_lines`. Overlap lines are prepended from the previous chunk
/// for context continuity.
pub fn chunk_markdown(
    content: &str,
    max_chunk_lines: usize,
    overlap_lines: usize,
) -> Vec<MemoryChunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_start = 1usize;

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;

        // Check if this is a heading (# ...) and we have accumulated content
        let is_heading = line.starts_with('#');
        if is_heading && !current_lines.is_empty() {
            // Emit current chunk
            let text = current_lines.join("\n");
            let hash = hash_text(&text);
            chunks.push(MemoryChunk {
                text,
                start_line: current_start,
                end_line: line_num - 1,
                hash,
            });

            // Start new chunk with overlap
            let overlap_start = current_lines.len().saturating_sub(overlap_lines);
            current_lines = current_lines[overlap_start..].to_vec();
            current_start = line_num.saturating_sub(current_lines.len());
        }

        current_lines.push(line);

        // Split if we've exceeded max lines
        if current_lines.len() >= max_chunk_lines {
            let text = current_lines.join("\n");
            let hash = hash_text(&text);
            chunks.push(MemoryChunk {
                text,
                start_line: current_start,
                end_line: line_num,
                hash,
            });

            let overlap_start = current_lines.len().saturating_sub(overlap_lines);
            current_lines = current_lines[overlap_start..].to_vec();
            current_start = line_num + 1 - current_lines.len();
        }
    }

    // Emit remaining lines
    if !current_lines.is_empty() {
        let text = current_lines.join("\n");
        let hash = hash_text(&text);
        chunks.push(MemoryChunk {
            text,
            start_line: current_start,
            end_line: lines.len(),
            hash,
        });
    }

    chunks
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_chunking() {
        let content = "# Title\n\nLine 1\nLine 2\n\n# Section 2\n\nLine 3\nLine 4";
        let chunks = chunk_markdown(content, 100, 0);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].text.contains("Title"));
        assert!(chunks[1].text.contains("Section 2"));
    }

    #[test]
    fn test_max_lines_split() {
        let lines: Vec<String> = (1..=20).map(|i| format!("Line {i}")).collect();
        let content = lines.join("\n");
        let chunks = chunk_markdown(&content, 5, 1);
        assert!(chunks.len() >= 4);
    }

    #[test]
    fn test_empty_content() {
        let chunks = chunk_markdown("", 100, 0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_hash_deterministic() {
        let chunks1 = chunk_markdown("Hello\nWorld", 100, 0);
        let chunks2 = chunk_markdown("Hello\nWorld", 100, 0);
        assert_eq!(chunks1[0].hash, chunks2[0].hash);
    }
}
