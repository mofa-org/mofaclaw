//! Spawn tool for creating and managing subagents

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::Result;
use async_trait::async_trait;
use mofa_sdk::kernel::ToolCategory;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Trait for subagent manager
#[async_trait]
pub trait SubagentManager: Send + Sync {
    /// Spawn a new subagent with the given prompt
    async fn spawn(&self, prompt: &str, origin_channel: &str, origin_chat_id: &str) -> Result<String>;
}

/// Internal state for SpawnTool with interior mutability
struct SpawnToolState {
    manager: Option<SpawnManagerHolder>,
    default_channel: String,
    default_chat_id: String,
}

/// Holder for the manager to avoid circular dependencies
#[derive(Clone)]
enum SpawnManagerHolder {
    Arc(Arc<dyn SubagentManager>),
}

impl SpawnManagerHolder {
    async fn spawn(&self, prompt: &str, origin_channel: &str, origin_chat_id: &str) -> Result<String> {
        match self {
            SpawnManagerHolder::Arc(m) => m.spawn(prompt, origin_channel, origin_chat_id).await,
        }
    }
}

/// Tool to spawn subagents for parallel processing
pub struct SpawnTool {
    inner: Arc<RwLock<SpawnToolState>>,
}

impl SpawnTool {
    /// Create a new SpawnTool
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SpawnToolState {
                manager: None,
                default_channel: String::new(),
                default_chat_id: String::new(),
            })),
        }
    }

    /// Create with a subagent manager
    pub fn with_manager(manager: Arc<dyn SubagentManager>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(SpawnToolState {
                manager: Some(SpawnManagerHolder::Arc(manager)),
                default_channel: String::new(),
                default_chat_id: String::new(),
            })),
        }
    }

    /// Set the current message context (uses interior mutability pattern)
    pub async fn set_context(&self, channel: impl Into<String>, chat_id: impl Into<String>) {
        let mut state = self.inner.write().await;
        state.default_channel = channel.into();
        state.default_chat_id = chat_id.into();
    }

    /// Set the subagent manager
    pub async fn set_manager(&self, manager: Arc<dyn SubagentManager>) {
        let mut state = self.inner.write().await;
        state.manager = Some(SpawnManagerHolder::Arc(manager));
    }
}

impl Default for SpawnTool {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SpawnToolState {
                manager: None,
                default_channel: String::new(),
                default_chat_id: String::new(),
            })),
        }
    }
}

#[async_trait]
impl SimpleTool for SpawnTool {
    fn name(&self) -> &str {
        "spawn"
    }

    fn description(&self) -> &str {
        "Spawn a subagent to handle a task in parallel. Use this for complex tasks that can be split up."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The task description for the subagent"
                },
                "channel": {
                    "type": "string",
                    "description": "Optional: origin channel for routing responses"
                },
                "chat_id": {
                    "type": "string",
                    "description": "Optional: origin chat ID for routing responses"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let prompt = match input.get_str("prompt") {
            Some(p) => p,
            None => return ToolResult::failure("Missing 'prompt' parameter"),
        };

        // Read the state to get defaults and manager
        let state = self.inner.read().await;

        let channel = input
            .get_str("channel")
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| state.default_channel.clone());

        let chat_id = input
            .get_str("chat_id")
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| state.default_chat_id.clone());

        // Clone the manager while holding the read lock
        let manager = state.manager.clone();

        // Release the read lock before calling the manager
        drop(state);

        if let Some(manager) = manager {
            match manager.spawn(prompt, &channel, &chat_id).await {
                Ok(result) => ToolResult::success_text(result),
                Err(e) => ToolResult::failure(format!("Failed to spawn subagent: {}", e)),
            }
        } else {
            ToolResult::failure("Error: Subagent manager not configured")
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Agent
    }
}

/// In-memory subagent manager for testing
pub struct InMemorySubagentManager {
    spawned_tasks: Arc<RwLock<Vec<String>>>,
}

impl InMemorySubagentManager {
    pub fn new() -> Self {
        Self {
            spawned_tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn get_tasks(&self) -> Vec<String> {
        self.spawned_tasks.read().await.clone()
    }
}

#[async_trait]
impl SubagentManager for InMemorySubagentManager {
    async fn spawn(&self, prompt: &str, _origin_channel: &str, _origin_chat_id: &str) -> Result<String> {
        self.spawned_tasks.write().await.push(prompt.to_string());
        Ok(format!("Subagent spawned with prompt: {}", prompt))
    }
}

impl Default for InMemorySubagentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_tool() {
        let manager = Arc::new(InMemorySubagentManager::new());
        let tool = SpawnTool::with_manager(manager.clone());

        let input = ToolInput::from_json(json!({"prompt": "Test task"}));

        let result = tool.execute(input).await;
        assert!(result.success);

        let tasks = manager.get_tasks().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0], "Test task");
    }

    #[tokio::test]
    async fn test_set_context() {
        let tool = SpawnTool::new();

        // Test async set_context
        tool.set_context("telegram", "456").await;

        let state = tool.inner.read().await;
        assert_eq!(state.default_channel, "telegram");
        assert_eq!(state.default_chat_id, "456");
    }
}
