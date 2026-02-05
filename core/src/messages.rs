//! Message event types for the message bus

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Message received from a chat channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    /// Channel type (telegram, discord, slack, whatsapp, cli)
    pub channel: String,
    /// User identifier (may include username for some platforms)
    pub sender_id: String,
    /// Chat/channel identifier
    pub chat_id: String,
    /// Message text content
    pub content: String,
    /// Message timestamp
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
    /// Media URLs (images, files, etc.)
    #[serde(default)]
    pub media: Vec<String>,
    /// Channel-specific metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl InboundMessage {
    /// Create a new inbound message
    pub fn new(
        channel: impl Into<String>,
        sender_id: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            sender_id: sender_id.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            timestamp: Utc::now(),
            media: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Get the unique session key for this message
    pub fn session_key(&self) -> String {
        format!("{}:{}", self.channel, self.chat_id)
    }

    /// Create a CLI direct message
    pub fn cli_direct(content: impl Into<String>) -> Self {
        Self::new("cli", "user", "direct", content)
    }

    /// Create a system message (for subagent communication)
    pub fn system(
        sender_id: impl Into<String>,
        origin_channel: impl Into<String>,
        origin_chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let origin_channel = origin_channel.into();
        let origin_chat_id = origin_chat_id.into();
        let mut msg = Self::new(
            "system",
            sender_id,
            format!("{}:{}", origin_channel, origin_chat_id),
            content,
        );
        msg.metadata.insert("origin_channel".to_string(), serde_json::json!(origin_channel));
        msg.metadata.insert("origin_chat_id".to_string(), serde_json::json!(origin_chat_id));
        msg
    }

    /// Create a message with metadata
    pub fn with_metadata(
        channel: impl Into<String>,
        sender_id: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Self {
        let mut msg = Self::new(channel, sender_id, chat_id, content);
        msg.metadata = metadata;
        msg
    }

    /// Add media to the message
    pub fn with_media(mut self, media: Vec<String>) -> Self {
        self.media = media;
        self
    }
}

/// Message to send to a chat channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    /// Channel type
    pub channel: String,
    /// Chat/channel identifier
    pub chat_id: String,
    /// Message content
    pub content: String,
    /// Optional message ID to reply to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// Media URLs
    #[serde(default)]
    pub media: Vec<String>,
    /// Channel-specific metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl OutboundMessage {
    /// Create a new outbound message
    pub fn new(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            reply_to: None,
            media: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Set the reply_to field
    pub fn with_reply_to(mut self, reply_to: impl Into<String>) -> Self {
        self.reply_to = Some(reply_to.into());
        self
    }

    /// Add media to the message
    pub fn with_media(mut self, media: Vec<String>) -> Self {
        self.media = media;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inbound_message_session_key() {
        let msg = InboundMessage::new("telegram", "123", "456", "Hello");
        assert_eq!(msg.session_key(), "telegram:456");
    }

    #[test]
    fn test_system_message() {
        let msg = InboundMessage::system("subagent_1", "whatsapp", "123456", "Task complete");
        assert_eq!(msg.channel, "system");
        assert_eq!(msg.chat_id, "whatsapp:123456");
    }

    #[test]
    fn test_outbound_message_builder() {
        let msg = OutboundMessage::new("telegram", "456", "Response")
            .with_reply_to("msg_123")
            .with_media(vec!["file.jpg".to_string()]);
        assert_eq!(msg.reply_to, Some("msg_123".to_string()));
        assert_eq!(msg.media.len(), 1);
    }
}
