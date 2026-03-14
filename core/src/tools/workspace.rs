//! Workspace management tools for multi-agent collaboration

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::agent::collaboration::{ArtifactType, SharedWorkspace};
use crate::agent::communication::AgentId;
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

/// Tool to create an artifact in the shared workspace
pub struct CreateArtifactTool {
    workspace: Arc<SharedWorkspace>,
    agent_id: Arc<AgentId>,
}

impl CreateArtifactTool {
    pub fn new(workspace: Arc<SharedWorkspace>, agent_id: Arc<AgentId>) -> Self {
        Self {
            workspace,
            agent_id,
        }
    }
}

#[async_trait]
impl SimpleTool for CreateArtifactTool {
    fn name(&self) -> &str {
        "create_artifact"
    }

    fn description(&self) -> &str {
        "Create a new artifact in the shared workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "artifact_id": {
                    "type": "string",
                    "description": "Unique identifier for the artifact"
                },
                "name": {
                    "type": "string",
                    "description": "Display name for the artifact"
                },
                "artifact_type": {
                    "type": "string",
                    "description": "Type of artifact",
                    "enum": ["code_file", "design_doc", "test_file", "review_comment", "other"]
                },
                "path": {
                    "type": "string",
                    "description": "File path for the artifact"
                },
                "content": {
                    "type": "string",
                    "description": "Content of the artifact (if storing directly)"
                }
            },
            "required": ["artifact_id", "name", "artifact_type", "path"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let artifact_id = match input.get_str("artifact_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'artifact_id' parameter"),
        };

        let name = match input.get_str("name") {
            Some(n) => n,
            None => return ToolResult::failure("Missing 'name' parameter"),
        };

        let artifact_type_str = match input.get_str("artifact_type") {
            Some(t) => t,
            None => return ToolResult::failure("Missing 'artifact_type' parameter"),
        };

        let path = match input.get_str("path") {
            Some(p) => PathBuf::from(p),
            None => return ToolResult::failure("Missing 'path' parameter"),
        };

        let path_for_content = path.clone();
        let artifact_type = match artifact_type_str {
            "code_file" => ArtifactType::CodeFile { path: path.clone() },
            "design_doc" => ArtifactType::DesignDoc { path: path.clone() },
            "test_file" => ArtifactType::TestFile { path: path.clone() },
            "review_comment" => ArtifactType::ReviewComment { path: path.clone() },
            "other" => ArtifactType::Other { path: path.clone() },
            _ => {
                return ToolResult::failure(format!(
                    "Invalid artifact_type: {}",
                    artifact_type_str
                ));
            }
        };

        let content = if let Some(content_str) = input.get_str("content") {
            crate::agent::collaboration::ArtifactContent::FileContent {
                content: content_str.to_string(),
            }
        } else {
            crate::agent::collaboration::ArtifactContent::FileReference {
                path: path_for_content,
            }
        };

        match self
            .workspace
            .create_artifact(
                artifact_id,
                name,
                artifact_type,
                content,
                (*self.agent_id).clone(),
            )
            .await
        {
            Ok(artifact) => ToolResult::success_text(
                serde_json::to_string(&json!({
                    "artifact_id": artifact.id,
                    "name": artifact.name,
                    "version": artifact.version,
                    "created_at": artifact.created_at.to_rfc3339()
                }))
                .unwrap_or_else(|_| format!("Artifact {} created", artifact.id)),
            ),
            Err(e) => ToolResult::failure(format!("Failed to create artifact: {}", e)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// Tool to get an artifact
pub struct GetArtifactTool {
    workspace: Arc<SharedWorkspace>,
}

impl GetArtifactTool {
    pub fn new(workspace: Arc<SharedWorkspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl SimpleTool for GetArtifactTool {
    fn name(&self) -> &str {
        "get_artifact"
    }

    fn description(&self) -> &str {
        "Retrieve an artifact from the shared workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "artifact_id": {
                    "type": "string",
                    "description": "Artifact identifier"
                }
            },
            "required": ["artifact_id"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let artifact_id = match input.get_str("artifact_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'artifact_id' parameter"),
        };

        match self.workspace.get_artifact(artifact_id).await {
            Some(artifact) => {
                let content_str = match &artifact.content {
                    crate::agent::collaboration::ArtifactContent::FileContent { content } => {
                        content.clone()
                    }
                    crate::agent::collaboration::ArtifactContent::FileReference { path } => {
                        format!("File reference: {}", path.display())
                    }
                };

                ToolResult::success_text(
                    serde_json::to_string(&json!({
                        "artifact_id": artifact.id,
                        "name": artifact.name,
                        "version": artifact.version,
                        "content": content_str,
                        "created_at": artifact.created_at.to_rfc3339(),
                        "modified_at": artifact.modified_at.to_rfc3339()
                    }))
                    .unwrap_or_else(|_| format!("Artifact: {}", artifact.name)),
                )
            }
            None => ToolResult::failure(format!("Artifact '{}' not found", artifact_id)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// Tool to list all artifacts
pub struct ListArtifactsTool {
    workspace: Arc<SharedWorkspace>,
}

impl ListArtifactsTool {
    pub fn new(workspace: Arc<SharedWorkspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl SimpleTool for ListArtifactsTool {
    fn name(&self) -> &str {
        "list_artifacts"
    }

    fn description(&self) -> &str {
        "List all artifacts in the shared workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _input: ToolInput) -> ToolResult {
        let artifacts = self.workspace.list_artifacts().await;
        ToolResult::success_text(
            serde_json::to_string(&json!({
                "artifacts": artifacts,
                "count": artifacts.len()
            }))
            .unwrap_or_else(|_| format!("Found {} artifacts", artifacts.len())),
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}
