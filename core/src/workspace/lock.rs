//! Lock manager – exclusive-access locks on artifacts
//!
//! Locks are stored as small JSON files in `state/locks/`.
//! The lock file name is `<artifact-id>.lock.json`.
//!
//! Locks have a TTL (default 10 minutes). Stale locks are automatically
//! cleaned up on access.

use crate::error::Result;
use crate::workspace::types::{AgentId, LockInfo};
use chrono::{Duration, Utc};
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Default lock TTL: 10 minutes.
const DEFAULT_LOCK_TTL_SECS: i64 = 600;

/// Manages per-artifact locks for exclusive access.
pub struct LockManager {
    root: PathBuf,
    lock_ttl: Duration,
}

impl LockManager {
    pub async fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).await?;
        Ok(Self {
            root,
            lock_ttl: Duration::seconds(DEFAULT_LOCK_TTL_SECS),
        })
    }

    fn lock_path(&self, artifact_id: Uuid) -> PathBuf {
        self.root.join(format!("{artifact_id}.lock.json"))
    }

    /// Returns true if a lock is stale (acquired longer ago than the TTL).
    fn is_stale(&self, info: &LockInfo) -> bool {
        Utc::now() - info.acquired_at > self.lock_ttl
    }

    /// Read a lock file, returning None if not found.  Handles TOCTOU by
    /// catching `NotFound` on the read rather than checking `exists()` first.
    async fn read_lock(&self, artifact_id: Uuid) -> Result<Option<LockInfo>> {
        let path = self.lock_path(artifact_id);
        match fs::read_to_string(&path).await {
            Ok(data) => Ok(Some(serde_json::from_str(&data)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Try to acquire a lock.  Returns `Ok(LockInfo)` on success, or an error
    /// if the artifact is already locked by a different agent.
    ///
    /// Stale locks (older than TTL) are automatically reclaimed.
    pub async fn acquire(&self, artifact_id: Uuid, agent: AgentId) -> Result<LockInfo> {
        let path = self.lock_path(artifact_id);
        let info = LockInfo {
            artifact_id,
            agent: agent.clone(),
            acquired_at: Utc::now(),
        };

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(mut file) => {
                let json = serde_json::to_string_pretty(&info)?;
                file.write_all(json.as_bytes()).await?;
                file.flush().await?;
                Ok(info)
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                // Read existing lock — handle race where file was just deleted
                let data = match fs::read_to_string(&path).await {
                    Ok(d) => d,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // Lock was just released; retry once via recursion
                        return Box::pin(self.acquire(artifact_id, agent)).await;
                    }
                    Err(e) => return Err(e.into()),
                };
                let existing: LockInfo = serde_json::from_str(&data)?;

                // Same agent → idempotent re-lock
                if existing.agent == agent {
                    return Ok(existing);
                }

                // Stale lock → reclaim it
                if self.is_stale(&existing) {
                    tracing::warn!(
                        "reclaiming stale lock on artifact {} (held by {} since {})",
                        artifact_id, existing.agent, existing.acquired_at
                    );
                    let _ = fs::remove_file(&path).await;
                    return Box::pin(self.acquire(artifact_id, agent)).await;
                }

                Err(crate::error::WorkspaceError::ArtifactLocked {
                    artifact_id,
                    held_by: existing.agent,
                }
                .into())
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Release a lock.  Only the agent holding the lock can release it.
    /// Returns whether a lock was actually removed.
    pub async fn release(&self, artifact_id: Uuid, agent: &AgentId) -> Result<bool> {
        let path = self.lock_path(artifact_id);
        let data = match fs::read_to_string(&path).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };

        let existing: LockInfo = serde_json::from_str(&data)?;
        if &existing.agent != agent {
            return Err(crate::error::WorkspaceError::LockNotOwned {
                artifact_id,
                held_by: existing.agent,
                requester: agent.clone(),
            }
            .into());
        }

        match fs::remove_file(&path).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Force-release a lock regardless of owner.
    pub async fn force_release(&self, artifact_id: Uuid) -> Result<bool> {
        let path = self.lock_path(artifact_id);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Check if an artifact is locked (ignoring stale locks).
    pub async fn is_locked(&self, artifact_id: Uuid) -> Result<Option<LockInfo>> {
        match self.read_lock(artifact_id).await? {
            Some(info) if self.is_stale(&info) => {
                // Clean up stale lock
                let _ = fs::remove_file(self.lock_path(artifact_id)).await;
                Ok(None)
            }
            other => Ok(other),
        }
    }

    /// List all active (non-stale) locks.
    pub async fn list_locks(&self) -> Result<Vec<LockInfo>> {
        let mut locks = Vec::new();
        let mut entries = fs::read_dir(&self.root).await?;
        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".lock.json") {
                    let data = match fs::read_to_string(entry.path()).await {
                        Ok(d) => d,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(e.into()),
                    };
                    if let Ok(info) = serde_json::from_str::<LockInfo>(&data) {
                        if !self.is_stale(&info) {
                            locks.push(info);
                        } else {
                            // Clean up stale lock
                            let _ = fs::remove_file(entry.path()).await;
                        }
                    }
                }
            }
        }
        Ok(locks)
    }
}
