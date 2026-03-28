//! Multi-Agent Tools to be registered in the ToolRegistry

use crate::skills::MultiAgentSkill;
use crate::tools::base::{SimpleTool, ToolInput, ToolResult};
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::{Value, json};
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
impl SimpleTool for SummonTeamTool {
    fn name(&self) -> &str {
        "summon_team"
    }

    fn description(&self) -> &str {
        "Summons a multi-agent team (Architect, Developer, Reviewer) to collaborate on a complex project or feature."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_description": {
                    "type": "string",
                    "description": "The full description of the task to be completed by the team"
                }
            },
            "required": ["task_description"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let task = match input.get_str("task_description") {
            Some(t) => t,
            None => return ToolResult::failure("Missing 'task_description' parameter"),
        };

        info!("Summoning team for task: {}", task);
        match self.skill.execute(task, None).await {
            Ok(res) => ToolResult::success_text(res),
            Err(e) => ToolResult::failure(format!("Error: {}", e)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }

    fn metadata(&self) -> mofa_sdk::kernel::ToolMetadata {
        mofa_sdk::kernel::ToolMetadata::new().with_category("agent")
    }
}
