//! Change history – append-only log of workspace mutations
//!
//! All changes are appended as newline-delimited JSON to `history/changes.log`.

use crate::error::Result;
use crate::workspace::types::{AgentId, ChangeAction, ChangeRecord};
use chrono::Utc;
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Manages the append-only change history.
pub struct ChangeHistory {
    log_path: PathBuf,
}

impl ChangeHistory {
    pub async fn new(history_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&history_dir).await?;
        let log_path = history_dir.join("changes.log");
        // Touch the file if it doesn't exist
        if !log_path.exists() {
            fs::write(&log_path, b"").await?;
        }
        Ok(Self { log_path })
    }

    /// Append a change record to the log.
    pub async fn record(
        &self,
        agent: AgentId,
        action: ChangeAction,
        artifact_id: Option<Uuid>,
        description: String,
    ) -> Result<ChangeRecord> {
        let record = ChangeRecord {
            timestamp: Utc::now(),
            agent,
            action,
            artifact_id,
            description,
        };

        let mut line = serde_json::to_string(&record)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.flush().await?;

        Ok(record)
    }

    /// Read the full history (newest last).
    pub async fn read_all(&self) -> Result<Vec<ChangeRecord>> {
        let data = fs::read_to_string(&self.log_path).await?;
        let mut records = Vec::new();
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<ChangeRecord>(line) {
                Ok(r) => records.push(r),
                Err(e) => {
                    tracing::warn!("skipping corrupt history line: {e}");
                }
            }
        }
        Ok(records)
    }

    /// Read recent N entries (newest first).
    pub async fn read_recent(&self, n: usize) -> Result<Vec<ChangeRecord>> {
        let mut all = self.read_all().await?;
        all.reverse();
        all.truncate(n);
        Ok(all)
    }
}
