//! DingTalk (钉钉) channel implementation with embedded Python bridge
//!
//! This channel embeds a Python bridge that uses the official
//! dingtalk-stream SDK for Stream mode communication.

use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::DingTalkConfig;
use crate::error::{ChannelError, Result};
use crate::messages::InboundMessage;
use crate::python_env::PythonEnv;
use async_trait::async_trait;
use futures_util::{SinkExt, stream::StreamExt};
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Child as TokioChild;
use tokio::process::Command as TokioCommand;
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{error, info, warn};

/// Python bridge code embedded in Rust
const PYTHON_BRIDGE_CODE: &str = r#"
import asyncio
import json
import sys
import os
import ssl
import websockets
import requests
from dingtalk_stream import AckMessage
import dingtalk_stream

# Use certifi for SSL certificates to fix macOS SSL verification issues
import certifi
import os

# Set SSL certificate file environment variable
os.environ['SSL_CERT_FILE'] = certifi.where()
os.environ['REQUESTS_CA_BUNDLE'] = certifi.where()

# Global variables for communication
websocket_ref = None
# Store sessionWebhook URLs for each conversation
session_webhooks = {}
# Store the event loop for the WebSocket server
ws_event_loop = None

class MofaclawDingTalkHandler(dingtalk_stream.ChatbotHandler):
    def __init__(self, logger=None):
        super(dingtalk_stream.ChatbotHandler, self).__init__()
        self.logger = logger or self._default_logger()

    def _default_logger(self):
        import logging
        logger = logging.getLogger(__name__)
        if not logger.handlers:
            handler = logging.StreamHandler()
            handler.setFormatter(logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s'))
            logger.addHandler(handler)
            logger.setLevel(logging.INFO)
        return logger

    async def process(self, callback):
        global websocket_ref, session_webhooks, ws_event_loop
        try:
            # Log the full callback data for debugging
            self.logger.info(f"Received callback data: {callback.data}")

            incoming_message = dingtalk_stream.ChatbotMessage.from_dict(callback.data)

            # Extract message data - use getattr with safe defaults
            sender_id = getattr(incoming_message, 'sender_id', None) or getattr(incoming_message, 'staff_id', None) or ""
            content = ""
            if hasattr(incoming_message, 'text') and incoming_message.text:
                content = getattr(incoming_message.text, 'content', '')
            elif hasattr(incoming_message, 'content'):
                content = str(incoming_message.content) if incoming_message.content else ""

            conversation_id = getattr(incoming_message, 'conversation_id', '')

            # Try to get sessionWebhook with multiple possible attribute names
            session_webhook = None
            for attr_name in ['sessionWebhook', 'session_webhook', 'SessionWebhook']:
                if hasattr(incoming_message, attr_name):
                    session_webhook = getattr(incoming_message, attr_name)
                    self.logger.info(f"Found sessionWebhook via attribute '{attr_name}': {session_webhook}")
                    break

            # Also check callback.data directly
            if not session_webhook and isinstance(callback.data, dict):
                for key in ['sessionWebhook', 'session_webhook', 'SessionWebhook']:
                    if key in callback.data:
                        session_webhook = callback.data.get(key)
                        self.logger.info(f"Found sessionWebhook in callback.data via key '{key}': {session_webhook}")
                        break

            # Store sessionWebhook if found
            if session_webhook and conversation_id:
                session_webhooks[conversation_id] = session_webhook
                self.logger.info(f"Stored sessionWebhook for conversation: {conversation_id}")
            else:
                self.logger.warning(f"sessionWebhook not found! conversation_id: {conversation_id}")

            # Log received message
            self.logger.info(f"Received DingTalk message from {sender_id}: {content[:100]}")

            message_data = {
                "type": "message",
                "sender": sender_id,
                "content": content,
                "conversationId": conversation_id,
                "conversationType": getattr(incoming_message, 'conversation_type', '1'),
                "staffId": getattr(incoming_message, 'staff_id', ''),
                "msgId": getattr(incoming_message, 'message_id', ''),
            }

            # Send to Rust via WebSocket if connected
            # Use run_coroutine_threadsafe because the callback runs in a different event loop
            if websocket_ref:
                try:
                    import asyncio
                    if ws_event_loop and ws_event_loop.is_running():
                        # Schedule the send coroutine on the WebSocket event loop
                        asyncio.run_coroutine_threadsafe(
                            websocket_ref.send(json.dumps(message_data)),
                            ws_event_loop
                        )
                        self.logger.info(f"Forwarded message to Rust: {content[:50]}")
                    else:
                        self.logger.warning("WebSocket event loop not available, message not forwarded")
                except Exception as e:
                    self.logger.error(f"Failed to send message via WebSocket: {e}")
            else:
                self.logger.warning("WebSocket not connected, message not forwarded")

            return AckMessage.STATUS_OK, "OK"

        except Exception as e:
            self.logger.error(f"Error processing message: {e}", exc_info=True)
            # Return simple OK status
            return "OK", "OK"

def send_message_via_session_webhook(conversation_id, content):
    """Send message using the sessionWebhook URL from the incoming message"""
    global session_webhooks

    if conversation_id not in session_webhooks:
        raise Exception(f"No sessionWebhook found for conversation: {conversation_id}")

    webhook_url = session_webhooks[conversation_id]

    # Prepare the message payload
    data = {
        "msgtype": "text",
        "text": {
            "content": content
        }
    }

    headers = {"Content-Type": "application/json"}
    response = requests.post(webhook_url, json=data, headers=headers, timeout=10)
    return response

async def handle_websocket(dingtalk_client=None, client_id=None, client_secret=None):
    """Handle incoming WebSocket connection from Rust"""
    global websocket_ref, ws_event_loop
    import logging
    logger = logging.getLogger(__name__)
    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
    )
    logger.info("Rust mofaclaw connected")

    # Store the event loop for cross-thread communication
    ws_event_loop = asyncio.get_running_loop()
    logger.info(f"WebSocket event loop: {ws_event_loop}")

    # Keep connection alive and handle any commands from Rust
    try:
        # Send initial greeting
        await websocket_ref.send(json.dumps({"type": "connected", "message": "WebSocket connected"}))

        async for message in websocket_ref:
            try:
                data = json.loads(message)
                msg_type = data.get("type", "")

                # Handle send command from Rust
                if msg_type == "send":
                    conversation_id = data.get("chatId", "")
                    content = data.get("content", "")

                    logger.info(f"Sending message to {conversation_id}: {content[:50]}")

                    result = {"type": "send_result", "success": True, "chatId": conversation_id}

                    try:
                        # Send message using sessionWebhook
                        response = send_message_via_session_webhook(conversation_id, content)
                        response.raise_for_status()
                        logger.info(f"Message sent successfully via sessionWebhook")
                        result_data = response.json() if response.content else {}
                        if result_data.get("errcode") != 0:
                            raise Exception(f"API returned error: {result_data}")

                    except Exception as send_error:
                        logger.error(f"Failed to send message: {send_error}")
                        result = {"type": "send_result", "success": False, "error": str(send_error)}

                    await websocket_ref.send(json.dumps(result))

                # Echo back for connection test
                elif msg_type == "ping":
                    await websocket_ref.send(json.dumps({"type": "pong"}))

            except json.JSONDecodeError as e:
                logger.warning(f"Invalid JSON received: {message}, error: {e}")
            except Exception as e:
                logger.error(f"Error handling message: {e}")
    except websockets.exceptions.ConnectionClosed:
        logger.info("Rust mofaclaw disconnected")
    except Exception as e:
        logger.error(f"WebSocket error: {e}")
    finally:
        logger.info("WebSocket handler ended")
        websocket_ref = None
        ws_event_loop = None

def run_dingtalk_client(client_id, client_secret, port=3003):
    """Run the DingTalk Stream client - note: start_forever() creates its own event loop"""
    import logging
    import threading
    logger = logging.getLogger(__name__)
    logging.basicConfig(
        level=logging.INFO,
        format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
    )

    # Create a container for the DingTalk client that can be shared
    client_container = {'client': None}

    def run_client():
        # Start WebSocket server for Rust to connect
        # Note: We need to run the ws_server in a separate thread since start_forever() blocks
        import asyncio

        async def ws_server_task():
            global websocket_ref
            ws_port = port
            # Create handler that can access the client and credentials
            async def handler_with_client(websocket):
                global websocket_ref
                websocket_ref = websocket
                await handle_websocket(dingtalk_client=client_container['client'], client_id=client_id, client_secret=client_secret)

            ws_server = await websockets.serve(handler_with_client, "127.0.0.1", ws_port)
            logger.info(f"WebSocket server listening on 127.0.0.1:{ws_port}")
            # Keep the server running
            await ws_server.wait_closed()

        # Run WebSocket server in a daemon thread with its own event loop
        def run_ws_in_thread():
            asyncio.run(ws_server_task())

        ws_thread = threading.Thread(target=run_ws_in_thread, daemon=True)
        ws_thread.start()

        # Create DingTalk Stream client
        credential = dingtalk_stream.Credential(client_id, client_secret)
        client = dingtalk_stream.DingTalkStreamClient(credential)

        # Store client in container for WebSocket handler to access
        client_container['client'] = client

        # Register handler for chatbot messages
        client.register_callback_handler(
            dingtalk_stream.chatbot.ChatbotMessage.TOPIC,
            MofaclawDingTalkHandler()
        )

        logger.info("Starting DingTalk Stream client...")

        # Start client (this blocks and creates its own event loop)
        client.start_forever()

    # Run in a new thread
    thread = threading.Thread(target=run_client, daemon=True)
    thread.start()

    # Keep main thread alive
    try:
        thread.join()
    except KeyboardInterrupt:
        print("\\nDingTalk bridge stopped")

def main():
    if len(sys.argv) < 3:
        print("Usage: python bridge.py <client_id> <client_secret> [port]", file=sys.stderr)
        sys.exit(1)

    client_id = sys.argv[1]
    client_secret = sys.argv[2]
    port = int(sys.argv[3]) if len(sys.argv) > 3 else 3003

    try:
        run_dingtalk_client(client_id, client_secret, port)
    except KeyboardInterrupt:
        print("\\nDingTalk bridge stopped")
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#;

/// DingTalk channel that manages an embedded Python bridge
pub struct DingTalkChannel {
    config: DingTalkConfig,
    bus: MessageBus,
    connected: Arc<RwLock<bool>>,
    running: Arc<RwLock<bool>>,
    bridge_port: u16,
    /// Channel sender for sending outbound messages to the WebSocket task
    outbound_tx: Arc<RwLock<Option<mpsc::Sender<String>>>>,
    /// Handle to the Python subprocess for proper shutdown
    python_child: Arc<Mutex<Option<TokioChild>>>,
}

impl DingTalkChannel {
    /// Create a new DingTalk channel
    pub fn new(config: DingTalkConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            connected: Arc::new(RwLock::new(false)),
            running: Arc::new(RwLock::new(false)),
            bridge_port: 3003, // Default port for Python bridge WebSocket
            outbound_tx: Arc::new(RwLock::new(None)),
            python_child: Arc::new(Mutex::new(None)),
        }
    }

    /// Check if connected to the bridge
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Start the embedded Python bridge
    async fn start_python_bridge(&self) -> Result<()> {
        let client_id = self.config.client_id.clone();
        let client_secret = self.config.client_secret.clone();
        let bridge_port = self.bridge_port;

        info!("Setting up DingTalk Python bridge...");

        // Check Python environment and install dependencies
        info!("Checking Python environment...");
        let python_env = PythonEnv::find().await?;
        python_env.verify_version()?;

        // DingTalk required packages
        let required_packages = ["dingtalk_stream", "websockets", "certifi"];
        python_env.ensure_packages(&required_packages).await?;

        let python_cmd_str = python_env.command().to_string();
        info!(
            "Using Python: {} ({})",
            python_cmd_str,
            python_env.version_string()
        );

        // Create Python script file in .mofaclaw directory
        let mofaclaw_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".mofaclaw");

        fs::create_dir_all(&mofaclaw_dir).map_err(|e| {
            ChannelError::SendFailed(format!("Failed to create .mofaclaw directory: {}", e))
        })?;

        let python_path = mofaclaw_dir.join("dingtalk_bridge.py");

        fs::write(&python_path, PYTHON_BRIDGE_CODE)
            .map_err(|e| ChannelError::SendFailed(format!("Failed to write Python code: {}", e)))?;

        info!("Python script created at: {}", python_path.display());

        let running_clone = Arc::clone(&self.running);
        let connected_stdout_clone = Arc::clone(&self.connected);
        let connected_main_clone = Arc::clone(&self.connected);
        let child_handle_clone = Arc::clone(&self.python_child);

        info!("Starting Python subprocess...");
        info!(
            "  Command: {} {} <client_id> <client_secret> {}",
            python_cmd_str,
            python_path.display(),
            bridge_port
        );

        // Spawn Python process
        let child = match TokioCommand::new(&python_cmd_str)
            .arg(&python_path)
            .arg(&client_id)
            .arg(&client_secret)
            .arg(bridge_port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => {
                info!("Python subprocess started with PID: {:?}", c.id());
                c
            }
            Err(e) => {
                let msg = format!("Failed to start Python bridge: {}", e);
                error!("{}", msg);
                error!("Possible causes:");
                error!("  1. Python is not installed or not in PATH");
                error!("  2. Required Python packages are missing");
                error!("  3. Check Python with: python3 --version");
                return Err(ChannelError::ConnectionFailed(msg).into());
            }
        };

        // Store child handle
        *child_handle_clone.lock().await = Some(child);

        // Now spawn a task to monitor the process
        tokio::spawn(async move {
            let mut child_guard = child_handle_clone.lock().await;
            let child = match child_guard.as_mut() {
                Some(c) => c,
                None => return,
            };

            // Monitor Python process output
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            if let Some(stdout) = stdout {
                let conn = Arc::clone(&connected_stdout_clone);
                tokio::spawn(async move {
                    use tokio::io::AsyncBufReadExt;
                    let mut reader = tokio::io::BufReader::new(stdout);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.is_ok() {
                        if !line.is_empty() {
                            let trimmed = line.trim();
                            info!("[Python] {}", trimmed);
                            if trimmed.contains("Starting DingTalk Stream client") {
                                info!("DingTalk Stream connection established!");
                                *conn.write().await = true;
                            }
                        }
                        line.clear();
                    }
                });
            }

            if let Some(stderr) = stderr {
                tokio::spawn(async move {
                    use tokio::io::AsyncBufReadExt;
                    let mut reader = tokio::io::BufReader::new(stderr);
                    let mut line = String::new();
                    while reader.read_line(&mut line).await.is_ok() {
                        if !line.is_empty() {
                            let trimmed = line.trim();
                            warn!("[Python-stderr] {}", trimmed);
                        }
                        line.clear();
                    }
                });
            }

            // Wait for process exit or shutdown signal
            loop {
                tokio::select! {
                    result = child.wait() => {
                        match result {
                            Ok(s) => {
                                if s.success() {
                                    info!("Python bridge exited normally");
                                } else {
                                    error!("Python bridge exited with error code: {:?}", s.code());
                                    error!("Check the [Python-stderr] logs above for details");
                                }
                            }
                            Err(e) => error!("Failed to wait for Python process: {}", e),
                        }
                        *connected_main_clone.write().await = false;
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {
                        if !*running_clone.read().await {
                            info!("Shutting down Python bridge...");
                            if let Err(e) = child.start_kill() {
                                error!("Failed to kill Python process: {}", e);
                            }
                            // Give it a moment to exit
                            let _ = tokio::time::timeout(
                                Duration::from_secs(5),
                                child.wait()
                            ).await;
                            break;
                        }
                    }
                }
            }
        });

        // Give Python time to start
        tokio::time::sleep(Duration::from_secs(2)).await;

        Ok(())
    }
}

#[async_trait]
impl Channel for DingTalkChannel {
    fn name(&self) -> &str {
        "dingtalk"
    }

    async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            info!("DingTalk channel disabled");
            return Ok(());
        }

        if self.config.client_id.is_empty() || self.config.client_secret.is_empty() {
            return Err(ChannelError::NotConfigured("DingTalk".to_string()).into());
        }

        *self.running.write().await = true;

        info!("Starting DingTalk channel (Stream mode with embedded Python bridge)");

        // Start Python bridge
        self.start_python_bridge().await?;

        let running = Arc::clone(&self.running);
        let connected = Arc::clone(&self.connected);
        let bus = self.bus.clone();
        let bridge_port = self.bridge_port;
        let outbound_tx = Arc::clone(&self.outbound_tx);

        // Wait for bridge to be ready
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Subscribe to outbound messages from the message bus
        let mut outbound_rx = bus.subscribe_outbound();
        let running_clone = Arc::clone(&running);
        let outbound_tx_for_spawn = Arc::clone(&outbound_tx);

        tokio::spawn(async move {
            while *running_clone.read().await {
                match outbound_rx.recv().await {
                    Ok(msg) if msg.channel == "dingtalk" => {
                        // Get the sender from the stored reference
                        let tx_guard = outbound_tx_for_spawn.read().await;
                        if let Some(tx) = tx_guard.as_ref() {
                            // Debug: print the chat_id being used
                            info!(
                                "📤 Sending outbound message to DingTalk - chat_id: {}, content: {}",
                                msg.chat_id,
                                msg.content.chars().take(50).collect::<String>()
                            );
                            // Create send message for Python bridge
                            let send_msg = json!({
                                "type": "send",
                                "chatId": msg.chat_id,
                                "content": msg.content,
                            });
                            if let Err(e) = tx.send(send_msg.to_string()).await {
                                error!("Failed to queue outbound message: {}", e);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!("Outbound message receive error: {}", e);
                        break;
                    }
                }
            }
        });

        let bridge_url = format!("ws://127.0.0.1:{}", bridge_port);

        // Connect to Python bridge via WebSocket (run in this task)
        while *running.read().await {
            match tokio_tungstenite::connect_async(&bridge_url).await {
                Ok((ws_stream, _)) => {
                    *connected.write().await = true;
                    info!("Connected to DingTalk Python bridge");

                    let (outbound_msg_tx, mut outbound_msg_rx) = mpsc::channel::<String>(100);
                    *outbound_tx.write().await = Some(outbound_msg_tx);

                    // Use the WebSocket stream directly without splitting
                    let mut ws_stream = ws_stream;

                    // Listen for messages from Python bridge
                    while *running.read().await {
                        tokio::select! {
                            // Receive inbound messages from Python bridge
                            msg = ws_stream.next() => {
                                match msg {
                                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                        // Parse message from Python bridge
                                        if let Ok(data) = serde_json::from_str::<Value>(&text) {
                                            let msg_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

                                            if msg_type == "message" {
                                                let sender = data.get("sender").and_then(|v| v.as_str()).unwrap_or("");
                                                let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                                                let conversation_id = data.get("conversationId").and_then(|v| v.as_str()).unwrap_or("");
                                                let conversation_type = data.get("conversationType").and_then(|v| v.as_str()).unwrap_or("1");
                                                let staff_id = data.get("staffId").and_then(|v| v.as_str()).unwrap_or("");
                                                let msg_id = data.get("msgId").and_then(|v| v.as_str()).unwrap_or("");

                                                // Print received DingTalk message
                                                info!("📨 收到钉钉消息:");
                                                info!("   发送者: {}", sender);
                                                info!("   内容: {}", content);
                                                info!("   会话ID: {}", conversation_id);
                                                info!("   会话类型: {}", conversation_type);
                                                info!("   员工ID: {}", staff_id);
                                                info!("   消息ID: {}", msg_id);

                                                let mut metadata = HashMap::new();
                                                metadata.insert("conversation_type".to_string(), Value::String(conversation_type.to_string()));
                                                metadata.insert("staff_id".to_string(), Value::String(staff_id.to_string()));
                                                metadata.insert("message_id".to_string(), Value::String(msg_id.to_string()));

                                                let msg = InboundMessage::with_metadata(
                                                    "dingtalk",
                                                    sender,          // sender_id (发送者ID)
                                                    conversation_id, // chat_id (会话ID，用于sessionWebhook查找)
                                                    content,
                                                    metadata,
                                                );

                                                if let Err(e) = bus.publish_inbound(msg).await {
                                                    error!("Failed to publish DingTalk message: {}", e);
                                                }
                                            } else if msg_type == "send_result" {
                                                let success = data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                                                if success {
                                                    info!("Message sent to DingTalk");
                                                } else {
                                                    let error_msg = data.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
                                                    warn!("Failed to send DingTalk message: {}", error_msg);
                                                }
                                            }
                                        }
                                    }
                                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                                        info!("DingTalk bridge connection closed");
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
                                    Some(Ok(_)) => {}
                                }
                            }
                            // Receive outbound messages to send to Python bridge
                            Some(msg) = outbound_msg_rx.recv() => {
                                if let Err(e) = ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(msg.into())).await {
                                    error!("Failed to send outbound message: {}", e);
                                    break;
                                }
                            }
                        }
                    }

                    *connected.write().await = false;
                    *outbound_tx.write().await = None;
                }
                Err(e) => {
                    *connected.write().await = false;
                    warn!("DingTalk bridge connection error: {}", e);

                    if *running.read().await {
                        info!("Reconnecting to bridge in 5 seconds...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }

        info!("DingTalk channel stopped");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping DingTalk channel...");

        // Set running flag to false - this will signal the monitoring task to kill the process
        *self.running.write().await = false;
        *self.connected.write().await = false;

        // Also try to directly kill the Python process if we have a handle
        let mut child_guard = self.python_child.lock().await;
        if let Some(mut child) = child_guard.take() {
            info!(
                "Terminating Python bridge process (PID: {:?})...",
                child.id()
            );
            match child.start_kill() {
                Ok(_) => {
                    info!("Sent termination signal to Python process");
                    // Wait for process to exit with timeout
                    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                        Ok(Ok(status)) => {
                            info!("Python bridge exited with status: {:?}", status);
                        }
                        Ok(Err(e)) => {
                            warn!("Error waiting for Python bridge exit: {}", e);
                        }
                        Err(_) => {
                            warn!(
                                "Python bridge did not exit within 5 seconds (may have been force-killed)"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to send termination signal: {}", e);
                }
            }
        }

        info!("DingTalk channel stopped");
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

// Sending is now handled through the message bus subscription in start()

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dingtalk_channel_name() {
        let config = DingTalkConfig::default();
        let bus = MessageBus::new();
        let channel = DingTalkChannel::new(config, bus);
        assert_eq!(channel.name(), "dingtalk");
    }

    #[test]
    fn test_dingtalk_is_enabled() {
        let mut config = DingTalkConfig::default();
        config.enabled = true;
        let bus = MessageBus::new();
        let channel = DingTalkChannel::new(config, bus);
        assert!(channel.is_enabled());
    }
}
