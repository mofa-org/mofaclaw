//! Feishu (Lark/飞书) channel implementation with embedded Python bridge
//!
//! This channel embeds a Python bridge that uses the official
//! lark-oapi SDK for event subscription and message sending.

use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::FeishuConfig;
use crate::error::{ChannelError, Result};
use crate::messages::InboundMessage;
use crate::python_env::PythonEnv;
use crate::rbac::{RbacManager, Role};
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
import collections
import json
import sys
import os
import threading
import websockets
import lark_oapi as lark
from lark_oapi.api.im.v1 import CreateMessageRequest, CreateMessageRequestBody
import logging

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)

# ── Global state ──────────────────────────────────────────────────────────────
# WebSocket connection back to Rust (set when Rust connects to our server)
websocket_ref = None
# The asyncio event loop shared by ws_server_task and event_subscription_task
main_loop = None
# Feishu API client (for outbound messages, initialised on first Rust connection)
feishu_handler = None

# ── Message queue (for messages arriving before Rust connects) ────────────────
# maxlen=50 automatically discards the oldest entry when the deque is full.
inbound_queue: collections.deque = collections.deque(maxlen=50)
queue_lock = threading.Lock()


class BridgeConfig:
    """Access-control config set once at startup; never mutated afterwards."""
    __slots__ = ("dm_policy", "group_policy", "typing_indicator")

    def __init__(self, dm_policy="open", group_policy="open", typing_indicator=False):
        self.dm_policy = dm_policy
        self.group_policy = group_policy
        self.typing_indicator = typing_indicator


# Populated in run_feishu_client() before any event can arrive
bridge_config = BridgeConfig()


# ── Feishu API helper (outbound) ──────────────────────────────────────────────
class FeishuEventHandler:
    """Wraps the lark-oapi REST client for outbound message sending."""

    def __init__(self, app_id, app_secret, encrypt_key=None, verify_token=None):
        self.app_id = app_id
        self.app_secret = app_secret
        self.encrypt_key = encrypt_key
        self.verify_token = verify_token
        self.client = None

    def create_client(self):
        """Create lark-oapi client"""
        self.client = lark.Client.builder() \
            .app_id(self.app_id) \
            .app_secret(self.app_secret) \
            .log_level(lark.LogLevel.INFO) \
            .build()
        return self.client

    def send_message(self, receive_id_type, receive_id, content_type, content):
        """Send message via Feishu API"""
        try:
            if self.client is None:
                self.create_client()

            # Build message content based on type
            if content_type == "text":
                msg_content = json.dumps({"text": content})
            elif content_type == "post":
                msg_content = json.dumps({
                    "post": {
                        "zh_cn": {
                            "title": "消息",
                            "content": [[{"tag": "text", "text": content}]]
                        }
                    }
                })
            else:
                msg_content = json.dumps({"text": content})

            request = CreateMessageRequest.builder() \
                .receive_id_type(receive_id_type) \
                .request_body(CreateMessageRequestBody.builder()
                    .receive_id(receive_id)
                    .msg_type(content_type)
                    .content(msg_content)
                    .build()) \
                .build()

            response = self.client.im.v1.message.create(request)

            if not response.success():
                logger.error(f"Failed to send message: {response.code} {response.msg}")
                return False, f"{response.code}: {response.msg}"

            logger.info(f"Message sent successfully, message_id: {response.data.message_id}")
            return True, response.data.message_id

        except Exception as e:
            logger.error(f"Error sending message: {e}")
            return False, str(e)


# ── Inbound event handler (called from lark-oapi long-connection thread) ──────
def handle_message_event(data):
    """
    Called by lark-oapi's ws.Client for every im.message.receive_v1 event.
    `data` is a P2ImMessageReceiveV1 object.

    This function runs in a thread-pool thread (not the asyncio event loop),
    so WebSocket sends are scheduled via asyncio.run_coroutine_threadsafe().
    """
    global websocket_ref, main_loop, bridge_config

    try:
        event = data.event
        if event is None:
            return

        # ── Sender ────────────────────────────────────────────────────────────
        sender_id = ""
        sender_type = ""
        if event.sender:
            sid = event.sender.sender_id
            if sid:
                # Prefer user_id; fall back through open_id → union_id
                sender_id = (
                    sid.user_id
                    or sid.open_id
                    or getattr(sid, 'union_id', None)
                    or ""
                )
            sender_type = getattr(event.sender, 'sender_type', '') or ""

        # Drop bot messages (including replies from this bot) to prevent loops
        if sender_type == "bot":
            logger.debug("Ignoring bot message to prevent reply loop")
            return

        # ── Message fields ────────────────────────────────────────────────────
        msg = event.message
        if msg is None:
            return

        message_id  = getattr(msg, 'message_id', '') or ""
        chat_id     = getattr(msg, 'chat_id',    '') or ""
        chat_type   = getattr(msg, 'chat_type',  '') or ""
        msg_type    = getattr(msg, 'msg_type',   'text') or "text"
        content_str = getattr(msg, 'content',    '') or ""

        # ── Access control ────────────────────────────────────────────────────
        if chat_type == "p2p" and bridge_config.dm_policy == "restricted":
            logger.info(f"Blocked DM from {sender_id!r} (dm_policy=restricted)")
            return
        if chat_type == "group" and bridge_config.group_policy == "restricted":
            logger.info(f"Blocked group message from {sender_id!r} (group_policy=restricted)")
            return

        # ── Parse content JSON ────────────────────────────────────────────────
        content = ""
        if content_str:
            try:
                content_data = json.loads(content_str)
                if isinstance(content_data, dict):
                    content = content_data.get('text', content_str)
                else:
                    content = content_str
            except (json.JSONDecodeError, ValueError):
                content = content_str

        # Drop empty / whitespace-only messages
        if not content.strip():
            logger.debug(f"Ignoring empty message from {sender_id!r}")
            return

        logger.info(f"Received Feishu message from {sender_id!r}: {content[:100]}")

        message_data = {
            "type":        "message",
            "sender":      sender_id,
            "content":     content,
            "chatId":      chat_id,
            "messageType": msg_type,
            "messageId":   message_id,
            "chatType":    chat_type,
        }

        # ── Typing indicator ──────────────────────────────────────────────────
        # Feishu has no native typing API; send a brief acknowledgment text so
        # the user knows their message was received and is being processed.
        # Only fires when the Rust side is already connected (feishu_handler
        # is initialised in handle_websocket).
        if bridge_config.typing_indicator and feishu_handler is not None and chat_id:
            try:
                feishu_handler.send_message(
                    "chat_id", chat_id, "text",
                    "⏳ 正在处理中，请稍候... / Processing, please wait...",
                )
            except Exception as ti_err:
                logger.debug(f"Could not send typing indicator: {ti_err}")

        # ── Forward to Rust ───────────────────────────────────────────────────
        # Schedule the async send on the main event loop with a 5 s timeout so
        # a slow Rust side never blocks the lark-oapi receiver thread forever.
        if websocket_ref is not None and main_loop is not None and main_loop.is_running():
            future = asyncio.run_coroutine_threadsafe(
                websocket_ref.send(json.dumps(message_data)),
                main_loop,
            )
            try:
                future.result(timeout=5.0)
                logger.info(f"Forwarded message to Rust: {content[:50]!r}")
            except Exception as fwd_err:
                logger.error(f"Failed to forward message to Rust: {fwd_err}")
                # ── Error feedback: WebSocket forward failed ──────────────────
                if feishu_handler is not None and chat_id:
                    try:
                        feishu_handler.send_message(
                            "chat_id", chat_id, "text",
                            "❌ 消息转发失败，请稍后重试。\n"
                            "Message forwarding failed. Please try again later.",
                        )
                    except Exception:
                        pass
        else:
            # ── Message queuing: Rust not yet connected ───────────────────────
            # Keep the message so it can be replayed when Rust reconnects.
            # The deque's maxlen cap ensures we never queue more than 50 items.
            with queue_lock:
                inbound_queue.append(message_data)
                logger.info(
                    f"Rust not connected — queued message "
                    f"(queue {len(inbound_queue)}/{inbound_queue.maxlen})"
                )

    except Exception as e:
        logger.error(f"Error handling message event: {e}", exc_info=True)


async def handle_websocket(app_id, app_secret, encrypt_key, verify_token):
    """Handle incoming WebSocket connection from Rust"""
    global websocket_ref, feishu_handler

    logger.info("Rust mofaclaw connected")

    # Initialize Feishu handler
    feishu_handler = FeishuEventHandler(app_id, app_secret, encrypt_key, verify_token)
    feishu_handler.create_client()

    # ── Flush messages queued while Rust was disconnected ────────────────────
    with queue_lock:
        pending = list(inbound_queue)
        inbound_queue.clear()
    if pending:
        logger.info(f"Flushing {len(pending)} queued message(s) to Rust")
        for queued_msg in pending:
            try:
                await websocket_ref.send(json.dumps(queued_msg))
            except Exception as flush_err:
                logger.error(f"Failed to flush queued message: {flush_err}")
                break  # abort flush on error; remaining messages are lost

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
                    chat_id = data.get("chatId", "")
                    content = data.get("content", "")
                    receive_id_type = data.get("receiveIdType", "chat_id")
                    message_type = data.get("messageType", "text")

                    logger.info(f"Sending message to {chat_id}: {content[:50]}")

                    result = {"type": "send_result", "success": True, "chatId": chat_id}

                    try:
                        # Send message via Feishu API
                        success, result_data = feishu_handler.send_message(
                            receive_id_type,
                            chat_id,
                            message_type,
                            content
                        )

                        if success:
                            logger.info(f"Message sent successfully")
                            result["messageId"] = result_data
                        else:
                            result = {"type": "send_result", "success": False, "error": result_data, "chatId": chat_id}

                    except Exception as send_error:
                        logger.error(f"Failed to send message: {send_error}")
                        result = {"type": "send_result", "success": False, "error": str(send_error), "chatId": chat_id}

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

async def event_subscription_task(app_id, app_secret, encrypt_key, verify_token):
    """
    Establish and maintain a lark-oapi long-connection (persistent WebSocket to
    Feishu's event-subscription service) and dispatch im.message.receive_v1
    events to handle_message_event().

    lark-oapi's ws.Client.start() is blocking and handles reconnection
    internally, so we run it in a thread-pool executor.  If it ever returns
    or raises, we restart it after an exponential back-off.
    """
    logger.info("Starting Feishu event subscription (long-connection mode)...")

    # Build the lark-oapi event dispatcher bound to our handler
    event_handler = (
        lark.EventDispatcherHandler
        .builder(encrypt_key or "", verify_token or "")
        .register_p2_im_message_receive_v1(handle_message_event)
        .build()
    )

    # Create the WebSocket long-connection client once; it manages its own
    # internal reconnection logic.
    ws_client = lark.ws.Client(
        app_id,
        app_secret,
        event_handler=event_handler,
        log_level=lark.LogLevel.INFO,
    )

    loop = asyncio.get_event_loop()
    backoff = 1
    max_backoff = 60

    while True:
        try:
            logger.info("Connecting to Feishu event-subscription service...")
            # Run the blocking ws_client.start() in a thread-pool executor so
            # the asyncio event loop (and the WebSocket server task) stays live.
            await loop.run_in_executor(None, ws_client.start)
            # start() returned cleanly (rare) — reconnect immediately
            logger.warning("Feishu long connection ended unexpectedly, reconnecting...")
            backoff = 1
        except asyncio.CancelledError:
            logger.info("Event subscription task cancelled, stopping.")
            break
        except Exception as e:
            logger.error(f"Event subscription error: {e}", exc_info=True)
            logger.info(f"Reconnecting in {backoff}s...")
            await asyncio.sleep(backoff)
            backoff = min(backoff * 2, max_backoff)

def run_feishu_client(
    app_id, app_secret, encrypt_key, verify_token,
    dm_policy="open", group_policy="open", typing_indicator=False, port=3004
):
    """
    Run the Feishu bridge: WebSocket server (for Rust ↔ Python IPC) and
    lark-oapi long-connection event subscription run concurrently in the same
    asyncio event loop via asyncio.gather().
    """
    import threading

    # Apply config before any event can arrive
    bridge_config.dm_policy = dm_policy
    bridge_config.group_policy = group_policy
    bridge_config.typing_indicator = typing_indicator

    def run_client():
        async def ws_server_task():
            """Accept a single Rust connection and proxy messages."""
            global websocket_ref

            async def handler_with_client(websocket):
                global websocket_ref
                websocket_ref = websocket
                await handle_websocket(app_id, app_secret, encrypt_key, verify_token)

            ws_server = await websockets.serve(handler_with_client, "127.0.0.1", port)
            logger.info(f"WebSocket server listening on 127.0.0.1:{port}")
            await ws_server.wait_closed()

        async def main_tasks():
            """Run WebSocket server and event subscription concurrently."""
            # Set main_loop here — before either task starts — so
            # handle_message_event() can always find it, even if the first
            # Feishu event arrived unusually early.
            global main_loop
            main_loop = asyncio.get_event_loop()
            await asyncio.gather(
                ws_server_task(),
                event_subscription_task(app_id, app_secret, encrypt_key, verify_token),
            )

        asyncio.run(main_tasks())

    thread = threading.Thread(target=run_client, daemon=True)
    thread.start()

    try:
        thread.join()
    except KeyboardInterrupt:
        print("\\nFeishu bridge stopped")

def main():
    if len(sys.argv) < 3:
        print(
            "Usage: python bridge.py <app_id> <app_secret> "
            "[encrypt_key] [verify_token] [dm_policy] [group_policy] [typing_indicator] [port]",
            file=sys.stderr,
        )
        sys.exit(1)

    app_id           = sys.argv[1]
    app_secret       = sys.argv[2]
    encrypt_key      = sys.argv[3] if len(sys.argv) > 3 else ""
    verify_token     = sys.argv[4] if len(sys.argv) > 4 else ""
    dm_policy        = sys.argv[5] if len(sys.argv) > 5 else "open"
    group_policy     = sys.argv[6] if len(sys.argv) > 6 else "open"
    typing_indicator = sys.argv[7].lower() in ("true", "1") if len(sys.argv) > 7 else False
    port             = int(sys.argv[8]) if len(sys.argv) > 8 else 3004

    try:
        run_feishu_client(
            app_id, app_secret, encrypt_key, verify_token,
            dm_policy, group_policy, typing_indicator, port,
        )
    except KeyboardInterrupt:
        print("\\nFeishu bridge stopped")
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
"#;

/// Feishu channel that manages an embedded Python bridge
pub struct FeishuChannel {
    config: FeishuConfig,
    bus: MessageBus,
    connected: Arc<RwLock<bool>>,
    running: Arc<RwLock<bool>>,
    bridge_port: u16,
    /// Channel sender for sending outbound messages to the WebSocket task
    outbound_tx: Arc<RwLock<Option<mpsc::Sender<String>>>>,
    /// Handle to the Python subprocess for proper shutdown
    python_child: Arc<Mutex<Option<TokioChild>>>,
    /// Optional RBAC manager for role-based access control
    rbac: Option<Arc<RbacManager>>,
}

impl FeishuChannel {
    /// Create a new Feishu channel
    pub fn new(config: FeishuConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            connected: Arc::new(RwLock::new(false)),
            running: Arc::new(RwLock::new(false)),
            bridge_port: 3004, // Default port for Python bridge WebSocket
            outbound_tx: Arc::new(RwLock::new(None)),
            python_child: Arc::new(Mutex::new(None)),
            rbac: None,
        }
    }

    /// Attach an RBAC manager for role-based access control
    pub fn with_rbac(mut self, rbac: Arc<RbacManager>) -> Self {
        self.rbac = Some(rbac);
        self
    }

    /// Check if connected to the bridge
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Start the embedded Python bridge
    async fn start_python_bridge(&self) -> Result<()> {
        let app_id = self.config.app_id.clone();
        let app_secret = self.config.app_secret.clone();
        let encrypt_key = self.config.encrypt_key.clone();
        let verification_token = self.config.verification_token.clone();
        let bridge_port = self.bridge_port;

        info!("Setting up Feishu Python bridge...");

        // Check Python environment and install dependencies
        info!("Checking Python environment...");
        let python_env = PythonEnv::find().await?;
        python_env.verify_version()?;

        // Feishu required packages
        let required_packages = ["lark_oapi", "websockets", "cryptography"];
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

        let python_path = mofaclaw_dir.join("feishu_bridge.py");

        fs::write(&python_path, PYTHON_BRIDGE_CODE)
            .map_err(|e| ChannelError::SendFailed(format!("Failed to write Python code: {}", e)))?;

        info!("Python script created at: {}", python_path.display());

        let running_clone = Arc::clone(&self.running);
        let connected_stdout_clone = Arc::clone(&self.connected);
        let connected_main_clone = Arc::clone(&self.connected);
        let child_handle_clone = Arc::clone(&self.python_child);

        info!("Starting Python subprocess...");
        info!(
            "  Command: {} {} <app_id> <app_secret> [encrypt_key] [verify_token] {}",
            python_cmd_str,
            python_path.display(),
            bridge_port
        );

        let dm_policy = self.config.dm_policy.clone();
        let group_policy = self.config.group_policy.clone();
        let typing_indicator = self.config.typing_indicator;

        // Spawn Python process
        // Arg order must match main() in PYTHON_BRIDGE_CODE:
        //   app_id  app_secret  encrypt_key  verify_token  dm_policy  group_policy  typing_indicator  port
        let child = match TokioCommand::new(&python_cmd_str)
            .arg(&python_path)
            .arg(&app_id)
            .arg(&app_secret)
            .arg(&encrypt_key)
            .arg(&verification_token)
            .arg(&dm_policy)
            .arg(&group_policy)
            .arg(typing_indicator.to_string())
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
                            if trimmed.contains("WebSocket server listening") {
                                info!("Feishu Python bridge connection established!");
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
impl Channel for FeishuChannel {
    fn name(&self) -> &str {
        "feishu"
    }

    async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Feishu channel disabled");
            return Ok(());
        }

        if self.config.app_id.is_empty() || self.config.app_secret.is_empty() {
            return Err(ChannelError::NotConfigured("Feishu".to_string()).into());
        }

        *self.running.write().await = true;

        info!("Starting Feishu channel (with embedded Python bridge)");

        // Start Python bridge
        self.start_python_bridge().await?;

        let running = Arc::clone(&self.running);
        let connected = Arc::clone(&self.connected);
        let bus = self.bus.clone();
        let bridge_port = self.bridge_port;
        let outbound_tx = Arc::clone(&self.outbound_tx);
        let message_type = self.config.message_type.clone();
        let rbac = self.rbac.clone();

        // Wait for bridge to be ready
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Subscribe to outbound messages from the message bus
        let mut outbound_rx = bus.subscribe_outbound();
        let running_clone = Arc::clone(&running);
        let outbound_tx_for_spawn = Arc::clone(&outbound_tx);
        let message_type_spawn = message_type.clone();

        tokio::spawn(async move {
            while *running_clone.read().await {
                match outbound_rx.recv().await {
                    Ok(msg) if msg.channel == "feishu" => {
                        // Get the sender from the stored reference
                        let tx_guard = outbound_tx_for_spawn.read().await;
                        if let Some(tx) = tx_guard.as_ref() {
                            info!(
                                "📤 Sending outbound message to Feishu - chat_id: {}, content: {}",
                                msg.chat_id,
                                msg.content.chars().take(50).collect::<String>()
                            );
                            // Create send message for Python bridge
                            let send_msg = json!({
                                "type": "send",
                                "chatId": msg.chat_id,
                                "content": msg.content,
                                "receiveIdType": "chat_id",
                                "messageType": message_type_spawn,
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
                    info!("Connected to Feishu Python bridge");

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
                                                let chat_id = data.get("chatId").and_then(|v| v.as_str()).unwrap_or("");
                                                let message_type = data.get("messageType").and_then(|v| v.as_str()).unwrap_or("text");
                                                let message_id = data.get("messageId").and_then(|v| v.as_str()).unwrap_or("");
                                                let chat_type = data.get("chatType").and_then(|v| v.as_str()).unwrap_or("");

                                                // Print received Feishu message
                                                info!("📨 收到飞书消息:");
                                                info!("   发送者: {}", sender);
                                                info!("   内容: {}", content);
                                                info!("   会话ID: {}", chat_id);
                                                info!("   消息类型: {}", message_type);
                                                info!("   消息ID: {}", message_id);
                                                info!("   会话类型: {}", chat_type);

                                                let mut metadata = HashMap::new();
                                                metadata.insert("message_type".to_string(), Value::String(message_type.to_string()));
                                                metadata.insert("message_id".to_string(), Value::String(message_id.to_string()));
                                                metadata.insert("chat_type".to_string(), Value::String(chat_type.to_string()));

                                                // Attach RBAC role to metadata when available
                                                if let Some(ref rbac_mgr) = rbac {
                                                    let role: Role = rbac_mgr.get_role_from_feishu(sender, &[]);
                                                    metadata.insert("role".to_string(), Value::String(role.as_str().to_string()));
                                                }

                                                let msg = InboundMessage::with_metadata(
                                                    "feishu",
                                                    sender,  // sender_id
                                                    chat_id, // chat_id
                                                    content,
                                                    metadata,
                                                );

                                                if let Err(e) = bus.publish_inbound(msg).await {
                                                    error!("Failed to publish Feishu message: {}", e);
                                                }
                                            } else if msg_type == "send_result" {
                                                let success = data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
                                                if success {
                                                    info!("Message sent to Feishu");
                                                } else {
                                                    let error_msg = data.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error");
                                                    warn!("Failed to send Feishu message: {}", error_msg);
                                                }
                                            } else if msg_type == "connected" {
                                                info!("Feishu Python bridge connected");
                                            }
                                        }
                                    }
                                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                                        info!("Feishu bridge connection closed");
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
                    warn!("Feishu bridge connection error: {}", e);

                    if *running.read().await {
                        info!("Reconnecting to bridge in 5 seconds...");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }

        info!("Feishu channel stopped");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping Feishu channel...");

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

        info!("Feishu channel stopped");
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feishu_channel_name() {
        let config = FeishuConfig::default();
        let bus = MessageBus::new();
        let channel = FeishuChannel::new(config, bus);
        assert_eq!(channel.name(), "feishu");
    }

    #[test]
    fn test_feishu_is_enabled() {
        let mut config = FeishuConfig::default();
        config.enabled = true;
        let bus = MessageBus::new();
        let channel = FeishuChannel::new(config, bus);
        assert!(channel.is_enabled());
    }
}
