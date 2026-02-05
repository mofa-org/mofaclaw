//! Tools system for Mofaclaw
//!
//! Tools are capabilities that the agent can use to interact with the environment,
//! such as reading files, executing commands, searching the web, etc.

pub mod registry;
pub mod base;
pub mod filesystem;
pub mod shell;
pub mod web;
pub mod message;
pub mod spawn;

pub use registry::{ToolRegistry, ToolRegistryExecutor};
pub use base::{ToolDefinition, ToolExecutor};
pub use filesystem::{ReadFileTool, WriteFileTool, EditFileTool, ListDirTool};
pub use shell::ExecTool;
pub use web::{WebSearchTool, WebFetchTool};
pub use message::MessageTool;
pub use spawn::{SpawnTool, SubagentManager, InMemorySubagentManager};
