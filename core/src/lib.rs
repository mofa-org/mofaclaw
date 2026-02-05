//! Mofaclaw Core Library
//!
//! This library contains the core functionality for the Mofaclaw AI assistant,
//! including types, configuration, tools, message bus, sessions, and agent logic.
//!
//! The LLM interaction now uses MoFA framework's LLMAgentBuilder directly.

pub mod types;
pub mod messages;
pub mod config;
pub mod error;
pub mod tools;
pub mod bus;
pub mod session;
pub mod agent;
pub mod provider;
pub mod channels;
pub mod cron;
pub mod heartbeat;
pub mod python_env;

// Re-exports for convenience
pub use types::*;
pub use messages::*;
pub use config::{Config, TelegramConfig, WhatsAppConfig, DingTalkConfig, FeishuConfig, AgentsConfig, ChannelsConfig, ProviderConfig, ProvidersConfig, GatewayConfig, ToolsConfig, WebToolsConfig, WebSearchConfig, TranscriptionConfig, AgentDefaults, get_config_dir, get_config_path, get_data_dir, get_workspace_path, default_config, load_config, save_config};
pub use error::*;
pub use tools::{ToolRegistry};
pub use bus::{MessageBus, InboundMessage, OutboundMessage};
pub use session::{
    Session, SessionManager, SessionExt, SessionInfo,
    messages_to_session_messages, session_messages_to_messages,
};
pub use agent::{AgentLoop, ContextBuilder, SubagentManager};
pub use provider::{GroqTranscriptionProvider, TranscriptionProvider};
pub use channels::{Channel, ChannelManager, TelegramChannel, DingTalkChannel, FeishuChannel};
pub use cron::{CronService, CronJob, CronSchedule, CronPayload};
pub use heartbeat::HeartbeatService;
pub use python_env::PythonEnv;