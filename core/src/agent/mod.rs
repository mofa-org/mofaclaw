//! Agent module containing the core agent logic

pub mod context;
pub mod subagent;
pub mod loop_;

pub use context::ContextBuilder;
pub use subagent::SubagentManager;
pub use loop_::{AgentLoop, ActiveSubagent};
