//! Heartbeat service for periodic wake-ups

use crate::error::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// Token that indicates "nothing to do"
pub const HEARTBEAT_OK_TOKEN: &str = "HEARTBEAT_OK";

/// The prompt sent to agent during heartbeat
const HEARTBEAT_PROMPT: &str = r#"Read HEARTBEAT.md in your workspace (if it exists).
Follow any instructions or tasks listed there.
If nothing needs attention, reply with just: HEARTBEAT_OK"#;

/// Callback type for heartbeat
pub type HeartbeatCallback = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>>
        + Send
        + Sync,
>;

/// Service for periodic heartbeat wake-ups
pub struct HeartbeatService {
    workspace: PathBuf,
    on_heartbeat: Option<HeartbeatCallback>,
    interval_s: u64,
    enabled: bool,
    running: Arc<RwLock<bool>>,
    task_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl HeartbeatService {
    /// Create a new heartbeat service
    pub fn new(workspace: PathBuf, interval_s: u64) -> Self {
        Self {
            workspace,
            on_heartbeat: None,
            interval_s,
            enabled: true,
            running: Arc::new(RwLock::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the heartbeat callback
    pub fn with_callback(mut self, callback: HeartbeatCallback) -> Self {
        self.on_heartbeat = Some(callback);
        self
    }

    /// Get the path to HEARTBEAT.md
    pub fn heartbeat_file(&self) -> PathBuf {
        self.workspace.join("HEARTBEAT.md")
    }

    /// Start the heartbeat service
    pub async fn start(&self) -> Result<()> {
        if !self.enabled {
            info!("Heartbeat disabled");
            return Ok(());
        }

        *self.running.write().await = true;

        let interval = Duration::from_secs(self.interval_s);
        let running = Arc::clone(&self.running);
        let on_heartbeat = self.on_heartbeat.clone();
        let workspace = self.workspace.clone();

        let handle = tokio::spawn(async move {
            info!("Heartbeat started (interval: {}s)", interval.as_secs());

            while *running.read().await {
                tokio::time::sleep(interval).await;

                if !*running.read().await {
                    break;
                }

                // Execute tick
                if let Err(e) = Self::tick_internal(&workspace, &on_heartbeat).await {
                    error!("Heartbeat tick failed: {}", e);
                }
            }

            info!("Heartbeat stopped");
        });

        *self.task_handle.write().await = Some(handle);

        Ok(())
    }

    /// Stop the heartbeat service
    pub async fn stop(&self) {
        *self.running.write().await = false;

        let mut handle = self.task_handle.write().await;
        if let Some(h) = handle.take() {
            h.abort();
        }
    }

    /// Manually trigger a heartbeat
    pub async fn trigger_now(&self) -> Option<Result<String>> {
        if let Some(callback) = &self.on_heartbeat {
            Some(callback(HEARTBEAT_PROMPT.to_string()).await)
        } else {
            None
        }
    }

    /// Internal tick implementation (shared between timer and manual trigger)
    async fn tick_internal(
        workspace: &PathBuf,
        on_heartbeat: &Option<HeartbeatCallback>,
    ) -> Result<()> {
        let heartbeat_path = workspace.join("HEARTBEAT.md");

        // Read HEARTBEAT.md content
        let content = if heartbeat_path.exists() {
            fs::read_to_string(&heartbeat_path).await.ok()
        } else {
            None
        };

        // Skip if HEARTBEAT.md is empty or doesn't exist
        if is_heartbeat_empty(content.as_deref()) {
            debug!("Heartbeat: no tasks (HEARTBEAT.md empty)");
            return Ok(());
        }

        info!("Heartbeat: checking for tasks...");

        if let Some(callback) = on_heartbeat {
            match callback(HEARTBEAT_PROMPT.to_string()).await {
                Ok(response) => {
                    // Check if agent said "nothing to do"
                    if response
                        .to_uppercase()
                        .replace('_', "")
                        .contains(HEARTBEAT_OK_TOKEN)
                    {
                        info!("Heartbeat: OK (no action needed)");
                    } else {
                        info!("Heartbeat: completed task");
                    }
                }
                Err(e) => {
                    error!("Heartbeat execution failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Check if the service is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Set whether the service is enabled
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// Check if HEARTBEAT.md has no actionable content
///
/// Skips:
/// - Empty lines
/// - Headers (starting with #)
/// - HTML comments (<!--)
/// - Empty checkboxes (- [ ], * [ ], - [x], * [x])
fn is_heartbeat_empty(content: Option<&str>) -> bool {
    let content = match content {
        Some(c) if !c.is_empty() => c,
        _ => return true,
    };

    // Lines to skip: empty, headers, HTML comments, and checkbox lines
    let skip_patterns = ["- [ ]", "* [ ]", "- [x]", "* [x]"];

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines, headers, HTML comments, and checkbox items
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("<!--")
            || skip_patterns.iter().any(|&p| line.starts_with(p))
        {
            continue;
        }

        // Found actionable content
        return false;
    }

    true
}

/// Build the heartbeat prompt (legacy, kept for compatibility)
#[cfg(test)]
fn build_heartbeat_prompt(workspace: &PathBuf) -> String {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();

    format!(
        r#"The current time is {}.

Please check if there's anything I should be aware of:

1. Check today's memory file at {}/memory/YYYY-MM-DD.md for any notes or reminders
2. Review any scheduled tasks or important events
3. Check if there are any pending items I should follow up on

Report anything important that needs attention."#,
        now,
        workspace.display()
    )
}

impl Clone for HeartbeatService {
    fn clone(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            on_heartbeat: self.on_heartbeat.clone(),
            interval_s: self.interval_s,
            enabled: self.enabled,
            running: Arc::clone(&self.running),
            task_handle: Arc::clone(&self.task_handle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_heartbeat_empty() {
        // Empty content
        assert!(is_heartbeat_empty(None));
        assert!(is_heartbeat_empty(Some("")));
        assert!(is_heartbeat_empty(Some("   \n\n  ")));

        // Only headers
        assert!(is_heartbeat_empty(Some("# Header\n## Another\n")));

        // Only checkboxes
        assert!(is_heartbeat_empty(Some(
            "- [ ] Task 1\n* [ ] Task 2\n- [x] Done\n* [x] Done too\n"
        )));

        // With HTML comment
        assert!(is_heartbeat_empty(Some("<!-- comment -->\n# Header\n")));

        // With actual content
        assert!(!is_heartbeat_empty(Some("Remember to check email")));

        // Mixed - header with content
        assert!(!is_heartbeat_empty(Some("# Tasks\nCheck email\n")));
    }

    #[test]
    fn test_heartbeat_prompt() {
        let workspace = PathBuf::from("/tmp/test");
        let prompt = build_heartbeat_prompt(&workspace);

        assert!(prompt.contains("current time"));
        assert!(prompt.contains("/tmp/test/memory"));
    }

    #[test]
    fn test_heartbeat_ok_token() {
        assert_eq!(HEARTBEAT_OK_TOKEN, "HEARTBEAT_OK");
    }

    #[test]
    fn test_heartbeat_file_path() {
        let service = HeartbeatService::new(PathBuf::from("/tmp/workspace"), 60);
        assert_eq!(
            service.heartbeat_file(),
            PathBuf::from("/tmp/workspace/HEARTBEAT.md")
        );
    }
}
