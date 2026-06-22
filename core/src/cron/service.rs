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
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, error, info};
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
    cancel_token: Arc<tokio::sync::Mutex<CancellationToken>>,
    job_cancel_token: Arc<tokio::sync::Mutex<CancellationToken>>,
    timer_task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    store_mutex: Arc<tokio::sync::Mutex<()>>,
    max_concurrent_jobs: Arc<tokio::sync::Semaphore>,
}

impl CronService {
    /// Create a new cron service
    pub fn new(store_path: PathBuf) -> Self {
        // Start with a pre-cancelled token so status() correctly reports
        // enabled:false until start() is called and arm_timer() replaces it.
        let initial_token = CancellationToken::new();
        initial_token.cancel();
        Self {
            store_path,
            store: Arc::new(RwLock::new(CronStore::default())),
            on_job: None,
            cancel_token: Arc::new(tokio::sync::Mutex::new(initial_token)),
            job_cancel_token: Arc::new(tokio::sync::Mutex::new(CancellationToken::new())),
            timer_task: Arc::new(RwLock::new(None)),
            store_mutex: Arc::new(tokio::sync::Mutex::new(())),
            max_concurrent_jobs: Arc::new(tokio::sync::Semaphore::new(50)), // Default 50 concurrent jobs
        }
    }

    /// Set the job execution callback
    pub fn with_callback(mut self, callback: CronCallback) -> Self {
        self.on_job = Some(callback);
        self
    }

    /// Set a custom concurrency limit for job execution
    pub fn with_concurrency_limit(mut self, limit: usize) -> Self {
        // Ensure at least 1 permit
        let lim = if limit == 0 { 1 } else { limit };
        self.max_concurrent_jobs = Arc::new(tokio::sync::Semaphore::new(lim));
        self
    }

    /// Start the cron service
    pub async fn start(&self) -> Result<()> {
        // Reset job cancellation token in case this is a restart
        *self.job_cancel_token.lock().await = CancellationToken::new();

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
        // Cancel the running timer loop
        let token = self.cancel_token.lock().await.clone();
        token.cancel();

        // Cancel running jobs
        self.job_cancel_token.lock().await.cancel();

        // Wait for the timer task to finish
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

    /// Save jobs to disk atomically (write to .tmp then rename)
    async fn save_store_atomic(
        store_path: &PathBuf,
        store: &Arc<RwLock<CronStore>>,
        store_mutex: &Arc<tokio::sync::Mutex<()>>,
    ) {
        let _guard = store_mutex.lock().await;

        let content = {
            let s = store.read().await;
            match serde_json::to_string_pretty(&*s) {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to serialize cron store: {}", e);
                    return;
                }
            }
        };

        if let Some(parent) = store_path.parent()
            && let Err(e) = fs::create_dir_all(parent).await
        {
            error!("Failed to create cron store directory: {}", e);
            return;
        }

        let tmp_path = store_path.with_extension("tmp");
        if let Err(e) = fs::write(&tmp_path, &content).await {
            error!("Failed to write tmp cron store: {}", e);
        } else {
            // On Windows, rename fails if destination exists.
            #[cfg(windows)]
            let _ = tokio::fs::remove_file(store_path).await;

            if let Err(e) = fs::rename(&tmp_path, store_path).await {
                error!("Failed to swap cron store atomically: {}", e);
                // Try to clean up tmp if rename failed
                let _ = tokio::fs::remove_file(&tmp_path).await;
            }
        }
    }

    /// Save jobs to disk (convenience wrapper for non-static contexts).
    /// Acquires `store_mutex` BEFORE reading the store to prevent a stale
    /// snapshot from overwriting a newer one written by `save_store_atomic`.
    async fn save_store(&self) -> Result<()> {
        let _guard = self.store_mutex.lock().await;

        let store = self.store.read().await;
        let content = serde_json::to_string_pretty(&*store)?;
        drop(store);

        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let tmp_path = self.store_path.with_extension("tmp");
        fs::write(&tmp_path, content).await?;

        // On Windows, rename fails if destination exists.
        #[cfg(windows)]
        let _ = tokio::fs::remove_file(&self.store_path).await;

        if let Err(e) = fs::rename(&tmp_path, &self.store_path).await {
            // Clean up tmp
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(e.into());
        }
        Ok(())
    }

    /// Recompute next run times for all enabled jobs
    async fn recompute_next_runs(&self) {
        let mut store = self.store.write().await;
        let now = Utc::now().timestamp_millis();

        for job in &mut store.jobs {
            if job.enabled {
                job.state.next_run_at_ms = compute_next_run(&job.schedule, now);
            }
        }
    }

    /// Get the earliest next run time across all jobs
    async fn get_next_wake_ms_from_store(store: &Arc<RwLock<CronStore>>) -> Option<i64> {
        let store = store.read().await;
        store
            .jobs
            .iter()
            .filter_map(|j| {
                if j.enabled {
                    j.state.next_run_at_ms
                } else {
                    None
                }
            })
            .min()
    }

    /// Schedule the next timer tick — persistent loop with cancellation support
    async fn arm_timer(&self) {
        // Serialize re-arming with a full write lock
        let mut timer_task = self.timer_task.write().await;

        // Cancel any existing timer loop
        let old_token = self.cancel_token.lock().await.clone();
        old_token.cancel();

        // Abort the old handle. We hold the lock so the task is instantly swapped.
        if let Some(handle) = timer_task.take() {
            handle.abort();
        }

        // Create a fresh cancellation token and store it so stop() can cancel the new loop.
        let cancel = CancellationToken::new();
        *self.cancel_token.lock().await = cancel.clone();

        let store = Arc::clone(&self.store);
        let store_mutex = Arc::clone(&self.store_mutex);
        let max_concurrent_jobs = Arc::clone(&self.max_concurrent_jobs);
        let on_job = self.on_job.clone();
        let store_path = self.store_path.clone();
        let cancel_clone = cancel.clone();
        let job_cancel = self.job_cancel_token.lock().await.clone();

        let handle = tokio::spawn(async move {
            loop {
                // Step 1: Find soonest wake time
                let next_wake = Self::get_next_wake_ms_from_store(&store).await;

                if let Some(next_wake) = next_wake {
                    let now = Utc::now().timestamp_millis();
                    let delay_ms = (next_wake - now).max(0) as u64;
                    let delay = Duration::from_millis(delay_ms);

                    debug!("Next cron wake in {}ms (at {}ms)", delay_ms, next_wake);

                    // Step 2: Sleep until next wake OR cancellation
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = cancel_clone.cancelled() => {
                            debug!("Cron timer loop cancelled");
                            break;
                        }
                    }
                } else {
                    // No jobs scheduled — park until cancelled (woken by add_job / enable_job)
                    debug!("No cron jobs scheduled, parking timer loop");
                    cancel_clone.cancelled().await;
                    break;
                }

                // Step 3: Collect due jobs (re-read fresh state)
                let now = Utc::now().timestamp_millis();
                let due_jobs: Vec<CronJob> = {
                    let s = store.read().await;
                    s.jobs
                        .iter()
                        .filter(|j| {
                            j.enabled && j.state.next_run_at_ms.map(|t| t <= now).unwrap_or(false)
                        })
                        .cloned()
                        .collect()
                };

                if due_jobs.is_empty() {
                    continue;
                }

                // Step 4: Update scheduling state (next_run_at_ms only — NOT completion state)
                {
                    let mut s = store.write().await;
                    let mut jobs_to_delete = std::collections::HashSet::new();

                    for due in &due_jobs {
                        if let Some(j) = s.jobs.iter_mut().find(|j| j.id == due.id) {
                            let scheduled_at = j.state.next_run_at_ms.unwrap_or(now);

                            match &j.schedule {
                                CronSchedule::Every { every_ms } => {
                                    // Anti-drift: anchor to scheduled time, not now()
                                    if let Some(interval) = every_ms {
                                        let mut next = scheduled_at + *interval as i64;
                                        // Snap forward if we fell behind
                                        if next <= now {
                                            next = now + *interval as i64;
                                        }
                                        j.state.next_run_at_ms = Some(next);
                                    }
                                }
                                CronSchedule::Cron { expr, .. } => {
                                    // Anti-drift for cron: anchor to scheduled time, then snap forward
                                    if let Some(e) = expr.as_ref() {
                                        let mut next = parse_cron_next(e, scheduled_at);
                                        if let Some(n) = next
                                            && n <= now
                                        {
                                            next = parse_cron_next(e, now);
                                        }
                                        j.state.next_run_at_ms = next;
                                    } else {
                                        j.state.next_run_at_ms = None;
                                    }
                                }
                                CronSchedule::At { .. } => {
                                    // One-shot: clear next run
                                    j.state.next_run_at_ms = None;
                                    // One-shot: add to delete list
                                    jobs_to_delete.insert(due.id.clone());
                                }
                            }

                            j.updated_at_ms = now;
                        }
                    }

                    // Remove one-shot jobs marked for deletion
                    if !jobs_to_delete.is_empty() {
                        s.jobs.retain(|j| !jobs_to_delete.contains(&j.id));
                    }
                }

                // Save scheduling state to disk atomically
                Self::save_store_atomic(&store_path, &store, &store_mutex).await;

                // Step 5: Execute due jobs (completion state updated inside spawned task)
                for job in due_jobs {
                    if let Some(callback) = &on_job {
                        let job_id = job.id.clone();
                        let job_name = job.name.clone();
                        let job_clone = job;
                        let callback_clone = callback.clone();
                        let store_clone = Arc::clone(&store);
                        let store_mutex_clone = Arc::clone(&store_mutex);
                        let store_path_clone = store_path.clone();

                        let span = tracing::info_span!("cron_job_exec", job_id = %job_id, job_name = %job_name);

                        // Clone the semaphore to move into the task
                        let semaphore_clone = max_concurrent_jobs.clone();

                        // Clone the global job cancellation token (aborted only on stop())
                        let job_cancel_clone = job_cancel.clone();

                        tokio::spawn(
                            async move {
                                tokio::select! {
                                    // If the service is stopped mid-execution, abort cleanly
                                    // without mutating or persisting state.
                                    _ = job_cancel_clone.cancelled() => {
                                        debug!(
                                            "Cron job '{}' ({}) cancelled during shutdown",
                                            job_name, job_id
                                        );
                                    }
                                    // Normal execution path
                                    _ = async {
                                        // Acquire a permit inside the spawned task to avoid blocking the timer loop
                                        let _permit = match semaphore_clone.acquire_owned().await {
                                            Ok(p) => p,
                                            Err(_) => {
                                                error!(
                                                    "Semaphore closed, dropping job execution for {}",
                                                    job_id
                                                );
                                                return;
                                            }
                                        };
                                        let result = callback_clone(job_clone).await;
                                        let finish_time = Utc::now().timestamp_millis();

                                        // Update completion state in store
                                        {
                                            let mut s = store_clone.write().await;
                                            if let Some(j) = s.jobs.iter_mut().find(|j| j.id == job_id) {
                                                j.state.last_run_at_ms = Some(finish_time);
                                                match result {
                                                    Ok(_) => {
                                                        j.state.last_status = Some("ok".to_string());
                                                        j.state.last_error = None;
                                                    }
                                                    Err(ref e) => {
                                                        j.state.last_status = Some("error".to_string());
                                                        j.state.last_error = Some(format!("{}", e));
                                                        error!(
                                                            "Cron job '{}' execution failed: {}",
                                                            job_id, e
                                                        );
                                                    }
                                                }
                                            }
                                        }

                                        // Persist updated completion state
                                        Self::save_store_atomic(
                                            &store_path_clone,
                                            &store_clone,
                                            &store_mutex_clone,
                                        )
                                        .await;
                                    } => {}
                                }
                            }
                            .instrument(span),
                        );
                    }
                }

                // Loop continues — will re-read store and compute next wake
            }
        });

        *timer_task = Some(handle);
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

        job.state.next_run_at_ms = compute_next_run(&job.schedule, now);

        let mut store = self.store.write().await;
        store.jobs.push(job.clone());
        drop(store);

        let _ = self.save_store().await;

        // Re-arm the timer loop to pick up new schedule
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
            drop(store);
            let _ = self.save_store().await;
            // Re-arm the timer loop to update schedule
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
                    job.state.next_run_at_ms = compute_next_run(&job.schedule, now);
                } else {
                    job.state.next_run_at_ms = None;
                }

                let job_clone = job.clone();
                drop(store);

                let _ = self.save_store().await;
                // Re-arm the timer loop to update schedule
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

        // Execute the job — completion state updated by the spawned task
        if let Some(callback) = &self.on_job {
            let job_id_owned = job.id.clone();
            let job_name_owned = job.name.clone();
            let job_clone = job.clone();
            let callback_clone = callback.clone();
            let store_clone = Arc::clone(&self.store);
            let store_mutex_clone = Arc::clone(&self.store_mutex);
            let store_path_clone = self.store_path.clone();

            let max_concurrent_jobs = Arc::clone(&self.max_concurrent_jobs);
            let span = tracing::info_span!("cron_job_exec", job_id = %job_id_owned, job_name = %job_name_owned, run_type = "manual");

            tokio::spawn(
                async move {
                    // Acquire permit inside the spawned task to avoid blocking the timer loop
                    let _permit = match max_concurrent_jobs.acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => {
                            error!(
                                "Semaphore closed, dropping manual job execution for {}",
                                job_id_owned
                            );
                            return;
                        }
                    };
                    let result = callback_clone(job_clone).await;
                    let finish_time = Utc::now().timestamp_millis();

                    {
                        let mut s = store_clone.write().await;
                        if let Some(j) = s.jobs.iter_mut().find(|j| j.id == job_id_owned) {
                            j.state.last_run_at_ms = Some(finish_time);
                            j.updated_at_ms = finish_time;
                            match result {
                                Ok(_) => {
                                    j.state.last_status = Some("ok".to_string());
                                    j.state.last_error = None;
                                }
                                Err(ref e) => {
                                    j.state.last_status = Some("error".to_string());
                                    j.state.last_error = Some(format!("{}", e));
                                    error!("Cron job '{}' execution failed: {}", job_id_owned, e);
                                }
                            }
                        }
                    }

                    Self::save_store_atomic(&store_path_clone, &store_clone, &store_mutex_clone)
                        .await;
                }
                .instrument(span),
            );
        }

        // Update scheduling state for recurring jobs
        let now = Utc::now().timestamp_millis();
        let mut store = self.store.write().await;
        for j in &mut store.jobs {
            if j.id == job_id {
                // Compute next run for recurring jobs
                if matches!(
                    j.schedule,
                    CronSchedule::Every { .. } | CronSchedule::Cron { .. }
                ) {
                    j.state.next_run_at_ms = compute_next_run(&j.schedule, now);
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
        // First, read the timer task status without holding the store lock.
        let is_running = self.timer_task.read().await.is_some();

        // Hold a single read lock and compute next_wake from it directly,
        // avoiding a second lock acquisition inside get_next_wake_ms_from_store.
        let store = self.store.read().await;
        let next_wake_at_ms = store
            .jobs
            .iter()
            .filter_map(|j| {
                if j.enabled {
                    j.state.next_run_at_ms
                } else {
                    None
                }
            })
            .min();
        CronStatus {
            enabled: is_running,
            jobs: store.jobs.len(),
            next_wake_at_ms,
        }
    }
}

/// Compute the next run time for a schedule (free function for use in static contexts)
fn compute_next_run(schedule: &CronSchedule, now_ms: i64) -> Option<i64> {
    match schedule {
        CronSchedule::At { at_ms } => at_ms.filter(|&at| at > now_ms),
        CronSchedule::Every { every_ms } => every_ms.map(|interval| now_ms + interval as i64),
        CronSchedule::Cron { expr, .. } => {
            expr.as_ref().and_then(|expr| parse_cron_next(expr, now_ms))
        }
    }
}

/// Parse cron expression and compute next run time
fn parse_cron_next(expr: &str, now_ms: i64) -> Option<i64> {
    let schedule = Schedule::try_from(expr).ok()?;
    let now = chrono::DateTime::<Utc>::from_timestamp_millis(now_ms)?;
    let next = schedule.after(&now).next()?;
    Some(next.timestamp_millis())
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

    #[tokio::test]
    async fn test_cron_service_remove_job() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("jobs.json");
        let service = CronService::new(store_path);

        let job = service
            .add_job(
                "Removable".to_string(),
                CronSchedule::every(30),
                "msg".to_string(),
                false,
                None,
                None,
            )
            .await;

        assert!(service.remove_job(&job.id).await);
        assert_eq!(service.list_jobs(true).await.len(), 0);
    }

    #[tokio::test]
    async fn test_cron_service_enable_disable() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("jobs.json");
        let service = CronService::new(store_path);

        let job = service
            .add_job(
                "Toggleable".to_string(),
                CronSchedule::every(60),
                "msg".to_string(),
                false,
                None,
                None,
            )
            .await;

        // Disable
        let disabled = service.enable_job(&job.id, false).await.unwrap();
        assert!(!disabled.enabled);
        assert!(disabled.state.next_run_at_ms.is_none());

        // Re-enable
        let enabled = service.enable_job(&job.id, true).await.unwrap();
        assert!(enabled.enabled);
        assert!(enabled.state.next_run_at_ms.is_some());
    }

    #[test]
    fn test_compute_next_run_every() {
        let now = 1000i64;
        let schedule = CronSchedule::every(60);
        let next = compute_next_run(&schedule, now);
        assert_eq!(next, Some(61000)); // now + 60*1000
    }

    #[test]
    fn test_compute_next_run_at_future() {
        let schedule = CronSchedule::at(5000);
        let next = compute_next_run(&schedule, 1000);
        assert_eq!(next, Some(5000));
    }

    #[test]
    fn test_compute_next_run_at_past() {
        let schedule = CronSchedule::at(500);
        let next = compute_next_run(&schedule, 1000);
        assert_eq!(next, None); // already past
    }

    #[tokio::test]
    async fn test_atomic_save() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("jobs.json");
        let service = CronService::new(store_path.clone());

        service
            .add_job(
                "Persist Me".to_string(),
                CronSchedule::every(120),
                "msg".to_string(),
                false,
                None,
                None,
            )
            .await;

        // Verify .tmp does NOT linger (rename happened)
        let tmp_path = store_path.with_extension("tmp");
        assert!(!tmp_path.exists());
        // Verify the main store file exists
        assert!(store_path.exists());
    }
}
