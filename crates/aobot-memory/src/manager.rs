//! Memory manager — integrates storage, embedding, sync, and search.

use anyhow::Result;
use std::path::PathBuf;

use crate::embeddings::EmbeddingProvider;
use crate::search::{MemorySearchResult, hybrid_search};
use crate::store::MemoryStore;
use crate::sync::{SyncResult, sync_memory_files};

/// Unified memory manager.
pub struct MemoryManager {
    store: MemoryStore,
    provider: Box<dyn EmbeddingProvider>,
    dirs: Vec<PathBuf>,
    chunk_max_lines: usize,
    chunk_overlap: usize,
}

impl MemoryManager {
    /// Create a new memory manager.
    pub fn new(
        store: MemoryStore,
        provider: Box<dyn EmbeddingProvider>,
        dirs: Vec<PathBuf>,
        chunk_max_lines: usize,
        chunk_overlap: usize,
    ) -> Self {
        Self {
            store,
            provider,
            dirs,
            chunk_max_lines,
            chunk_overlap,
        }
    }

    /// Sync all configured memory directories.
    pub async fn sync(&self, force: bool) -> Result<SyncResult> {
        sync_memory_files(
            &self.store,
            self.provider.as_ref(),
            &self.dirs,
            self.chunk_max_lines,
            self.chunk_overlap,
            force,
        )
        .await
    }

    /// Search memory using hybrid search.
    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
        min_score: Option<f32>,
    ) -> Result<Vec<MemorySearchResult>> {
        hybrid_search(
            &self.store,
            self.provider.as_ref(),
            query,
            max_results,
            min_score,
        )
        .await
    }

    /// Get a chunk by ID.
    pub fn get_chunk(&self, id: &str) -> Result<Option<crate::store::StoredChunk>> {
        self.store.get_chunk(id)
    }

    /// Get the underlying store.
    pub fn store(&self) -> &MemoryStore {
        &self.store
    }
}
