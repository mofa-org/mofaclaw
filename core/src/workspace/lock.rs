//! Lock manager – exclusive-access locks on artifacts
//!
//! Locks are stored as small JSON files in `state/locks/`.
//! The lock file name is `<artifact-id>.lock.json`.

use crate::error::Result;
use crate::workspace::types::{AgentId, LockInfo};
use chrono::Utc;
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Manages per-artifact locks for exclusive access.
pub struct LockManager {
    root: PathBuf,
}

impl LockManager {
    pub async fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).await?;
        Ok(Self { root })
    }

    fn lock_path(&self, artifact_id: Uuid) -> PathBuf {
        self.root.join(format!("{artifact_id}.lock.json"))
    }

    /// Try to acquire a lock.  Returns `Ok(LockInfo)` on success, or an error
    /// if the artifact is already locked by a different agent.
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
                let data = fs::read_to_string(&path).await?;
                let existing: LockInfo = serde_json::from_str(&data)?;
                if existing.agent == agent {
                    return Ok(existing);
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

    /// Release a lock.  Only the agent holding the lock (or a forced release)
    /// can release it. Returns whether a lock was actually removed.
    pub async fn release(&self, artifact_id: Uuid, agent: &AgentId) -> Result<bool> {
        let path = self.lock_path(artifact_id);
        if !path.exists() {
            return Ok(false);
        }

        let data = fs::read_to_string(&path).await?;
        let existing: LockInfo = serde_json::from_str(&data)?;
        if &existing.agent != agent {
            return Err(crate::error::WorkspaceError::LockNotOwned {
                artifact_id,
                held_by: existing.agent,
                requester: agent.clone(),
            }
            .into());
        }

        fs::remove_file(&path).await?;
        Ok(true)
    }

    /// Force-release a lock regardless of owner.
    pub async fn force_release(&self, artifact_id: Uuid) -> Result<bool> {
        let path = self.lock_path(artifact_id);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path).await?;
        Ok(true)
    }

    /// Check if an artifact is locked.
    pub async fn is_locked(&self, artifact_id: Uuid) -> Result<Option<LockInfo>> {
        let path = self.lock_path(artifact_id);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path).await?;
        Ok(Some(serde_json::from_str(&data)?))
    }

    /// List all active locks.
    pub async fn list_locks(&self) -> Result<Vec<LockInfo>> {
        let mut locks = Vec::new();
        let mut entries = fs::read_dir(&self.root).await?;
        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".lock.json") {
                    let data = fs::read_to_string(entry.path()).await?;
                    if let Ok(info) = serde_json::from_str::<LockInfo>(&data) {
                        locks.push(info);
                    }
                }
            }
        }
        Ok(locks)
    }
}
