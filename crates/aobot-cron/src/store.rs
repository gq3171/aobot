//! SQLite-backed cron job storage.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use crate::CronJob;

/// Persistent storage for cron jobs.
pub struct CronStore {
    conn: Mutex<Connection>,
}

impl CronStore {
    /// Open or create a cron store.
    pub fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;

             CREATE TABLE IF NOT EXISTS cron_jobs (
                 id TEXT PRIMARY KEY,
                 schedule TEXT NOT NULL,
                 task TEXT NOT NULL,
                 agent_id TEXT NOT NULL,
                 session_key TEXT NOT NULL,
                 enabled INTEGER NOT NULL DEFAULT 1,
                 last_run TEXT,
                 next_run TEXT,
                 created_at TEXT NOT NULL
             );",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// List all cron jobs.
    pub fn list_jobs(&self) -> Result<Vec<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, schedule, task, agent_id, session_key, enabled, last_run, next_run, created_at FROM cron_jobs",
        )?;
        let jobs = stmt
            .query_map([], |row| {
                Ok(CronJob {
                    id: row.get(0)?,
                    schedule: row.get(1)?,
                    task: row.get(2)?,
                    agent_id: row.get(3)?,
                    session_key: row.get(4)?,
                    enabled: row.get::<_, i64>(5)? != 0,
                    last_run: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| s.parse().ok()),
                    next_run: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| s.parse().ok()),
                    created_at: row
                        .get::<_, String>(8)?
                        .parse()
                        .unwrap_or_else(|_| chrono::Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    /// Insert or update a cron job.
    pub fn upsert_job(&self, job: &CronJob) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO cron_jobs (id, schedule, task, agent_id, session_key, enabled, last_run, next_run, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                job.id,
                job.schedule,
                job.task,
                job.agent_id,
                job.session_key,
                job.enabled as i64,
                job.last_run.map(|t| t.to_rfc3339()),
                job.next_run.map(|t| t.to_rfc3339()),
                job.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Delete a cron job.
    pub fn delete_job(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count = conn.execute("DELETE FROM cron_jobs WHERE id = ?1", rusqlite::params![id])?;
        Ok(count > 0)
    }

    /// Get a cron job by ID.
    pub fn get_job(&self, id: &str) -> Result<Option<CronJob>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, schedule, task, agent_id, session_key, enabled, last_run, next_run, created_at FROM cron_jobs WHERE id = ?1",
        )?;
        let result = stmt.query_row(rusqlite::params![id], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                schedule: row.get(1)?,
                task: row.get(2)?,
                agent_id: row.get(3)?,
                session_key: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                last_run: row
                    .get::<_, Option<String>>(6)?
                    .and_then(|s| s.parse().ok()),
                next_run: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|s| s.parse().ok()),
                created_at: row
                    .get::<_, String>(8)?
                    .parse()
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
        });
        match result {
            Ok(j) => Ok(Some(j)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
