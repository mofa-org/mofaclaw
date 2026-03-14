//! Inter-agent communication system
//!
//! This module provides structured messaging between agents in a team,
//! enabling collaboration and coordination.

pub mod bus;
pub mod protocol;

pub use bus::AgentMessageBus;
pub use protocol::{AgentId, AgentMessage, AgentMessageType, RequestType};
