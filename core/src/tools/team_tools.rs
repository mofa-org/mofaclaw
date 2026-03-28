//! Multi-Agent Tools to be registered in the ToolRegistry

use crate::error::Result;
use crate::skills::MultiAgentSkill;
use crate::tools::base::{Tool, ToolEnv, ToolParam};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

pub struct SummonTeamTool {
    skill: Arc<MultiAgentSkill>,
}

impl SummonTeamTool {
    pub fn new(skill: Arc<MultiAgentSkill>) -> Self {
        Self { skill }
    }
}

#[async_trait]
impl Tool for SummonTeamTool {
    fn name(&self) -> &str {
        "summon_team"
    }

    fn description(&self) -> &str {
        "Summons a multi-agent team (Architect, Developer, Reviewer) to collaborate on a complex project or feature."
    }

    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            ToolParam {
                name: "task_description".to_string(),
                description: "The full description of the task to be completed by the team".to_string(),
                param_type: "string".to_string(),
                required: true,
            }
        ]
    }

    async fn execute(
        &self,
        args: &std::collections::HashMap<String, Value>,
        _env: ToolEnv,
    ) -> Result<String> {
        let task = match args.get("task_description").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return Err(crate::error::ToolError::InvalidParameters("Missing task_description".to_string()).into()),
        };

        info!("Summoning team for task: {}", task);
        self.skill.execute(task, None).await
    }
}
