//! Tools system for Mofaclaw
//!
//! Tools are capabilities that the agent can use to interact with the environment,
//! such as reading files, executing commands, searching the web, etc.

pub mod base;
pub mod filesystem;
pub mod message;
pub mod registry;
pub mod shell;
pub mod spawn;
pub mod videolize;
pub mod web;

pub use base::{ToolDefinition, ToolExecutor};
pub use filesystem::{EditFileTool, ListDirTool, ReadFileTool, WriteFileTool};
pub use message::MessageTool;
pub use registry::{ToolRegistry, ToolRegistryExecutor};
pub use shell::ExecTool;
pub use spawn::{InMemorySubagentManager, SpawnTool, SubagentManager};
pub use videolize::VideolizeTool;
pub use web::{WebFetchTool, WebSearchTool};
