//! Session management for conversation history
//!
//! This module re-exports mofa_sdk's session management types with
//! mofaclaw-specific conversion utilities.

use chrono::{DateTime, Utc};
use serde_json::json;
use std::path::PathBuf;

// Re-export mofa_sdk's Session types
pub use mofa_sdk::agent::{
    JsonlSessionStorage, MemorySessionStorage, Session, SessionManager as MofaSessionManager,
    SessionMessage, SessionStorage,
};

use crate::error::Result;
use crate::types::{Message, MessageContent, MessageRole};
use crate::{Config, get_data_dir};

const STRUCTURED_CONTENT_PREFIX: &str = "__mofaclaw_content__:";
const SESSION_SCHEMA_VERSION: u32 = 1;

/// Information about a session for listing purposes
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub key: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub path: PathBuf,
}

/// mofaclaw SessionManager - thin wrapper around mofa's SessionManager
///
/// Uses mofa_sdk's SessionManager with JsonlSessionStorage,
/// using mofaclaw's config for the sessions directory.
pub struct SessionManager {
    /// Inner mofa_sdk SessionManager
    inner: MofaSessionManager,
    /// Sessions directory (for SessionInfo path)
    sessions_dir: PathBuf,
}

impl SessionManager {
    /// Create a new session manager from config
    pub fn new(_config: &Config) -> Self {
        let sessions_dir = get_data_dir().join("sessions");

        // Use a blocking task to create the async SessionManager
        let sessions_dir_clone = sessions_dir.clone();
        let inner = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                // Ensure directory exists
                tokio::fs::create_dir_all(&sessions_dir_clone).await.ok();

                // Create JsonlSessionStorage and SessionManager
                match JsonlSessionStorage::new(&sessions_dir_clone).await {
                    Ok(storage) => MofaSessionManager::with_storage(Box::new(storage)),
                    Err(_) => {
                        // Fallback to memory storage if file storage fails
                        MofaSessionManager::with_storage(Box::new(MemorySessionStorage::new()))
                    }
                }
            })
        });

        Self {
            inner,
            sessions_dir,
        }
    }

    /// Create with custom sessions directory
    pub fn with_sessions_dir(sessions_dir: PathBuf) -> Self {
        let sessions_dir_clone = sessions_dir.clone();
        let inner = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                tokio::fs::create_dir_all(&sessions_dir_clone).await.ok();

                match JsonlSessionStorage::new(&sessions_dir_clone).await {
                    Ok(storage) => MofaSessionManager::with_storage(Box::new(storage)),
                    Err(_) => {
                        MofaSessionManager::with_storage(Box::new(MemorySessionStorage::new()))
                    }
                }
            })
        });

        Self {
            inner,
            sessions_dir,
        }
    }

    /// Get an existing session or create a new one (delegates to mofa)
    pub async fn get_or_create(&self, key: impl Into<String>) -> Session {
        self.inner.get_or_create(&key.into()).await
    }

    /// Load a session from disk
    pub async fn load(&self, key: &str) -> Result<Session> {
        let session = self.get_or_create(key).await;

        // Check if file exists on disk
        let path = self.get_session_path(key);
        if session.is_empty() && !path.exists() {
            return Err(crate::error::SessionError::NotFound(key.to_string()).into());
        }

        Ok(session)
    }

    /// Save a session to disk (delegates to mofa)
    pub async fn save(&self, session: &Session) -> Result<()> {
        let mut session = session.clone();
        session
            .metadata
            .entry("schema_version".to_string())
            .or_insert_with(|| json!(SESSION_SCHEMA_VERSION));

        self.inner
            .save(&session)
            .await
            .map_err(|e| crate::error::SessionError::SaveFailed(e.to_string()))?;
        Ok(())
    }

    /// Delete a session (delegates to mofa)
    pub async fn delete(&self, key: &str) -> Result<bool> {
        self.inner
            .delete(key)
            .await
            .map_err(|e| crate::error::SessionError::LoadFailed(e.to_string()).into())
    }

    /// List all sessions with metadata
    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>> {
        let keys = self
            .inner
            .list()
            .await
            .map_err(|e| crate::error::SessionError::LoadFailed(e.to_string()))?;

        let mut sessions = Vec::new();
        for key in keys {
            let path = self.get_session_path(&key);
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if let Some(first_line) = content.lines().next() {
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(first_line) {
                        let info = SessionInfo {
                            key: key.clone(),
                            created_at: data
                                .get("created_at")
                                .and_then(|v| v.as_str())
                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                .map(|dt| dt.with_timezone(&Utc)),
                            updated_at: data
                                .get("updated_at")
                                .and_then(|v| v.as_str())
                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                .map(|dt| dt.with_timezone(&Utc)),
                            path: path.clone(),
                        };
                        sessions.push(info);
                    }
                }
            }
        }

        sessions.sort_by(|a, b| {
            b.updated_at
                .unwrap_or(Utc::now())
                .cmp(&a.updated_at.unwrap_or(Utc::now()))
        });

        Ok(sessions)
    }

    /// Get the file path for a session
    fn get_session_path(&self, key: &str) -> PathBuf {
        let safe_key = safe_filename(&key.replace(':', "_"));
        self.sessions_dir.join(format!("{}.jsonl", safe_key))
    }

    /// Get the inner mofa SessionManager (for advanced usage)
    pub fn inner(&self) -> &MofaSessionManager {
        &self.inner
    }
}

/// Convert a filename to a safe filename
fn safe_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ============================================================================
// Conversion utilities between mofaclaw Message and mofa SessionMessage
// ============================================================================

/// Convert mofaclaw Messages to SessionMessages for storage
pub fn messages_to_session_messages(messages: &[Message]) -> Vec<SessionMessage> {
    messages
        .iter()
        .map(|m| SessionMessage {
            role: format!("{:?}", m.role).to_lowercase(),
            content: encode_message_content(m.content.as_ref()),
            timestamp: Utc::now(),
        })
        .collect()
}

/// Convert SessionMessages to mofaclaw Messages for LLM context
///
/// Note: Structured content is preserved by encoding it into the content string.
pub fn session_messages_to_messages(session_messages: &[SessionMessage]) -> Vec<Message> {
    session_messages
        .iter()
        .map(|sm| {
            let role = match sm.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                "tool" => MessageRole::Tool,
                _ => MessageRole::User,
            };
            Message {
                role,
                content: decode_message_content(&sm.content),
                tool_call_id: None,
                name: None,
                tool_calls: Vec::new(),
            }
        })
        .collect()
}

/// Extension trait to add mofaclaw-compatible methods to mofa's Session
pub trait SessionExt {
    /// Get message history as mofaclaw Messages (for LLM context)
    fn get_history_as_messages(&self, max_messages: usize) -> Vec<Message>;

    /// Add a mofaclaw Message to the session
    fn add_message_from(&mut self, message: &Message);
}

impl SessionExt for Session {
    fn get_history_as_messages(&self, max_messages: usize) -> Vec<Message> {
        let recent = if self.messages.len() > max_messages {
            &self.messages[self.messages.len() - max_messages..]
        } else {
            &self.messages
        };

        session_messages_to_messages(recent)
    }

    fn add_message_from(&mut self, message: &Message) {
        let session_msgs = messages_to_session_messages(std::slice::from_ref(message));
        if let Some(msg) = session_msgs.first() {
            self.messages.push(msg.clone());
            self.updated_at = Utc::now();
        }
    }
}

fn encode_message_content(content: Option<&MessageContent>) -> String {
    let Some(content) = content else {
        return String::new();
    };

    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Array(_) => {
            let payload = serde_json::to_string(content).unwrap_or_else(|_| "[]".to_string());
            format!("{}{}", STRUCTURED_CONTENT_PREFIX, payload)
        }
    }
}

fn decode_message_content(content: &str) -> Option<MessageContent> {
    if let Some(payload) = content.strip_prefix(STRUCTURED_CONTENT_PREFIX)
        && let Ok(parsed) = serde_json::from_str::<MessageContent>(payload)
    {
        Some(parsed)
    } else {
        Some(MessageContent::Text(content.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_safe_filename() {
        assert_eq!(safe_filename("test:key"), "test_key");
        assert_eq!(safe_filename("test/file@name"), "test_file_name");
    }

    #[test]
    fn test_message_conversion() {
        // Test Message -> SessionMessage
        let msg = Message::user("Hello world");
        let session_msgs = messages_to_session_messages(&[msg]);
        assert_eq!(session_msgs.len(), 1);
        assert_eq!(session_msgs[0].role, "user");
        assert_eq!(session_msgs[0].content, "Hello world");

        // Test SessionMessage -> Message
        let messages = session_messages_to_messages(&session_msgs);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[0].content_as_text(), "Hello world");

        // Structured content round-trip
        let structured = Message::user_with_content(MessageContent::Array(vec![
            json!({
                "type": "image_url",
                "image_url": { "url": "data:image/png;base64,AAA" }
            }),
            json!({
                "type": "text",
                "text": "Look at this"
            }),
        ]));
        let session_msgs = messages_to_session_messages(&[structured.clone()]);
        assert_eq!(session_msgs.len(), 1);
        assert!(
            session_msgs[0]
                .content
                .starts_with(STRUCTURED_CONTENT_PREFIX)
        );

        let messages = session_messages_to_messages(&session_msgs);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::User);
        assert!(matches!(
            messages[0].content,
            Some(MessageContent::Array(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_session_manager_creation() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let manager = SessionManager::with_sessions_dir(temp_dir.path().to_path_buf());

        let session = manager.get_or_create("test:session").await;
        assert_eq!(session.key, "test:session");

        manager.save(&session).await.unwrap();

        // Reload
        let loaded = manager.get_or_create("test:session").await;
        assert_eq!(loaded.key, "test:session");
    }

    #[tokio::test]
    async fn test_session_ext() {
        use mofa_sdk::agent::Session;

        let mut session = Session::new("test:ext");

        // Add message from mofaclaw Message
        let msg = Message::user("Test message");
        session.add_message_from(&msg);

        assert_eq!(session.len(), 1);

        // Get as mofaclaw Messages
        let history = session.get_history_as_messages(10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, MessageRole::User);
        assert_eq!(history[0].content_as_text(), "Test message");
    }
}
