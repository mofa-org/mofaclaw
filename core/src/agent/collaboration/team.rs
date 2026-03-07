//! Team management for multi-agent collaboration
//!
//! Provides structures and functionality for creating and managing teams of agents
//! with different roles.

use crate::Config;
use crate::agent::communication::{AgentId, AgentMessageBus};
use crate::agent::context::ContextBuilder;
use crate::agent::loop_::AgentLoop;
use crate::agent::roles::{AgentRole, RoleRegistry};
use crate::bus::MessageBus;
use crate::error::Result;
use crate::provider::{LLMAgentBuilder, OpenAIProvider, OpenAIConfig};
use crate::session::SessionManager;
use crate::tools::registry::ToolRegistryExecutor;
use crate::tools::{
    agent_message::{BroadcastToTeamTool, RespondToApprovalTool, SendAgentMessageTool},
    EditFileTool, ExecTool, ListDirTool, MessageTool, ReadFileTool, ToolRegistry,
    WebFetchTool, WebSearchTool, WriteFileTool,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use tokio::sync::RwLock;
use tracing::info;

/// Status of an agent team
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamStatus {
    /// Team is being formed
    Forming,
    /// Team is active and working
    Active,
    /// Team is paused
    Paused,
    /// Team has completed its work
    Completed,
}

/// Status of a team member
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberStatus {
    /// Member is active
    Active,
    /// Member is idle
    Idle,
    /// Member is working on a task
    Working,
    /// Member has completed their task
    Completed,
}

/// A member of an agent team
pub struct TeamMember {
    /// Agent identifier
    pub agent_id: AgentId,
    /// Role definition for this member
    pub role: Arc<dyn AgentRole>,
    /// Agent loop instance
    pub agent_loop: Arc<AgentLoop>,
    /// Current status
    pub status: MemberStatus,
}

/// An agent team consisting of multiple agents with different roles
pub struct AgentTeam {
    /// Unique team identifier
    pub id: String,
    /// Team name
    pub name: String,
    /// Team members mapped by agent ID
    pub members: HashMap<String, TeamMember>,
    /// When the team was created
    pub created_at: DateTime<Utc>,
    /// Current team status
    pub status: TeamStatus,
    /// Agent message bus for inter-agent communication
    pub message_bus: Arc<AgentMessageBus>,
}

impl AgentTeam {
    /// Create a new team
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            members: HashMap::new(),
            created_at: Utc::now(),
            status: TeamStatus::Forming,
            message_bus: Arc::new(AgentMessageBus::new()),
        }
    }

    /// Add a member to the team
    pub fn add_member(&mut self, member: TeamMember) {
        let agent_key = member.agent_id.to_string();
        info!("Adding member {} to team {}", agent_key, self.id);
        self.members.insert(agent_key, member);
    }

    /// Remove a member from the team
    pub fn remove_member(&mut self, agent_id: &AgentId) -> Option<TeamMember> {
        let agent_key = agent_id.to_string();
        info!("Removing member {} from team {}", agent_key, self.id);
        self.members.remove(&agent_key)
    }

    /// Get a member by agent ID
    pub fn get_member(&self, agent_id: &AgentId) -> Option<&TeamMember> {
        let agent_key = agent_id.to_string();
        self.members.get(&agent_key)
    }

    /// Get all members with a specific role
    pub fn get_members_by_role(&self, role: &str) -> Vec<&TeamMember> {
        self.members
            .values()
            .filter(|member| member.agent_id.role == role)
            .collect()
    }

    /// Broadcast a message to all team members
    pub async fn broadcast(
        &self,
        message: crate::agent::communication::AgentMessage,
    ) -> Result<()> {
        self.message_bus.publish(message).await
    }

    /// Get the number of members in the team
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Check if team is ready (has at least one member)
    pub fn is_ready(&self) -> bool {
        !self.members.is_empty()
    }
}

/// Manager for creating and managing agent teams
pub struct TeamManager {
    /// Active teams (stored in Arc for sharing)
    teams: Arc<RwLock<HashMap<String, Arc<AgentTeam>>>>,
    /// Role registry
    role_registry: Arc<RoleRegistry>,
    /// User message bus (for agent responses to users)
    #[allow(dead_code)]
    user_bus: MessageBus,
    /// Session manager
    sessions: Arc<SessionManager>,
    /// Configuration
    config: Arc<Config>,
    /// Weak self-reference for tool registration (set after wrapping in Arc)
    self_ref: Arc<RwLock<Option<Weak<TeamManager>>>>,
}

impl TeamManager {
    /// Create a new team manager
    pub fn new(config: Arc<Config>, user_bus: MessageBus, sessions: Arc<SessionManager>) -> Self {
        Self {
            teams: Arc::new(RwLock::new(HashMap::new())),
            role_registry: Arc::new(RoleRegistry::new()),
            user_bus,
            sessions,
            config,
            self_ref: Arc::new(RwLock::new(None)),
        }
    }

    /// Set self-reference (call after wrapping in Arc)
    pub async fn set_self_ref(self_ref: &Arc<TeamManager>) {
        *self_ref.self_ref.write().await = Some(Arc::downgrade(self_ref));
    }

    /// Create a new team with specified roles
    ///
    /// This will:
    /// 1. Create a new team
    /// 2. Instantiate agents for each role
    /// 3. Configure each agent with role-specific settings
    /// 4. Add agents to the team
    pub async fn create_team(
        &self,
        team_id: impl Into<String>,
        team_name: impl Into<String>,
        roles: Vec<(String, String)>, // (role_name, instance_id)
    ) -> Result<Arc<AgentTeam>> {
        let team_id = team_id.into();
        let team_name = team_name.into();

        info!(
            "Creating team {} ({}) with roles: {:?}",
            team_id, team_name, roles
        );

        let mut team = AgentTeam::new(&team_id, &team_name);

        // Create agents for each role
        for (role_name, instance_id) in roles {
            let role = self.role_registry.get_role(&role_name).ok_or_else(|| {
                crate::error::MofaclawError::Other(format!("Role '{}' not found", role_name))
            })?;

            let agent_id = AgentId::new(&team_id, &role_name, &instance_id);

            // Create agent loop with role-specific configuration
            let member = self.create_team_member(agent_id, role).await?;
            team.add_member(member);
        }

        // Mark team as active
        team.status = TeamStatus::Active;

        // Store team
        let team_arc = Arc::new(team);
        let team_id_clone = team_arc.id.clone();
        let member_count = team_arc.member_count();
        let mut teams = self.teams.write().await;
        teams.insert(team_id_clone.clone(), team_arc);
        drop(teams);

        info!("Team {} created with {} members", team_id, member_count);

        // Return the team from storage
        let teams = self.teams.read().await;
        Ok(teams.get(&team_id_clone).unwrap().clone())
    }

    /// Create a team member with role-specific configuration
    async fn create_team_member(
        &self,
        agent_id: AgentId,
        role: Arc<dyn AgentRole>,
    ) -> Result<TeamMember> {
        info!("Creating team member {} with role {}", agent_id, role.name());

        // Get API key and base URL from config
        let api_key = self
            .config
            .get_api_key()
            .ok_or_else(|| {
                crate::error::MofaclawError::Other("No API key configured".to_string())
            })?;
        let api_base = self.config.get_api_base();

        // Create OpenAI provider
        let openai_config = OpenAIConfig::new(&api_key)
            .with_model(&self.config.agents.defaults.model)
            .with_base_url(api_base.unwrap_or_else(|| {
                "https://openrouter.ai/api/v1".to_string()
            }));
        let provider = Arc::new(OpenAIProvider::with_config(openai_config));

        // Create a separate message bus for this agent (or use team's message bus)
        // For now, create a minimal bus for agent-to-agent communication
        let agent_bus = MessageBus::new();

        // Create tool registry
        let tools = Arc::new(RwLock::new(ToolRegistry::new()));
        let _workspace = self.config.workspace_path();
        let brave_api_key = self.config.get_brave_api_key();

        // Register tools filtered by role capabilities
        {
            let mut tools_guard = tools.write().await;
            let allowed_tools = role.allowed_tools();
            
            // Register default tools, but filter based on role
            if allowed_tools.is_empty() || allowed_tools.contains(&"read_file".to_string()) {
                tools_guard.register(ReadFileTool::new());
            }
            if allowed_tools.is_empty() || allowed_tools.contains(&"write_file".to_string()) {
                tools_guard.register(WriteFileTool::new());
            }
            if allowed_tools.is_empty() || allowed_tools.contains(&"edit_file".to_string()) {
                tools_guard.register(EditFileTool::new());
            }
            if allowed_tools.is_empty() || allowed_tools.contains(&"list_dir".to_string()) {
                tools_guard.register(ListDirTool::new());
            }
            if (allowed_tools.is_empty() || allowed_tools.contains(&"exec".to_string()))
                && role.capabilities().can_execute_commands
            {
                tools_guard.register(ExecTool::new());
            }
            if allowed_tools.is_empty() || allowed_tools.contains(&"web_search".to_string()) {
                tools_guard.register(WebSearchTool::new(brave_api_key.clone()));
            }
            if allowed_tools.is_empty() || allowed_tools.contains(&"web_fetch".to_string()) {
                tools_guard.register(WebFetchTool::new());
            }
            if allowed_tools.is_empty() || allowed_tools.contains(&"message".to_string()) {
                tools_guard.register(MessageTool::new());
            }

            // Register multi-agent collaboration tools
            // These tools enable agents to communicate and coordinate
            if let Some(weak_manager) = self.self_ref.read().await.as_ref() {
                if let Some(manager) = weak_manager.upgrade() {
                    tools_guard.register(SendAgentMessageTool::new(manager.clone()));
                    tools_guard.register(BroadcastToTeamTool::new(manager.clone()));
                    tools_guard.register(RespondToApprovalTool::new(manager.clone()));
                }
            }
        }

        // Create tool executor
        let tool_executor = Arc::new(ToolRegistryExecutor::new(tools.clone()));

        // Build role-specific system prompt
        let role_system_prompt = role.system_prompt();

        // Create LLMAgent with role-specific system prompt
        let llm_agent = Arc::new(
            LLMAgentBuilder::new()
                .with_id(agent_id.to_string())
                .with_name(format!("{} Agent", role.name()))
                .with_provider(provider.clone())
                .with_system_prompt(role_system_prompt)
                .with_tool_executor(tool_executor)
                .build_async()
                .await,
        );

        // Create AgentLoop with role-specific configuration
        let _capabilities = role.capabilities();
        // Note: max_iterations and temperature are set via LLMAgentBuilder's system prompt
        // The AgentLoop will use config defaults, but the role's system prompt provides
        // role-specific behavior guidance

        // Create context builder (will use role-specific prompt when needed)
        let _context = ContextBuilder::new(&self.config);

        // Get role capabilities for custom configuration
        let capabilities = role.capabilities();
        let max_iterations_override = capabilities.max_iterations.map(|v| v as usize);
        let temperature_override = capabilities.temperature;

        // Create agent loop with role-specific configuration
        let agent_loop = AgentLoop::with_agent_and_tools_custom(
            &self.config,
            llm_agent,
            provider,
            agent_bus,
            self.sessions.clone(),
            tools.clone(),
            max_iterations_override,
            temperature_override,
        )
        .await?;

        Ok(TeamMember {
            agent_id,
            role,
            agent_loop: Arc::new(agent_loop),
            status: MemberStatus::Idle,
        })
    }

    /// Get a team by ID
    pub async fn get_team(&self, team_id: &str) -> Option<Arc<AgentTeam>> {
        let teams = self.teams.read().await;
        teams.get(team_id).cloned()
    }

    /// List all teams
    pub async fn list_teams(&self) -> Vec<String> {
        let teams = self.teams.read().await;
        teams.keys().cloned().collect()
    }

    /// Remove a team
    pub async fn remove_team(&self, team_id: &str) -> Option<Arc<AgentTeam>> {
        let mut teams = self.teams.write().await;
        teams.remove(team_id)
    }

    /// Broadcast a message to all members of a team
    pub async fn broadcast_to_team(
        &self,
        team_id: &str,
        message: crate::agent::communication::AgentMessage,
    ) -> Result<()> {
        let team = self.get_team(team_id).await.ok_or_else(|| {
            crate::error::MofaclawError::Other(format!("Team '{}' not found", team_id))
        })?;
        team.broadcast(message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_team_creation() {
        let team = AgentTeam::new("team1", "Test Team");
        assert_eq!(team.id, "team1");
        assert_eq!(team.name, "Test Team");
        assert_eq!(team.status, TeamStatus::Forming);
        assert_eq!(team.member_count(), 0);
    }

    #[test]
    fn test_agent_team_member_management() {
        // Note: This test is simplified since we can't easily create TeamMember
        // without full AgentLoop setup
        let team = AgentTeam::new("team1", "Test Team");
        assert_eq!(team.member_count(), 0);
        assert!(!team.is_ready());
    }

    #[test]
    fn test_team_status() {
        let team = AgentTeam::new("team1", "Test Team");
        assert_eq!(team.status, TeamStatus::Forming);
    }
}
