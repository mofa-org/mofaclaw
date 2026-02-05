//! Async message queue for decoupled channel-agent communication

use crate::error::{ChannelError, Result};
use crate::messages::{InboundMessage, OutboundMessage};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

/// Async message bus that decouples chat channels from the agent core
///
/// Channels push messages to the inbound queue, and the agent processes
/// them and pushes responses to the outbound queue.
#[derive(Clone)]
pub struct MessageBus {
    inbound: broadcast::Sender<InboundMessage>,
    outbound: broadcast::Sender<OutboundMessage>,
    outbound_subscribers: Arc<RwLock<HashMap<String, Vec<OutboundCallback>>>>,
}

type OutboundCallback = Arc<dyn Fn(OutboundMessage) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync>;

impl MessageBus {
    /// Create a new message bus
    pub fn new() -> Self {
        let (inbound_tx, _) = broadcast::channel(100);
        let (outbound_tx, _) = broadcast::channel(100);

        Self {
            inbound: inbound_tx,
            outbound: outbound_tx,
            outbound_subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish a message from a channel to the agent
    pub async fn publish_inbound(&self, msg: InboundMessage) -> Result<()> {
        match self.inbound.send(msg) {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!("Failed to publish inbound message: {}", e);
                Err(ChannelError::SendFailed(e.to_string()).into())
            }
        }
    }

    /// Subscribe to inbound messages
    pub fn subscribe_inbound(&self) -> broadcast::Receiver<InboundMessage> {
        self.inbound.subscribe()
    }

    /// Publish a response from the agent to channels
    pub async fn publish_outbound(&self, msg: OutboundMessage) -> Result<()> {
        match self.outbound.send(msg) {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!("Failed to publish outbound message: {}", e);
                Err(ChannelError::SendFailed(e.to_string()).into())
            }
        }
    }

    /// Subscribe to outbound messages
    pub fn subscribe_outbound(&self) -> broadcast::Receiver<OutboundMessage> {
        self.outbound.subscribe()
    }

    /// Subscribe to outbound messages for a specific channel with a callback
    pub async fn subscribe_outbound_channel<F, Fut>(&self, channel: String, callback: F)
    where
        F: Fn(OutboundMessage) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let mut subscribers = self.outbound_subscribers.write().await;
        subscribers
            .entry(channel)
            .or_insert_with(Vec::new)
            .push(Arc::new(move |msg| Box::pin(callback(msg))));
    }

    /// Dispatch outbound messages to subscribed channels
    pub async fn dispatch_outbound(&self) -> Result<()> {
        let mut rx = self.outbound.subscribe();

        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let channel = msg.channel.clone();
                    let subscribers = self.outbound_subscribers.read().await;

                    if let Some(callbacks) = subscribers.get(&channel) {
                        for callback in callbacks.iter() {
                            let msg_clone = msg.clone();
                            if let Err(e) = callback(msg_clone).await {
                                error!("Error dispatching to {}: {}", channel, e);
                            }
                        }
                    } else {
                        debug!("No subscribers for channel: {}", channel);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Outbound receiver lagged, missed {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Outbound message bus closed");
                    return Ok(());
                }
            }
        }
    }

    /// Get the number of inbound subscribers
    pub async fn inbound_subscriber_count(&self) -> usize {
        self.inbound.receiver_count()
    }

    /// Get the number of outbound subscribers
    pub async fn outbound_subscriber_count(&self) -> usize {
        self.outbound.receiver_count()
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_message_bus_publish() {
        let bus = MessageBus::new();

        // Subscribe to inbound messages
        let mut rx = bus.subscribe_inbound();

        // Publish a message
        let msg = InboundMessage::new("test", "user", "chat", "Hello");
        tokio::spawn(async move {
            bus.publish_inbound(msg).await.unwrap();
        });

        // Receive the message
        let received = rx.recv().await.unwrap();
        assert_eq!(received.content, "Hello");
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = MessageBus::new();

        let mut rx1 = bus.subscribe_inbound();
        let mut rx2 = bus.subscribe_inbound();

        let msg = InboundMessage::new("test", "user", "chat", "Hello");
        bus.publish_inbound(msg).await.unwrap();

        let received1 = rx1.recv().await.unwrap();
        let received2 = rx2.recv().await.unwrap();

        assert_eq!(received1.content, "Hello");
        assert_eq!(received2.content, "Hello");
    }
}
