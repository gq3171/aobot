//! SQLite-backed vector storage for memory chunks.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

/// Vector store backed by SQLite with FTS5.
pub struct MemoryStore {
    conn: Mutex<Connection>,
}

/// A stored memory chunk.
#[derive(Debug, Clone)]
pub struct StoredChunk {
    pub id: String,
    pub path: String,
    pub source: String,
    pub start_line: usize,
    pub end_line: usize,
    pub hash: String,
    pub model: String,
    pub text: String,
    pub embedding: Vec<f32>,
    pub updated_at: i64,
}

/// File metadata record.
#[derive(Debug, Clone)]
pub struct FileRecord {
    pub path: String,
    pub source: String,
    pub hash: String,
    pub mtime: Option<i64>,
    pub size: Option<i64>,
}

impl MemoryStore {
    /// Open or create a memory store at the given path.
    pub fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS files (
                 path TEXT PRIMARY KEY,
                 source TEXT NOT NULL,
                 hash TEXT NOT NULL,
                 mtime INTEGER,
                 size INTEGER
             );

             CREATE TABLE IF NOT EXISTS chunks (
                 id TEXT PRIMARY KEY,
                 path TEXT NOT NULL,
                 source TEXT NOT NULL,
                 start_line INTEGER,
                 end_line INTEGER,
                 hash TEXT NOT NULL,
                 model TEXT NOT NULL,
                 text TEXT NOT NULL,
                 embedding BLOB NOT NULL,
                 updated_at INTEGER NOT NULL
             );

             CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                 text, id UNINDEXED, path UNINDEXED, source UNINDEXED
             );",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Insert or replace a file record.
    pub fn upsert_file(&self, file: &FileRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO files (path, source, hash, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![file.path, file.source, file.hash, file.mtime, file.size],
        )?;
        Ok(())
    }

    /// Get a file record by path.
    pub fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT path, source, hash, mtime, size FROM files WHERE path = ?1")?;
        let result = stmt.query_row(rusqlite::params![path], |row| {
            Ok(FileRecord {
                path: row.get(0)?,
                source: row.get(1)?,
                hash: row.get(2)?,
                mtime: row.get(3)?,
                size: row.get(4)?,
            })
        });
        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Insert or replace a chunk.
    pub fn upsert_chunk(&self, chunk: &StoredChunk) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let embedding_bytes = embedding_to_bytes(&chunk.embedding);
        conn.execute(
            "INSERT OR REPLACE INTO chunks (id, path, source, start_line, end_line, hash, model, text, embedding, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                chunk.id, chunk.path, chunk.source, chunk.start_line, chunk.end_line,
                chunk.hash, chunk.model, chunk.text, embedding_bytes, chunk.updated_at
            ],
        )?;
        // Also update FTS index
        conn.execute(
            "INSERT OR REPLACE INTO chunks_fts (rowid, text, id, path, source) VALUES (
                 (SELECT rowid FROM chunks WHERE id = ?1), ?2, ?1, ?3, ?4
             )",
            rusqlite::params![chunk.id, chunk.text, chunk.path, chunk.source],
        )?;
        Ok(())
    }

    /// Delete all chunks for a given path.
    pub fn delete_chunks_for_path(&self, path: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count = conn.execute(
            "DELETE FROM chunks WHERE path = ?1",
            rusqlite::params![path],
        )?;
        Ok(count)
    }

    /// Get all chunks (for vector search).
    pub fn all_chunks(&self) -> Result<Vec<StoredChunk>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, source, start_line, end_line, hash, model, text, embedding, updated_at FROM chunks",
        )?;
        let chunks = stmt
            .query_map([], |row| {
                let embedding_bytes: Vec<u8> = row.get(8)?;
                Ok(StoredChunk {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    source: row.get(2)?,
                    start_line: row.get::<_, i64>(3)? as usize,
                    end_line: row.get::<_, i64>(4)? as usize,
                    hash: row.get(5)?,
                    model: row.get(6)?,
                    text: row.get(7)?,
                    embedding: bytes_to_embedding(&embedding_bytes),
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(chunks)
    }

    /// Full-text search using FTS5.
    pub fn fts_search(&self, query: &str, limit: usize) -> Result<Vec<(String, f64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, rank FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let results = stmt
            .query_map(rusqlite::params![query, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    /// Get a chunk by ID.
    pub fn get_chunk(&self, id: &str) -> Result<Option<StoredChunk>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, path, source, start_line, end_line, hash, model, text, embedding, updated_at FROM chunks WHERE id = ?1",
        )?;
        let result = stmt.query_row(rusqlite::params![id], |row| {
            let embedding_bytes: Vec<u8> = row.get(8)?;
            Ok(StoredChunk {
                id: row.get(0)?,
                path: row.get(1)?,
                source: row.get(2)?,
                start_line: row.get::<_, i64>(3)? as usize,
                end_line: row.get::<_, i64>(4)? as usize,
                hash: row.get(5)?,
                model: row.get(6)?,
                text: row.get(7)?,
                embedding: bytes_to_embedding(&embedding_bytes),
                updated_at: row.get(9)?,
            })
        });
        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_store_open_and_upsert() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_memory.db");
        let store = MemoryStore::open(&db_path).unwrap();

        let file = FileRecord {
            path: "/test/file.md".to_string(),
            source: "local".to_string(),
            hash: "abc123".to_string(),
            mtime: Some(1000),
            size: Some(500),
        };
        store.upsert_file(&file).unwrap();

        let loaded = store.get_file("/test/file.md").unwrap().unwrap();
        assert_eq!(loaded.hash, "abc123");
    }

    #[test]
    fn test_chunk_upsert_and_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_memory.db");
        let store = MemoryStore::open(&db_path).unwrap();

        let chunk = StoredChunk {
            id: "chunk-1".to_string(),
            path: "/test/file.md".to_string(),
            source: "local".to_string(),
            start_line: 1,
            end_line: 10,
            hash: "def456".to_string(),
            model: "text-embedding-3-small".to_string(),
            text: "Hello world this is a test".to_string(),
            embedding: vec![0.1, 0.2, 0.3],
            updated_at: 1000,
        };
        store.upsert_chunk(&chunk).unwrap();

        let loaded = store.get_chunk("chunk-1").unwrap().unwrap();
        assert_eq!(loaded.text, "Hello world this is a test");
        assert_eq!(loaded.embedding, vec![0.1, 0.2, 0.3]);
    }
}
