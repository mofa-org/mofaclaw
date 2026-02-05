//! Error types for Mofaclaw

use std::path::PathBuf;
use thiserror::Error;

/// Result type for Mofaclaw operations
pub type Result<T> = std::result::Result<T, MofaclawError>;

/// Main error type for Mofaclaw
#[derive(Error, Debug)]
pub enum MofaclawError {
    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// Tool execution errors
    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    /// Provider errors
    #[error("LLM provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Session errors
    #[error("Session error: {0}")]
    Session(#[from] SessionError),

    /// Channel errors
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),

    /// Agent errors
    #[error("Agent error: {0}")]
    Agent(#[from] AgentError),

    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic errors
    #[error("{0}")]
    Other(String),
}

/// Configuration-related errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    NotFound(PathBuf),

    #[error("Failed to parse config: {0}")]
    Parse(String),

    #[error("Invalid configuration: {0}")]
    Invalid(String),

    #[error("Missing required configuration: {0}")]
    Missing(String),
}

/// Tool execution errors
#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid tool parameters: {0}")]
    InvalidParameters(String),

    #[error("Tool timeout after {0}s")]
    Timeout(u64),

    #[error("File error: {0}")]
    File(String),

    #[error("Command error: {0}")]
    Command(String),
}

/// LLM provider errors
#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("API request failed: {0}")]
    RequestFailed(String),

    #[error("Invalid API response: {0}")]
    InvalidResponse(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("No API key configured")]
    NoApiKey,

    #[error("Unsupported model: {0}")]
    UnsupportedModel(String),

    #[error("Provider error: {0}")]
    Provider(String),
}

/// Session-related errors
#[derive(Error, Debug)]
pub enum SessionError {
    #[error("Failed to load session: {0}")]
    LoadFailed(String),

    #[error("Failed to save session: {0}")]
    SaveFailed(String),

    #[error("Invalid session format: {0}")]
    InvalidFormat(String),

    #[error("Session not found: {0}")]
    NotFound(String),
}

/// Channel-related errors
#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("Channel not configured: {0}")]
    NotConfigured(String),

    #[error("Failed to send message: {0}")]
    SendFailed(String),

    #[error("Channel connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed for channel: {0}")]
    AuthenticationFailed(String),

    #[error("Message not delivered")]
    NotDelivered,

    /// Python not installed
    #[error("Python is not installed or not found in PATH")]
    PythonNotInstalled,

    /// Python version too old
    #[error("Python version is too old: {current}, required: {required}")]
    PythonVersionTooOld { current: String, required: String },

    /// Python package installation failed
    #[error("Failed to install Python package '{package}': {error}")]
    PythonPackageInstallFailed { package: String, error: String },
}

/// Agent-related errors
#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Agent loop stopped")]
    Stopped,

    #[error("Max iterations exceeded")]
    MaxIterationsExceeded,

    #[error("Context building failed: {0}")]
    ContextFailed(String),

    #[error("Context error: {0}")]
    ContextError(String),

    #[error("Memory error: {0}")]
    Memory(String),

    #[error("Subagent error: {0}")]
    Subagent(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Cron error: {0}")]
    Cron(String),
}

impl From<anyhow::Error> for MofaclawError {
    fn from(err: anyhow::Error) -> Self {
        MofaclawError::Other(err.to_string())
    }
}

impl From<serde_json::Error> for MofaclawError {
    fn from(err: serde_json::Error) -> Self {
        MofaclawError::Other(format!("JSON error: {}", err))
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(err: reqwest::Error) -> Self {
        ProviderError::RequestFailed(err.to_string())
    }
}

impl From<reqwest::Error> for MofaclawError {
    fn from(err: reqwest::Error) -> Self {
        MofaclawError::Provider(ProviderError::RequestFailed(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ToolError::NotFound("test_tool".to_string());
        assert_eq!(err.to_string(), "Tool not found: test_tool");
    }

    #[test]
    fn test_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let mofaclaw_err: MofaclawError = io_err.into();
        assert!(matches!(mofaclaw_err, MofaclawError::Io(_)));
    }
}
