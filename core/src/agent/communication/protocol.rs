//! Agent message protocol definitions
//!
//! Defines the structured message format for inter-agent communication.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an agent in a team
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId {
    /// Team identifier this agent belongs to
    pub team_id: String,
    /// Role name (e.g., "architect", "developer")
    pub role: String,
    /// Instance identifier (unique within team and role)
    pub instance_id: String,
}

impl AgentId {
    /// Create a new agent ID
    pub fn new(
        team_id: impl Into<String>,
        role: impl Into<String>,
        instance_id: impl Into<String>,
    ) -> Self {
        Self {
            team_id: team_id.into(),
            role: role.into(),
            instance_id: instance_id.into(),
        }
    }

    /// Parse an agent ID from a string (format: "team_id:role:instance_id")
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 3 {
            Some(Self {
                team_id: parts[0].to_string(),
                role: parts[1].to_string(),
                instance_id: parts[2].to_string(),
            })
        } else {
            None
        }
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.team_id, self.role, self.instance_id)
    }
}

/// Types of requests agents can make to each other
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestType {
    /// Ask a question to another agent
    AskQuestion,
    /// Request a code review
    RequestReview,
    /// Request implementation of a feature
    RequestImplementation,
    /// Request testing of code
    RequestTesting,
    /// Share a finding or observation
    ShareFinding,
    /// Request approval for something
    RequestApproval,
    /// Request information or clarification
    RequestInformation,
}

/// Types of agent messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessageType {
    /// Request message from one agent to another
    Request {
        /// Type of request
        request_type: RequestType,
        /// Request payload (JSON)
        payload: serde_json::Value,
    },
    /// Response to a request
    Response {
        /// Whether the request was successful
        success: bool,
        /// Response payload (JSON)
        payload: serde_json::Value,
    },
    /// Broadcast message to all team members or a topic
    Broadcast {
        /// Topic/channel for the broadcast
        topic: String,
        /// Broadcast payload (JSON)
        payload: serde_json::Value,
    },
    /// Notification of artifact update
    ArtifactUpdate {
        /// ID of the artifact that was updated
        artifact_id: String,
        /// New version number
        version: u32,
        /// Summary of changes
        change_summary: String,
    },
}

/// Structured message between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Unique message ID
    pub id: String,
    /// Message timestamp
    pub timestamp: DateTime<Utc>,
    /// Message type and content
    pub message_type: AgentMessageType,
    /// Sender agent ID
    pub from: AgentId,
    /// Recipient agent ID (None for broadcast messages)
    pub to: Option<AgentId>,
    /// Correlation ID for linking requests and responses
    pub correlation_id: Option<String>,
}

impl AgentMessage {
    /// Create a new request message
    pub fn request(
        from: AgentId,
        to: AgentId,
        request_type: RequestType,
        payload: serde_json::Value,
        correlation_id: Option<String>,
    ) -> Self {
        Self {
            id: format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
            timestamp: Utc::now(),
            message_type: AgentMessageType::Request {
                request_type,
                payload,
            },
            from,
            to: Some(to),
            correlation_id,
        }
    }

    /// Create a new response message
    pub fn response(
        from: AgentId,
        to: AgentId,
        correlation_id: String,
        success: bool,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
            timestamp: Utc::now(),
            message_type: AgentMessageType::Response { success, payload },
            from,
            to: Some(to),
            correlation_id: Some(correlation_id),
        }
    }

    /// Create a new broadcast message
    pub fn broadcast(from: AgentId, topic: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
            timestamp: Utc::now(),
            message_type: AgentMessageType::Broadcast {
                topic: topic.into(),
                payload,
            },
            from,
            to: None,
            correlation_id: None,
        }
    }

    /// Create a new artifact update notification
    pub fn artifact_update(
        from: AgentId,
        artifact_id: impl Into<String>,
        version: u32,
        change_summary: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
            timestamp: Utc::now(),
            message_type: AgentMessageType::ArtifactUpdate {
                artifact_id: artifact_id.into(),
                version,
                change_summary: change_summary.into(),
            },
            from,
            to: None, // Artifact updates are broadcast to team
            correlation_id: None,
        }
    }

    /// Check if this is a request message
    pub fn is_request(&self) -> bool {
        matches!(self.message_type, AgentMessageType::Request { .. })
    }

    /// Check if this is a response message
    pub fn is_response(&self) -> bool {
        matches!(self.message_type, AgentMessageType::Response { .. })
    }

    /// Check if this is a broadcast message
    pub fn is_broadcast(&self) -> bool {
        matches!(self.message_type, AgentMessageType::Broadcast { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_id_creation() {
        let id = AgentId::new("team1", "developer", "dev-1");
        assert_eq!(id.team_id, "team1");
        assert_eq!(id.role, "developer");
        assert_eq!(id.instance_id, "dev-1");
    }

    #[test]
    fn test_agent_id_to_string() {
        let id = AgentId::new("team1", "developer", "dev-1");
        assert_eq!(id.to_string(), "team1:developer:dev-1");
    }

    #[test]
    fn test_agent_id_from_str() {
        let id = AgentId::from_str("team1:developer:dev-1").unwrap();
        assert_eq!(id.team_id, "team1");
        assert_eq!(id.role, "developer");
        assert_eq!(id.instance_id, "dev-1");
    }

    #[test]
    fn test_agent_id_from_str_invalid() {
        assert!(AgentId::from_str("invalid").is_none());
        assert!(AgentId::from_str("team1:developer").is_none());
    }

    #[test]
    fn test_agent_message_request() {
        let from = AgentId::new("team1", "developer", "dev-1");
        let to = AgentId::new("team1", "reviewer", "rev-1");
        let payload = serde_json::json!({"question": "Can you review this code?"});

        let msg = AgentMessage::request(
            from.clone(),
            to.clone(),
            RequestType::RequestReview,
            payload.clone(),
            None,
        );

        assert_eq!(msg.from, from);
        assert_eq!(msg.to, Some(to));
        assert!(msg.is_request());
        assert!(!msg.is_response());
        assert!(!msg.is_broadcast());
    }

    #[test]
    fn test_agent_message_response() {
        let from = AgentId::new("team1", "reviewer", "rev-1");
        let to = AgentId::new("team1", "developer", "dev-1");
        let payload = serde_json::json!({"approved": true});

        let msg = AgentMessage::response(
            from.clone(),
            to.clone(),
            "corr-123".to_string(),
            true,
            payload.clone(),
        );

        assert_eq!(msg.from, from);
        assert_eq!(msg.to, Some(to));
        assert!(msg.is_response());
        assert_eq!(msg.correlation_id, Some("corr-123".to_string()));
    }

    #[test]
    fn test_agent_message_broadcast() {
        let from = AgentId::new("team1", "developer", "dev-1");
        let payload = serde_json::json!({"status": "feature complete"});

        let msg = AgentMessage::broadcast(from.clone(), "status", payload.clone());

        assert_eq!(msg.from, from);
        assert_eq!(msg.to, None);
        assert!(msg.is_broadcast());
    }

    #[test]
    fn test_agent_message_artifact_update() {
        let from = AgentId::new("team1", "developer", "dev-1");

        let msg =
            AgentMessage::artifact_update(from.clone(), "artifact-123", 2, "Added new feature");

        assert_eq!(msg.from, from);
        assert_eq!(msg.to, None);

        if let AgentMessageType::ArtifactUpdate {
            artifact_id,
            version,
            change_summary,
        } = &msg.message_type
        {
            assert_eq!(artifact_id, "artifact-123");
            assert_eq!(*version, 2);
            assert_eq!(change_summary, "Added new feature");
        } else {
            panic!("Expected ArtifactUpdate message type");
        }
    }

    #[test]
    fn test_agent_message_serialization() {
        let from = AgentId::new("team1", "developer", "dev-1");
        let to = AgentId::new("team1", "reviewer", "rev-1");
        let payload = serde_json::json!({"question": "Can you review this?"});

        let msg = AgentMessage::request(from, to, RequestType::AskQuestion, payload, None);

        // Test serialization
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.is_empty());

        // Test deserialization
        let deserialized: AgentMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg.id, deserialized.id);
        assert!(deserialized.is_request());
    }
}
