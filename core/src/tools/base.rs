//! Base tool types and utilities
//!
//! This module provides helper types for tool implementation.
//! Tools should directly implement mofa-sdk's SimpleTool trait
//! for seamless integration.

use serde_json::{json, Value};
use std::collections::HashMap;

// Re-export mofa-sdk's SimpleTool and related types
pub use mofa_sdk::kernel::{
    SimpleTool, SimpleToolAdapter, as_tool,
    Tool, ToolInput, ToolResult, ToolMetadata,
};

/// Definition of a tool's parameters in JSON Schema format
pub type ToolParameters = Value;

/// Helper trait for executing tools with typed parameters
pub trait ToolExecutor {
    /// Parse parameters from a HashMap
    fn parse_params<T>(&self, params: &HashMap<String, Value>) -> Result<T, String>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        serde_json::from_value(json!(params)).map_err(|e| format!("Invalid parameters: {}", e))
    }
}

/// Wrapper type for a tool definition
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: ToolParameters,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: ToolParameters) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    pub fn to_schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters
            }
        })
    }
}
