//! Tools system for Mofaclaw
//!
//! Tools are capabilities that the agent can use to interact with the environment,
//! such as reading files, executing commands, searching the web, etc.

pub mod agent_message;
pub mod base;
pub mod filesystem;
pub mod message;
pub mod multi_agent;
pub mod registry;
pub mod shell;
pub mod spawn;
pub mod team;
pub mod web;
pub mod workflow;
pub mod workspace;

pub use agent_message::{BroadcastToTeamTool, RespondToApprovalTool, SendAgentMessageTool};
pub use base::{ToolDefinition, ToolExecutor};
pub use filesystem::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
pub use message::MessageTool;
pub use multi_agent::register_multi_agent_tools;
pub use registry::{ToolRegistry, ToolRegistryExecutor};
pub use shell::ExecTool;
pub use spawn::{InMemorySubagentManager, SpawnTool, SubagentManager};
pub use team::{CreateTeamTool, GetTeamStatusTool, ListTeamsTool};
pub use web::{WebFetchTool, WebSearchTool};
pub use workflow::{GetWorkflowStatusTool, ListWorkflowsTool, StartWorkflowTool};
pub use workspace::{CreateArtifactTool, GetArtifactTool, ListArtifactsTool};
