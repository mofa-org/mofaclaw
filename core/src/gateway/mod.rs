//! HTTP REST API Gateway for mofaclaw
//!
//! Exposes an OpenAI-compatible API on port 18790 (default) so any tool that
//! speaks the OpenAI chat-completions protocol can talk to the mofaclaw agent.
//!
//! # Endpoints
//!
//! | Method | Path                      | Description                     |
//! |--------|---------------------------|---------------------------------|
//! | GET    | `/`                       | Welcome / info                  |
//! | GET    | `/health`                 | Health check (returns 200 OK)   |
//! | GET    | `/v1/models`              | List available models           |
//! | POST   | `/v1/chat/completions`    | Chat (streaming and non-stream) |
//!
//! # Quick start
//! ```bash
//! curl http://localhost:18790/v1/chat/completions \
//!   -H "Content-Type: application/json" \
//!   -d '{"model":"mofaclaw","messages":[{"role":"user","content":"Hello!"}]}'
//! ```

pub mod types;

use std::{
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::stream;
use tokio::sync::{Mutex, oneshot};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::{
    MessageBus,
    messages::InboundMessage,
};

use types::{
    ApiError, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ModelList,
};

// ─────────────────────────── Pending-request map ──────────────────────────

/// In-flight HTTP requests waiting for an agent response.
///
/// Key: `chat_id` used for the InboundMessage (unique per request unless the
///      caller supplies a `conversation_id`).
/// Value: oneshot sender – fires when the agent publishes an outbound reply.
type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

// ─────────────────────────── Shared state ─────────────────────────────────

#[derive(Clone)]
pub struct GatewayState {
    bus: MessageBus,
    pending: PendingMap,
    model: String,
}

// ─────────────────────────── Server ───────────────────────────────────────

/// The HTTP gateway server.
pub struct GatewayServer {
    bus: MessageBus,
    host: String,
    port: u16,
    model: String,
}

impl GatewayServer {
    pub fn new(bus: MessageBus, host: impl Into<String>, port: u16, model: impl Into<String>) -> Self {
        Self {
            bus,
            host: host.into(),
            port,
            model: model.into(),
        }
    }

    /// Start the HTTP server.  Runs until the process is stopped.
    pub async fn run(self) -> anyhow::Result<()> {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));

        // Spawn the outbound-dispatch background task.
        // It subscribes to the message bus and resolves pending HTTP requests.
        let pending_for_dispatch = pending.clone();
        let mut outbound_rx = self.bus.subscribe_outbound();
        tokio::spawn(async move {
            loop {
                match outbound_rx.recv().await {
                    Ok(msg) => {
                        if msg.channel != "api" {
                            continue;
                        }
                        debug!("API gateway received outbound for chat_id={}", msg.chat_id);
                        let mut map = pending_for_dispatch.lock().await;
                        if let Some(tx) = map.remove(&msg.chat_id) {
                            let _ = tx.send(msg.content);
                        } else {
                            warn!("No pending request for chat_id={}", msg.chat_id);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("API gateway outbound receiver lagged by {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("API gateway outbound bus closed – stopping dispatch task");
                        break;
                    }
                }
            }
        });

        let state = GatewayState {
            bus: self.bus,
            pending,
            model: self.model,
        };

        let app = Router::new()
            .route("/", get(handle_root))
            .route("/health", get(handle_health))
            .route("/v1/models", get(handle_models))
            .route("/v1/chat/completions", post(handle_chat_completions))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr: SocketAddr = format!("{}:{}", self.host, self.port).parse()?;
        info!("HTTP API gateway listening on http://{}", addr);
        println!("HTTP API: http://{}  (OpenAI-compatible)", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        Ok(())
    }
}

// ─────────────────────────── Handlers ─────────────────────────────────────

async fn handle_root() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "mofaclaw",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Mofaclaw AI assistant – OpenAI-compatible HTTP API",
        "endpoints": {
            "GET  /health":                 "Health check",
            "GET  /v1/models":              "List models",
            "POST /v1/chat/completions":    "Chat (stream=false|true)"
        },
        "docs": "https://github.com/mofa-org/mofaclaw"
    }))
}

async fn handle_health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

async fn handle_models(State(state): State<GatewayState>) -> impl IntoResponse {
    Json(ModelList::mofaclaw_models(&state.model))
}

async fn handle_chat_completions(
    State(state): State<GatewayState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    // Determine session / chat_id
    let chat_id = req
        .conversation_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Extract the latest user message (the session carries prior history)
    let user_content = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    if user_content.is_empty() {
        let status = StatusCode::BAD_REQUEST;
        let body = Json(ApiError::internal("No user message found in request"));
        return (status, body).into_response();
    }

    // Register pending request BEFORE publishing so the reply isn't missed
    let (tx, rx) = oneshot::channel::<String>();
    {
        let mut map = state.pending.lock().await;
        map.insert(chat_id.clone(), tx);
    }

    // Publish to the agent via the message bus
    let inbound = InboundMessage::new("api", "api-user", &chat_id, &user_content);
    if let Err(e) = state.bus.publish_inbound(inbound).await {
        error!("Failed to publish inbound message: {}", e);
        let mut map = state.pending.lock().await;
        map.remove(&chat_id);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::internal(format!("Bus error: {e}"))),
        )
            .into_response();
    }

    // Wait for the agent response (2-minute hard timeout)
    let response_text = match tokio::time::timeout(Duration::from_secs(120), rx).await {
        Ok(Ok(text)) => text,
        Ok(Err(_)) => {
            // sender dropped without sending
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::internal("Agent response channel closed unexpectedly")),
            )
                .into_response();
        }
        Err(_) => {
            // Timed out – clean up the pending entry
            let mut map = state.pending.lock().await;
            map.remove(&chat_id);
            return (StatusCode::GATEWAY_TIMEOUT, Json(ApiError::timeout())).into_response();
        }
    };

    let completion_id = format!("chatcmpl-{}", Uuid::new_v4().simple());

    if req.stream {
        // ── Streaming path: SSE with OpenAI-format chunks ──────────────────
        // The agent processes in one shot, so we stream the response
        // word-by-word to give clients the streaming experience.
        let model = state.model.clone();
        let id_for_stream = completion_id.clone();

        let words: Vec<String> = response_text
            .split_inclusive(' ')
            .map(|s| s.to_string())
            .collect();

        let event_stream = {
            let model_role = model.clone();
            let id_role = id_for_stream.clone();
            let model_words = model.clone();
            let id_words = id_for_stream.clone();
            let model_stop = model.clone();
            let id_stop = id_for_stream.clone();

            // Build a Vec of SSE events: role → words → stop
            let mut events: Vec<Result<Event, Infallible>> = Vec::new();

            // Role event
            let role_chunk = ChatCompletionChunk::role_chunk(&id_role, &model_role);
            if let Ok(data) = serde_json::to_string(&role_chunk) {
                events.push(Ok(Event::default().data(data)));
            }

            // Word events
            for word in words {
                let chunk = ChatCompletionChunk::content_chunk(&id_words, &model_words, word);
                if let Ok(data) = serde_json::to_string(&chunk) {
                    events.push(Ok(Event::default().data(data)));
                }
            }

            // Stop event
            let stop_chunk = ChatCompletionChunk::stop_chunk(&id_stop, &model_stop);
            if let Ok(data) = serde_json::to_string(&stop_chunk) {
                events.push(Ok(Event::default().data(data)));
            }

            // Terminator
            events.push(Ok(Event::default().data("[DONE]")));

            stream::iter(events)
        };

        Sse::new(event_stream)
            .keep_alive(KeepAlive::default())
            .into_response()
    } else {
        // ── Non-streaming path ──────────────────────────────────────────────
        Json(ChatCompletionResponse::new(
            completion_id,
            &state.model,
            response_text,
        ))
        .into_response()
    }
}
