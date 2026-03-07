//! Artifact store – persistent CRUD with version history
//!
//! Each artifact is stored as a JSON metadata file plus a raw content file inside
//! `artifacts/<type>/`.  Version history is kept as `<id>.v<N>.json` snapshots.

use crate::error::Result;
use crate::workspace::fs::{ExclusiveLock, atomic_write, atomic_write_json};
use crate::workspace::types::{AgentId, Artifact, ArtifactFilter, ArtifactType, ConflictStrategy};
use chrono::Utc;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, warn};
use uuid::Uuid;

/// Manages artifact persistence on disk.
pub struct ArtifactStore {
    /// Root path for artifacts (e.g. `~/.mofaclaw/workspace/artifacts`)
    root: PathBuf,
}

impl ArtifactStore {
    /// Create a new store rooted at `root`, ensuring sub-directories exist.
    pub async fn new(root: PathBuf) -> Result<Self> {
        for sub in &["designs", "code", "reviews", "tests", "other"] {
            fs::create_dir_all(root.join(sub)).await?;
        }
        fs::create_dir_all(root.join(".locks")).await?;
        Ok(Self { root })
    }

    // ── helpers ────────────────────────────────────────────────────────

    fn type_dir(&self, at: &ArtifactType) -> PathBuf {
        let name = match at {
            ArtifactType::Design => "designs",
            ArtifactType::Code => "code",
            ArtifactType::Review => "reviews",
            ArtifactType::Test => "tests",
            ArtifactType::Other(_) => "other",
        };
        self.root.join(name)
    }

    fn meta_path(&self, at: &ArtifactType, id: Uuid) -> PathBuf {
        self.type_dir(at).join(format!("{id}.json"))
    }

    fn content_path(&self, at: &ArtifactType, id: Uuid) -> PathBuf {
        self.type_dir(at).join(format!("{id}.content"))
    }

    fn version_path(&self, at: &ArtifactType, id: Uuid, version: u32) -> PathBuf {
        self.type_dir(at).join(format!("{id}.v{version}.json"))
    }

    fn mutation_lock_path(&self, id: Uuid) -> PathBuf {
        self.root.join(".locks").join(format!("{id}.write.lock"))
    }

    async fn write_artifact(&self, artifact: &Artifact) -> Result<()> {
        let meta = self.meta_path(&artifact.artifact_type, artifact.id);
        let content = self.content_path(&artifact.artifact_type, artifact.id);

        // Write metadata (without bulky content duplicated – content lives in the .content file)
        atomic_write_json(&meta, artifact).await?;
        atomic_write(&content, &artifact.content).await?;

        Ok(())
    }

    async fn save_version_snapshot(&self, artifact: &Artifact) -> Result<()> {
        let path = self.version_path(
            &artifact.artifact_type,
            artifact.id,
            artifact.version,
        );
        atomic_write_json(&path, artifact).await?;
        Ok(())
    }

    async fn read_meta(&self, path: &Path) -> Result<Option<Artifact>> {
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(path).await?;
        match serde_json::from_str::<Artifact>(&data) {
            Ok(a) => Ok(Some(a)),
            Err(e) => {
                warn!("corrupt artifact metadata at {}: {e}", path.display());
                Ok(None)
            }
        }
    }

    // ── public API ────────────────────────────────────────────────────

    /// Create a new artifact and persist it.  Returns the assigned UUID.
    pub async fn create(
        &self,
        name: String,
        artifact_type: ArtifactType,
        owner: AgentId,
        content: Vec<u8>,
    ) -> Result<Artifact> {
        let artifact_id = Uuid::new_v4();
        let _guard = ExclusiveLock::acquire(self.mutation_lock_path(artifact_id)).await?;
        let now = Utc::now();
        let artifact = Artifact {
            id: artifact_id,
            name,
            artifact_type,
            owner,
            version: 1,
            content,
            created_at: now,
            updated_at: now,
        };
        self.write_artifact(&artifact).await?;
        self.save_version_snapshot(&artifact).await?;
        debug!("created artifact {} ({})", artifact.id, artifact.name);
        Ok(artifact)
    }

    /// Get an artifact by its ID.  We search all type sub-dirs if the type is
    /// unknown.
    pub async fn get(&self, id: Uuid) -> Result<Option<Artifact>> {
        for sub in &["designs", "code", "reviews", "tests", "other"] {
            let meta = self.root.join(sub).join(format!("{id}.json"));
            if let Some(a) = self.read_meta(&meta).await? {
                return Ok(Some(a));
            }
        }
        Ok(None)
    }

    /// Update an existing artifact's content, bumping its version.
    pub async fn update(
        &self,
        id: Uuid,
        content: Vec<u8>,
        updater: AgentId,
        expected_version: Option<u32>,
        strategy: ConflictStrategy,
    ) -> Result<Artifact> {
        let _guard = ExclusiveLock::acquire(self.mutation_lock_path(id)).await?;
        let mut artifact = self
            .get(id)
            .await?
            .ok_or_else(|| crate::error::WorkspaceError::ArtifactNotFound(id))?;

        if let Some(expected) = expected_version {
            if artifact.version != expected && matches!(strategy, ConflictStrategy::Reject) {
                return Err(crate::error::WorkspaceError::VersionConflict {
                    artifact_id: id,
                    expected,
                    actual: artifact.version,
                }
                .into());
            }
        }

        artifact.version += 1;
        artifact.content = content;
        artifact.owner = updater;
        artifact.updated_at = Utc::now();

        self.write_artifact(&artifact).await?;
        self.save_version_snapshot(&artifact).await?;
        debug!("updated artifact {} to v{}", id, artifact.version);
        Ok(artifact)
    }

    /// Delete an artifact and all its version snapshots.
    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let _guard = ExclusiveLock::acquire(self.mutation_lock_path(id)).await?;
        let artifact = match self.get(id).await? {
            Some(a) => a,
            None => return Ok(false),
        };
        let dir = self.type_dir(&artifact.artifact_type);

        // Remove meta, content, and all version snapshots
        let _ = fs::remove_file(self.meta_path(&artifact.artifact_type, id)).await;
        let _ = fs::remove_file(self.content_path(&artifact.artifact_type, id)).await;

        let mut entries = fs::read_dir(&dir).await?;
        let prefix = format!("{id}.v");
        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&prefix) {
                    let _ = fs::remove_file(entry.path()).await;
                }
            }
        }

        debug!("deleted artifact {id}");
        Ok(true)
    }

    /// List artifacts matching optional filters.
    pub async fn list(&self, filter: &ArtifactFilter) -> Result<Vec<Artifact>> {
        let mut results = Vec::new();

        let subdirs: Vec<&str> = if let Some(ref at) = filter.artifact_type {
            vec![match at {
                ArtifactType::Design => "designs",
                ArtifactType::Code => "code",
                ArtifactType::Review => "reviews",
                ArtifactType::Test => "tests",
                ArtifactType::Other(_) => "other",
            }]
        } else {
            vec!["designs", "code", "reviews", "tests", "other"]
        };

        for sub in subdirs {
            let dir = self.root.join(sub);
            if !dir.exists() {
                continue;
            }
            let mut entries = fs::read_dir(&dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                // Only read top-level meta files (not version snapshots or content)
                if name.ends_with(".json") && !name.contains(".v") {
                    if let Some(artifact) = self.read_meta(&entry.path()).await? {
                        // Apply filters
                        if let Some(ref owner) = filter.owner {
                            if &artifact.owner != owner {
                                continue;
                            }
                        }
                        if let Some(ref needle) = filter.name_contains {
                            if !artifact.name.to_lowercase().contains(&needle.to_lowercase()) {
                                continue;
                            }
                        }
                        results.push(artifact);
                    }
                }
            }
        }

        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(results)
    }

    /// Get all version snapshots for an artifact, ordered oldest → newest.
    pub async fn get_versions(&self, id: Uuid) -> Result<Vec<Artifact>> {
        let current = match self.get(id).await? {
            Some(a) => a,
            None => return Ok(Vec::new()),
        };
        let dir = self.type_dir(&current.artifact_type);
        let prefix = format!("{id}.v");
        let mut versions = Vec::new();

        let mut entries = fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&prefix) && name.ends_with(".json") {
                    if let Some(a) = self.read_meta(&entry.path()).await? {
                        versions.push(a);
                    }
                }
            }
        }

        versions.sort_by_key(|a| a.version);
        Ok(versions)
    }

    /// Rollback an artifact to a previous version.
    pub async fn rollback(
        &self,
        id: Uuid,
        target_version: u32,
        agent: AgentId,
    ) -> Result<Artifact> {
        let versions = self.get_versions(id).await?;
        let snapshot = versions
            .into_iter()
            .find(|a| a.version == target_version)
            .ok_or_else(|| {
                crate::error::WorkspaceError::VersionNotFound(id, target_version)
            })?;

        // Create a new version whose content matches the old snapshot
        self.update(
            id,
            snapshot.content,
            agent,
            None,
            ConflictStrategy::Reject,
        )
        .await
    }
}
