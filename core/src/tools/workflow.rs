//! Workflow management tools for multi-agent collaboration

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::agent::collaboration::{
    WorkflowEngine, create_code_review_workflow, create_design_workflow, team::TeamManager,
};
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

/// Start a workflow execution asynchronously.
///
/// Returns immediately with the workflow ID.  Use `get_workflow_status` to
/// poll for progress — the workflow runs in a background task.
pub struct StartWorkflowTool {
    workflow_engine: Arc<WorkflowEngine>,
    team_manager: Arc<TeamManager>,
}

impl StartWorkflowTool {
    pub fn new(workflow_engine: Arc<WorkflowEngine>, team_manager: Arc<TeamManager>) -> Self {
        Self {
            workflow_engine,
            team_manager,
        }
    }
}

#[async_trait]
impl SimpleTool for StartWorkflowTool {
    fn name(&self) -> &str {
        "start_workflow"
    }

    fn description(&self) -> &str {
        "Start a multi-agent workflow in the background. Returns a workflow_id immediately. \
         Use get_workflow_status to poll for progress. Available workflows: 'code_review', 'design'"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_name": {
                    "type": "string",
                    "description": "Name of the workflow to execute (code_review, design)",
                    "enum": ["code_review", "design"]
                },
                "team_id": {
                    "type": "string",
                    "description": "Team ID to execute the workflow with"
                },
                "initial_context": {
                    "type": "object",
                    "description": "Initial context variables for the workflow (e.g. {\"user_request\": \"...\"})",
                    "additionalProperties": true
                }
            },
            "required": ["workflow_name", "team_id"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let workflow_name = match input.get_str("workflow_name") {
            Some(name) => name,
            None => return ToolResult::failure("Missing 'workflow_name' parameter"),
        };

        let team_id = match input.get_str("team_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'team_id' parameter"),
        };

        let initial_context: HashMap<String, serde_json::Value> =
            match input.get::<serde_json::Value>("initial_context") {
                Some(v) => {
                    if let Some(obj) = v.as_object() {
                        obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                    } else {
                        HashMap::new()
                    }
                }
                None => HashMap::new(),
            };

        let team = match self.team_manager.get_team(team_id).await {
            Some(t) => t,
            None => return ToolResult::failure(format!("Team '{}' not found", team_id)),
        };

        let workflow = match workflow_name {
            "code_review" => create_code_review_workflow(),
            "design" => create_design_workflow(),
            _ => return ToolResult::failure(format!("Unknown workflow: '{}'", workflow_name)),
        };

        // Non-blocking: spawn in the background, return ID immediately.
        let workflow_id = self
            .workflow_engine
            .start_async(workflow, team, initial_context)
            .await;

        ToolResult::success_text(
            serde_json::to_string(&json!({
                "workflow_id": workflow_id,
                "status": "started",
                "message": "Workflow is running in the background. Poll get_workflow_status with this workflow_id to track progress."
            }))
            .unwrap_or_else(|_| format!("Workflow started: {}", workflow_id)),
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// Get the current status of a running or completed workflow.
pub struct GetWorkflowStatusTool {
    workflow_engine: Arc<WorkflowEngine>,
}

impl GetWorkflowStatusTool {
    pub fn new(workflow_engine: Arc<WorkflowEngine>) -> Self {
        Self { workflow_engine }
    }
}

#[async_trait]
impl SimpleTool for GetWorkflowStatusTool {
    fn name(&self) -> &str {
        "get_workflow_status"
    }

    fn description(&self) -> &str {
        "Get the current status and progress of a workflow execution"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": {
                    "type": "string",
                    "description": "Workflow identifier returned by start_workflow"
                }
            },
            "required": ["workflow_id"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let workflow_id = match input.get_str("workflow_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'workflow_id' parameter"),
        };

        match self.workflow_engine.get_workflow(workflow_id).await {
            Some(wf) => {
                let step_results: Vec<serde_json::Value> = wf
                    .step_results
                    .values()
                    .map(|r| {
                        json!({
                            "step_id": r.step_id,
                            "success": r.success,
                            "output_preview": r.output.chars().take(200).collect::<String>(),
                            "error": r.error,
                            "completed_at": r.completed_at.to_rfc3339()
                        })
                    })
                    .collect();

                ToolResult::success_text(
                    serde_json::to_string(&json!({
                        "workflow_id": wf.id,
                        "workflow_name": wf.name,
                        "status": format!("{:?}", wf.status),
                        "current_step": wf.current_step,
                        "total_steps": wf.steps.len(),
                        "completed_step_count": wf.step_results.len(),
                        "started_at": wf.started_at.map(|d| d.to_rfc3339()),
                        "completed_at": wf.completed_at.map(|d| d.to_rfc3339()),
                        "step_results": step_results
                    }))
                    .unwrap_or_else(|_| format!("Workflow: {}", wf.name)),
                )
            }
            None => ToolResult::failure(format!("Workflow '{}' not found", workflow_id)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// List all active and recently-completed workflow executions.
pub struct ListWorkflowsTool {
    workflow_engine: Arc<WorkflowEngine>,
}

impl ListWorkflowsTool {
    pub fn new(workflow_engine: Arc<WorkflowEngine>) -> Self {
        Self { workflow_engine }
    }
}

#[async_trait]
impl SimpleTool for ListWorkflowsTool {
    fn name(&self) -> &str {
        "list_workflows"
    }

    fn description(&self) -> &str {
        "List all workflow executions (running, completed, and failed) plus available workflow definitions"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _input: ToolInput) -> ToolResult {
        let workflows = self.workflow_engine.list_workflows().await;
        ToolResult::success_text(
            serde_json::to_string(&json!({
                "workflows": workflows,
                "available_definitions": ["code_review", "design"]
            }))
            .unwrap_or_else(|_| format!("Found {} workflows", workflows.len())),
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}
