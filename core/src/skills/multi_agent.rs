//! Multi-Agent Skill
//!
//! Provides the ability to spawn a multi-agent team to tackle a complex task.

use crate::team::{TeamManager, WorkflowEngine};
use std::sync::Arc;

pub struct MultiAgentSkill {
    team_manager: Arc<TeamManager>,
}

impl MultiAgentSkill {
    pub fn new(team_manager: Arc<TeamManager>) -> Self {
        Self { team_manager }
    }

    /// Executes the multi-agent skill for a given user prompt
    pub async fn execute(&self, task: &str, workspace_path: Option<std::path::PathBuf>) -> crate::error::Result<String> {
        let team_id = uuid::Uuid::new_v4().to_string();
        let team = self.team_manager.create_team(team_id.clone(), workspace_path).await;
        
        let workflow = WorkflowEngine::new(team.clone());
        workflow.run_standard_dev_flow(task).await?;
        
        Ok(format!("Started multi-agent team '{}' for task: {}", team_id, task))
    }
}
