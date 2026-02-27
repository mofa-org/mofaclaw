//! Mofaclaw Core Library
//!
//! This library contains the core functionality for the Mofaclaw AI assistant,
//! including types, configuration, tools, message bus, sessions, and agent logic.
//!
//! The LLM interaction now uses MoFA framework's LLMAgentBuilder directly.

pub mod agent;
pub mod bus;
pub mod channels;
pub mod config;
pub mod cron;
pub mod error;
pub mod heartbeat;
pub mod messages;
pub mod permissions;
pub mod provider;
pub mod python_env;
pub mod session;
pub mod tools;
pub mod types;

// Re-exports for convenience
pub use agent::{AgentLoop, ContextBuilder, SubagentManager};
pub use bus::{InboundMessage, MessageBus, OutboundMessage};
pub use channels::{Channel, ChannelManager, DingTalkChannel, FeishuChannel, TelegramChannel};
pub use config::{
    AgentDefaults, AgentsConfig, ChannelsConfig, Config, DingTalkConfig, FeishuConfig,
    GatewayConfig, ProviderConfig, ProvidersConfig, TelegramConfig, ToolsConfig,
    TranscriptionConfig, WebSearchConfig, WebToolsConfig, WhatsAppConfig, default_config,
    get_config_dir, get_config_path, get_data_dir, get_workspace_path, load_config, save_config,
};
pub use cron::{CronJob, CronPayload, CronSchedule, CronService};
pub use error::*;
pub use heartbeat::HeartbeatService;
pub use permissions::{PermissionLevel, PermissionManager};
pub use provider::{GroqTranscriptionProvider, TranscriptionProvider};
pub use python_env::PythonEnv;
pub use session::{
    Session, SessionExt, SessionInfo, SessionManager, messages_to_session_messages,
    session_messages_to_messages,
};
pub use tools::ToolRegistry;
pub use types::*;
