//! Shared workspace for agent team collaboration
//!
//! Provides artifact management, shared context, locking, and change history
//! so that multiple agents can work on a common set of artifacts without
//! conflicts.

pub mod artifact;
pub mod context;
mod fs;
pub mod history;
pub mod lock;
pub mod types;

use crate::error::Result;
use artifact::ArtifactStore;
use context::ContextStore;
use fs::{ExclusiveLock, atomic_write_json};
use history::ChangeHistory;
use lock::LockManager;
use std::path::{Path, PathBuf};
use types::*;
use uuid::Uuid;

/// Façade that ties together all shared-workspace subsystems.
pub struct SharedWorkspace {
    /// Root directory of the shared workspace
    path: PathBuf,
    /// Artifact CRUD + versioning
    pub(crate) artifacts: ArtifactStore,
    /// Shared project knowledge
    pub(crate) context: ContextStore,
    /// Exclusive-access locks
    pub(crate) locks: LockManager,
    /// Append-only change log
    pub(crate) history: ChangeHistory,
    /// Active tasks state
    tasks_path: PathBuf,
}

impl SharedWorkspace {
    /// Initialise (or open) a shared workspace at `root`.
    ///
    /// Creates the full directory tree if it does not exist:
    /// ```text
    /// root/
    /// ├── artifacts/{designs,code,reviews,tests,other}
    /// ├── context/
    /// ├── state/
    /// │   ├── locks/
    /// │   └── active_tasks.json
    /// └── history/
    /// ```
    pub async fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&root).await?;

        let artifacts = ArtifactStore::new(root.join("artifacts")).await?;
        let context = ContextStore::new(root.join("context")).await?;

        let state_dir = root.join("state");
        tokio::fs::create_dir_all(&state_dir).await?;
        let locks = LockManager::new(state_dir.join("locks")).await?;

        let tasks_path = state_dir.join("active_tasks.json");
        if !tasks_path.exists() {
            tokio::fs::write(&tasks_path, b"[]").await?;
        }

        let history = ChangeHistory::new(root.join("history")).await?;

        Ok(Self {
            path: root,
            artifacts,
            context,
            locks,
            history,
            tasks_path,
        })
    }

    /// The root directory of the shared workspace.
    pub fn path(&self) -> &Path {
        &self.path
    }

    // ── Direct artifact access (delegates without history) ────────────

    /// Get an artifact by its ID.
    pub async fn get_artifact(&self, id: Uuid) -> Result<Option<Artifact>> {
        self.artifacts.get(id).await
    }

    /// List artifacts matching optional filters.
    pub async fn list_artifacts(&self, filter: &ArtifactFilter) -> Result<Vec<Artifact>> {
        self.artifacts.list(filter).await
    }

    /// Get all version snapshots for an artifact.
    pub async fn get_artifact_versions(&self, id: Uuid) -> Result<Vec<Artifact>> {
        self.artifacts.get_versions(id).await
    }

    // ── Direct context access ─────────────────────────────────────────

    /// List all decisions.
    pub async fn list_decisions(&self) -> Result<Vec<Decision>> {
        self.context.list_decisions().await
    }

    /// List all constraints.
    pub async fn list_constraints(&self) -> Result<Vec<Constraint>> {
        self.context.list_constraints().await
    }

    /// List all glossary entries.
    pub async fn list_glossary(&self) -> Result<Vec<GlossaryEntry>> {
        self.context.list_glossary().await
    }

    // ── Direct history access ─────────────────────────────────────────

    /// Read the full change history.
    pub async fn read_history(&self) -> Result<Vec<ChangeRecord>> {
        self.history.read_all().await
    }

    /// Read recent N history entries (newest first).
    pub async fn read_recent_history(&self, n: usize) -> Result<Vec<ChangeRecord>> {
        self.history.read_recent(n).await
    }

    // ── Artifact convenience wrappers (with automatic history) ────────

    /// Create an artifact, record the change, and return it.
    pub async fn create_artifact(
        &self,
        name: String,
        artifact_type: ArtifactType,
        owner: AgentId,
        content: Vec<u8>,
    ) -> Result<Artifact> {
        let artifact = self
            .artifacts
            .create(name.clone(), artifact_type, owner.clone(), content)
            .await?;
        self.history
            .record(
                owner,
                ChangeAction::Create,
                Some(artifact.id),
                format!("created artifact '{name}'"),
            )
            .await?;
        Ok(artifact)
    }

    /// Update an artifact (respecting locks), record the change.
    pub async fn update_artifact(
        &self,
        id: Uuid,
        content: Vec<u8>,
        agent: AgentId,
    ) -> Result<Artifact> {
        self.update_artifact_with_strategy(id, content, agent, None, ConflictStrategy::Reject)
            .await
    }

    /// Update an artifact with optional optimistic concurrency checks.
    pub async fn update_artifact_with_strategy(
        &self,
        id: Uuid,
        content: Vec<u8>,
        agent: AgentId,
        expected_version: Option<u32>,
        strategy: ConflictStrategy,
    ) -> Result<Artifact> {
        let _mutation_guard = self.artifacts.acquire_mutation_lock(id).await?;

        // Check lock – if locked by someone else, reject
        if let Some(lock) = self.locks.is_locked(id).await? {
            if lock.agent != agent {
                return Err(crate::error::WorkspaceError::ArtifactLocked {
                    artifact_id: id,
                    held_by: lock.agent,
                }
                .into());
            }
        }

        let artifact = match self
            .artifacts
            .update_inner(id, content, agent.clone(), expected_version, strategy)
            .await
        {
            Ok(artifact) => artifact,
            Err(err) => {
                if let crate::error::MofaclawError::Workspace(
                    crate::error::WorkspaceError::VersionConflict {
                        artifact_id,
                        expected,
                        actual,
                    },
                ) = &err
                {
                    let _ = self
                        .history
                        .record(
                            agent.clone(),
                            ChangeAction::Conflict,
                            Some(*artifact_id),
                            format!(
                                "version conflict on artifact {artifact_id}: expected v{expected}, found v{actual}"
                            ),
                        )
                        .await;
                }
                return Err(err);
            }
        };

        if let Some(expected) = expected_version {
            if artifact.version != expected + 1 {
                self.history
                    .record(
                        agent.clone(),
                        ChangeAction::Conflict,
                        Some(id),
                        format!(
                            "stale update overwritten on artifact {id}: caller expected v{expected}, wrote v{}",
                            artifact.version
                        ),
                    )
                    .await?;
            }
        }

        self.history
            .record(
                agent,
                ChangeAction::Update,
                Some(id),
                format!(
                    "updated artifact '{}' to v{}",
                    artifact.name, artifact.version
                ),
            )
            .await?;
        Ok(artifact)
    }

    /// Update an artifact and reject stale versions.
    pub async fn update_artifact_if_version(
        &self,
        id: Uuid,
        content: Vec<u8>,
        agent: AgentId,
        expected_version: u32,
    ) -> Result<Artifact> {
        self.update_artifact_with_strategy(
            id,
            content,
            agent,
            Some(expected_version),
            ConflictStrategy::Reject,
        )
        .await
    }

    /// Delete an artifact, record the change.
    pub async fn delete_artifact(&self, id: Uuid, agent: AgentId) -> Result<bool> {
        let _mutation_guard = self.artifacts.acquire_mutation_lock(id).await?;

        if let Some(lock) = self.locks.is_locked(id).await? {
            if lock.agent != agent {
                return Err(crate::error::WorkspaceError::ArtifactLocked {
                    artifact_id: id,
                    held_by: lock.agent,
                }
                .into());
            }
        }

        let deleted = self.artifacts.delete_inner(id).await?;
        if deleted {
            self.history
                .record(
                    agent,
                    ChangeAction::Delete,
                    Some(id),
                    format!("deleted artifact {id}"),
                )
                .await?;
        }
        Ok(deleted)
    }

    /// Roll back an artifact to a previous snapshot and record the change.
    pub async fn rollback_artifact(
        &self,
        id: Uuid,
        target_version: u32,
        agent: AgentId,
    ) -> Result<Artifact> {
        let _mutation_guard = self.artifacts.acquire_mutation_lock(id).await?;

        if let Some(lock) = self.locks.is_locked(id).await? {
            if lock.agent != agent {
                return Err(crate::error::WorkspaceError::ArtifactLocked {
                    artifact_id: id,
                    held_by: lock.agent,
                }
                .into());
            }
        }

        let artifact = self
            .artifacts
            .rollback_inner(id, target_version, agent.clone())
            .await?;
        self.history
            .record(
                agent,
                ChangeAction::Update,
                Some(id),
                format!(
                    "rolled back artifact '{}' to v{} (new head v{})",
                    artifact.name, target_version, artifact.version
                ),
            )
            .await?;
        Ok(artifact)
    }

    /// Lock an artifact for exclusive access.
    pub async fn lock_artifact(&self, id: Uuid, agent: AgentId) -> Result<LockInfo> {
        // Ensure artifact exists
        self.artifacts
            .get(id)
            .await?
            .ok_or(crate::error::WorkspaceError::ArtifactNotFound(id))?;

        let lock = self.locks.acquire(id, agent.clone()).await?;
        self.history
            .record(
                agent,
                ChangeAction::Lock,
                Some(id),
                format!("locked artifact {id}"),
            )
            .await?;
        Ok(lock)
    }

    /// Unlock an artifact.
    pub async fn unlock_artifact(&self, id: Uuid, agent: &AgentId) -> Result<bool> {
        let released = self.locks.release(id, agent).await?;
        if released {
            self.history
                .record(
                    agent.clone(),
                    ChangeAction::Unlock,
                    Some(id),
                    format!("unlocked artifact {id}"),
                )
                .await?;
        }
        Ok(released)
    }

    // ── Active tasks ──────────────────────────────────────────────────

    pub async fn list_tasks(&self) -> Result<Vec<ActiveTask>> {
        let data = tokio::fs::read_to_string(&self.tasks_path).await?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn add_task(&self, agent: AgentId, description: String) -> Result<ActiveTask> {
        self.add_task_with_status(agent, description, TaskStatus::Pending)
            .await
    }

    pub async fn add_task_with_status(
        &self,
        agent: AgentId,
        description: String,
        status: TaskStatus,
    ) -> Result<ActiveTask> {
        let _guard = ExclusiveLock::acquire(self.tasks_path.with_extension("json.lock")).await?;
        let mut tasks = self.list_tasks().await?;
        let now = chrono::Utc::now();
        let task = ActiveTask {
            id: Uuid::new_v4(),
            agent: agent.clone(),
            description: description.clone(),
            status: status.clone(),
            created_at: now,
            updated_at: now,
        };
        tasks.push(task.clone());
        atomic_write_json(&self.tasks_path, &tasks).await?;
        self.history
            .record(
                agent,
                ChangeAction::TaskUpdate,
                None,
                format!(
                    "created task '{}' with status {}",
                    description,
                    status_label(&status)
                ),
            )
            .await?;
        Ok(task)
    }

    pub async fn update_task_status(&self, task_id: Uuid, status: TaskStatus) -> Result<bool> {
        let _guard = ExclusiveLock::acquire(self.tasks_path.with_extension("json.lock")).await?;
        let mut tasks = self.list_tasks().await?;
        let mut found = false;
        let mut task_description = None;
        let mut task_agent = None;
        for t in &mut tasks {
            if t.id == task_id {
                t.status = status.clone();
                t.updated_at = chrono::Utc::now();
                task_description = Some(t.description.clone());
                task_agent = Some(t.agent.clone());
                found = true;
                break;
            }
        }
        if found {
            atomic_write_json(&self.tasks_path, &tasks).await?;
            self.history
                .record(
                    task_agent.unwrap_or_else(|| AgentId("system".to_string())),
                    ChangeAction::TaskUpdate,
                    None,
                    format!(
                        "updated task '{}' to {}",
                        task_description.unwrap_or_else(|| task_id.to_string()),
                        status_label(&status)
                    ),
                )
                .await?;
        }
        Ok(found)
    }

    pub async fn remove_task(&self, task_id: Uuid) -> Result<bool> {
        let _guard = ExclusiveLock::acquire(self.tasks_path.with_extension("json.lock")).await?;
        let mut tasks = self.list_tasks().await?;
        let before = tasks.len();
        let removed_task = tasks.iter().find(|task| task.id == task_id).cloned();
        tasks.retain(|t| t.id != task_id);
        if tasks.len() == before {
            return Ok(false);
        }
        atomic_write_json(&self.tasks_path, &tasks).await?;
        if let Some(task) = removed_task {
            self.history
                .record(
                    task.agent,
                    ChangeAction::TaskUpdate,
                    None,
                    format!("removed task '{}'", task.description),
                )
                .await?;
        }
        Ok(true)
    }

    pub async fn add_decision(
        &self,
        title: String,
        rationale: String,
        agent: AgentId,
    ) -> Result<Decision> {
        let decision = self
            .context
            .add_decision(title.clone(), rationale, agent.clone())
            .await?;
        self.history
            .record(
                agent,
                ChangeAction::ContextUpdate,
                None,
                format!("added decision '{title}'"),
            )
            .await?;
        Ok(decision)
    }

    pub async fn remove_decision(&self, id: Uuid, agent: AgentId) -> Result<bool> {
        let removed = self.context.remove_decision(id).await?;
        if removed {
            self.history
                .record(
                    agent,
                    ChangeAction::ContextUpdate,
                    None,
                    format!("removed decision {id}"),
                )
                .await?;
        }
        Ok(removed)
    }

    pub async fn add_constraint(
        &self,
        name: String,
        description: String,
        agent: AgentId,
    ) -> Result<Constraint> {
        let constraint = self
            .context
            .add_constraint(name.clone(), description, agent.clone())
            .await?;
        self.history
            .record(
                agent,
                ChangeAction::ContextUpdate,
                None,
                format!("added constraint '{name}'"),
            )
            .await?;
        Ok(constraint)
    }

    pub async fn remove_constraint(&self, id: Uuid, agent: AgentId) -> Result<bool> {
        let removed = self.context.remove_constraint(id).await?;
        if removed {
            self.history
                .record(
                    agent,
                    ChangeAction::ContextUpdate,
                    None,
                    format!("removed constraint {id}"),
                )
                .await?;
        }
        Ok(removed)
    }

    pub async fn add_glossary_entry(
        &self,
        term: String,
        definition: String,
        agent: AgentId,
    ) -> Result<GlossaryEntry> {
        let entry = self
            .context
            .add_glossary_entry(term.clone(), definition, agent.clone())
            .await?;
        self.history
            .record(
                agent,
                ChangeAction::ContextUpdate,
                None,
                format!("added glossary term '{term}'"),
            )
            .await?;
        Ok(entry)
    }

    pub async fn remove_glossary_entry(&self, term: &str, agent: AgentId) -> Result<bool> {
        let removed = self.context.remove_glossary_entry(term).await?;
        if removed {
            self.history
                .record(
                    agent,
                    ChangeAction::ContextUpdate,
                    None,
                    format!("removed glossary term '{term}'"),
                )
                .await?;
        }
        Ok(removed)
    }

    pub async fn dashboard(&self, recent_changes: usize) -> Result<WorkspaceDashboard> {
        let artifact_count = self.artifacts.list(&ArtifactFilter::default()).await?.len();
        let active_tasks = self
            .list_tasks()
            .await?
            .into_iter()
            .filter(|task| matches!(task.status, TaskStatus::Pending | TaskStatus::InProgress))
            .collect();
        let locks = self.locks.list_locks().await?;
        let recent_changes = self.history.read_recent(recent_changes).await?;

        Ok(WorkspaceDashboard {
            artifact_count,
            active_tasks,
            locks,
            recent_changes,
        })
    }
}

fn status_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
    }
}
