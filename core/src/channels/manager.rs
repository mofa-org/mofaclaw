//! Channel manager for multiple messaging platforms

use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::{ChannelsConfig, Config};
use crate::error::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::spawn;
use tokio::sync::RwLock;
use tracing::{error, info};

/// Manages multiple messaging channels
#[derive(Clone)]
pub struct ChannelManager {
    config: ChannelsConfig,
    _bus: MessageBus,
    channels: Arc<RwLock<Vec<Arc<dyn Channel>>>>,
    running: Arc<RwLock<bool>>,
}

impl ChannelManager {
    /// Create a new channel manager
    pub fn new(config: &Config, bus: MessageBus) -> Self {
        let channels_config = config.channels.clone();
        let channels = Arc::new(RwLock::new(Vec::new()));

        Self {
            config: channels_config,
            _bus: bus,
            channels,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Register a channel
    pub async fn register_channel(&self, channel: Arc<dyn Channel>) {
        self.channels.write().await.push(channel);
    }

    /// Get list of enabled channel names
    pub fn enabled_channels(&self) -> Vec<String> {
        let mut enabled = Vec::new();

        if self.config.telegram.enabled {
            enabled.push("telegram".to_string());
        }
        if self.config.whatsapp.enabled {
            enabled.push("whatsapp".to_string());
        }
        if self.config.dingtalk.enabled {
            enabled.push("dingtalk".to_string());
        }
        if self.config.feishu.enabled {
            enabled.push("feishu".to_string());
        }
        if self.config.discord.enabled {
            enabled.push("discord".to_string());
        }

        enabled
    }

    /// Start all enabled channels (runs until channels are stopped)
    pub async fn start_all(&self) -> Result<()> {
        *self.running.write().await = true;

        let channels = self.channels.read().await;
        let enabled_names: HashSet<_> = self.enabled_channels().into_iter().collect();

        let mut handles = Vec::new();

        for channel in channels.iter() {
            if enabled_names.contains(channel.name()) {
                let channel_clone = channel.clone();
                let running = self.running.clone();
                let name = channel.name().to_string();

                let handle = spawn(async move {
                    info!("Starting channel: {}", name);

                    while *running.read().await {
                        if let Err(e) = channel_clone.start().await {
                            error!("Channel {} error: {}", name, e);
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        } else {
                            break;
                        }
                    }

                    info!("Channel {} stopped", name);
                });

                handles.push(handle);
            }
        }

        // Wait for all channel tasks to complete
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Channel task error: {}", e);
            }
        }

        Ok(())
    }

    /// Stop all channels
    pub async fn stop_all(&self) -> Result<()> {
        *self.running.write().await = false;

        let channels = self.channels.read().await;
        for channel in channels.iter() {
            if let Err(e) = channel.stop().await {
                error!("Error stopping channel {}: {}", channel.name(), e);
            }
        }

        Ok(())
    }

    /// Check if any channels are enabled
    pub fn has_enabled_channels(&self) -> bool {
        self.config.telegram.enabled
            || self.config.whatsapp.enabled
            || self.config.dingtalk.enabled
            || self.config.feishu.enabled
            || self.config.discord.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_manager() {
        let bus = MessageBus::new();
        let config = Config::default();
        let manager = ChannelManager::new(&config, bus);

        assert!(!manager.has_enabled_channels());
        assert!(manager.enabled_channels().is_empty());
    }
}
