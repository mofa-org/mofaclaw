//! Agent loop: the core processing engine
//!
//! This module handles message processing and subagent spawning.
//! Uses mofa framework's LLMAgent for LLM interaction.

use crate::agent::ContextBuilder;
use crate::bus::MessageBus;
use crate::error::Result;
use crate::messages::{InboundMessage, OutboundMessage};
use crate::session::{SessionManager, SessionExt};
use crate::tools::{MessageTool, SpawnTool, ToolRegistry, ToolRegistryExecutor};
use crate::tools::filesystem::{ReadFileTool, WriteFileTool, EditFileTool, ListDirTool};
use crate::tools::shell::ExecTool;
use crate::tools::web::{WebSearchTool, WebFetchTool};
use crate::{Config};
use crate::agent::SubagentManager;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

// Import mofa framework components
use mofa_sdk::llm::{
    LLMAgent, LLMAgentBuilder, Tool, ToolExecutor,
    ChatMessage, task_orchestrator::{TaskOrchestrator, TaskOrigin},
};
use mofa_sdk::kernel::ToolRegistry as MofaToolRegistry;

/// Information about an active subagent
#[derive(Debug, Clone)]
pub struct ActiveSubagent {
    pub id: String,
    pub prompt: String,
    pub origin_channel: String,
    pub origin_chat_id: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

/// The agent loop is the core processing engine
///
/// It:
/// 1. Receives messages from the bus
/// 2. Builds context with history
/// 3. Calls the LLM (via mofa's LLMAgent)
/// 4. Executes tool calls (via mofa's LLMAgent)
/// 5. Sends responses back
/// 6. Can spawn subagents for parallel tasks (via mofa's TaskOrchestrator)
pub struct AgentLoop {
    /// Inner mofa LLMAgent for LLM interaction
    agent: Arc<LLMAgent>,
    /// LLM Provider
    provider: Arc<dyn mofa_sdk::llm::LLMProvider>,
    /// Tool registry (also used by agent)
    tools: Arc<RwLock<ToolRegistry>>,
    /// Message bus
    bus: MessageBus,
    /// Session manager
    sessions: Arc<SessionManager>,
    /// Context builder
    context: ContextBuilder,
    /// Running state
    running: Arc<RwLock<bool>>,
    /// Task orchestrator for subagent spawning
    task_orchestrator: Arc<TaskOrchestrator>,
    /// Max tool iterations
    max_iterations: usize,
}

impl AgentLoop {
    /// Create a new agent loop with a pre-built LLMAgent
    /// This is the recommended method for using MoFA framework directly
    pub async fn with_agent(
        config: &Config,
        agent: Arc<LLMAgent>,
        provider: Arc<dyn mofa_sdk::llm::LLMProvider>,
        bus: MessageBus,
        sessions: Arc<SessionManager>,
    ) -> Result<Self> {
        let workspace = config.workspace_path();
        let max_iterations = config.agents.defaults.max_tool_iterations;
        let brave_api_key = config.get_brave_api_key();

        let context = ContextBuilder::new(config);
        let tools = Arc::new(RwLock::new(ToolRegistry::new()));

        // Register default tools
        let mut tools_guard = tools.write().await;
        Self::register_default_tools(
            &mut tools_guard,
            &workspace,
            brave_api_key,
            bus.clone(),
        );
        drop(tools_guard);

        // Create mofa TaskOrchestrator for subagent spawning
        let task_orchestrator = Arc::new(TaskOrchestrator::with_defaults(provider.clone()));

        Ok(Self {
            agent,
            provider,
            tools,
            bus,
            sessions,
            context,
            running: Arc::new(RwLock::new(false)),
            task_orchestrator,
            max_iterations,
        })
    }

    /// Create a new agent loop with a pre-built LLMAgent and ToolRegistry
    ///
    /// This is the recommended method when tools need to be configured
    /// before creating the LLMAgent.
    pub async fn with_agent_and_tools(
        config: &Config,
        agent: Arc<LLMAgent>,
        provider: Arc<dyn mofa_sdk::llm::LLMProvider>,
        bus: MessageBus,
        sessions: Arc<SessionManager>,
        tools: Arc<RwLock<ToolRegistry>>,
    ) -> Result<Self> {
        let max_iterations = config.agents.defaults.max_tool_iterations;
        let context = ContextBuilder::new(config);

        // Create mofa TaskOrchestrator for subagent spawning
        let task_orchestrator = Arc::new(TaskOrchestrator::with_defaults(provider.clone()));

        Ok(Self {
            agent,
            provider,
            tools,
            bus,
            sessions,
            context,
            running: Arc::new(RwLock::new(false)),
            task_orchestrator,
            max_iterations,
        })
    }

    /// Register the default set of tools (without spawn tool)
    pub fn register_default_tools(
        registry: &mut ToolRegistry,
        workspace: &std::path::Path,
        brave_api_key: Option<String>,
        bus: MessageBus,
    ) {
        use std::sync::Arc;

        // File tools
        registry.register(ReadFileTool::new());
        registry.register(WriteFileTool::new());
        registry.register(EditFileTool::new());
        registry.register(ListDirTool::new());

        // Shell tool
        registry.register(ExecTool::new());

        // Web tools
        registry.register(WebSearchTool::new(brave_api_key));
        registry.register(WebFetchTool::new());

        // Message tool with bus callback
        let message_tool = MessageTool::with_callback(Arc::new(move |msg| {
            let bus = bus.clone();
            Box::pin(async move {
                bus.publish_outbound(msg).await
            })
        }));
        registry.register(message_tool);
    }

    /// Register spawn tool with subagent manager
    pub async fn register_spawn_tool(&self, subagent_manager: Arc<SubagentManager>) {
        let mut tools_guard = self.tools.write().await;
        let spawn_tool = SpawnTool::with_manager(subagent_manager);
        tools_guard.register(spawn_tool);
    }

    /// Run the agent loop, processing messages from the bus
    pub async fn run(&self) -> Result<()> {
        *self.running.write().await = true;
        info!("AgentLoop started");

        let mut rx = self.bus.subscribe_inbound();

        while *self.running.read().await {
            match tokio::time::timeout(tokio::time::Duration::from_secs(1), rx.recv()).await {
                Ok(Ok(msg)) => {
                    match self.process_message(msg).await {
                        Ok(Some(outbound_msg)) => {
                            // Publish the response to the message bus
                            if let Err(e) = self.bus.publish_outbound(outbound_msg).await {
                                tracing::warn!("Failed to publish outbound message: {}", e);
                            }
                        }
                        Ok(None) => {
                            // No response generated (this is okay)
                        }
                        Err(e) => {
                            tracing::warn!("Error processing message: {}", e);
                        }
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!("Message bus error: {}", e);
                }
                Err(_) => {
                    // Timeout, continue loop
                }
            }
        }

        info!("AgentLoop stopped");
        Ok(())
    }

    /// Stop the agent loop
    pub async fn stop(&self) {
        *self.running.write().await = false;
        info!("AgentLoop stopping");
    }

    /// Process a single message from the bus
    async fn process_message(&self, msg: InboundMessage) -> Result<Option<OutboundMessage>> {
        let (response_channel, response_chat_id) = if msg.channel == "system" {
            // Parse origin from chat_id (format: "channel:chat_id")
            if let Some(pos) = msg.chat_id.find(':') {
                (msg.chat_id[..pos].to_string(), msg.chat_id[pos + 1..].to_string())
            } else {
                ("cli".to_string(), msg.chat_id.clone())
            }
        } else {
            (msg.channel.clone(), msg.chat_id.clone())
        };

        let session_key = format!("{}:{}", response_channel, response_chat_id);
        let session = self.sessions.get_or_create(&session_key).await;

        // Build messages (convert mofa session messages to mofaclaw messages)
        let history = session.get_history_as_messages(50);
        let messages = self
            .context
            .build_messages(
                history,
                &msg.content,
                None,
                if msg.media.is_empty() { None } else { Some(&msg.media) },
            )
            .await?;

        // Run agent loop
        let final_content = self.run_agent_loop(messages).await?;

        // Save to session
        let mut session_updated = session.clone();
        let user_msg = if msg.channel == "system" {
            format!("[System: {}] {}", msg.sender_id, msg.content)
        } else {
            msg.content.clone()
        };
        session_updated.add_message("user", &user_msg);
        if let Some(ref content) = final_content {
            session_updated.add_message("assistant", content);
        }

        info!("Saving session: {} with {} messages", session_key, session_updated.len());
        if let Err(e) = self.sessions.save(&session_updated).await {
            error!("Failed to save session {}: {}", session_key, e);
            return Err(e.into());
        }
        info!("Session {} saved successfully", session_key);

        Ok(final_content.map(|content| OutboundMessage::new(&response_channel, &response_chat_id, content)))
    }

    /// Run the main agent loop using mofa framework's LLMClient with proper tool handling
    async fn run_agent_loop(&self, messages: Vec<crate::types::Message>) -> Result<Option<String>> {
        use mofa_sdk::llm::LLMClient;

        let client = LLMClient::new(self.provider.clone());

        // Get tool definitions and executor
        let tool_definitions: Vec<mofa_sdk::llm::Tool> = {
            let tools_guard = self.tools.read().await;
            MofaToolRegistry::list(tools_guard.inner()).iter().map(|t| {
                let params_value: serde_json::Value = if t.parameters_schema.is_string() {
                    serde_json::from_str(t.parameters_schema.as_str().unwrap_or("{}"))
                        .unwrap_or_else(|_| serde_json::json!({}))
                } else {
                    t.parameters_schema.clone()
                };
                mofa_sdk::llm::Tool::function(&t.name, &t.description, params_value)
            }).collect()
        };

        let tool_executor = Arc::new(ToolRegistryExecutor::new(self.tools.clone())) as Arc<dyn mofa_sdk::llm::ToolExecutor>;

        // Build chat messages from history
        let mut chat_messages = Vec::new();

        for msg in &messages {
            match msg.role {
                crate::types::MessageRole::System => {
                    if let Some(crate::types::MessageContent::Text(ref content)) = msg.content {
                        chat_messages.push(mofa_sdk::llm::ChatMessage::system(content.clone()));
                    }
                }
                crate::types::MessageRole::User => {
                    if let Some(crate::types::MessageContent::Text(ref content)) = msg.content {
                        chat_messages.push(mofa_sdk::llm::ChatMessage::user(content.clone()));
                    }
                }
                crate::types::MessageRole::Assistant => {
                    if let Some(crate::types::MessageContent::Text(ref content)) = msg.content {
                        chat_messages.push(mofa_sdk::llm::ChatMessage::assistant(content.clone()));
                    }
                }
                crate::types::MessageRole::Tool => {
                    // Skip - tool results will be added during iteration
                }
            }
        }

        // Run the agent loop with tool iteration
        let mut current_messages = chat_messages;
        let mut final_content = None;

        for _iteration in 0..self.max_iterations {
            // Create chat builder and send in one go
            let response = if tool_definitions.is_empty() {
                // No tools, just send messages
                client.chat()
                    .messages(current_messages.clone())
                    .send()
                    .await
                    .map_err(|e| crate::error::AgentError::ProviderError(e.to_string()))?
            } else {
                // With tools
                client.chat()
                    .messages(current_messages.clone())
                    .tools(tool_definitions.clone())
                    .with_tool_executor(tool_executor.clone())
                    .send_with_tools()
                    .await
                    .map_err(|e| crate::error::AgentError::ProviderError(e.to_string()))?
            };

            // Check if there are tool calls
            let tool_calls = response.tool_calls();
            if tool_calls.is_none() || tool_calls.map(|t| t.is_empty()).unwrap_or(true) {
                // No more tool calls, we're done
                final_content = response.content().map(|s| s.to_string());
                break;
            }

            // Add assistant response with tool calls
            let tool_calls_vec = tool_calls.unwrap();
            let response_content = response.content().map(|s| s.to_string()).unwrap_or_default();
            current_messages.push(mofa_sdk::llm::ChatMessage::assistant(response_content));

            // Execute each tool call and add results
            for tool_call in tool_calls_vec {
                // Execute tool
                let result = mofa_sdk::llm::ToolExecutor::execute(
                    &*tool_executor,
                    &tool_call.function.name,
                    &tool_call.function.arguments
                ).await
                    .map_err(|e| crate::error::AgentError::ProviderError(format!("Tool execution failed: {}", e)))?;

                // Add tool result message
                current_messages.push(mofa_sdk::llm::ChatMessage::tool_result(
                    &tool_call.id,
                    &result
                ));
            }
        }

        if final_content.is_none() {
            final_content = Some("I completed the tool calls but have no additional response.".to_string());
        }

        Ok(final_content)
    }

    /// Spawn a subagent for background task processing using mofa's TaskOrchestrator
    pub async fn spawn_subagent(
        &self,
        prompt: &str,
        origin_channel: &str,
        origin_chat_id: &str,
    ) -> Result<String> {
        let display_label = if prompt.len() > 30 {
            format!("{}...", &prompt[..30])
        } else {
            prompt.to_string()
        };

        info!("Spawning subagent for task: {}", display_label);

        // Create task origin for routing results
        let origin = TaskOrigin::from_channel(origin_channel, origin_chat_id);

        // Spawn using mofa's TaskOrchestrator
        let task_id = self.task_orchestrator.spawn(prompt, origin).await
            .map_err(|e| crate::error::AgentError::ProviderError(e.to_string()))?;

        // Subscribe to results and forward to message bus
        let mut result_rx = self.task_orchestrator.subscribe_results();
        let bus = self.bus.clone();
        let task_id_clone = task_id.clone();
        let label_clone = display_label.clone();
        let prompt_clone = prompt.to_string();
        let origin_channel_clone = origin_channel.to_string();
        let origin_chat_id_clone = origin_chat_id.to_string();

        tokio::spawn(async move {
            // Wait for this task's result
            while let Ok(result) = result_rx.recv().await {
                if result.task_id == task_id_clone {
                    Self::announce_subagent_result(
                        &bus,
                        &label_clone,
                        &prompt_clone,
                        &result.content,
                        &origin_channel_clone,
                        &origin_chat_id_clone,
                        result.success,
                    ).await;
                    break;
                }
            }
        });

        Ok(format!(
            "Subagent [{}] started. I'll notify you when it completes.",
            display_label
        ))
    }

    /// Announce the subagent result via the message bus
    async fn announce_subagent_result(
        bus: &MessageBus,
        label: &str,
        task: &str,
        result: &str,
        origin_channel: &str,
        origin_chat_id: &str,
        success: bool,
    ) {
        let status_text = if success { "completed successfully" } else { "failed" };

        let announce_content = format!(
            r#"[Subagent '{}' {}]

Task: {}

Result:
{}

Summarize this naturally for the user. Keep it brief (1-2 sentences). Do not mention technical details like "subagent" or task IDs."#,
            label, status_text, task, result
        );

        let msg = InboundMessage::system(
            &format!("subagent_{}", uuid::Uuid::new_v4()),
            origin_channel,
            origin_chat_id,
            &announce_content,
        );

        if let Err(e) = bus.publish_inbound(msg).await {
            error!("Failed to announce subagent result: {}", e);
        }
    }

    /// Get all active subagents from mofa's TaskOrchestrator
    pub async fn get_active_subagents(&self) -> Vec<ActiveSubagent> {
        let mofa_tasks = self.task_orchestrator.get_active_tasks().await;
        mofa_tasks.into_iter().map(|t| {
            let parts: Vec<&str> = t.origin.routing_key.split(':').collect();
            ActiveSubagent {
                id: t.id,
                prompt: t.prompt,
                origin_channel: parts.get(0).unwrap_or(&"").to_string(),
                origin_chat_id: parts.get(1).unwrap_or(&"").to_string(),
                started_at: t.started_at,
            }
        }).collect()
    }

    /// Process a message directly (for CLI usage)
    pub async fn process_direct(&self, content: &str, session_key: &str) -> Result<String> {
        // Create inbound message for CLI
        let msg = InboundMessage::new("cli", "user", session_key, content);

        // Process and get response
        if let Some(response) = self.process_message(msg).await? {
            Ok(response.content)
        } else {
            Ok("No response generated.".to_string())
        }
    }

    /// Get the context builder
    pub fn context(&self) -> &ContextBuilder {
        &self.context
    }

    /// Get the tool registry
    pub fn tools(&self) -> &Arc<RwLock<ToolRegistry>> {
        &self.tools
    }
}

/// Implement the spawn tool's SubagentManager trait directly on AgentLoop
#[async_trait::async_trait]
impl crate::tools::spawn::SubagentManager for AgentLoop {
    async fn spawn(&self, prompt: &str, origin_channel: &str, origin_chat_id: &str) -> Result<String> {
        self.spawn_subagent(prompt, origin_channel, origin_chat_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: The core agent loop logic is now provided by mofa_foundation::llm::AgentLoop
    // Tests for that functionality are in the mofa framework

    #[test]
    fn test_active_subagent_display_label() {
        let label = if "This is a very long prompt that should be truncated".len() > 30 {
            format!("{}...", &"This is a very long prompt that should be truncated"[..30])
        } else {
            "This is a very long prompt that should be truncated".to_string()
        };
        assert_eq!(label, "This is a very long prompt that...");
    }
}
