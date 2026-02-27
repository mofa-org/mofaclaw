//! Tool registry for managing agent tools
//!
//! This registry uses mofa-sdk's SimpleToolRegistry directly,
//! providing a simplified interface for managing tools.

use crate::error::{Result, ToolError};
use mofa_sdk::agent::{SimpleTool, SimpleToolRegistry, ToolRegistry as MofaToolRegistry, as_tool};
use mofa_sdk::kernel::{AgentContext, Tool, ToolInput};
use mofa_sdk::llm::Tool as MofaTool;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Registry for managing mofaclaw tools
///
/// This registry is a thin wrapper around mofa-foundation's SimpleToolRegistry,
/// providing backwards compatibility with existing mofaclaw code.
///
/// It also implements mofa's unified ToolExecutor trait for use with LLM tool calling.
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
        MofaToolRegistry::list(&self.inner)
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters_schema
                    }
                })
            })
            .collect()
    }

    /// Execute a tool by name with given parameters (legacy API)
    pub async fn execute(&self, name: &str, params: &HashMap<String, Value>) -> Result<String> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        let input = ToolInput::from_json(serde_json::json!(params));
        let ctx = AgentContext::new(format!("tool-execution-{}", name));

        let result = mofa_sdk::kernel::Tool::execute(tool.as_ref(), input, &ctx).await;

        if result.success {
            Ok(result.to_string_output())
        } else {
            Err(ToolError::ExecutionFailed(format!(
                "{}: {}",
                name,
                result.error.unwrap_or_default()
            ))
            .into())
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

/// Implement unified mofa ToolExecutor trait for direct usage
#[async_trait::async_trait]
impl mofa_sdk::llm::ToolExecutor for ToolRegistry {
    async fn execute(&self, name: &str, arguments: &str) -> mofa_sdk::llm::LLMResult<String> {
        // Parse arguments string to HashMap
        let value: Value =
            serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!({}));

        let params: HashMap<String, Value> = value
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        self.execute(name, &params)
            .await
            .map_err(|e| mofa_sdk::llm::LLMError::Other(format!("Tool execution failed: {}", e)))
    }

    async fn available_tools(&self) -> mofa_sdk::llm::LLMResult<Vec<MofaTool>> {
        let tools = MofaToolRegistry::list(&self.inner)
            .iter()
            .map(|t| MofaTool::function(&t.name, &t.description, t.parameters_schema.clone()))
            .collect();

        Ok(tools)
    }
}

/// Wrapper for Arc<RwLock<ToolRegistry>> that implements unified ToolExecutor
pub struct ToolRegistryExecutor {
    inner: Arc<RwLock<ToolRegistry>>,
}

impl ToolRegistryExecutor {
    pub fn new(inner: Arc<RwLock<ToolRegistry>>) -> Self {
        Self { inner }
    }
}

// Implement unified mofa_sdk::llm::ToolExecutor
#[async_trait::async_trait]
impl mofa_sdk::llm::ToolExecutor for ToolRegistryExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> mofa_sdk::llm::LLMResult<String> {
        let registry = self.inner.read().await;

        // Parse arguments string to HashMap
        let value: Value =
            serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!({}));

        let params: HashMap<String, Value> = value
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        registry
            .execute(name, &params)
            .await
            .map_err(|e| mofa_sdk::llm::LLMError::Other(format!("Tool execution failed: {}", e)))
    }

    async fn available_tools(&self) -> mofa_sdk::llm::LLMResult<Vec<MofaTool>> {
        let registry = self.inner.read().await;

        let tools = MofaToolRegistry::list(registry.inner())
            .iter()
            .map(|t| MofaTool::function(&t.name, &t.description, t.parameters_schema.clone()))
            .collect();

        Ok(tools)
    }
}
