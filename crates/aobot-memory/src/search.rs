//! Hybrid search combining vector similarity and full-text search.

use anyhow::Result;
use std::collections::HashMap;

use crate::embeddings::EmbeddingProvider;
use crate::store::MemoryStore;

/// A search result from hybrid search.
#[derive(Debug, Clone)]
pub struct MemorySearchResult {
    pub chunk_id: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
    pub score: f32,
    pub source: SearchSource,
}

/// Source of the search result.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchSource {
    Vector,
    FullText,
    Hybrid,
}

/// Perform hybrid search: vector similarity + FTS5 keyword matching.
pub async fn hybrid_search(
    store: &MemoryStore,
    provider: &dyn EmbeddingProvider,
    query: &str,
    max_results: usize,
    min_score: Option<f32>,
) -> Result<Vec<MemorySearchResult>> {
    let min_score = min_score.unwrap_or(0.0);

    // Vector search
    let query_embedding = provider.embed_query(query).await?;
    let all_chunks = store.all_chunks()?;

    let mut vector_scores: HashMap<String, f32> = HashMap::new();
    for chunk in &all_chunks {
        let score = cosine_similarity(&query_embedding, &chunk.embedding);
        if score >= min_score {
            vector_scores.insert(chunk.id.clone(), score);
        }
    }

    // FTS search
    let fts_results = store.fts_search(query, max_results * 2)?;
    let mut fts_scores: HashMap<String, f32> = HashMap::new();
    for (id, rank) in &fts_results {
        // FTS5 rank is negative (lower = better), normalize to 0..1
        let score = 1.0 / (1.0 + rank.abs() as f32);
        fts_scores.insert(id.clone(), score);
    }

    // Merge results with reciprocal rank fusion
    let mut combined: HashMap<String, f32> = HashMap::new();
    for (id, score) in &vector_scores {
        *combined.entry(id.clone()).or_default() += score * 0.7; // 70% weight to vector
    }
    for (id, score) in &fts_scores {
        *combined.entry(id.clone()).or_default() += score * 0.3; // 30% weight to FTS
    }

    // Sort by combined score
    let mut ranked: Vec<(String, f32)> = combined.into_iter().collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(max_results);

    // Build results
    let mut results = Vec::new();
    for (id, score) in ranked {
        if let Some(chunk) = store.get_chunk(&id)? {
            let source = match (
                vector_scores.contains_key(&id),
                fts_scores.contains_key(&id),
            ) {
                (true, true) => SearchSource::Hybrid,
                (true, false) => SearchSource::Vector,
                (false, true) => SearchSource::FullText,
                (false, false) => continue,
            };
            results.push(MemorySearchResult {
                chunk_id: id,
                path: chunk.path,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                text: chunk.text,
                score,
                source,
            });
        }
    }

    Ok(results)
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }
}
