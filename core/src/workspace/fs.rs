use crate::error::{Result, WorkspaceError};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::time::{Duration, Instant, sleep};
use uuid::Uuid;

/// Small cross-process lock backed by `create_new`.
pub struct ExclusiveLock {
    path: PathBuf,
}

impl ExclusiveLock {
    pub async fn acquire(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut backoff = Duration::from_millis(10);
        const MAX_BACKOFF: Duration = Duration::from_millis(200);

        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .await
            {
                Ok(mut file) => {
                    file.write_all(b"locked").await?;
                    file.flush().await?;
                    return Ok(Self { path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Instant::now() >= deadline {
                        return Err(WorkspaceError::Busy(path.display().to_string()).into());
                    }
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        WorkspaceError::Busy(format!("cannot atomically write {}", path.display()))
    })?;
    fs::create_dir_all(parent).await?;

    let temp_name = format!(".{}.{}.tmp", path.file_name().unwrap_or_default().to_string_lossy(), Uuid::new_v4());
    let temp_path = parent.join(temp_name);

    let result = async {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .await?;
        file.write_all(bytes).await?;
        file.sync_all().await?; // fsync for crash safety
        drop(file);

        fs::rename(&temp_path, path).await?;
        Ok::<(), std::io::Error>(())
    }
    .await;

    if let Err(e) = result {
        // Clean up temp file on failure
        let _ = fs::remove_file(&temp_path).await;
        return Err(e.into());
    }

    Ok(())
}

pub async fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write(path, &bytes).await
}
