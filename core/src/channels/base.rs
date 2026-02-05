//! Base channel trait

use crate::bus::MessageBus;
use crate::error::Result;
use async_trait::async_trait;

/// Base trait for chat channels
///
/// Channels are messaging platforms like Telegram, Discord, Slack, WhatsApp, etc.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Get the name of this channel
    fn name(&self) -> &str;

    /// Start the channel (begin receiving messages)
    async fn start(&self) -> Result<()>;

    /// Stop the channel
    async fn stop(&self) -> Result<()>;

    /// Check if the channel is enabled
    fn is_enabled(&self) -> bool;
}

/// Configuration for a channel
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub enabled: bool,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyChannel {
        enabled: bool,
    }

    #[async_trait]
    impl Channel for DummyChannel {
        fn name(&self) -> &str {
            "dummy"
        }

        async fn start(&self) -> Result<()> {
            Ok(())
        }

        async fn stop(&self) -> Result<()> {
            Ok(())
        }

        fn is_enabled(&self) -> bool {
            self.enabled
        }
    }

    #[tokio::test]
    async fn test_channel_trait() {
        let channel = DummyChannel { enabled: true };
        assert_eq!(channel.name(), "dummy");
        assert!(channel.is_enabled());
    }
}
