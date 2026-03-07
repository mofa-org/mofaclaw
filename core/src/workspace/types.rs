
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for AgentId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Type of artifact stored in the workspace
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    /// Architecture / design documents
    Design,
    /// Code changes in progress
    Code,
    /// Review feedback
    Review,
    /// Test results
    Test,
    /// Generic artifact
    Other(String),
}

impl fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Design => write!(f, "design"),
            Self::Code => write!(f, "code"),
            Self::Review => write!(f, "review"),
            Self::Test => write!(f, "test"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

/// A shared artifact stored in the workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Unique identifier
    pub id: Uuid,
    /// Human-readable name
    pub name: String,
    /// Classification of the artifact
    pub artifact_type: ArtifactType,
    /// Agent that originally created the artifact
    pub created_by: AgentId,
    /// Agent that last modified the artifact
    pub last_modified_by: AgentId,
    /// Monotonically increasing version number
    pub version: u32,
    /// Raw content bytes (base64-encoded in JSON)
    #[serde(with = "base64_bytes")]
    pub content: Vec<u8>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

/// Filter criteria when listing artifacts
#[derive(Debug, Clone, Default)]
pub struct ArtifactFilter {
    /// Filter by artifact type
    pub artifact_type: Option<ArtifactType>,
    /// Filter by owner
    pub owner: Option<AgentId>,
    /// Filter by name substring (case-insensitive)
    pub name_contains: Option<String>,
}

/// A record of a change to the workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRecord {
    /// Timestamp of the change
    pub timestamp: DateTime<Utc>,
    /// Agent that made the change
    pub agent: AgentId,
    /// Kind of change
    pub action: ChangeAction,
    /// ID of the affected artifact (if applicable)
    pub artifact_id: Option<Uuid>,
    /// Free-form description
    pub description: String,
}

/// Types of changes tracked in the history
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeAction {
    Create,
    Update,
    Delete,
    Lock,
    Unlock,
    ContextUpdate,
    TaskUpdate,
    Conflict,
}

/// Strategy to apply when an update sees an unexpected artifact version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    /// Reject the update and surface a version-conflict error.
    Reject,
    /// Accept the update even if the caller's view is stale.
    Overwrite,
}

/// An entry in the decision log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// Unique id
    pub id: Uuid,
    /// Short title
    pub title: String,
    /// Full rationale
    pub rationale: String,
    /// Agent that recorded the decision
    pub decided_by: AgentId,
    /// When it was recorded
    pub decided_at: DateTime<Utc>,
}

/// A constraint registered in the workspace context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    /// Unique id
    pub id: Uuid,
    /// Short label
    pub name: String,
    /// Detailed description
    pub description: String,
    /// Agent that registered the constraint
    pub added_by: AgentId,
    /// When it was added
    pub added_at: DateTime<Utc>,
}

/// A glossary term for shared domain terminology
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlossaryEntry {
    /// The term
    pub term: String,
    /// Its definition
    pub definition: String,
    /// Agent that added it
    pub added_by: AgentId,
    /// When it was added
    pub added_at: DateTime<Utc>,
}

/// Information about an active lock
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    /// Artifact being locked
    pub artifact_id: Uuid,
    /// Agent holding the lock
    pub agent: AgentId,
    /// When the lock was acquired
    pub acquired_at: DateTime<Utc>,
}

/// Active task entry for state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveTask {
    /// Unique task id
    pub id: Uuid,
    /// Agent assigned to the task
    pub agent: AgentId,
    /// Description of the task
    pub description: String,
    /// Current status
    pub status: TaskStatus,
    /// When the task was created
    pub created_at: DateTime<Utc>,
    /// When the task was last updated
    pub updated_at: DateTime<Utc>,
}

/// Status of an active task
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Snapshot of the shared workspace state for dashboards or status views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDashboard {
    /// Number of artifacts currently stored in the workspace.
    pub artifact_count: usize,
    /// Tasks that are still pending or in progress.
    pub active_tasks: Vec<ActiveTask>,
    /// Active artifact locks.
    pub locks: Vec<LockInfo>,
    /// Most recent history entries, newest first.
    pub recent_changes: Vec<ChangeRecord>,
}

// --- helpers for base64-encoding Vec<u8> in serde ---

mod base64_bytes {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}
