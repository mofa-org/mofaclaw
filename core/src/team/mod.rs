//! Multi-Agent Team Infrastructure
//!
//! Provides the TeamManager, AgentTeam, and core abstractions 
//! for orchestrating multiple agents collaboratively.

pub mod bus;
pub mod manager;
pub mod team;
pub mod workflow;
pub mod workspace;

pub use bus::{AgentMessageBus, TeamMessage};
pub use manager::TeamManager;
pub use team::{AgentTeam, Role};
pub use workflow::WorkflowEngine;
pub use workspace::SharedWorkspace;
