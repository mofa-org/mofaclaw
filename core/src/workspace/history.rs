//! Change history – append-only log of workspace mutations
//!
//! All changes are appended as newline-delimited JSON to `history/changes.log`.
//! Appends are serialized with an `ExclusiveLock` to prevent interleaved writes.

use crate::error::Result;
use crate::workspace::fs::ExclusiveLock;
use crate::workspace::types::{AgentId, ChangeAction, ChangeRecord};
use chrono::Utc;
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Manages the append-only change history.
pub struct ChangeHistory {
    log_path: PathBuf,
    lock_path: PathBuf,
}

impl ChangeHistory {
    pub async fn new(history_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&history_dir).await?;
        let log_path = history_dir.join("changes.log");
        let lock_path = history_dir.join("changes.log.lock");
        // Touch the file if it doesn't exist
        if !log_path.exists() {
            fs::write(&log_path, b"").await?;
        }
        Ok(Self { log_path, lock_path })
    }

    /// Append a change record to the log.  The write is serialized via
    /// an `ExclusiveLock` so concurrent appends cannot interleave.
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

        let _guard = ExclusiveLock::acquire(&self.lock_path).await?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.sync_all().await?;

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
    ///
    /// Reads the file from the end to avoid parsing the entire log when only
    /// the tail is needed.
    pub async fn read_recent(&self, n: usize) -> Result<Vec<ChangeRecord>> {
        if n == 0 {
            return Ok(Vec::new());
        }

        let data = fs::read(&self.log_path).await?;
        if data.is_empty() {
            return Ok(Vec::new());
        }

        // Scan backwards to find the last `n` newlines
        let mut records = Vec::with_capacity(n);
        let mut end = data.len();
        // Skip trailing newline
        if end > 0 && data[end - 1] == b'\n' {
            end -= 1;
        }

        while records.len() < n && end > 0 {
            // Find the previous newline (or start of file)
            let start = match data[..end].iter().rposition(|&b| b == b'\n') {
                Some(pos) => pos + 1,
                None => 0,
            };

            let line = std::str::from_utf8(&data[start..end]).unwrap_or("").trim();
            if !line.is_empty() {
                if let Ok(record) = serde_json::from_str::<ChangeRecord>(line) {
                    records.push(record);
                }
            }

            end = if start > 0 { start - 1 } else { 0 };
        }

        Ok(records)
    }
}
