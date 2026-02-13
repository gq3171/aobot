//! aobot-memory: Vector storage, embedding, chunking, and hybrid search for RAG.
//!
//! Provides:
//! - SQLite-backed vector storage with FTS5 full-text search
//! - Multiple embedding provider support (OpenAI, Gemini, Voyage)
//! - Markdown-aware chunking with overlap
//! - Incremental file sync (hash-based change detection)
//! - Hybrid search (vector similarity + keyword matching)

pub mod chunking;
pub mod embeddings;
pub mod manager;
pub mod search;
pub mod store;
pub mod sync;
