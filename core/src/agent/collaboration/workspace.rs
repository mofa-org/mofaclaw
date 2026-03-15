//! Shared workspace for multi-agent collaboration
//!
//! Provides artifact management, versioning, and conflict resolution for
//! shared artifacts across agents in a team.

use crate::agent::communication::AgentId;
use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Type of artifact
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactType {
    /// Code file
    CodeFile { path: PathBuf },
    /// Design document
    DesignDoc { path: PathBuf },
    /// Test file
    TestFile { path: PathBuf },
    /// Review comment
    ReviewComment { path: PathBuf },
    /// Other type of artifact
    Other { path: PathBuf },
}

impl ArtifactType {
    /// Get the path for this artifact type
    pub fn path(&self) -> &Path {
        match self {
            ArtifactType::CodeFile { path }
            | ArtifactType::DesignDoc { path }
            | ArtifactType::TestFile { path }
            | ArtifactType::ReviewComment { path }
            | ArtifactType::Other { path } => path,
        }
    }
}

/// Content of an artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArtifactContent {
    /// File content stored directly
    FileContent { content: String },
    /// Reference to a file path
    FileReference { path: PathBuf },
}

/// An artifact in the shared workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Unique artifact identifier
    pub id: String,
    /// Artifact name
    pub name: String,
    /// Type of artifact
    pub artifact_type: ArtifactType,
    /// Artifact content
    pub content: ArtifactContent,
    /// Current version number
    pub version: u32,
    /// Agent that created this artifact
    pub created_by: AgentId,
    /// When the artifact was created
    pub created_at: DateTime<Utc>,
    /// Agent that last modified this artifact
    pub modified_by: AgentId,
    /// When the artifact was last modified
    pub modified_at: DateTime<Utc>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Artifact {
    /// Create a new artifact
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        artifact_type: ArtifactType,
        content: ArtifactContent,
        created_by: AgentId,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: name.into(),
            artifact_type,
            content,
            version: 1,
            created_by: created_by.clone(),
            created_at: now,
            modified_by: created_by,
            modified_at: now,
            metadata: HashMap::new(),
        }
    }

    /// Create a new version of this artifact
    pub fn new_version(&self, content: ArtifactContent, modified_by: AgentId) -> Self {
        let mut new = self.clone();
        new.content = content;
        new.version += 1;
        new.modified_by = modified_by;
        new.modified_at = Utc::now();
        new
    }
}

/// Conflict resolution strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Last write wins (overwrites previous version)
    LastWriteWins,
    /// Manual merge required
    ManualMerge,
    /// Automatic merge (for simple cases)
    AutomaticMerge,
}

/// Shared workspace for a team
pub struct SharedWorkspace {
    /// Team ID this workspace belongs to
    team_id: String,
    /// Artifacts by ID
    artifacts: Arc<RwLock<HashMap<String, Artifact>>>,
    /// Artifact versions (artifact_id -> Vec<version_number>)
    artifact_versions: Arc<RwLock<HashMap<String, Vec<u32>>>>,
    /// Base directory for this workspace
    base_dir: PathBuf,
    /// Conflict resolution strategy
    conflict_strategy: ConflictStrategy,
}

impl SharedWorkspace {
    /// Create a new shared workspace
    pub fn new(team_id: impl Into<String>, base_dir: PathBuf) -> Self {
        Self {
            team_id: team_id.into(),
            artifacts: Arc::new(RwLock::new(HashMap::new())),
            artifact_versions: Arc::new(RwLock::new(HashMap::new())),
            base_dir,
            conflict_strategy: ConflictStrategy::LastWriteWins,
        }
    }

    /// Load workspace from disk
    pub async fn load(team_id: impl Into<String>, base_dir: PathBuf) -> Result<Self> {
        let team_id = team_id.into();
        let workspace_dir = base_dir.join(&team_id);
        let artifacts_file = workspace_dir.join("artifacts.json");

        let workspace = Self::new(&team_id, base_dir);

        // Load artifacts from disk if they exist
        if artifacts_file.exists() {
            match fs::read_to_string(&artifacts_file).await {
                Ok(content) => match serde_json::from_str::<HashMap<String, Artifact>>(&content) {
                    Ok(artifacts) => {
                        let mut artifacts_guard = workspace.artifacts.write().await;
                        for (id, artifact) in artifacts {
                            let version = artifact.version;
                            artifacts_guard.insert(id.clone(), artifact);
                            workspace
                                .artifact_versions
                                .write()
                                .await
                                .entry(id)
                                .or_insert_with(Vec::new)
                                .push(version);
                        }
                        info!("Loaded {} artifacts from disk", artifacts_guard.len());
                    }
                    Err(e) => warn!("Failed to parse artifacts file: {}", e),
                },
                Err(e) => warn!("Failed to read artifacts file: {}", e),
            }
        }

        Ok(workspace)
    }

    /// Save workspace to disk
    async fn save_to_disk(&self) -> Result<()> {
        let workspace_dir = self.base_dir.join(&self.team_id);
        fs::create_dir_all(&workspace_dir).await?;

        let artifacts_file = workspace_dir.join("artifacts.json");
        let artifacts = self.artifacts.read().await;
        let json = serde_json::to_string_pretty(&*artifacts).map_err(|e| {
            crate::error::MofaclawError::Other(format!("Serialization error: {}", e))
        })?;
        fs::write(&artifacts_file, json).await?;

        debug!("Saved {} artifacts to disk", artifacts.len());
        Ok(())
    }

    /// Create a new artifact
    pub async fn create_artifact(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
        artifact_type: ArtifactType,
        content: ArtifactContent,
        created_by: AgentId,
    ) -> Result<Artifact> {
        self.create_artifact_with_rbac(id, name, artifact_type, content, created_by, None)
            .await
    }

    /// Create a new artifact with RBAC check
    pub async fn create_artifact_with_rbac(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
        artifact_type: ArtifactType,
        content: ArtifactContent,
        created_by: AgentId,
        role_capabilities: Option<&crate::agent::roles::RoleCapabilities>,
    ) -> Result<Artifact> {
        let id = id.into();

        // RBAC check: verify agent can write files
        if let Some(caps) = role_capabilities
            && !caps.can_write_files
        {
            return Err(crate::error::MofaclawError::Other(format!(
                "Agent {} does not have permission to create artifacts (write_files capability required)",
                created_by
            )));
        }

        info!("Creating artifact {} in workspace {}", id, self.team_id);

        // Check if artifact already exists
        let mut artifacts = self.artifacts.write().await;
        if artifacts.contains_key(&id) {
            return Err(crate::error::MofaclawError::Other(format!(
                "Artifact with id {} already exists in workspace {}",
                id, self.team_id
            )));
        }

        let artifact = Artifact::new(id.clone(), name, artifact_type, content, created_by);

        // Store artifact
        artifacts.insert(id.clone(), artifact.clone());
        drop(artifacts);

        // Track version
        let mut versions = self.artifact_versions.write().await;
        versions.insert(id, vec![1]);
        drop(versions);

        // Save to disk
        if let Err(e) = self.save_to_disk().await {
            warn!("Failed to save artifact to disk: {}", e);
        }

        debug!("Artifact {} created (version 1)", artifact.id);
        Ok(artifact)
    }

    /// Update an existing artifact (creates a new version)
    pub async fn update_artifact(
        &self,
        artifact_id: &str,
        content: ArtifactContent,
        modified_by: AgentId,
    ) -> Result<Artifact> {
        self.update_artifact_with_rbac(artifact_id, content, modified_by, None)
            .await
    }

    /// Update an existing artifact with RBAC check
    pub async fn update_artifact_with_rbac(
        &self,
        artifact_id: &str,
        content: ArtifactContent,
        modified_by: AgentId,
        role_capabilities: Option<&crate::agent::roles::RoleCapabilities>,
    ) -> Result<Artifact> {
        // RBAC check: verify agent can write files
        if let Some(caps) = role_capabilities
            && !caps.can_write_files
        {
            return Err(crate::error::MofaclawError::Other(format!(
                "Agent {} does not have permission to update artifacts (write_files capability required)",
                modified_by
            )));
        }

        info!(
            "Updating artifact {} in workspace {}",
            artifact_id, self.team_id
        );

        let mut artifacts = self.artifacts.write().await;
        let artifact = artifacts.get(artifact_id).ok_or_else(|| {
            crate::error::MofaclawError::Other(format!("Artifact {} not found", artifact_id))
        })?;

        // Create new version
        let new_artifact = artifact.new_version(content, modified_by);

        // Store new version
        artifacts.insert(artifact_id.to_string(), new_artifact.clone());
        drop(artifacts);

        // Track version
        let mut versions = self.artifact_versions.write().await;
        versions
            .entry(artifact_id.to_string())
            .or_insert_with(Vec::new)
            .push(new_artifact.version);
        drop(versions);

        // Save to disk
        if let Err(e) = self.save_to_disk().await {
            warn!("Failed to save artifact to disk: {}", e);
        }

        debug!(
            "Artifact {} updated to version {}",
            artifact_id, new_artifact.version
        );
        Ok(new_artifact)
    }

    /// Get an artifact by ID
    pub async fn get_artifact(&self, artifact_id: &str) -> Option<Artifact> {
        self.get_artifact_with_rbac(artifact_id, None).await
    }

    /// Get an artifact by ID with RBAC check
    pub async fn get_artifact_with_rbac(
        &self,
        artifact_id: &str,
        role_capabilities: Option<&crate::agent::roles::RoleCapabilities>,
    ) -> Option<Artifact> {
        // RBAC check: verify agent can read files
        if let Some(caps) = role_capabilities
            && !caps.can_read_files
        {
            warn!(
                "Agent does not have permission to read artifacts (read_files capability required)"
            );
            return None;
        }

        let artifacts = self.artifacts.read().await;
        artifacts.get(artifact_id).cloned()
    }

    /// Get artifact version history
    pub async fn get_artifact_versions(&self, artifact_id: &str) -> Vec<u32> {
        let versions = self.artifact_versions.read().await;
        versions.get(artifact_id).cloned().unwrap_or_default()
    }

    /// List all artifacts
    pub async fn list_artifacts(&self) -> Vec<String> {
        let artifacts = self.artifacts.read().await;
        artifacts.keys().cloned().collect()
    }

    /// Get artifacts by type
    pub async fn get_artifacts_by_type(&self, artifact_type: &ArtifactType) -> Vec<Artifact> {
        let artifacts = self.artifacts.read().await;
        artifacts
            .values()
            .filter(|a| {
                matches!(
                    (&a.artifact_type, artifact_type),
                    (ArtifactType::CodeFile { .. }, ArtifactType::CodeFile { .. })
                        | (
                            ArtifactType::DesignDoc { .. },
                            ArtifactType::DesignDoc { .. }
                        )
                        | (ArtifactType::TestFile { .. }, ArtifactType::TestFile { .. })
                        | (
                            ArtifactType::ReviewComment { .. },
                            ArtifactType::ReviewComment { .. }
                        )
                        | (ArtifactType::Other { .. }, ArtifactType::Other { .. })
                )
            })
            .cloned()
            .collect()
    }

    /// Resolve conflicts when multiple agents modify the same artifact
    pub async fn resolve_conflicts(
        &self,
        artifact_id: &str,
        strategy: ConflictStrategy,
    ) -> Result<()> {
        warn!(
            "Conflict resolution for artifact {} using strategy {:?}",
            artifact_id, strategy
        );

        match strategy {
            ConflictStrategy::LastWriteWins => {
                // Already handled by update_artifact - last write wins by default
                Ok(())
            }
            ConflictStrategy::ManualMerge => {
                // Mark artifact as needing manual merge
                let mut artifacts = self.artifacts.write().await;
                if let Some(artifact) = artifacts.get_mut(artifact_id) {
                    artifact
                        .metadata
                        .insert("merge_required".to_string(), serde_json::json!(true));
                }
                Ok(())
            }
            ConflictStrategy::AutomaticMerge => {
                // Implement simple automatic merge for text content
                let mut artifacts = self.artifacts.write().await;
                if let Some(artifact) = artifacts.get(artifact_id) {
                    // For file content, attempt simple merge
                    if let ArtifactContent::FileContent { content } = &artifact.content {
                        // Simple merge: append conflict markers
                        // In a real implementation, this would use a proper diff/merge algorithm
                        let merged_content = format!(
                            "{}\n\n<<<<<<< CONFLICT\n=======\n>>>>>>> END CONFLICT\n",
                            content
                        );
                        let mut merged_artifact = artifact.clone();
                        merged_artifact.content = ArtifactContent::FileContent {
                            content: merged_content,
                        };
                        merged_artifact
                            .metadata
                            .insert("auto_merged".to_string(), serde_json::json!(true));
                        artifacts.insert(artifact_id.to_string(), merged_artifact);
                        info!("Auto-merged artifact {}", artifact_id);
                    } else {
                        // For non-file content, fall back to last write wins
                        warn!(
                            "Automatic merge only supported for file content, using last write wins"
                        );
                    }
                }
                Ok(())
            }
        }
    }

    /// Get the base directory for this workspace
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Set conflict resolution strategy
    pub fn set_conflict_strategy(&mut self, strategy: ConflictStrategy) {
        self.conflict_strategy = strategy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_artifact_creation() {
        let workspace = SharedWorkspace::new("team1", PathBuf::from("/tmp"));
        let agent_id = AgentId::new("team1", "developer", "dev-1");

        let artifact = workspace
            .create_artifact(
                "artifact1",
                "Test Artifact",
                ArtifactType::CodeFile {
                    path: PathBuf::from("test.rs"),
                },
                ArtifactContent::FileContent {
                    content: "fn main() {}".to_string(),
                },
                agent_id,
            )
            .await
            .unwrap();

        assert_eq!(artifact.id, "artifact1");
        assert_eq!(artifact.version, 1);
    }

    #[tokio::test]
    async fn test_artifact_update() {
        let workspace = SharedWorkspace::new("team1", PathBuf::from("/tmp"));
        let agent_id = AgentId::new("team1", "developer", "dev-1");
        let reviewer_id = AgentId::new("team1", "reviewer", "rev-1");

        // Create artifact
        workspace
            .create_artifact(
                "artifact1",
                "Test Artifact",
                ArtifactType::CodeFile {
                    path: PathBuf::from("test.rs"),
                },
                ArtifactContent::FileContent {
                    content: "fn main() {}".to_string(),
                },
                agent_id.clone(),
            )
            .await
            .unwrap();

        // Update artifact
        let updated = workspace
            .update_artifact(
                "artifact1",
                ArtifactContent::FileContent {
                    content: "fn main() { println!(\"Hello\"); }".to_string(),
                },
                reviewer_id,
            )
            .await
            .unwrap();

        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn test_get_artifact() {
        let workspace = SharedWorkspace::new("team1", PathBuf::from("/tmp"));
        let agent_id = AgentId::new("team1", "developer", "dev-1");

        workspace
            .create_artifact(
                "artifact1",
                "Test",
                ArtifactType::CodeFile {
                    path: PathBuf::from("test.rs"),
                },
                ArtifactContent::FileContent {
                    content: "test".to_string(),
                },
                agent_id,
            )
            .await
            .unwrap();

        let artifact = workspace.get_artifact("artifact1").await;
        assert!(artifact.is_some());
        assert_eq!(artifact.unwrap().id, "artifact1");
    }

    #[tokio::test]
    async fn test_list_artifacts() {
        let workspace = SharedWorkspace::new("team1", PathBuf::from("/tmp"));
        let agent_id = AgentId::new("team1", "developer", "dev-1");

        workspace
            .create_artifact(
                "artifact1",
                "Test 1",
                ArtifactType::CodeFile {
                    path: PathBuf::from("test1.rs"),
                },
                ArtifactContent::FileContent {
                    content: "test1".to_string(),
                },
                agent_id.clone(),
            )
            .await
            .unwrap();

        workspace
            .create_artifact(
                "artifact2",
                "Test 2",
                ArtifactType::CodeFile {
                    path: PathBuf::from("test2.rs"),
                },
                ArtifactContent::FileContent {
                    content: "test2".to_string(),
                },
                agent_id,
            )
            .await
            .unwrap();

        let artifacts = workspace.list_artifacts().await;
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts.contains(&"artifact1".to_string()));
        assert!(artifacts.contains(&"artifact2".to_string()));
    }
}
