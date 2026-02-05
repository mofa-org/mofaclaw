//! WhatsApp channel implementation using WebSocket bridge

use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::WhatsAppConfig;
use crate::error::Result;
use crate::messages::{InboundMessage, OutboundMessage};
use async_trait::async_trait;
use futures_util::stream::StreamExt;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// WhatsApp channel that connects to a Node.js bridge
///
/// The bridge uses @whiskeysockets/baileys to handle the WhatsApp Web protocol.
/// Communication between Rust and Node.js is via WebSocket.
pub struct WhatsAppChannel {
    config: WhatsAppConfig,
    bus: MessageBus,
    connected: Arc<RwLock<bool>>,
    running: Arc<RwLock<bool>>,
}

impl WhatsAppChannel {
    /// Create a new WhatsApp channel
    pub fn new(config: WhatsAppConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            connected: Arc::new(RwLock::new(false)),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Check if connected to the bridge
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Handle a message from the bridge
    async fn handle_bridge_message(&self, bus: &MessageBus, raw: &str) {
        // Parse JSON
        let data: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(_) => {
                warn!("Invalid JSON from bridge: {}", &raw[..raw.len().min(100)]);
                return;
            }
        };

        let msg_type = match data.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => {
                warn!("Bridge message missing 'type' field");
                return;
            }
        };

        match msg_type {
            "message" => {
                // Incoming message from WhatsApp
                let sender = data.get("sender").and_then(|v| v.as_str()).unwrap_or("");
                let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("");

                // sender is typically: <phone>@s.whatsapp.net
                // Extract just the phone number as chat_id
                let chat_id = if let Some(pos) = sender.find('@') {
                    &sender[..pos]
                } else {
                    sender
                };

                // Build metadata
                let mut json_metadata = Map::new();
                if let Some(id) = data.get("id") {
                    json_metadata.insert("message_id".to_string(), id.clone());
                }
                if let Some(timestamp) = data.get("timestamp") {
                    json_metadata.insert("timestamp".to_string(), timestamp.clone());
                }
                if let Some(is_group) = data.get("isGroup") {
                    json_metadata.insert("is_group".to_string(), is_group.clone());
                }

                // Convert to HashMap
                let metadata: HashMap<String, Value> = json_metadata.into_iter().collect();

                let msg = InboundMessage::with_metadata(
                    "whatsapp",
                    chat_id,
                    sender,
                    content,
                    metadata,
                );

                // Publish to bus
                if let Err(e) = bus.publish_inbound(msg).await {
                    error!("Failed to publish WhatsApp message: {}", e);
                }
            }
            "status" => {
                // Connection status update
                if let Some(status) = data.get("status").and_then(|v| v.as_str()) {
                    info!("WhatsApp status: {}", status);

                    match status {
                        "connected" => *self.connected.write().await = true,
                        "disconnected" => *self.connected.write().await = false,
                        _ => {}
                    }
                }
            }
            "qr" => {
                // QR code for authentication
                info!("Scan QR code in the bridge terminal to connect WhatsApp");
            }
            "error" => {
                if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
                    error!("WhatsApp bridge error: {}", err);
                }
            }
            _ => {
                warn!("Unknown bridge message type: {}", msg_type);
            }
        }
    }
}

#[async_trait]
impl Channel for WhatsAppChannel {
    fn name(&self) -> &str {
        "whatsapp"
    }

    async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            info!("WhatsApp channel disabled");
            return Ok(());
        }

        *self.running.write().await = true;
        let bridge_url = self.config.bridge_url.clone();

        info!("Connecting to WhatsApp bridge at {}...", bridge_url);

        let running = Arc::clone(&self.running);
        let connected = Arc::clone(&self.connected);
        let bus = self.bus.clone();

        let handle = tokio::spawn(async move {
            while *running.read().await {
                match tokio_tungstenite::connect_async(&bridge_url).await {
                    Ok((ws_stream, _)) => {
                        *connected.write().await = true;
                        info!("Connected to WhatsApp bridge");

                        let mut ws_stream = ws_stream;

                        // Listen for messages
                        while *running.read().await {
                            match ws_stream.next().await {
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                    // Handle the message - we need a channel instance here
                                    // For now, just log it
                                    info!("Received from WhatsApp bridge: {}", text);
                                }
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                                    info!("WhatsApp bridge connection closed");
                                    break;
                                }
                                Some(Err(e)) => {
                                    error!("WebSocket error: {}", e);
                                    break;
                                }
                                None => {
                                    info!("WebSocket stream ended");
                                    break;
                                }
                                Some(Ok(_)) => {
                                    // Ignore other message types (ping, pong, binary)
                                }
                            }
                        }

                        *connected.write().await = false;
                    }
                    Err(e) => {
                        *connected.write().await = false;
                        warn!("WhatsApp bridge connection error: {}", e);

                        if *running.read().await {
                            info!("Reconnecting in 5 seconds...");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            }

            info!("WhatsApp channel stopped");
        });

        // Detach the task (in production, you'd store the handle for cleanup)
        tokio::spawn(async move {
            let _ = handle.await;
        });

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        *self.running.write().await = false;
        *self.connected.write().await = false;
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

impl WhatsAppChannel {
    /// Send a message through WhatsApp
    pub async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        if !*self.connected.read().await {
            warn!("WhatsApp bridge not connected");
            return Ok(()); // Don't error, just skip
        }

        // In a full implementation, we'd need to store the WebSocket sender
        // For now, this is a placeholder that logs the message
        info!("Sending WhatsApp message to {}: {}", msg.chat_id, msg.content);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whatsapp_channel_name() {
        let config = WhatsAppConfig {
            enabled: true,
            bridge_url: "ws://localhost:3001".to_string(),
            allow_from: vec![],
        };
        let bus = MessageBus::new();
        let channel = WhatsAppChannel::new(config, bus);
        assert_eq!(channel.name(), "whatsapp");
    }

    #[test]
    fn test_whatsapp_is_enabled() {
        let config = WhatsAppConfig {
            enabled: true,
            bridge_url: "ws://localhost:3001".to_string(),
            allow_from: vec![],
        };
        let bus = MessageBus::new();
        let channel = WhatsAppChannel::new(config, bus);
        assert!(channel.is_enabled());
    }
}
