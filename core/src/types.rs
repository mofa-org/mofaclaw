//! Core types for mofaclaw

use chrono::{Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tool call request from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    /// Unique ID for this tool call
    pub id: String,
    /// Name of the tool to call
    pub name: String,
    /// Arguments to pass to the tool
    pub arguments: HashMap<String, serde_json::Value>,
}

/// Response from an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    /// Text content of the response
    pub content: Option<String>,
    /// Tool calls requested by the LLM
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRequest>,
    /// Reason the generation finished
    #[serde(default = "default_finish_reason")]
    pub finish_reason: String,
    /// Token usage information
    #[serde(default)]
    pub usage: HashMap<String, u32>,
}

fn default_finish_reason() -> String {
    "stop".to_string()
}

impl LLMResponse {
    /// Check if response contains tool calls
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// Message role in a conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A message in a conversation
#[derive(Debug, Clone)]
pub struct Message {
    /// Message role
    pub role: MessageRole,
    /// Message content (can be null for tool calls, or structured content for vision)
    pub content: Option<MessageContent>,
    /// Tool call ID (for tool role messages)
    pub tool_call_id: Option<String>,
    /// Tool name (for tool role messages)
    pub name: Option<String>,
    /// Tool calls (for assistant messages)
    pub tool_calls: Vec<ToolCallRequest>,
}

/// Message content can be either a string or structured content (for vision models)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Array(Vec<serde_json::Value>),
}

/// Helper to convert content to API format
impl MessageContent {
    pub fn is_empty(&self) -> bool {
        match self {
            MessageContent::Text(s) => s.is_empty(),
            MessageContent::Array(arr) => arr.is_empty(),
        }
    }
}

/// Implement Serialize/Deserialize for Message manually
impl Serialize for Message {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;

        map.serialize_entry("role", &format!("{:?}", self.role).to_lowercase())?;

        if let Some(content) = &self.content {
            match content {
                MessageContent::Text(text) => {
                    map.serialize_entry("content", text)?;
                }
                MessageContent::Array(arr) => {
                    map.serialize_entry("content", arr)?;
                }
            }
        }

        if let Some(ref tool_call_id) = self.tool_call_id {
            map.serialize_entry("tool_call_id", tool_call_id)?;
        }

        if let Some(ref name) = self.name {
            map.serialize_entry("name", name)?;
        }

        if !self.tool_calls.is_empty() {
            map.serialize_entry("tool_calls", &self.tool_calls)?;
        }

        map.end()
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        struct MessageVisitor;

        impl<'de> Visitor<'de> for MessageVisitor {
            type Value = Message;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a message object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut role = None;
                let mut content = None;
                let mut tool_call_id = None;
                let mut name = None;
                let mut tool_calls = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "role" => {
                            let role_str = map.next_value::<String>()?;
                            role = match role_str.as_str() {
                                "system" => Some(MessageRole::System),
                                "user" => Some(MessageRole::User),
                                "assistant" => Some(MessageRole::Assistant),
                                "tool" => Some(MessageRole::Tool),
                                _ => return Err(serde::de::Error::unknown_variant(&role_str, &["system", "user", "assistant", "tool"])),
                            };
                        }
                        "content" => {
                            let value = map.next_value::<serde_json::Value>()?;
                            content = if value.is_string() {
                                Some(MessageContent::Text(value.as_str().unwrap().to_string()))
                            } else if value.is_array() {
                                Some(MessageContent::Array(serde_json::from_value(value).unwrap()))
                            } else if value.is_null() {
                                None
                            } else {
                                Some(MessageContent::Text(value.to_string()))
                            };
                        }
                        "tool_call_id" => {
                            tool_call_id = Some(map.next_value()?);
                        }
                        "name" => {
                            name = Some(map.next_value()?);
                        }
                        "tool_calls" => {
                            tool_calls = Some(map.next_value()?);
                        }
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                let role = role.ok_or_else(|| serde::de::Error::missing_field("role"))?;

                Ok(Message {
                    role,
                    content,
                    tool_call_id,
                    name,
                    tool_calls: tool_calls.unwrap_or_default(),
                })
            }
        }

        deserializer.deserialize_map(MessageVisitor)
    }
}

impl Message {
    /// Create a new system message
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: Some(MessageContent::Text(content.into())),
            tool_call_id: None,
            name: None,
            tool_calls: Vec::new(),
        }
    }

    /// Create a new user message
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(MessageContent::Text(content.into())),
            tool_call_id: None,
            name: None,
            tool_calls: Vec::new(),
        }
    }

    /// Create a new user message with structured content (for vision models)
    pub fn user_with_content(content: MessageContent) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(content),
            tool_call_id: None,
            name: None,
            tool_calls: Vec::new(),
        }
    }

    /// Create a new user message (accepts both string and MessageContent)
    pub fn user_maybe_content<C: Into<MessageContent>>(content: C) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(content.into()),
            tool_call_id: None,
            name: None,
            tool_calls: Vec::new(),
        }
    }

    /// Create a new assistant message
    pub fn assistant(content: Option<String>, tool_calls: Vec<ToolCallRequest>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.map(MessageContent::Text),
            tool_call_id: None,
            name: None,
            tool_calls,
        }
    }

    /// Create a new tool result message
    pub fn tool(tool_call_id: impl Into<String>, name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: Some(MessageContent::Text(content.into())),
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
            tool_calls: Vec::new(),
        }
    }

    /// Convert to the format expected by the LLM API
    pub fn to_api_format(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "role": format!("{:?}", self.role).to_lowercase(),
        });

        if let Some(content) = &self.content {
            match content {
                MessageContent::Text(text) => {
                    value["content"] = serde_json::json!(text);
                }
                MessageContent::Array(arr) => {
                    value["content"] = serde_json::json!(arr);
                }
            }
        }

        if let Some(tool_call_id) = &self.tool_call_id {
            value["tool_call_id"] = serde_json::json!(tool_call_id);
        }

        if let Some(name) = &self.name {
            value["name"] = serde_json::json!(name);
        }

        if !self.tool_calls.is_empty() {
            let tool_calls: Vec<serde_json::Value> = self.tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()
                        }
                    })
                })
                .collect();
            value["tool_calls"] = serde_json::json!(tool_calls);
        }

        value
    }

    /// Get content as text (returns empty string if content is structured)
    pub fn content_as_text(&self) -> String {
        match &self.content {
            Some(MessageContent::Text(s)) => s.clone(),
            Some(MessageContent::Array(_)) => "[structured content]".to_string(),
            None => String::new(),
        }
    }

    /// Check if content is structured (contains images)
    pub fn has_structured_content(&self) -> bool {
        matches!(&self.content, Some(MessageContent::Array(_)))
    }

    /// Get the text part of the content (for structured content, extracts the text item)
    pub fn get_text_only(&self) -> Option<String> {
        match &self.content {
            Some(MessageContent::Text(s)) => Some(s.clone()),
            Some(MessageContent::Array(arr)) => {
                // Find the text item in the array
                for item in arr {
                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                        return Some(text.to_string());
                    }
                }
                None
            }
            None => None,
        }
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        MessageContent::Text(s)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        MessageContent::Text(s.to_string())
    }
}

/// A session message stored in JSONL format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl SessionMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            timestamp: Some(Utc::now().to_rfc3339()),
            extra: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_response_tool_calls() {
        let response = LLMResponse {
            content: Some("Let me check that".to_string()),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            usage: HashMap::new(),
        };
        assert!(!response.has_tool_calls());

        let response = LLMResponse {
            content: None,
            tool_calls: vec![ToolCallRequest {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: HashMap::new(),
            }],
            finish_reason: "tool_calls".to_string(),
            usage: HashMap::new(),
        };
        assert!(response.has_tool_calls());
    }
}
