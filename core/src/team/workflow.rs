//! WorkflowEngine Coordinator
//!
//! Handles sequential or graph-based execution of team tasks.

use super::team::AgentTeam;
use super::bus::TeamMessage;
use std::sync::Arc;

/// Coordinates the execution flow of an AgentTeam
pub struct WorkflowEngine {
    team: Arc<AgentTeam>,
}

impl WorkflowEngine {
    pub fn new(team: Arc<AgentTeam>) -> Self {
        Self { team }
    }

    /// Starts a simple linear workflow path
    pub async fn run_standard_dev_flow(&self, task_description: &str) -> crate::error::Result<()> {
        let msg = TeamMessage::new(
            "WorkflowEngine",
            "Coordinator",
            "TaskAssigned",
            task_description,
        );
        self.team.bus.publish(msg)?;
        Ok(())
    }
}
