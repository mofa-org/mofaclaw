//! Async message bus for inter-agent communication within a team

use crate::error::{ChannelError, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::warn;

/// Represents an event or message sent within an AgentTeam
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamMessage {
    pub sender_id: String,
    pub sender_role: String,
    pub event_type: String,
    pub content: String,
    pub metadata: std::collections::HashMap<String, String>,
}

impl TeamMessage {
    pub fn new(sender_id: &str, sender_role: &str, event_type: &str, content: &str) -> Self {
        Self {
            sender_id: sender_id.to_string(),
            sender_role: sender_role.to_string(),
            event_type: event_type.to_string(),
            content: content.to_string(),
            metadata: std::collections::HashMap::new(),
        }
    }
}

/// A localized event bus exclusively for a multi-agent team
#[derive(Clone)]
pub struct AgentMessageBus {
    tx: broadcast::Sender<TeamMessage>,
}

impl AgentMessageBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { tx }
    }

    pub fn publish(&self, msg: TeamMessage) -> Result<()> {
        match self.tx.send(msg) {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!("Failed to publish team message: {}", e);
                // Map this into existing ChannelError
                Err(ChannelError::SendFailed(e.to_string()).into())
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TeamMessage> {
        self.tx.subscribe()
    }
}

impl Default for AgentMessageBus {
    fn default() -> Self {
        Self::new()
    }
}
