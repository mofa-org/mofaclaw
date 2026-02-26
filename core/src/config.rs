//! Configuration system for Mofaclaw
//!
//! This module provides configuration loading from files and environment variables.
//! It mirrors the Python config/schema.py structure.

use crate::error::{ConfigError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

/// WhatsApp channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WhatsAppConfig {
    /// Whether WhatsApp is enabled
    #[serde(default)]
    pub enabled: bool,
    /// WebSocket bridge URL
    #[serde(default = "default_bridge_url")]
    pub bridge_url: String,
    /// Allowed phone numbers
    #[serde(default)]
    pub allow_from: Vec<String>,
}

fn default_bridge_url() -> String {
    "ws://localhost:3001".to_string()
}

/// Telegram channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    /// Whether Telegram is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Bot token from @BotFather
    #[serde(default)]
    pub token: String,
    /// Allowed user IDs or usernames
    #[serde(default)]
    pub allow_from: Vec<String>,
}

/// DingTalk channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DingTalkConfig {
    /// Whether DingTalk is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Client ID (AppKey)
    #[serde(default)]
    pub client_id: String,
    /// Client Secret (AppSecret)
    #[serde(default)]
    pub client_secret: String,
    /// Robot code (optional, for some API calls)
    #[serde(default)]
    pub robot_code: String,
    /// Corporation ID (optional)
    #[serde(default)]
    pub corp_id: String,
    /// Agent ID (optional)
    #[serde(default)]
    pub agent_id: u64,
    /// Direct message policy: "open" or "restricted"
    #[serde(default = "default_dm_policy")]
    pub dm_policy: String,
    /// Group message policy: "open" or "restricted"
    #[serde(default = "default_group_policy")]
    pub group_policy: String,
    /// Message type: "markdown" or "text"
    #[serde(default = "default_message_type")]
    pub message_type: String,
    /// Debug mode
    #[serde(default)]
    pub debug: bool,
    /// Python bridge WebSocket URL (optional, defaults to ws://localhost:3002)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,
}

/// Feishu (Lark) channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeishuConfig {
    /// Whether Feishu is enabled
    #[serde(default)]
    pub enabled: bool,
    /// App ID (from Feishu open platform)
    #[serde(default)]
    pub app_id: String,
    /// App Secret (from Feishu open platform)
    #[serde(default)]
    pub app_secret: String,
    /// Encrypt key for event subscription (optional)
    #[serde(default)]
    pub encrypt_key: String,
    /// Verification token for event subscription (optional)
    #[serde(default)]
    pub verification_token: String,
    /// Message type: "text", "post", or "interactive"
    #[serde(default = "default_feishu_message_type")]
    pub message_type: String,
    /// Direct message policy: "open" or "restricted"
    #[serde(default = "default_feishu_dm_policy")]
    pub dm_policy: String,
    /// Group message policy: "open" or "restricted"
    #[serde(default = "default_feishu_group_policy")]
    pub group_policy: String,
    /// Debug mode
    #[serde(default)]
    pub debug: bool,
    /// Python bridge WebSocket URL (optional, defaults to ws://localhost:3004)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,
}

/// Discord channel configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscordConfig {
    /// Whether Discord is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Bot token from Discord Developer Portal
    #[serde(default)]
    pub token: String,
    /// Application ID (Bot Application ID)
    #[serde(default)]
    pub application_id: u64,
    /// Guild ID (optional, for guild-specific commands)
    #[serde(default)]
    pub guild_id: Option<u64>,
    /// Allowed users/roles (user IDs or role IDs as strings)
    #[serde(default)]
    pub allow_from: Vec<String>,
    /// Admin role list (role IDs as strings)
    #[serde(default)]
    pub admin_roles: Vec<String>,
    /// Member role list (role IDs as strings, for granular permissions)
    #[serde(default)]
    pub member_roles: Vec<String>,
}

fn default_feishu_message_type() -> String {
    "text".to_string()
}

fn default_feishu_dm_policy() -> String {
    "open".to_string()
}

fn default_feishu_group_policy() -> String {
    "open".to_string()
}

fn default_dm_policy() -> String {
    "open".to_string()
}

fn default_group_policy() -> String {
    "open".to_string()
}

fn default_message_type() -> String {
    "markdown".to_string()
}

/// Configuration for chat channels
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    /// WhatsApp configuration
    #[serde(default)]
    pub whatsapp: WhatsAppConfig,
    /// Telegram configuration
    #[serde(default)]
    pub telegram: TelegramConfig,
    /// DingTalk configuration
    #[serde(default)]
    pub dingtalk: DingTalkConfig,
    /// Feishu (Lark) configuration
    #[serde(default)]
    pub feishu: FeishuConfig,
    /// Discord configuration
    #[serde(default)]
    pub discord: DiscordConfig,
}

/// Default agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefaults {
    /// Workspace directory path
    #[serde(default = "default_workspace")]
    pub workspace: String,
    /// Default model to use
    #[serde(default = "default_model")]
    pub model: String,
    /// Maximum tokens for generation
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    /// Sampling temperature
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// Maximum tool iterations
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
            model: default_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            max_tool_iterations: default_max_tool_iterations(),
        }
    }
}

fn default_workspace() -> String {
    "~/.mofaclaw/workspace".to_string()
}

fn default_model() -> String {
    "anthropic/claude-opus-4-5".to_string()
}

fn default_max_tokens() -> usize {
    8192
}

fn default_temperature() -> f64 {
    0.7
}

fn default_max_tool_iterations() -> usize {
    20
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentsConfig {
    /// Default agent settings
    #[serde(default)]
    pub defaults: AgentDefaults,
}

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    /// API key for the provider
    #[serde(default)]
    pub api_key: String,
    /// Custom API base URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
}

/// Configuration for LLM providers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersConfig {
    /// Anthropic configuration
    #[serde(default)]
    pub anthropic: ProviderConfig,
    /// OpenAI configuration
    #[serde(default)]
    pub openai: ProviderConfig,
    /// OpenRouter configuration
    #[serde(default)]
    pub openrouter: ProviderConfig,
    /// Zhipu AI configuration
    #[serde(default)]
    pub zhipu: ProviderConfig,
    /// vLLM configuration
    #[serde(default)]
    pub vllm: ProviderConfig,
    /// Gemini configuration
    #[serde(default)]
    pub gemini: ProviderConfig,
    /// Groq configuration
    #[serde(default)]
    pub groq: ProviderConfig,
}

/// Gateway/server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Host to bind to
    #[serde(default = "default_gateway_host")]
    pub host: String,
    /// Port to listen on
    #[serde(default = "default_gateway_port")]
    pub port: u16,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_gateway_host(),
            port: default_gateway_port(),
        }
    }
}

fn default_gateway_host() -> String {
    "0.0.0.0".to_string()
}

fn default_gateway_port() -> u16 {
    18790
}

/// Web search tool configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebSearchConfig {
    /// Brave Search API key
    #[serde(default)]
    pub api_key: String,
    /// Maximum number of results
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    5
}

/// Web tools configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebToolsConfig {
    /// Search configuration
    #[serde(default)]
    pub search: WebSearchConfig,
}

/// Tools configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Web tools configuration
    #[serde(default)]
    pub web: WebToolsConfig,
    /// Transcription configuration
    #[serde(default)]
    pub transcription: TranscriptionConfig,
}

/// Transcription configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranscriptionConfig {
    /// Groq API key for transcription
    #[serde(default)]
    pub groq_api_key: String,
}

/// Root configuration for Mofaclaw
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Agent configuration
    #[serde(default)]
    pub agents: AgentsConfig,
    /// Channels configuration
    #[serde(default)]
    pub channels: ChannelsConfig,
    /// Providers configuration
    #[serde(default)]
    pub providers: ProvidersConfig,
    /// Gateway configuration
    #[serde(default)]
    pub gateway: GatewayConfig,
    /// Tools configuration
    #[serde(default)]
    pub tools: ToolsConfig,
}

impl Config {
    /// Get the expanded workspace path
    pub fn workspace_path(&self) -> PathBuf {
        expand_tilde(&self.agents.defaults.workspace)
    }

    /// Get API key in priority order
    pub fn get_api_key(&self) -> Option<String> {
        self.providers
            .openrouter
            .api_key
            .is_empty()
            .then(|| self.providers.anthropic.api_key.clone())
            .filter(|k| !k.is_empty())
            .or_else(|| {
                (!self.providers.openai.api_key.is_empty())
                    .then(|| self.providers.openai.api_key.clone())
            })
            .or_else(|| {
                (!self.providers.gemini.api_key.is_empty())
                    .then(|| self.providers.gemini.api_key.clone())
            })
            .or_else(|| {
                (!self.providers.zhipu.api_key.is_empty())
                    .then(|| self.providers.zhipu.api_key.clone())
            })
            .or_else(|| {
                (!self.providers.groq.api_key.is_empty())
                    .then(|| self.providers.groq.api_key.clone())
            })
            .or_else(|| {
                (!self.providers.vllm.api_key.is_empty())
                    .then(|| self.providers.vllm.api_key.clone())
            })
            .or_else(|| {
                (!self.providers.openrouter.api_key.is_empty())
                    .then(|| self.providers.openrouter.api_key.clone())
            })
    }

    /// Get API base URL if using custom endpoint
    pub fn get_api_base(&self) -> Option<String> {
        if !self.providers.openrouter.api_key.is_empty() {
            Some(
                self.providers
                    .openrouter
                    .api_base
                    .clone()
                    .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            )
        } else if !self.providers.zhipu.api_key.is_empty() {
            self.providers.zhipu.api_base.clone()
        } else if self.providers.vllm.api_base.is_some() {
            self.providers.vllm.api_base.clone()
        } else {
            None
        }
    }

    /// Get Brave Search API key
    pub fn get_brave_api_key(&self) -> Option<String> {
        (!self.tools.web.search.api_key.is_empty()).then(|| self.tools.web.search.api_key.clone())
    }

    /// Get Groq transcription API key
    pub fn get_groq_transcription_key(&self) -> Option<String> {
        if !self.tools.transcription.groq_api_key.is_empty() {
            Some(self.tools.transcription.groq_api_key.clone())
        } else if !self.providers.groq.api_key.is_empty() {
            Some(self.providers.groq.api_key.clone())
        } else {
            std::env::var("GROQ_API_KEY").ok()
        }
    }
}

/// Expand tilde in path
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

/// Get the default config directory
pub fn get_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mofaclaw")
}

/// Get the config file path
pub fn get_config_path() -> PathBuf {
    get_config_dir().join("config.json")
}

/// Get the data directory
pub fn get_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mofaclaw")
}

/// Get the workspace directory path
pub fn get_workspace_path() -> PathBuf {
    expand_tilde("~/.mofaclaw/workspace")
}

/// Load configuration from file
pub async fn load_config() -> Result<Config> {
    let config_path = get_config_path();

    if !config_path.exists() {
        return Err(ConfigError::NotFound(config_path).into());
    }

    let contents = fs::read_to_string(&config_path).await?;

    // Parse as JSON first (our preferred format)
    let config: Config = serde_json::from_str(&contents)
        .map_err(|e| ConfigError::Parse(format!("Failed to parse config JSON: {}", e)))?;

    // Apply environment overrides
    apply_env_overrides(&config);

    Ok(config)
}

/// Apply environment variable overrides to config
fn apply_env_overrides(_config: &Config) {
    // Environment variables override config values
    // Format: MOFACLAW__SECTION__KEY=value
    // For example: MOFACLAW__PROVIDERS__OPENROUTER__API_KEY=sk-...

    if let Ok(_key) = std::env::var("MOFACLAW_PROVIDERS_OPENROUTER_API_KEY") {
        // We can't modify the config in place easily, so this is handled at load time
        // For now, just log it (in real implementation, we'd handle this)
        tracing::debug!("OpenRouter API key from environment");
    }

    // Additional env vars can be added here
}

/// Save configuration to file
pub async fn save_config(config: &Config) -> Result<()> {
    let config_path = get_config_path();

    // Ensure config directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // Serialize to JSON with pretty formatting
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| ConfigError::Parse(format!("Failed to serialize config: {}", e)))?;

    fs::write(&config_path, json).await?;

    Ok(())
}

/// Create a default configuration
pub fn default_config() -> Config {
    Config::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = default_config();
        assert_eq!(config.agents.defaults.model, "anthropic/claude-opus-4-5");
        assert_eq!(config.agents.defaults.max_tokens, 8192);
        assert_eq!(config.gateway.port, 18790);
    }

    #[test]
    fn test_workspace_path_expansion() {
        let config = Config::default();
        let path = config.workspace_path();
        // Should expand ~ to the actual home directory
        assert!(!path.starts_with("~"));
    }

    #[test]
    fn test_config_paths() {
        let config_dir = get_config_dir();
        let config_path = get_config_path();
        assert!(config_path.starts_with(&config_dir));
    }
}
