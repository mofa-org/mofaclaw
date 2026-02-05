//! Tool registry for managing agent tools
//!
//! This registry uses mofa-sdk's SimpleToolRegistry directly,
//! providing a simplified interface for managing tools.

use crate::error::{Result, ToolError};
use mofa_sdk::kernel::{
    SimpleTool, SimpleToolRegistry, as_tool, Tool, ToolInput, ToolResult,
    ToolRegistry as MofaToolRegistry, CoreAgentContext,
};
use mofa_sdk::llm::Tool as MofaTool;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Registry for managing mofaclaw tools
///
/// This registry is a thin wrapper around mofa-foundation's SimpleToolRegistry,
/// providing backwards compatibility with existing mofaclaw code.
///
/// It also implements mofa_foundation's ToolExecutor trait for use with AgentLoop.
pub struct ToolRegistry {
    inner: SimpleToolRegistry,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self {
            inner: SimpleToolRegistry::new(),
        }
    }

    /// Register a SimpleTool
    pub fn register<T: SimpleTool + Send + Sync + 'static>(&mut self, tool: T) {
        let tool_ref = as_tool(tool);
        let _ = MofaToolRegistry::register(&mut self.inner, tool_ref);
    }

    /// Register a tool that's already wrapped as Arc<dyn Tool>
    pub fn register_tool(&mut self, tool: Arc<dyn Tool>) {
        let _ = MofaToolRegistry::register(&mut self.inner, tool);
    }

    /// Unregister a tool by name
    pub fn unregister(&mut self, name: &str) {
        let _ = MofaToolRegistry::unregister(&mut self.inner, name);
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        MofaToolRegistry::get(&self.inner, name)
    }

    /// Check if a tool is registered
    pub fn has(&self, name: &str) -> bool {
        MofaToolRegistry::contains(&self.inner, name)
    }

    /// Get all tool definitions in OpenAI format
    pub fn get_definitions(&self) -> Vec<Value> {
        MofaToolRegistry::list(&self.inner).iter().map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters_schema
                }
            })
        }).collect()
    }

    /// Execute a tool by name with given parameters (legacy API)
    pub async fn execute(&self, name: &str, params: &HashMap<String, Value>) -> Result<String> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        let input = ToolInput::from_json(serde_json::json!(params));
        let ctx = CoreAgentContext::new(format!("tool-execution-{}", name));

        let result = mofa_sdk::kernel::Tool::execute(
            tool.as_ref(),
            input,
            &ctx
        ).await;

        if result.success {
            Ok(result.to_string_output())
        } else {
            Err(ToolError::ExecutionFailed(format!("{}: {}", name, result.error.unwrap_or_default())).into())
        }
    }

    /// Get list of registered tool names
    pub fn tool_names(&self) -> Vec<String> {
        MofaToolRegistry::list_names(&self.inner)
    }

    /// Get the number of registered tools
    pub fn len(&self) -> usize {
        MofaToolRegistry::count(&self.inner)
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        MofaToolRegistry::count(&self.inner) == 0
    }

    /// Get access to the inner registry
    pub fn inner(&self) -> &SimpleToolRegistry {
        &self.inner
    }

    /// Get mutable access to the inner registry
    pub fn inner_mut(&mut self) -> &mut SimpleToolRegistry {
        &mut self.inner
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Implement mofa_foundation's ToolExecutor trait for use with AgentLoop
#[async_trait::async_trait]
impl mofa_sdk::llm::agent_loop::ToolExecutor for ToolRegistry {
    /// Execute a tool call
    async fn execute(&self, name: &str, arguments: &str) -> anyhow::Result<String> {
        // Parse arguments string to HashMap
        let value: Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({}));

        let params: HashMap<String, Value> = value.as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_else(HashMap::new);

        self.execute(name, &params).await
            .map_err(|e| anyhow::anyhow!("Tool execution failed: {}", e))
    }

    /// Get available tool definitions
    fn available_tools(&self) -> Vec<MofaTool> {
        MofaToolRegistry::list(&self.inner).iter().map(|t| {
            // Parse the parameters schema - it's stored as a JSON string
            let params_value: Value = if t.parameters_schema.is_string() {
                serde_json::from_str(t.parameters_schema.as_str().unwrap_or("{}"))
                    .unwrap_or_else(|_| json!({}))
            } else {
                t.parameters_schema.clone()
            };
            MofaTool::function(
                &t.name,
                &t.description,
                params_value,
            )
        }).collect()
    }
}

/// Wrapper for Arc<RwLock<ToolRegistry>> that implements both ToolExecutor traits
///
/// This is needed because mofa has two ToolExecutor traits:
/// - mofa_sdk::llm::ToolExecutor (for LLMAgentBuilder)
/// - mofa_sdk::llm::agent_loop::ToolExecutor (for AgentLoop)
pub struct ToolRegistryExecutor {
    inner: Arc<RwLock<ToolRegistry>>,
}

impl ToolRegistryExecutor {
    pub fn new(inner: Arc<RwLock<ToolRegistry>>) -> Self {
        Self { inner }
    }
}

// Implement mofa_sdk::llm::ToolExecutor for LLMAgentBuilder
#[async_trait::async_trait]
impl mofa_sdk::llm::ToolExecutor for ToolRegistryExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> mofa_sdk::llm::LLMResult<String> {
        let registry = self.inner.read().await;

        // Parse arguments string to HashMap
        let value: Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({}));

        let params: HashMap<String, Value> = value.as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_else(HashMap::new);

        registry.execute(name, &params).await
            .map_err(|e| mofa_sdk::llm::LLMError::Other(format!("Tool execution failed: {}", e)))
    }

    fn available_tools(&self) -> Vec<MofaTool> {
        // Note: This is a synchronous method, so we can't await here
        // Return empty for now, tools will be registered directly
        Vec::new()
    }
}

// Implement mofa_sdk::llm::agent_loop::ToolExecutor for AgentLoop
#[async_trait::async_trait]
impl mofa_sdk::llm::agent_loop::ToolExecutor for ToolRegistryExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> anyhow::Result<String> {
        let registry = self.inner.read().await;

        // Parse arguments string to HashMap
        let value: Value = serde_json::from_str(arguments)
            .unwrap_or_else(|_| serde_json::json!({}));

        let params: HashMap<String, Value> = value.as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_else(HashMap::new);

        registry.execute(name, &params).await
            .map_err(|e| anyhow::anyhow!("Tool execution failed: {}", e))
    }

    fn available_tools(&self) -> Vec<MofaTool> {
        // Note: This is a synchronous method, so we can't await here
        // Return empty for now, tools will be registered directly
        Vec::new()
    }
}
