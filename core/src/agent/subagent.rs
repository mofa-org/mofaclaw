//! Subagent manager for spawning parallel background tasks
//!
//! This module wraps AgentLoop which uses mofa framework's TaskOrchestrator
//! internally for background task spawning.

use crate::agent::AgentLoop;
use std::sync::Arc;

/// Manages spawning and coordination of subagents
///
/// This is a thin wrapper around AgentLoop which uses mofa framework's
/// TaskOrchestrator internally for subagent spawning.
///
/// The TaskOrchestrator handles:
/// - Background task spawning via tokio::spawn
/// - Origin-based result routing
/// - Task lifecycle management
/// - Result streaming via channels
pub struct SubagentManager {
    /// The inner agent loop (contains mofa's TaskOrchestrator)
    inner: Arc<AgentLoop>,
}

impl SubagentManager {
    /// Create a new subagent manager that wraps an existing AgentLoop
    pub fn new(agent_loop: Arc<AgentLoop>) -> Self {
        Self { inner: agent_loop }
    }

    /// Get the inner agent loop
    pub fn inner(&self) -> &Arc<AgentLoop> {
        &self.inner
    }

    /// Spawn a new subagent with the given prompt
    pub async fn spawn(
        &self,
        prompt: &str,
        origin_channel: &str,
        origin_chat_id: &str,
    ) -> crate::error::Result<String> {
        self.inner.spawn_subagent(prompt, origin_channel, origin_chat_id).await
    }

    /// Get all active subagents
    pub async fn get_active_subagents(&self) -> Vec<crate::agent::ActiveSubagent> {
        self.inner.get_active_subagents().await
    }

    /// Mark a subagent as complete (no-op, auto-cleanup)
    pub async fn complete_subagent(&self, _subagent_id: &str) {
        // Auto-cleanup is handled by the agent loop
    }
}

/// Implement the spawn tool's SubagentManager trait for SubagentManager
#[async_trait::async_trait]
impl crate::tools::spawn::SubagentManager for SubagentManager {
    async fn spawn(&self, prompt: &str, origin_channel: &str, origin_chat_id: &str) -> crate::error::Result<String> {
        self.spawn(prompt, origin_channel, origin_chat_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_manager_type() {
        let _ = std::marker::PhantomData::<SubagentManager>;
    }
}
