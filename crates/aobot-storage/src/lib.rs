//! aobot-storage: SQLite-based persistence for gateway metadata.
//!
//! Stores session metadata and channel bindings in SQLite.
//! Message content is managed separately by pi-agent's JSONL persistence.

use std::path::Path;
use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Blocking task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, StorageError>;

/// Metadata about a gateway session, stored in SQLite.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMetadata {
    pub session_key: String,
    pub agent_name: String,
    pub model_id: String,
    pub created_at: i64,
    pub last_active_at: i64,
    pub message_count: i64,
    pub is_active: bool,
    /// pi-agent-rs session ID for JSONL history restoration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pi_session_id: Option<String>,
}

/// SQLite-based storage for aobot gateway metadata.
pub struct AoBotStorage {
    conn: Arc<Mutex<Connection>>,
}

impl AoBotStorage {
    /// Open (or create) the SQLite database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS gateway_sessions (
                session_key TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                model_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_active_at INTEGER NOT NULL,
                message_count INTEGER DEFAULT 0,
                is_active INTEGER DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS channel_bindings (
                channel_id TEXT NOT NULL,
                session_key TEXT NOT NULL,
                bound_at INTEGER NOT NULL,
                PRIMARY KEY (channel_id, session_key),
                FOREIGN KEY (session_key) REFERENCES gateway_sessions(session_key)
            );",
        )?;

        // Migration: add pi_session_id column (ignore error if already exists)
        let _ = conn.execute_batch("ALTER TABLE gateway_sessions ADD COLUMN pi_session_id TEXT;");

        tracing::info!("Storage opened: {}", path.display());

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS gateway_sessions (
                session_key TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                model_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_active_at INTEGER NOT NULL,
                message_count INTEGER DEFAULT 0,
                is_active INTEGER DEFAULT 1,
                pi_session_id TEXT
            );

            CREATE TABLE IF NOT EXISTS channel_bindings (
                channel_id TEXT NOT NULL,
                session_key TEXT NOT NULL,
                bound_at INTEGER NOT NULL,
                PRIMARY KEY (channel_id, session_key),
                FOREIGN KEY (session_key) REFERENCES gateway_sessions(session_key)
            );",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    // ─── Session Metadata ───────────────────────────────────

    /// Save or update session metadata.
    pub async fn save_session(&self, meta: &SessionMetadata) -> Result<()> {
        let conn = self.conn.clone();
        let meta = meta.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT INTO gateway_sessions
                    (session_key, agent_name, model_id, created_at, last_active_at, message_count, is_active, pi_session_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(session_key) DO UPDATE SET
                    agent_name = excluded.agent_name,
                    model_id = excluded.model_id,
                    last_active_at = excluded.last_active_at,
                    message_count = excluded.message_count,
                    is_active = excluded.is_active,
                    pi_session_id = COALESCE(excluded.pi_session_id, gateway_sessions.pi_session_id)",
                rusqlite::params![
                    meta.session_key,
                    meta.agent_name,
                    meta.model_id,
                    meta.created_at,
                    meta.last_active_at,
                    meta.message_count,
                    meta.is_active as i32,
                    meta.pi_session_id,
                ],
            )?;
            Ok(())
        })
        .await?
    }

    /// Get session metadata by key.
    pub async fn get_session(&self, key: &str) -> Result<Option<SessionMetadata>> {
        let conn = self.conn.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT session_key, agent_name, model_id, created_at, last_active_at, message_count, is_active, pi_session_id
                 FROM gateway_sessions WHERE session_key = ?1",
            )?;
            let result = stmt
                .query_row(rusqlite::params![key], |row| {
                    Ok(SessionMetadata {
                        session_key: row.get(0)?,
                        agent_name: row.get(1)?,
                        model_id: row.get(2)?,
                        created_at: row.get(3)?,
                        last_active_at: row.get(4)?,
                        message_count: row.get(5)?,
                        is_active: row.get::<_, i32>(6)? != 0,
                        pi_session_id: row.get(7)?,
                    })
                })
                .optional()?;
            Ok(result)
        })
        .await?
    }

    /// List all active sessions.
    pub async fn list_sessions(&self) -> Result<Vec<SessionMetadata>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare(
                "SELECT session_key, agent_name, model_id, created_at, last_active_at, message_count, is_active, pi_session_id
                 FROM gateway_sessions WHERE is_active = 1 ORDER BY last_active_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(SessionMetadata {
                        session_key: row.get(0)?,
                        agent_name: row.get(1)?,
                        model_id: row.get(2)?,
                        created_at: row.get(3)?,
                        last_active_at: row.get(4)?,
                        message_count: row.get(5)?,
                        is_active: row.get::<_, i32>(6)? != 0,
                        pi_session_id: row.get(7)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?
    }

    /// Update last_active_at timestamp and increment message_count.
    pub async fn update_session_activity(&self, key: &str) -> Result<()> {
        let conn = self.conn.clone();
        let key = key.to_string();
        let now = chrono::Utc::now().timestamp_millis();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "UPDATE gateway_sessions SET last_active_at = ?1, message_count = message_count + 1 WHERE session_key = ?2",
                rusqlite::params![now, key],
            )?;
            Ok(())
        })
        .await?
    }

    /// Save the pi-agent-rs session ID for a gateway session.
    pub async fn save_pi_session_id(&self, session_key: &str, pi_session_id: &str) -> Result<()> {
        let conn = self.conn.clone();
        let session_key = session_key.to_string();
        let pi_session_id = pi_session_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "UPDATE gateway_sessions SET pi_session_id = ?1 WHERE session_key = ?2",
                rusqlite::params![pi_session_id, session_key],
            )?;
            Ok(())
        })
        .await?
    }

    /// Soft-delete a session (mark as inactive).
    pub async fn delete_session(&self, key: &str) -> Result<()> {
        let conn = self.conn.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "UPDATE gateway_sessions SET is_active = 0 WHERE session_key = ?1",
                rusqlite::params![key],
            )?;
            Ok(())
        })
        .await?
    }

    // ─── Channel Bindings ───────────────────────────────────

    /// Bind a channel to a session.
    pub async fn bind_channel(&self, channel_id: &str, session_key: &str) -> Result<()> {
        let conn = self.conn.clone();
        let channel_id = channel_id.to_string();
        let session_key = session_key.to_string();
        let now = chrono::Utc::now().timestamp_millis();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO channel_bindings (channel_id, session_key, bound_at)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![channel_id, session_key, now],
            )?;
            Ok(())
        })
        .await?
    }

    /// Get the session key bound to a channel.
    pub async fn get_channel_session(&self, channel_id: &str) -> Result<Option<String>> {
        let conn = self.conn.clone();
        let channel_id = channel_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let result = conn
                .query_row(
                    "SELECT session_key FROM channel_bindings WHERE channel_id = ?1",
                    rusqlite::params![channel_id],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(result)
        })
        .await?
    }

    /// Unbind a channel from its session.
    pub async fn unbind_channel(&self, channel_id: &str) -> Result<()> {
        let conn = self.conn.clone();
        let channel_id = channel_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "DELETE FROM channel_bindings WHERE channel_id = ?1",
                rusqlite::params![channel_id],
            )?;
            Ok(())
        })
        .await?
    }
}

// We need `optional()` on Statement results
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_save_and_get_session() {
        let storage = AoBotStorage::open_in_memory().unwrap();
        let meta = SessionMetadata {
            session_key: "sess-1".into(),
            agent_name: "default".into(),
            model_id: "anthropic/claude-sonnet-4".into(),
            created_at: 1700000000000,
            last_active_at: 1700000000000,
            message_count: 0,
            is_active: true,
            pi_session_id: None,
        };
        storage.save_session(&meta).await.unwrap();

        let loaded = storage.get_session("sess-1").await.unwrap().unwrap();
        assert_eq!(loaded.session_key, "sess-1");
        assert_eq!(loaded.agent_name, "default");
        assert_eq!(loaded.model_id, "anthropic/claude-sonnet-4");
        assert!(loaded.is_active);
    }

    #[tokio::test]
    async fn test_get_session_not_found() {
        let storage = AoBotStorage::open_in_memory().unwrap();
        let result = storage.get_session("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let storage = AoBotStorage::open_in_memory().unwrap();

        for i in 0..3 {
            let meta = SessionMetadata {
                session_key: format!("sess-{i}"),
                agent_name: "default".into(),
                model_id: "test-model".into(),
                created_at: 1700000000000 + i,
                last_active_at: 1700000000000 + i,
                message_count: 0,
                is_active: true,
                pi_session_id: None,
            };
            storage.save_session(&meta).await.unwrap();
        }

        let sessions = storage.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 3);
        // Ordered by last_active_at DESC
        assert_eq!(sessions[0].session_key, "sess-2");
    }

    #[tokio::test]
    async fn test_update_activity() {
        let storage = AoBotStorage::open_in_memory().unwrap();
        let meta = SessionMetadata {
            session_key: "sess-1".into(),
            agent_name: "default".into(),
            model_id: "test-model".into(),
            created_at: 1700000000000,
            last_active_at: 1700000000000,
            message_count: 0,
            is_active: true,
            pi_session_id: None,
        };
        storage.save_session(&meta).await.unwrap();

        storage.update_session_activity("sess-1").await.unwrap();
        let loaded = storage.get_session("sess-1").await.unwrap().unwrap();
        assert_eq!(loaded.message_count, 1);
        assert!(loaded.last_active_at > 1700000000000);
    }

    #[tokio::test]
    async fn test_delete_session() {
        let storage = AoBotStorage::open_in_memory().unwrap();
        let meta = SessionMetadata {
            session_key: "sess-1".into(),
            agent_name: "default".into(),
            model_id: "test-model".into(),
            created_at: 1700000000000,
            last_active_at: 1700000000000,
            message_count: 0,
            is_active: true,
            pi_session_id: None,
        };
        storage.save_session(&meta).await.unwrap();

        storage.delete_session("sess-1").await.unwrap();
        // Soft-deleted: still exists but not in active list
        let loaded = storage.get_session("sess-1").await.unwrap().unwrap();
        assert!(!loaded.is_active);
        assert!(storage.list_sessions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_channel_bindings() {
        let storage = AoBotStorage::open_in_memory().unwrap();

        // First create a session
        let meta = SessionMetadata {
            session_key: "sess-1".into(),
            agent_name: "default".into(),
            model_id: "test-model".into(),
            created_at: 1700000000000,
            last_active_at: 1700000000000,
            message_count: 0,
            is_active: true,
            pi_session_id: None,
        };
        storage.save_session(&meta).await.unwrap();

        // Bind channel
        storage
            .bind_channel("tg:bot1:user1", "sess-1")
            .await
            .unwrap();
        let session = storage.get_channel_session("tg:bot1:user1").await.unwrap();
        assert_eq!(session, Some("sess-1".into()));

        // Unbind channel
        storage.unbind_channel("tg:bot1:user1").await.unwrap();
        let session = storage.get_channel_session("tg:bot1:user1").await.unwrap();
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn test_upsert_session() {
        let storage = AoBotStorage::open_in_memory().unwrap();
        let meta = SessionMetadata {
            session_key: "sess-1".into(),
            agent_name: "default".into(),
            model_id: "model-a".into(),
            created_at: 1700000000000,
            last_active_at: 1700000000000,
            message_count: 0,
            is_active: true,
            pi_session_id: None,
        };
        storage.save_session(&meta).await.unwrap();

        // Update with new model
        let meta2 = SessionMetadata {
            model_id: "model-b".into(),
            message_count: 5,
            ..meta.clone()
        };
        storage.save_session(&meta2).await.unwrap();

        let loaded = storage.get_session("sess-1").await.unwrap().unwrap();
        assert_eq!(loaded.model_id, "model-b");
        assert_eq!(loaded.message_count, 5);
    }
}
