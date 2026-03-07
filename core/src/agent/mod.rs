//! Agent module containing the core agent logic

pub mod collaboration;
pub mod communication;
pub mod context;
pub mod loop_;
pub mod roles;
pub mod subagent;

pub use collaboration::{AgentTeam, MemberStatus, TeamManager, TeamMember, TeamStatus};
pub use communication::{AgentId, AgentMessage, AgentMessageBus, AgentMessageType, RequestType};
pub use context::ContextBuilder;
pub use loop_::{ActiveSubagent, AgentLoop};
pub use roles::{AgentRole, RoleCapabilities, RoleRegistry};
pub use subagent::SubagentManager;
