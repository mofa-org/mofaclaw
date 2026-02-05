//! Types for cron scheduling

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Schedule type for a cron job
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum CronSchedule {
    /// Run once at a specific time
    #[serde(rename = "at")]
    At {
        /// Timestamp in milliseconds
        at_ms: Option<i64>,
    },
    /// Run every N seconds
    #[serde(rename = "every")]
    Every {
        /// Interval in milliseconds
        every_ms: Option<u64>,
    },
    /// Run on a cron schedule
    #[serde(rename = "cron")]
    Cron {
        /// Cron expression
        expr: Option<String>,
        /// Timezone
        tz: Option<String>,
    },
}

impl CronSchedule {
    /// Create an "every" schedule
    pub fn every(seconds: u64) -> Self {
        CronSchedule::Every {
            every_ms: Some(seconds * 1000),
        }
    }

    /// Create a cron schedule
    pub fn cron(expr: impl Into<String>) -> Self {
        CronSchedule::Cron {
            expr: Some(expr.into()),
            tz: None,
        }
    }

    /// Create an "at" schedule
    pub fn at(timestamp_ms: i64) -> Self {
        CronSchedule::At {
            at_ms: Some(timestamp_ms),
        }
    }
}

/// Payload for a cron job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronPayload {
    /// Payload kind
    #[serde(default = "default_payload_kind")]
    pub kind: String,
    /// Message to send to agent
    #[serde(default)]
    pub message: String,
    /// Whether to deliver response to a channel
    #[serde(default)]
    pub deliver: bool,
    /// Channel to deliver to
    pub channel: Option<String>,
    /// Recipient
    pub to: Option<String>,
}

fn default_payload_kind() -> String {
    "agent_turn".to_string()
}

/// State of a cron job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobState {
    /// Next run time in milliseconds
    pub next_run_at_ms: Option<i64>,
    /// Last run time in milliseconds
    pub last_run_at_ms: Option<i64>,
    /// Last run status
    pub last_status: Option<String>,
    /// Last error message
    pub last_error: Option<String>,
}

impl Default for CronJobState {
    fn default() -> Self {
        Self {
            next_run_at_ms: None,
            last_run_at_ms: None,
            last_status: None,
            last_error: None,
        }
    }
}

/// A cron job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Job ID
    pub id: String,
    /// Job name
    pub name: String,
    /// Whether the job is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Schedule
    pub schedule: CronSchedule,
    /// Payload
    pub payload: CronPayload,
    /// State
    #[serde(default)]
    pub state: CronJobState,
    /// Creation timestamp in milliseconds
    #[serde(default)]
    pub created_at_ms: i64,
    /// Update timestamp in milliseconds
    #[serde(default)]
    pub updated_at_ms: i64,
    /// Delete after running (for one-shot jobs)
    #[serde(default)]
    pub delete_after_run: bool,
}

impl CronJob {
    /// Create a new cron job
    pub fn new(id: String, name: String, schedule: CronSchedule, message: String) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id,
            name,
            enabled: true,
            schedule,
            payload: CronPayload {
                kind: "agent_turn".to_string(),
                message,
                deliver: false,
                channel: None,
                to: None,
            },
            state: CronJobState::default(),
            created_at_ms: now,
            updated_at_ms: now,
            delete_after_run: false,
        }
    }
}

/// Store for cron jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronStore {
    pub version: u32,
    pub jobs: Vec<CronJob>,
}

impl Default for CronStore {
    fn default() -> Self {
        Self {
            version: 1,
            jobs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_schedule_creation() {
        let schedule = CronSchedule::every(60);
        match schedule {
            CronSchedule::Every { every_ms } => {
                assert_eq!(every_ms, Some(60000));
            }
            _ => panic!("Wrong schedule type"),
        }
    }

    #[test]
    fn test_cron_job_creation() {
        let job = CronJob::new(
            "test_id".to_string(),
            "Test Job".to_string(),
            CronSchedule::every(300),
            "Test message".to_string(),
        );

        assert_eq!(job.id, "test_id");
        assert_eq!(job.name, "Test Job");
        assert!(job.enabled);
    }
}
