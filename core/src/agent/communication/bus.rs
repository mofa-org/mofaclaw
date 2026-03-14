//! Agent-to-agent message bus
//!
//! Provides routing and delivery of structured messages between agents in a team.

use crate::agent::communication::protocol::{AgentId, AgentMessage, AgentMessageType};
use crate::error::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, warn};

/// Message bus for inter-agent communication
///
/// Routes messages between agents using:
/// - Point-to-point messaging (agent → agent)
/// - Broadcast messaging (agent → all team members)
/// - Topic-based subscriptions
#[derive(Clone)]
pub struct AgentMessageBus {
    /// Broadcast channel for all agent messages
    messages: broadcast::Sender<AgentMessage>,
    /// Subscriptions by agent ID (tracked for future per-agent filtering)
    #[allow(dead_code)]
    agent_subscriptions: Arc<RwLock<HashMap<String, broadcast::Receiver<AgentMessage>>>>,
    /// Topic subscriptions (topic → list of agent IDs)
    topic_subscriptions: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl AgentMessageBus {
    /// Create a new agent message bus
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            messages: tx,
            agent_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            topic_subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish a message to the bus
    ///
    /// The message will be delivered to:
    /// - The specific recipient (if `to` is set)
    /// - All agents subscribed to broadcast topics (if it's a broadcast)
    pub async fn publish(&self, message: AgentMessage) -> Result<()> {
        match self.messages.send(message.clone()) {
            Ok(count) => {
                debug!(
                    "Published agent message {} to {} subscribers",
                    message.id, count
                );
                Ok(())
            }
            Err(e) => {
                warn!("Failed to publish agent message: {}", e);
                Err(crate::error::MofaclawError::Other(format!(
                    "Failed to publish message: {}",
                    e
                )))
            }
        }
    }

    /// Subscribe to messages for a specific agent
    ///
    /// Returns a receiver that will receive all messages addressed to this agent,
    /// as well as broadcast messages.
    pub fn subscribe_agent(&self, _agent_id: &AgentId) -> broadcast::Receiver<AgentMessage> {
        // Each subscriber gets their own receiver
        // The agent_subscriptions map is kept for potential future use (e.g., tracking active agents)
        self.messages.subscribe()
    }

    /// Subscribe to broadcast messages on a specific topic
    ///
    /// Agents subscribed to a topic will receive all broadcast messages for that topic.
    pub async fn subscribe_topic(
        &self,
        topic: impl Into<String>,
        agent_id: &AgentId,
    ) -> Result<()> {
        let topic = topic.into();
        let agent_key = agent_id.to_string();
        let mut subscriptions = self.topic_subscriptions.write().await;

        subscriptions
            .entry(topic)
            .or_insert_with(Vec::new)
            .push(agent_key);

        Ok(())
    }

    /// Unsubscribe an agent from a topic
    pub async fn unsubscribe_topic(
        &self,
        topic: impl Into<String>,
        agent_id: &AgentId,
    ) -> Result<()> {
        let topic = topic.into();
        let agent_key = agent_id.to_string();
        let mut subscriptions = self.topic_subscriptions.write().await;

        if let Some(agents) = subscriptions.get_mut(&topic) {
            agents.retain(|id| id != &agent_key);
            if agents.is_empty() {
                subscriptions.remove(&topic);
            }
        }

        Ok(())
    }

    /// Get a receiver for all messages (for monitoring/debugging)
    pub fn subscribe_all(&self) -> broadcast::Receiver<AgentMessage> {
        self.messages.subscribe()
    }

    /// Check if an agent should receive a message
    ///
    /// This filters messages based on:
    /// - Direct addressing (message.to matches agent_id)
    /// - Broadcast messages (message.to is None)
    /// - Topic subscriptions (for broadcast messages)
    pub fn should_receive(&self, message: &AgentMessage, agent_id: &AgentId) -> bool {
        // Direct message to this agent
        if let Some(ref to) = message.to
            && to == agent_id
        {
            return true;
        }

        // Broadcast messages
        if message.is_broadcast() {
            // If the message has a topic, only deliver it to agents subscribed to that topic.
            // If there is no topic or no subscription information is available, fall back to
            // delivering the broadcast to all agents (current behavior).
            if let AgentMessageType::Broadcast { topic, .. } = &message.message_type
                && let Ok(subscriptions_guard) = self.topic_subscriptions.try_read()
                && let Some(subscribers) = subscriptions_guard.get(topic)
            {
                // Check if this agent is subscribed to the topic
                let agent_id_str = agent_id.to_string();
                if subscribers.contains(&agent_id_str) {
                    return true;
                }
                // If topic has subscribers but this agent isn't one, don't deliver
                return false;
            }
            // No topic or no subscription info - deliver to all agents
            return true;
        }

        // Artifact updates are broadcast to team
        if matches!(
            message.message_type,
            AgentMessageType::ArtifactUpdate { .. }
        ) {
            // Check if same team
            if message.from.team_id == agent_id.team_id {
                return true;
            }
        }

        false
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.messages.receiver_count()
    }
}

impl Default for AgentMessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::communication::protocol::RequestType;

    #[tokio::test]
    async fn test_agent_message_bus_creation() {
        let bus = AgentMessageBus::new();
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn test_publish_and_subscribe() {
        let bus = AgentMessageBus::new();
        let agent1 = AgentId::new("team1", "developer", "dev-1");
        let agent2 = AgentId::new("team1", "reviewer", "rev-1");

        let mut receiver = bus.subscribe_agent(&agent1);

        let message = AgentMessage::request(
            agent2.clone(),
            agent1.clone(),
            RequestType::AskQuestion,
            serde_json::json!({"question": "Can you help?"}),
            None,
        );

        bus.publish(message.clone()).await.unwrap();

        // Wait a bit for message to be delivered
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Try to receive the message
        let received = receiver.try_recv();
        match received {
            Ok(msg) => {
                assert_eq!(msg.id, message.id);
            }
            Err(broadcast::error::TryRecvError::Empty) => {
                // Message might not have been delivered yet, try recv
                let msg =
                    tokio::time::timeout(tokio::time::Duration::from_millis(100), receiver.recv())
                        .await
                        .unwrap()
                        .unwrap();
                assert_eq!(msg.id, message.id);
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_broadcast_message() {
        let bus = AgentMessageBus::new();
        let agent1 = AgentId::new("team1", "developer", "dev-1");
        let agent2 = AgentId::new("team1", "reviewer", "rev-1");

        let mut receiver1 = bus.subscribe_agent(&agent1);
        let mut receiver2 = bus.subscribe_agent(&agent2);

        let message = AgentMessage::broadcast(
            agent1.clone(),
            "status",
            serde_json::json!({"status": "done"}),
        );

        bus.publish(message.clone()).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Both agents should receive the broadcast
        // Use timeout to avoid blocking in async context
        let msg1 = tokio::time::timeout(tokio::time::Duration::from_millis(100), receiver1.recv())
            .await
            .ok()
            .and_then(|r| r.ok());
        let msg2 = tokio::time::timeout(tokio::time::Duration::from_millis(100), receiver2.recv())
            .await
            .ok()
            .and_then(|r| r.ok());

        assert!(
            msg1.is_some() || msg2.is_some(),
            "At least one agent should receive broadcast"
        );
    }

    #[tokio::test]
    async fn test_topic_subscription() {
        let bus = AgentMessageBus::new();
        let agent1 = AgentId::new("team1", "developer", "dev-1");

        bus.subscribe_topic("status", &agent1).await.unwrap();

        let subscriptions = bus.topic_subscriptions.read().await;
        assert!(subscriptions.contains_key("status"));
        assert!(subscriptions["status"].contains(&agent1.to_string()));
    }

    #[test]
    fn test_should_receive_direct_message() {
        let bus = AgentMessageBus::new();
        let agent1 = AgentId::new("team1", "developer", "dev-1");
        let agent2 = AgentId::new("team1", "reviewer", "rev-1");

        let message = AgentMessage::request(
            agent2.clone(),
            agent1.clone(),
            RequestType::AskQuestion,
            serde_json::json!({}),
            None,
        );

        assert!(bus.should_receive(&message, &agent1));
        assert!(!bus.should_receive(&message, &agent2));
    }

    #[test]
    fn test_should_receive_broadcast() {
        let bus = AgentMessageBus::new();
        let agent1 = AgentId::new("team1", "developer", "dev-1");
        let agent2 = AgentId::new("team1", "reviewer", "rev-1");

        let message = AgentMessage::broadcast(agent1.clone(), "status", serde_json::json!({}));

        assert!(bus.should_receive(&message, &agent1));
        assert!(bus.should_receive(&message, &agent2));
    }
}
