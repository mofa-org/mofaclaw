//! Multi-agent tool registration helper

use crate::agent::collaboration::team::TeamManager;
use crate::agent::collaboration::workflow::WorkflowEngine;
use crate::agent::collaboration::workspace::SharedWorkspace;
use crate::agent::communication::AgentId;
use crate::tools::{
    ToolRegistry,
    agent_message::{BroadcastToTeamTool, RespondToApprovalTool, SendAgentMessageTool},
    team::{CreateTeamTool, GetTeamStatusTool, ListTeamsTool},
    workflow::{GetWorkflowStatusTool, ListWorkflowsTool, StartWorkflowTool},
    workspace::{CreateArtifactTool, GetArtifactTool, ListArtifactsTool},
};
use std::sync::Arc;

/// Register all multi-agent collaboration tools into `registry`.
///
/// `agent_id` identifies the calling agent for artifact authorship.  Pass
/// `None` for the main orchestrator agent — a synthetic orchestrator identity
/// (`main:orchestrator:main`) will be used so that `create_artifact` is still
/// available.
pub async fn register_multi_agent_tools(
    registry: &mut ToolRegistry,
    team_manager: Arc<TeamManager>,
    workflow_engine: Arc<WorkflowEngine>,
    workspace: Arc<SharedWorkspace>,
    agent_id: Option<Arc<AgentId>>,
) {
    // Team management
    registry.register(CreateTeamTool::new(team_manager.clone()));
    registry.register(ListTeamsTool::new(team_manager.clone()));
    registry.register(GetTeamStatusTool::new(team_manager.clone()));

    // Workflow orchestration
    registry.register(StartWorkflowTool::new(
        workflow_engine.clone(),
        team_manager.clone(),
    ));
    registry.register(GetWorkflowStatusTool::new(workflow_engine.clone()));
    registry.register(ListWorkflowsTool::new(workflow_engine.clone()));

    // Workspace / artifact tools — always available, using the provided or
    // a synthetic orchestrator identity.
    let effective_id =
        agent_id.unwrap_or_else(|| Arc::new(AgentId::new("main", "orchestrator", "main")));
    registry.register(CreateArtifactTool::new(workspace.clone(), effective_id));
    registry.register(GetArtifactTool::new(workspace.clone()));
    registry.register(ListArtifactsTool::new(workspace.clone()));

    // Agent communication
    registry.register(SendAgentMessageTool::new(team_manager.clone()));
    registry.register(BroadcastToTeamTool::new(team_manager.clone()));
    registry.register(RespondToApprovalTool::new(team_manager.clone()));
}
