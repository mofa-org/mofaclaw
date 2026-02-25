//! Cron service for scheduling agent tasks

use super::types::{CronJob, CronSchedule, CronStore};
use crate::error::Result;
use chrono::Utc;
use cron::Schedule;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Callback type for executing a cron job
pub type CronCallback = Arc<
    dyn Fn(
            CronJob,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<String>>> + Send>>
        + Send
        + Sync,
>;

/// Service for managing and executing scheduled jobs
pub struct CronService {
    store_path: PathBuf,
    store: Arc<RwLock<CronStore>>,
    on_job: Option<CronCallback>,
    running: Arc<RwLock<bool>>,
    timer_task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl CronService {
    /// Create a new cron service
    pub fn new(store_path: PathBuf) -> Self {
        Self {
            store_path,
            store: Arc::new(RwLock::new(CronStore::default())),
            on_job: None,
            running: Arc::new(RwLock::new(false)),
            timer_task: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the job execution callback
    pub fn with_callback(mut self, callback: CronCallback) -> Self {
        self.on_job = Some(callback);
        self
    }

    /// Start the cron service
    pub async fn start(&self) -> Result<()> {
        *self.running.write().await = true;

        // Load jobs from disk
        self.load_store().await?;

        // Recompute next run times
        self.recompute_next_runs().await;

        info!(
            "Cron service started with {} jobs",
            self.store.read().await.jobs.len()
        );

        // Arm the timer
        self.arm_timer().await;

        Ok(())
    }

    /// Stop the cron service
    pub async fn stop(&self) {
        *self.running.write().await = false;

        // Cancel timer task
        let mut timer_task = self.timer_task.write().await;
        if let Some(handle) = timer_task.take() {
            handle.abort();
        }

        info!("Cron service stopped");
    }

    /// Load jobs from disk
    async fn load_store(&self) -> Result<()> {
        if !self.store_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.store_path).await?;
        let store: CronStore = serde_json::from_str(&content)?;
        *self.store.write().await = store;
        Ok(())
    }

    /// Save jobs to disk
    async fn save_store(&self) -> Result<()> {
        let store = self.store.read().await;
        let content = serde_json::to_string_pretty(&*store)?;

        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(&self.store_path, content).await?;
        Ok(())
    }

    /// Recompute next run times for all enabled jobs
    async fn recompute_next_runs(&self) {
        let mut store = self.store.write().await;
        let now = Utc::now().timestamp_millis();

        for job in &mut store.jobs {
            if job.enabled {
                job.state.next_run_at_ms = self.compute_next_run(&job.schedule, now);
            }
        }
    }

    /// Compute the next run time for a schedule
    fn compute_next_run(&self, schedule: &CronSchedule, now_ms: i64) -> Option<i64> {
        match schedule {
            CronSchedule::At { at_ms } => at_ms.filter(|&at| at > now_ms),
            CronSchedule::Every { every_ms } => every_ms.map(|interval| now_ms + interval as i64),
            CronSchedule::Cron { expr, .. } => {
                expr.as_ref().and_then(|expr| {
                    // Try to parse as cron expression
                    self.parse_cron_next(expr, now_ms)
                })
            }
        }
    }

    /// Parse cron expression and compute next run time
    fn parse_cron_next(&self, expr: &str, now_ms: i64) -> Option<i64> {
        let schedule = Schedule::try_from(expr).ok()?;
        let now = chrono::DateTime::<Utc>::from_timestamp_millis(now_ms)?;
        let next = schedule.after(&now).next()?;
        Some(next.timestamp_millis())
    }

    /// Get the earliest next run time across all jobs
    async fn get_next_wake_ms(&self) -> Option<i64> {
        let store = self.store.read().await;
        store
            .jobs
            .iter()
            .filter_map(|j| if j.enabled { j.state.next_run_at_ms } else { None })
            .min()
    }

    /// Schedule the next timer tick
    async fn arm_timer(&self) {
        let next_wake = match self.get_next_wake_ms().await {
            Some(t) => t,
            None => return,
        };

        let now = Utc::now().timestamp_millis();
        let delay_ms = (next_wake - now).max(0) as u64;
        let delay = Duration::from_millis(delay_ms);

        debug!("Next cron wake in {}ms (at {}ms)", delay_ms, next_wake);

        let running = Arc::clone(&self.running);
        let store = Arc::clone(&self.store);
        let on_job = self.on_job.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(delay).await;

            if !*running.read().await {
                return;
            }

            // Execute due jobs
            let now = Utc::now().timestamp_millis();
            let due_jobs: Vec<_> = store
                .read()
                .await
                .jobs
                .iter()
                .filter(|j| j.enabled && j.state.next_run_at_ms.map(|t| t <= now).unwrap_or(false))
                .cloned()
                .collect();

            for job in due_jobs {
                if let Some(callback) = &on_job {
                    let job_clone = job.clone();
                    let callback_clone = callback.clone();
                    tokio::spawn(async move {
                        if let Err(e) = callback_clone(job_clone).await {
                            error!("Cron job execution failed: {}", e);
                        }
                    });
                }
            }

            // Save and re-arm (would be done properly)
        });

        *self.timer_task.write().await = Some(handle);
    }

    /// List all jobs
    pub async fn list_jobs(&self, include_disabled: bool) -> Vec<CronJob> {
        let store = self.store.read().await;
        if include_disabled {
            store.jobs.clone()
        } else {
            store.jobs.iter().filter(|j| j.enabled).cloned().collect()
        }
    }

    /// Add a new job
    pub async fn add_job(
        &self,
        name: String,
        schedule: CronSchedule,
        message: String,
        deliver: bool,
        to: Option<String>,
        channel: Option<String>,
    ) -> CronJob {
        let now = Utc::now().timestamp_millis();

        let mut job = CronJob::new(
            Uuid::new_v4().to_string()[..8].to_string(),
            name,
            schedule,
            message,
        );

        job.payload.deliver = deliver;
        job.payload.to = to;
        job.payload.channel = channel;

        job.state.next_run_at_ms = self.compute_next_run(&job.schedule, now);

        let mut store = self.store.write().await;
        store.jobs.push(job.clone());
        drop(store);

        let _ = self.save_store().await;
        self.arm_timer().await;

        info!("Added cron job '{}' ({})", job.name, job.id);
        job
    }

    /// Remove a job
    pub async fn remove_job(&self, job_id: &str) -> bool {
        let mut store = self.store.write().await;
        let before = store.jobs.len();
        store.jobs.retain(|j| j.id != job_id);
        let removed = store.jobs.len() < before;

        if removed {
            let _ = self.save_store().await;
            self.arm_timer().await;
            info!("Removed cron job {}", job_id);
        }

        removed
    }

    /// Enable or disable a job
    pub async fn enable_job(&self, job_id: &str, enabled: bool) -> Option<CronJob> {
        let mut store = self.store.write().await;
        let now = Utc::now().timestamp_millis();

        for job in &mut store.jobs {
            if job.id == job_id {
                job.enabled = enabled;
                job.updated_at_ms = now;

                if enabled {
                    job.state.next_run_at_ms = self.compute_next_run(&job.schedule, now);
                } else {
                    job.state.next_run_at_ms = None;
                }

                let job_clone = job.clone();
                drop(store);

                let _ = self.save_store().await;
                self.arm_timer().await;

                return Some(job_clone);
            }
        }

        None
    }

    /// Manually run a job
    pub async fn run_job(&self, job_id: &str, force: bool) -> bool {
        // Find the job
        let job = {
            let store = self.store.read().await;
            store.jobs.iter().find(|j| j.id == job_id).cloned()
        };

        let job = match job {
            Some(j) => j,
            None => return false,
        };

        // Check if job is enabled (unless force)
        if !force && !job.enabled {
            return false;
        }

        // Execute the job
        if let Some(callback) = &self.on_job {
            let job_clone = job.clone();
            let callback_clone = callback.clone();
            tokio::spawn(async move {
                if let Err(e) = callback_clone(job_clone).await {
                    error!("Cron job execution failed: {}", e);
                }
            });
        }

        // Update job state
        let now = Utc::now().timestamp_millis();
        let mut store = self.store.write().await;
        for j in &mut store.jobs {
            if j.id == job_id {
                j.state.last_run_at_ms = Some(now);
                j.updated_at_ms = now;

                // Compute next run for recurring jobs
                if matches!(
                    j.schedule,
                    CronSchedule::Every { .. } | CronSchedule::Cron { .. }
                ) {
                    j.state.next_run_at_ms = self.compute_next_run(&j.schedule, now);
                }

                break;
            }
        }
        drop(store);

        let _ = self.save_store().await;
        self.arm_timer().await;

        true
    }

    /// Get service status
    pub async fn status(&self) -> CronStatus {
        let store = self.store.read().await;
        CronStatus {
            enabled: *self.running.read().await,
            jobs: store.jobs.len(),
            next_wake_at_ms: self.get_next_wake_ms().await,
        }
    }
}

/// Status of the cron service
#[derive(Debug, Clone)]
pub struct CronStatus {
    pub enabled: bool,
    pub jobs: usize,
    pub next_wake_at_ms: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cron_service() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("jobs.json");

        let service = CronService::new(store_path);

        // Add a job
        let job = service
            .add_job(
                "Test Job".to_string(),
                CronSchedule::every(60),
                "Test message".to_string(),
                false,
                None,
                None,
            )
            .await;

        assert_eq!(job.name, "Test Job");
        assert!(job.enabled);

        // List jobs
        let jobs = service.list_jobs(false).await;
        assert_eq!(jobs.len(), 1);
    }
}
