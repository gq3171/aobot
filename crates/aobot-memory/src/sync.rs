//! Incremental file sync for memory indexing.

use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

use crate::chunking::chunk_markdown;
use crate::embeddings::EmbeddingProvider;
use crate::store::{FileRecord, MemoryStore, StoredChunk};

/// Result of a sync operation.
#[derive(Debug, Default)]
pub struct SyncResult {
    pub files_scanned: usize,
    pub files_updated: usize,
    pub chunks_added: usize,
    pub chunks_removed: usize,
}

/// Sync memory files from the given directories.
///
/// Only re-indexes files whose content hash has changed.
pub async fn sync_memory_files(
    store: &MemoryStore,
    provider: &dyn EmbeddingProvider,
    dirs: &[PathBuf],
    chunk_max_lines: usize,
    chunk_overlap: usize,
    force: bool,
) -> Result<SyncResult> {
    let mut result = SyncResult::default();

    for dir in dirs {
        let files = collect_memory_files(dir)?;
        result.files_scanned += files.len();

        for file_path in &files {
            let path_str = file_path.to_string_lossy().to_string();
            let content = tokio::fs::read_to_string(file_path).await?;
            let hash = hash_content(&content);

            // Check if file has changed
            if !force {
                if let Some(existing) = store.get_file(&path_str)? {
                    if existing.hash == hash {
                        debug!(path = %path_str, "File unchanged, skipping");
                        continue;
                    }
                }
            }

            info!(path = %path_str, "Syncing file");

            // Delete old chunks
            let removed = store.delete_chunks_for_path(&path_str)?;
            result.chunks_removed += removed;

            // Chunk the content
            let chunks = chunk_markdown(&content, chunk_max_lines, chunk_overlap);

            // Embed all chunks in batch
            let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
            let embeddings = if !texts.is_empty() {
                provider.embed_batch(&texts).await?
            } else {
                vec![]
            };

            // Store chunks
            let now = chrono::Utc::now().timestamp();
            for (i, (chunk, embedding)) in chunks.iter().zip(embeddings.iter()).enumerate() {
                let stored = StoredChunk {
                    id: format!("{path_str}::{i}"),
                    path: path_str.clone(),
                    source: "local".to_string(),
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    hash: chunk.hash.clone(),
                    model: provider.model().to_string(),
                    text: chunk.text.clone(),
                    embedding: embedding.clone(),
                    updated_at: now,
                };
                store.upsert_chunk(&stored)?;
                result.chunks_added += 1;
            }

            // Update file record
            let metadata = tokio::fs::metadata(file_path).await.ok();
            store.upsert_file(&FileRecord {
                path: path_str,
                source: "local".to_string(),
                hash,
                mtime: metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64),
                size: metadata.map(|m| m.len() as i64),
            })?;

            result.files_updated += 1;
        }
    }

    Ok(result)
}

/// Collect markdown files from a path (file or directory).
fn collect_memory_files(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    if !path.is_dir() {
        return Ok(vec![]);
    }

    let mut files = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_file() {
            if let Some(ext) = entry_path.extension() {
                if ext == "md" || ext == "txt" || ext == "markdown" {
                    files.push(entry_path);
                }
            }
        } else if entry_path.is_dir() {
            files.extend(collect_memory_files(&entry_path)?);
        }
    }

    Ok(files)
}

fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}
