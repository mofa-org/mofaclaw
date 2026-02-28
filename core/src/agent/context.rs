//! Context builder for assembling agent prompts
//!
//! This module integrates mofa-sdk's PromptContext with mofaclaw-specific
//! features like skills loading and vision model support.

use crate::Config;
use crate::error::{AgentError, Result};
use crate::types::{Message, MessageContent};
use mofa_sdk::agent::{AgentIdentity, PromptContextBuilder};
use mofa_sdk::skills::SkillsManager;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::debug;

/// Builds the context (system prompt + messages) for the agent
///
/// This wraps mofa-sdk's PromptContext while adding mofaclaw-specific features:
/// - Skills system with progressive loading
/// - Vision model support
///
/// Memory (long-term and daily notes) is handled by mofa's PromptContext.
#[derive(Clone)]
pub struct ContextBuilder {
    /// Skills manager (from mofa-sdk)
    skills: Arc<SkillsManager>,
    /// Workspace path (cached)
    workspace: PathBuf,
}

impl ContextBuilder {
    /// Bootstrap files to load
    const BOOTSTRAP_FILES: &'static [&'static str] =
        &["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md", "IDENTITY.md"];

    /// Create a new context builder
    pub fn new(config: &Config) -> Self {
        let workspace = config.workspace_path();

        // Create SkillsManager with workspace + builtin skills
        let workspace_skills = workspace.join("skills");
        let mut builtin_skills = SkillsManager::find_builtin_skills();

        // Fallback: try to find skills directory relative to the executable or CARGO_MANIFEST_DIR
        if builtin_skills.is_none()
            && let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR")
        {
            let manifest_path = std::path::PathBuf::from(&manifest_dir);
            // Try ../skills (cli/../skills -> project/skills)
            let skills_path = manifest_path.join("../skills");
            if skills_path.exists() {
                builtin_skills = Some(skills_path);
            }
            // Try ../../skills (for deeper nesting)
            if builtin_skills.is_none() {
                let skills_path = manifest_path.join("../../skills");
                if skills_path.exists() {
                    builtin_skills = Some(skills_path);
                }
            }
        }

        // Second try: relative to executable
        if builtin_skills.is_none()
            && let Ok(exe) = std::env::current_exe()
        {
            // Try exe parent/skills (development build)
            if let Some(parent) = exe.parent() {
                let skills_path = parent.join("skills");
                if skills_path.exists() {
                    builtin_skills = Some(skills_path);
                }
            }
            // Try exe parent/../skills (for target/debug/mofaclaw -> target/skills)
            if builtin_skills.is_none()
                && let Some(parent) = exe.parent().and_then(|p| p.parent())
            {
                let skills_path = parent.join("skills");
                if skills_path.exists() {
                    builtin_skills = Some(skills_path);
                }
            }
            // Try exe parent/../../skills (for target/debug/mofaclaw -> project/skills)
            if builtin_skills.is_none()
                && let Some(parent) = exe
                    .parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
            {
                let skills_path = parent.join("skills");
                if skills_path.exists() {
                    builtin_skills = Some(skills_path);
                }
            }
        }

        let skills = if let Some(builtin) = builtin_skills {
            tracing::info!("Using both workspace and builtin skills");
            let manager = SkillsManager::with_search_dirs(vec![workspace_skills.clone(), builtin])
                .unwrap_or_else(|_| SkillsManager::new(&workspace_skills).unwrap());
            // Log the number of skills found
            let all_metadata = manager.get_all_metadata();
            tracing::info!("Found {} skills", all_metadata.len());
            for meta in &all_metadata {
                tracing::info!("  - {} (requires: {:?})", meta.name, meta.requires);
            }
            manager
        } else {
            tracing::info!("No builtin skills found, using only workspace skills");
            SkillsManager::new(&workspace_skills).unwrap()
        };

        Self {
            skills: Arc::new(skills),
            workspace,
        }
    }

    /// Build the system prompt from bootstrap files, memory, and skills
    pub async fn build_system_prompt(&self, skill_names: Option<&[String]>) -> Result<String> {
        let mut parts = Vec::new();

        // Build base prompt using mofa's PromptContextBuilder
        // Memory context (long-term + today's notes) is integrated automatically
        let identity = AgentIdentity {
            name: "mofaclaw".to_string(),
            description: "a helpful AI assistant".to_string(),
            icon: Some("🐈".to_string()),
        };

        let mut prompt_ctx = PromptContextBuilder::new(&self.workspace)
            .with_identity(identity)
            .with_bootstrap_files(
                Self::BOOTSTRAP_FILES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            )
            .build()
            .await
            .map_err(|e| AgentError::Memory(format!("Failed to build prompt: {}", e)))?;

        let base_prompt = prompt_ctx
            .build_system_prompt()
            .await
            .map_err(|e| AgentError::ContextError(e.to_string()))?;

        parts.push(base_prompt);

        // Add skills - progressive loading
        let always_skills = self.skills.get_always_skills_async().await;
        if !always_skills.is_empty() {
            let always_content = self.skills.load_skills_for_context(&always_skills).await;
            if !always_content.is_empty() {
                parts.push(format!("# Active Skills\n\n{}", always_content));
            }
        }

        // Requested skills
        if let Some(names) = skill_names {
            if !names.is_empty() {
                let skills_content = self.skills.load_skills_for_context(names).await;
                if !skills_content.is_empty() {
                    parts.push(format!("# Requested Skills\n\n{}", skills_content));
                }
            }
        }

        // Skills summary
        let skills_summary = self.skills.build_skills_summary().await;
        if !skills_summary.is_empty() {
            parts.push(format!(
                r#"# Available Skills

You have access to the following skills that extend your capabilities.

## When users ask about your skills

When users ask questions like:
- "What skills do you have?"
- "What can you do?"
- "What are your capabilities?"
- "List your skills"
- "有什么技能" (What skills do you have - Chinese)

Provide a friendly, concise summary of the available skills listed below. For each skill, mention:
1. The skill name
2. A brief description of what it does
3. Whether it's currently available (✓) or needs dependencies (✗)

Example response format:
"I have access to several skills including:
- **skill1**: description...
- **skill2**: description...

To use any skill, I can read its documentation for detailed instructions."

## To use a skill

Read the skill's SKILL.md file using the `read_file` tool to learn how to use it.

## Skills List

{}"#,
                skills_summary
            ));
        }

        Ok(parts.join("\n\n---\n\n"))
    }

    /// Build the complete message list for an LLM call
    pub async fn build_messages(
        &self,
        history: Vec<Message>,
        current_message: &str,
        skill_names: Option<&[String]>,
        media: Option<&[String]>,
    ) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        // System prompt
        let system_prompt = self.build_system_prompt(skill_names).await?;
        messages.push(Message::system(system_prompt));

        // History
        messages.extend(history);

        // Current message (with optional image attachments)
        let user_content = self.build_user_content(current_message, media)?;
        messages.push(Message::user_with_content(user_content));

        Ok(messages)
    }

    /// Build user message content with optional base64-encoded images
    fn build_user_content(&self, text: &str, media: Option<&[String]>) -> Result<MessageContent> {
        use base64::Engine;
        use std::fs;

        if let Some(media_paths) = media {
            if !media_paths.is_empty() {
                let mut images = Vec::new();

                for path in media_paths {
                    let path_obj = Path::new(path);
                    if !path_obj.exists() {
                        debug!("Media file does not exist: {}", path);
                        continue;
                    }

                    // Detect MIME type and encode
                    if let Ok(Some(info)) = infer::get_from_path(path_obj) {
                        let mime = info.mime_type();
                        if mime.starts_with("image/") {
                            if let Ok(bytes) = fs::read(path_obj) {
                                let base64_str =
                                    base64::engine::general_purpose::STANDARD_NO_PAD.encode(&bytes);
                                let data_url = format!("data:{};base64,{}", mime, base64_str);
                                images.push(serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {"url": data_url}
                                }));
                            }
                        }
                    }
                }

                if !images.is_empty() {
                    let mut content = images;
                    content.push(serde_json::json!({
                        "type": "text",
                        "text": text
                    }));
                    return Ok(MessageContent::Array(content));
                }
            }
        }

        Ok(MessageContent::Text(text.to_string()))
    }

    /// Add a tool result to the message list
    pub fn add_tool_result(
        messages: &mut Vec<Message>,
        tool_call_id: &str,
        tool_name: &str,
        result: &str,
    ) {
        messages.push(Message::tool(tool_call_id, tool_name, result));
    }

    /// Add an assistant message to the message list
    pub fn add_assistant_message(
        messages: &mut Vec<Message>,
        content: Option<&str>,
        tool_calls: Vec<Value>,
    ) {
        let tool_calls_parsed = tool_calls
            .iter()
            .filter_map(|tc| {
                let id = tc.get("id")?.as_str()?;
                let func = tc.get("function")?;
                let name = func.get("name")?.as_str()?;
                let args_str = func.get("arguments")?.as_str()?;

                let arguments: HashMap<String, Value> = serde_json::from_str(args_str).ok()?;

                Some(crate::types::ToolCallRequest {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments,
                })
            })
            .collect();

        messages.push(Message::assistant(
            content.map(|s| s.to_string()),
            tool_calls_parsed,
        ));
    }

    /// Get the workspace path
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Get the skills manager
    pub fn skills(&self) -> &Arc<SkillsManager> {
        &self.skills
    }
}
