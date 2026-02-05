//! LLM provider module
//!
//! This module provides LLM provider abstractions with mofa_sdk integration.
//!
//! LLM interaction and transcription now use MoFA framework directly.

// Re-export ToolCallRequest from types since it's defined there
pub use crate::types::ToolCallRequest;

// Re-export mofa_sdk LLM types for convenience
pub use mofa_sdk::llm::{
    ChatMessage, ChatCompletionRequest, ChatCompletionResponse,
    LLMProvider as MofaLLMProvider, LLMError, LLMResult,
    Tool as MofaTool, ToolCall as MofaToolCall,
    LLMAgent, LLMAgentBuilder, openai::OpenAIProvider, openai::OpenAIConfig,
    TranscriptionProvider, GroqTranscriptionProvider, OpenAITranscriptionProvider,
};
