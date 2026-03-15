//! Team management tools for multi-agent collaboration

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::agent::collaboration::TeamManager;
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::json;
use std::sync::Arc;

/// Tool to create a new agent team
pub struct CreateTeamTool {
    team_manager: Arc<TeamManager>,
}

impl CreateTeamTool {
    pub fn new(team_manager: Arc<TeamManager>) -> Self {
        Self { team_manager }
    }
}

#[async_trait]
impl SimpleTool for CreateTeamTool {
    fn name(&self) -> &str {
        "create_team"
    }

    fn description(&self) -> &str {
        "Create a new agent team with specified roles. Returns the team ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "Unique identifier for the team"
                },
                "team_name": {
                    "type": "string",
                    "description": "Display name for the team"
                },
                "roles": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {
                                "type": "string",
                                "description": "Role name (architect, developer, reviewer, tester)"
                            },
                            "instance_id": {
                                "type": "string",
                                "description": "Unique instance identifier for this role"
                            }
                        },
                        "required": ["role", "instance_id"]
                    },
                    "description": "List of roles and their instance IDs"
                }
            },
            "required": ["team_id", "team_name", "roles"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let team_id = match input.get_str("team_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'team_id' parameter"),
        };

        let team_name = match input.get_str("team_name") {
            Some(name) => name,
            None => return ToolResult::failure("Missing 'team_name' parameter"),
        };

        let roles_value = match input.get::<serde_json::Value>("roles") {
            Some(v) => v,
            None => return ToolResult::failure("Missing 'roles' parameter"),
        };

        let roles_array: &[serde_json::Value] = match roles_value.as_array() {
            Some(arr) => arr,
            None => return ToolResult::failure("'roles' must be an array"),
        };

        let mut role_list: Vec<(String, String)> = Vec::new();
        for role_obj in roles_array {
            let role_name: &str = match role_obj.get("role") {
                Some(v) => match v.as_str() {
                    Some(s) => s,
                    None => return ToolResult::failure("'role' must be a string"),
                },
                None => return ToolResult::failure("Missing 'role' in role object"),
            };
            let instance_id: &str = match role_obj.get("instance_id") {
                Some(v) => match v.as_str() {
                    Some(s) => s,
                    None => return ToolResult::failure("'instance_id' must be a string"),
                },
                None => return ToolResult::failure("Missing 'instance_id' in role object"),
            };
            role_list.push((role_name.to_string(), instance_id.to_string()));
        }

        match self
            .team_manager
            .create_team(team_id, team_name, role_list)
            .await
        {
            Ok(team) => ToolResult::success_text(
                serde_json::to_string(&json!({
                    "team_id": team.id,
                    "team_name": team.name,
                    "member_count": team.member_count(),
                    "status": format!("{:?}", team.status)
                }))
                .unwrap_or_else(|_| "Team created successfully".to_string()),
            ),
            Err(e) => ToolResult::failure(format!("Failed to create team: {}", e)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// Tool to list all teams
pub struct ListTeamsTool {
    team_manager: Arc<TeamManager>,
}

impl ListTeamsTool {
    pub fn new(team_manager: Arc<TeamManager>) -> Self {
        Self { team_manager }
    }
}

#[async_trait]
impl SimpleTool for ListTeamsTool {
    fn name(&self) -> &str {
        "list_teams"
    }

    fn description(&self) -> &str {
        "List all active agent teams"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _input: ToolInput) -> ToolResult {
        let summaries = self.team_manager.list_teams_detailed().await;
        let teams_json: Vec<serde_json::Value> = summaries
            .iter()
            .map(|s| {
                json!({
                    "team_id": s.id,
                    "name": s.name,
                    "member_count": s.member_count,
                    "created_at": s.created_at.to_rfc3339(),
                    "active": s.active,
                    "note": if s.active {
                        "Ready to use"
                    } else {
                        "Inactive (from previous session) — recreate with create_team to use"
                    }
                })
            })
            .collect();
        ToolResult::success_text(
            serde_json::to_string(&json!({
                "teams": teams_json,
                "total": summaries.len(),
                "active": summaries.iter().filter(|s| s.active).count(),
                "inactive": summaries.iter().filter(|s| !s.active).count()
            }))
            .unwrap_or_else(|_| format!("Found {} teams", summaries.len())),
        )
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// Tool to get team status
pub struct GetTeamStatusTool {
    team_manager: Arc<TeamManager>,
}

impl GetTeamStatusTool {
    pub fn new(team_manager: Arc<TeamManager>) -> Self {
        Self { team_manager }
    }
}

#[async_trait]
impl SimpleTool for GetTeamStatusTool {
    fn name(&self) -> &str {
        "get_team_status"
    }

    fn description(&self) -> &str {
        "Get the status of a specific team by team ID"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "Team identifier"
                }
            },
            "required": ["team_id"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let team_id = match input.get_str("team_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'team_id' parameter"),
        };

        // Check active (in-memory) team first
        if let Some(team) = self.team_manager.get_team(team_id).await {
            return ToolResult::success_text(
                serde_json::to_string(&json!({
                    "team_id": team.id,
                    "team_name": team.name,
                    "member_count": team.member_count(),
                    "status": format!("{:?}", team.status),
                    "active": true,
                    "created_at": team.created_at.to_rfc3339()
                }))
                .unwrap_or_else(|_| format!("Team: {}", team.name)),
            );
        }

        // Fall back to persisted snapshot (inactive team from previous session)
        if let Some(snap) = self.team_manager.get_team_snapshot(team_id).await {
            return ToolResult::success_text(
                serde_json::to_string(&json!({
                    "team_id": snap.id,
                    "team_name": snap.name,
                    "member_count": snap.member_count,
                    "status": "Inactive",
                    "active": false,
                    "created_at": snap.created_at.to_rfc3339(),
                    "note": "Team exists from a previous session. Use create_team to recreate it."
                }))
                .unwrap_or_else(|_| format!("Team {} (inactive)", snap.name)),
            );
        }

        ToolResult::failure(format!("Team '{}' not found", team_id))
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}
