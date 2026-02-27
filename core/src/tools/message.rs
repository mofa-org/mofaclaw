//! Message tool for sending messages to users

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::Result;
use crate::messages::OutboundMessage;
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Callback type for sending messages
pub type SendCallback = Arc<
    dyn Fn(
            OutboundMessage,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
>;

/// Internal state for MessageTool with interior mutability
struct MessageToolState {
    send_callback: Option<SendCallback>,
    default_channel: String,
    default_chat_id: String,
}

/// Tool to send messages to users on chat channels
pub struct MessageTool {
    inner: Arc<RwLock<MessageToolState>>,
}

impl MessageTool {
    /// Create a new MessageTool
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MessageToolState {
                send_callback: None,
                default_channel: String::new(),
                default_chat_id: String::new(),
            })),
        }
    }

    /// Create with a send callback
    pub fn with_callback(callback: SendCallback) -> Self {
        Self {
            inner: Arc::new(RwLock::new(MessageToolState {
                send_callback: Some(callback),
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

    /// Set the current message context (blocking version for sync contexts)
    pub fn set_context_blocking(&self, channel: impl Into<String>, chat_id: impl Into<String>) {
        let rt = tokio::runtime::Handle::try_current();
        if let Ok(handle) = rt {
            let mut state = handle.block_on(self.inner.write());
            state.default_channel = channel.into();
            state.default_chat_id = chat_id.into();
        } else {
            // Fallback: spawn a task and wait for it
            let channel = channel.into();
            let chat_id = chat_id.into();
            let inner = self.inner.clone();
            tokio::spawn(async move {
                let mut state = inner.write().await;
                state.default_channel = channel;
                state.default_chat_id = chat_id;
            });
        }
    }

    /// Set the callback for sending messages
    pub async fn set_send_callback(&self, callback: SendCallback) {
        let mut state = self.inner.write().await;
        state.send_callback = Some(callback);
    }
}

impl Default for MessageTool {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MessageToolState {
                send_callback: None,
                default_channel: String::new(),
                default_chat_id: String::new(),
            })),
        }
    }
}

#[async_trait]
impl SimpleTool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }

    fn description(&self) -> &str {
        "Send a message to the user. Use this when you want to communicate something."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The message content to send"
                },
                "channel": {
                    "type": "string",
                    "description": "Optional: target channel (telegram, discord, etc.)"
                },
                "chat_id": {
                    "type": "string",
                    "description": "Optional: target chat/user ID"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let content = match input.get_str("content") {
            Some(c) => c,
            None => return ToolResult::failure("Missing 'content' parameter"),
        };

        // Read the state to get defaults and callback in one scope
        let (channel, chat_id, callback) = {
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

            let callback = state.send_callback.clone();

            (channel, chat_id, callback)
        };

        if channel.is_empty() || chat_id.is_empty() {
            return ToolResult::failure("Error: No target channel/chat specified");
        }

        if let Some(callback) = callback {
            let msg = OutboundMessage::new(&channel, &chat_id, content);
            match callback(msg).await {
                Ok(_) => {
                    ToolResult::success_text(format!("Message sent to {}:{}", channel, chat_id))
                }
                Err(e) => ToolResult::failure(format!("Error sending message: {}", e)),
            }
        } else {
            ToolResult::failure("Error: Message sending not configured")
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Communication
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_message_tool() {
        let tool = MessageTool::new();

        let input = ToolInput::from_json(json!({
            "content": "Hello!",
            "channel": "telegram",
            "chat_id": "123"
        }));

        // Should fail because no callback is set
        let result = tool.execute(input).await;
        // We expect an error since no callback is configured
        assert!(!result.success || result.as_text().unwrap().contains("not configured"));
    }

    #[tokio::test]
    async fn test_set_context() {
        let tool = MessageTool::new();

        // Test async set_context
        tool.set_context("telegram", "456").await;

        let state = tool.inner.read().await;
        assert_eq!(state.default_channel, "telegram");
        assert_eq!(state.default_chat_id, "456");
    }
}
