//! OpenAI-compatible API types for the mofaclaw HTTP gateway
//!
//! These types allow any OpenAI-compatible client (curl, Python openai SDK,
//! LangChain, etc.) to talk to mofaclaw using the familiar `/v1/chat/completions` endpoint.

use chrono::Utc;
use serde::{Deserialize, Serialize};

// ─────────────────────────── Request types ────────────────────────────────

/// A single message in the conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// "system", "user", or "assistant"
    pub role: String,
    /// Message text content
    pub content: String,
}

/// POST /v1/chat/completions request body
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model name (ignored – mofaclaw uses its configured model)
    #[serde(default = "default_model")]
    pub model: String,
    /// Conversation messages
    pub messages: Vec<ChatMessage>,
    /// Whether to stream the response via SSE
    #[serde(default)]
    pub stream: bool,
    /// Optional caller-supplied conversation ID for multi-turn sessions.
    /// If omitted a new UUID is generated for each request.
    #[serde(default)]
    pub conversation_id: Option<String>,
}

fn default_model() -> String {
    "mofaclaw".to_string()
}

// ─────────────────────────── Response types ───────────────────────────────

/// Non-streaming chat completion response  (mirrors OpenAI schema)
#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl ChatCompletionResponse {
    pub fn new(id: impl Into<String>, model: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        let completion_tokens = content.split_whitespace().count() as u32;
        Self {
            id: id.into(),
            object: "chat.completion".to_string(),
            created: Utc::now().timestamp(),
            model: model.into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: content.clone(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens,
                total_tokens: completion_tokens,
            },
        }
    }
}

// ─────────────────────── Streaming chunk types ────────────────────────────

/// One SSE chunk for streaming responses
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl ChatCompletionChunk {
    /// First chunk – sets role, no content yet
    pub fn role_chunk(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            object: "chat.completion.chunk".to_string(),
            created: Utc::now().timestamp(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta {
                    role: Some("assistant".to_string()),
                    content: None,
                },
                finish_reason: None,
            }],
        }
    }

    /// Content chunk
    pub fn content_chunk(
        id: impl Into<String>,
        model: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            object: "chat.completion.chunk".to_string(),
            created: Utc::now().timestamp(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta {
                    role: None,
                    content: Some(content.into()),
                },
                finish_reason: None,
            }],
        }
    }

    /// Final chunk – signals stream end
    pub fn stop_chunk(id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            object: "chat.completion.chunk".to_string(),
            created: Utc::now().timestamp(),
            model: model.into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
        }
    }
}

// ─────────────────────────── Models endpoint ──────────────────────────────

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

impl ModelList {
    pub fn mofaclaw_models(model: impl Into<String>) -> Self {
        let model = model.into();
        Self {
            object: "list".to_string(),
            data: vec![ModelInfo {
                id: model,
                object: "model".to_string(),
                created: 1_700_000_000,
                owned_by: "mofaclaw".to_string(),
            }],
        }
    }
}

// ─────────────────────────── Error response ───────────────────────────────

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: ApiErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    pub message: String,
    pub r#type: String,
    pub code: Option<String>,
}

impl ApiError {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            error: ApiErrorBody {
                message: msg.into(),
                r#type: "internal_error".to_string(),
                code: None,
            },
        }
    }

    pub fn timeout() -> Self {
        Self {
            error: ApiErrorBody {
                message: "Request timed out waiting for agent response".to_string(),
                r#type: "timeout".to_string(),
                code: Some("timeout".to_string()),
            },
        }
    }
}
