//! Async message bus for decoupled channel-agent communication

pub mod queue;

pub use queue::MessageBus;
// Re-export messages directly since they're defined in the messages module
pub use crate::messages::{InboundMessage, OutboundMessage};
