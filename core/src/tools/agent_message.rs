//! Agent communication tools for multi-agent collaboration

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::agent::collaboration::team::TeamManager;
use crate::agent::communication::{AgentId, AgentMessage, RequestType};
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::json;
use std::sync::Arc;

/// Tool to send a message to another agent
pub struct SendAgentMessageTool {
    team_manager: Arc<TeamManager>,
}

impl SendAgentMessageTool {
    pub fn new(team_manager: Arc<TeamManager>) -> Self {
        Self { team_manager }
    }
}

#[async_trait]
impl SimpleTool for SendAgentMessageTool {
    fn name(&self) -> &str {
        "send_agent_message"
    }

    fn description(&self) -> &str {
        "Send a structured message to another agent in a team. Specify the target agent ID, message type, and payload."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "The ID of the team"
                },
                "from_agent_id": {
                    "type": "string",
                    "description": "Agent ID of the sender (format: team_id:role:instance_id)"
                },
                "to_agent_id": {
                    "type": "string",
                    "description": "Agent ID of the recipient (format: team_id:role:instance_id)"
                },
                "request_type": {
                    "type": "string",
                    "description": "Type of request (ask_question, request_review, request_implementation, request_testing, share_finding, request_approval, request_information)",
                    "enum": ["ask_question", "request_review", "request_implementation", "request_testing", "share_finding", "request_approval", "request_information"]
                },
                "payload": {
                    "type": "object",
                    "description": "Message payload as JSON object",
                    "additionalProperties": true
                },
                "context_id": {
                    "type": "string",
                    "description": "Optional context ID for linking to workflow step or shared context"
                }
            },
            "required": ["team_id", "from_agent_id", "to_agent_id", "request_type", "payload"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let team_id = match input.get_str("team_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'team_id' parameter"),
        };

        let from_agent_id_str = match input.get_str("from_agent_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'from_agent_id' parameter"),
        };

        let to_agent_id_str = match input.get_str("to_agent_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'to_agent_id' parameter"),
        };

        let request_type_str = match input.get_str("request_type") {
            Some(t) => t,
            None => return ToolResult::failure("Missing 'request_type' parameter"),
        };

        let payload: serde_json::Value = match input.get::<serde_json::Value>("payload") {
            Some(p) => p.clone(),
            None => return ToolResult::failure("Missing 'payload' parameter"),
        };

        let context_id = input.get_str("context_id").map(|s| s.to_string());

        // Parse agent IDs
        let from_agent_id = match AgentId::from_str(from_agent_id_str) {
            Some(id) => id,
            None => {
                return ToolResult::failure(format!(
                    "Invalid from_agent_id format: {}",
                    from_agent_id_str
                ));
            }
        };
        let to_agent_id = match AgentId::from_str(to_agent_id_str) {
            Some(id) => id,
            None => {
                return ToolResult::failure(format!(
                    "Invalid to_agent_id format: {}",
                    to_agent_id_str
                ));
            }
        };

        // Parse request type
        let request_type = match request_type_str {
            "ask_question" => RequestType::AskQuestion,
            "request_review" => RequestType::RequestReview,
            "request_implementation" => RequestType::RequestImplementation,
            "request_testing" => RequestType::RequestTesting,
            "share_finding" => RequestType::ShareFinding,
            "request_approval" => RequestType::RequestApproval,
            "request_information" => RequestType::RequestInformation,
            _ => return ToolResult::failure(format!("Invalid request_type: {}", request_type_str)),
        };

        // Get team
        let team = match self.team_manager.get_team(team_id).await {
            Some(t) => t,
            None => return ToolResult::failure(format!("Team '{}' not found", team_id)),
        };

        // Create message
        let message = AgentMessage::request(
            from_agent_id,
            to_agent_id.clone(),
            request_type,
            payload,
            context_id,
        );

        // Send message via team's message bus
        match team.broadcast(message.clone()).await {
            Ok(_) => {
                let result_json = json!({
                    "message_id": message.id,
                    "from": from_agent_id_str,
                    "to": to_agent_id_str,
                    "status": "sent",
                    "timestamp": message.timestamp.to_rfc3339(),
                });
                ToolResult::success_text(
                    serde_json::to_string(&result_json)
                        .unwrap_or_else(|_| format!("Message sent to agent {}", to_agent_id_str)),
                )
            }
            Err(e) => ToolResult::failure(format!("Failed to send message: {}", e)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// Tool to broadcast a message to all team members
pub struct BroadcastToTeamTool {
    team_manager: Arc<TeamManager>,
}

impl BroadcastToTeamTool {
    pub fn new(team_manager: Arc<TeamManager>) -> Self {
        Self { team_manager }
    }
}

#[async_trait]
impl SimpleTool for BroadcastToTeamTool {
    fn name(&self) -> &str {
        "broadcast_to_team"
    }

    fn description(&self) -> &str {
        "Broadcast a message to all members of a team on a specific topic."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "The ID of the team"
                },
                "from_agent_id": {
                    "type": "string",
                    "description": "Agent ID of the sender (format: team_id:role:instance_id)"
                },
                "topic": {
                    "type": "string",
                    "description": "Topic for the broadcast (e.g., 'status', 'artifact_update', 'new_task')"
                },
                "payload": {
                    "type": "object",
                    "description": "Message payload as JSON object",
                    "additionalProperties": true
                }
            },
            "required": ["team_id", "from_agent_id", "topic", "payload"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let team_id = match input.get_str("team_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'team_id' parameter"),
        };

        let from_agent_id_str = match input.get_str("from_agent_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'from_agent_id' parameter"),
        };

        let topic = match input.get_str("topic") {
            Some(t) => t,
            None => return ToolResult::failure("Missing 'topic' parameter"),
        };

        let payload: serde_json::Value = match input.get::<serde_json::Value>("payload") {
            Some(p) => p.clone(),
            None => return ToolResult::failure("Missing 'payload' parameter"),
        };

        // Parse agent ID
        let from_agent_id = match AgentId::from_str(from_agent_id_str) {
            Some(id) => id,
            None => {
                return ToolResult::failure(format!(
                    "Invalid from_agent_id format: {}",
                    from_agent_id_str
                ));
            }
        };

        // Get team
        let team = match self.team_manager.get_team(team_id).await {
            Some(t) => t,
            None => return ToolResult::failure(format!("Team '{}' not found", team_id)),
        };

        // Create broadcast message
        let message = AgentMessage::broadcast(from_agent_id, topic, payload);

        // Broadcast message
        match team.broadcast(message.clone()).await {
            Ok(_) => {
                let result_json = json!({
                    "message_id": message.id,
                    "from": from_agent_id_str,
                    "topic": topic,
                    "recipient_count": team.member_count(),
                    "status": "broadcast",
                    "timestamp": message.timestamp.to_rfc3339(),
                });
                ToolResult::success_text(serde_json::to_string(&result_json).unwrap_or_else(|_| {
                    format!("Message broadcast to {} team members", team.member_count())
                }))
            }
            Err(e) => ToolResult::failure(format!("Failed to broadcast message: {}", e)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// Tool to respond to an approval request
pub struct RespondToApprovalTool {
    team_manager: Arc<TeamManager>,
}

impl RespondToApprovalTool {
    pub fn new(team_manager: Arc<TeamManager>) -> Self {
        Self { team_manager }
    }
}

#[async_trait]
impl SimpleTool for RespondToApprovalTool {
    fn name(&self) -> &str {
        "respond_to_approval"
    }

    fn description(&self) -> &str {
        "Respond to an approval request from a workflow. Requires correlation_id (from the approval request), approval status (approved/rejected), and optional comment."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "The ID of the team"
                },
                "from_agent_id": {
                    "type": "string",
                    "description": "Agent ID of the responder (format: team_id:role:instance_id)"
                },
                "correlation_id": {
                    "type": "string",
                    "description": "Correlation ID from the approval request message"
                },
                "approved": {
                    "type": "boolean",
                    "description": "Whether to approve (true) or reject (false)"
                },
                "comment": {
                    "type": "string",
                    "description": "Optional comment explaining the decision"
                }
            },
            "required": ["team_id", "from_agent_id", "correlation_id", "approved"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let team_id = match input.get_str("team_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'team_id' parameter"),
        };
        let from_agent_id_str = match input.get_str("from_agent_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'from_agent_id' parameter"),
        };
        let correlation_id = match input.get_str("correlation_id") {
            Some(id) => id,
            None => return ToolResult::failure("Missing 'correlation_id' parameter"),
        };
        let approved = match input.get::<serde_json::Value>("approved") {
            Some(v) => match v.as_bool() {
                Some(b) => b,
                None => {
                    return ToolResult::failure("Invalid 'approved' parameter: expected a boolean");
                }
            },
            None => return ToolResult::failure("Missing 'approved' parameter"),
        };
        let comment = input.get_str("comment").map(|s| s.to_string());

        // Parse agent ID
        let from_agent_id = match AgentId::from_str(from_agent_id_str) {
            Some(id) => id,
            None => {
                return ToolResult::failure(format!(
                    "Invalid from_agent_id format: {}",
                    from_agent_id_str
                ));
            }
        };

        // Get team
        let team = match self.team_manager.get_team(team_id).await {
            Some(t) => t,
            None => return ToolResult::failure(format!("Team '{}' not found", team_id)),
        };

        // Find the workflow engine or approver (for now, send to workflow engine)
        let to_agent_id = AgentId::new(team_id, "workflow", "engine");

        // Create response message
        let payload = json!({
            "approved": approved,
            "comment": comment,
            "context_id": correlation_id
        });

        let response = AgentMessage::response(
            from_agent_id.clone(),
            to_agent_id,
            correlation_id.to_string(),
            approved,
            payload,
        );

        // Send response via team's message bus
        match team.message_bus.publish(response.clone()).await {
            Ok(_) => {
                let result_json = json!({
                    "message_id": response.id,
                    "from": from_agent_id_str,
                    "correlation_id": correlation_id,
                    "approved": approved,
                    "status": "sent",
                    "timestamp": response.timestamp.to_rfc3339(),
                });
                ToolResult::success_text(serde_json::to_string(&result_json).unwrap_or_else(|_| {
                    format!(
                        "Approval response sent: {}",
                        if approved { "approved" } else { "rejected" }
                    )
                }))
            }
            Err(e) => ToolResult::failure(format!("Failed to send approval response: {}", e)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use crate::bus::MessageBus;
    use crate::session::SessionManager;
    use tokio::runtime::Runtime;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore] // TODO: Fix SessionManager blocking issue
    async fn test_send_agent_message_tool() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let config = Arc::new(Config::default());
            let user_bus = MessageBus::new();
            let sessions = Arc::new(SessionManager::new(&config));
            let team_manager = Arc::new(TeamManager::new(config, user_bus, sessions));
            let tool = SendAgentMessageTool::new(team_manager.clone());

            // This test will fail because team doesn't exist, but tests the tool structure
            let input = ToolInput::from_json(json!({
                "team_id": "test-team",
                "from_agent_id": "test-team:developer:dev-1",
                "to_agent_id": "test-team:reviewer:rev-1",
                "request_type": "ask_question",
                "payload": {"question": "Can you review this?"}
            }));

            let result = tool.execute(input).await;
            assert!(!result.success); // Should fail because team doesn't exist
            assert!(
                result
                    .as_text()
                    .unwrap()
                    .contains("Team 'test-team' not found")
            );
        });
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore] // TODO: Fix SessionManager blocking issue
    async fn test_broadcast_to_team_tool() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let config = Arc::new(Config::default());
            let user_bus = MessageBus::new();
            let sessions = Arc::new(SessionManager::new(&config));
            let team_manager = Arc::new(TeamManager::new(config, user_bus, sessions));
            let tool = BroadcastToTeamTool::new(team_manager.clone());

            let input = ToolInput::from_json(json!({
                "team_id": "test-team",
                "from_agent_id": "test-team:developer:dev-1",
                "topic": "status",
                "payload": {"status": "working"}
            }));

            let result = tool.execute(input).await;
            assert!(!result.success); // Should fail because team doesn't exist
            assert!(
                result
                    .as_text()
                    .unwrap()
                    .contains("Team 'test-team' not found")
            );
        });
    }
}
