//! aobot-cron: Scheduled task management.
//!
//! Provides a cron-like scheduler for periodic tasks that can be managed
//! by AI agents through the cron tool.

pub mod scheduler;
pub mod store;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A scheduled cron job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job ID.
    pub id: String,
    /// Cron expression (e.g. "0 * * * *" for every hour).
    pub schedule: String,
    /// Task description to execute.
    pub task: String,
    /// Agent ID to run the task on.
    pub agent_id: String,
    /// Session key for the task.
    pub session_key: String,
    /// Whether this job is enabled.
    pub enabled: bool,
    /// Last execution time.
    pub last_run: Option<DateTime<Utc>>,
    /// Next scheduled execution time.
    pub next_run: Option<DateTime<Utc>>,
    /// Creation time.
    pub created_at: DateTime<Utc>,
}
