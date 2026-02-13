//! Cron job scheduler — evaluates cron expressions and triggers execution.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::CronJob;
use crate::store::CronStore;

/// Manages cron job scheduling and execution.
pub struct CronManager {
    store: Arc<CronStore>,
    jobs: RwLock<Vec<CronJob>>,
}

impl CronManager {
    /// Create a new cron manager.
    pub fn new(store: Arc<CronStore>) -> Self {
        Self {
            store,
            jobs: RwLock::new(Vec::new()),
        }
    }

    /// Load jobs from storage.
    pub async fn load(&self) -> anyhow::Result<()> {
        let jobs = self.store.list_jobs()?;
        info!("Loaded {} cron jobs", jobs.len());
        *self.jobs.write().await = jobs;
        Ok(())
    }

    /// Add a new cron job.
    pub async fn add_job(&self, job: CronJob) -> anyhow::Result<()> {
        self.store.upsert_job(&job)?;
        self.jobs.write().await.push(job);
        Ok(())
    }

    /// Remove a cron job.
    pub async fn remove_job(&self, id: &str) -> anyhow::Result<bool> {
        let removed = self.store.delete_job(id)?;
        if removed {
            self.jobs.write().await.retain(|j| j.id != id);
        }
        Ok(removed)
    }

    /// List all jobs.
    pub async fn list_jobs(&self) -> Vec<CronJob> {
        self.jobs.read().await.clone()
    }

    /// Update a job's enabled status.
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<bool> {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            job.enabled = enabled;
            self.store.upsert_job(job)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get due jobs that should be executed now.
    pub async fn get_due_jobs(&self) -> Vec<CronJob> {
        let now = chrono::Utc::now();
        let jobs = self.jobs.read().await;
        jobs.iter()
            .filter(|j| j.enabled && j.next_run.is_some_and(|next| next <= now))
            .cloned()
            .collect()
    }

    /// Mark a job as having run and compute next run time.
    pub async fn mark_ran(&self, id: &str) -> anyhow::Result<()> {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            job.last_run = Some(chrono::Utc::now());
            // Simple next-run computation: parse cron expression would go here.
            // For now, we just clear next_run and let the scheduler recompute.
            job.next_run = None;
            self.store.upsert_job(job)?;
        }
        Ok(())
    }

    /// Start the scheduler loop (runs in background).
    pub async fn run_scheduler(
        self: Arc<Self>,
        task_sender: tokio::sync::mpsc::UnboundedSender<CronJob>,
    ) {
        info!("Cron scheduler started");
        loop {
            let due_jobs = self.get_due_jobs().await;
            for job in due_jobs {
                info!(job_id = %job.id, task = %job.task, "Executing cron job");
                if let Err(e) = task_sender.send(job.clone()) {
                    warn!("Failed to dispatch cron job {}: {e}", job.id);
                }
                if let Err(e) = self.mark_ran(&job.id).await {
                    warn!("Failed to mark cron job {} as ran: {e}", job.id);
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    }
}
