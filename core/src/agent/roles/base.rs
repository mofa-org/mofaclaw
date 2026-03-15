//! Base trait and types for agent roles

use crate::error::Result;
use async_trait::async_trait;

/// Capabilities and constraints for an agent role
#[derive(Debug, Clone, PartialEq)]
pub struct RoleCapabilities {
    /// Can read files
    pub can_read_files: bool,
    /// Can write/modify files
    pub can_write_files: bool,
    /// Can execute shell commands
    pub can_execute_commands: bool,
    /// Can spawn subagents
    pub can_spawn_subagents: bool,
    /// Maximum tool iterations (None = use default)
    pub max_iterations: Option<u32>,
    /// Temperature setting (None = use default)
    pub temperature: Option<f32>,
}

impl Default for RoleCapabilities {
    fn default() -> Self {
        Self {
            can_read_files: true,
            can_write_files: true,
            can_execute_commands: true,
            can_spawn_subagents: true,
            max_iterations: None,
            temperature: None,
        }
    }
}

/// Trait for defining agent roles
///
/// Each role defines:
/// - A name and description
/// - A specialized system prompt
/// - Allowed tools
/// - Capabilities and constraints
#[async_trait]
pub trait AgentRole: Send + Sync {
    /// Get the role name (e.g., "architect", "developer")
    fn name(&self) -> &str;

    /// Get a brief description of the role
    fn description(&self) -> &str;

    /// Get the system prompt for this role
    ///
    /// This prompt defines the agent's personality, responsibilities,
    /// and how it should approach tasks.
    fn system_prompt(&self) -> String;

    /// Get the list of allowed tool names for this role
    ///
    /// Returns a vector of tool names (e.g., ["read_file", "write_file", "exec"])
    /// that this role is allowed to use. Empty vector means all tools are allowed.
    fn allowed_tools(&self) -> Vec<String>;

    /// Get the capabilities and constraints for this role
    fn capabilities(&self) -> RoleCapabilities;

    /// Hook called before an agent with this role starts a task
    ///
    /// Can be used for role-specific initialization or validation.
    /// Default implementation does nothing.
    async fn before_task(&self, _task: &str) -> Result<()> {
        Ok(())
    }

    /// Hook called after an agent with this role completes a task
    ///
    /// Can be used for role-specific cleanup or result processing.
    /// Default implementation does nothing.
    async fn after_task(&self, _result: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRole {
        name: String,
        description: String,
        system_prompt: String,
        allowed_tools: Vec<String>,
        capabilities: RoleCapabilities,
    }

    #[async_trait]
    impl AgentRole for TestRole {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn system_prompt(&self) -> String {
            self.system_prompt.clone()
        }

        fn allowed_tools(&self) -> Vec<String> {
            self.allowed_tools.clone()
        }

        fn capabilities(&self) -> RoleCapabilities {
            self.capabilities.clone()
        }
    }

    #[test]
    fn test_role_capabilities_default() {
        let caps = RoleCapabilities::default();
        assert!(caps.can_read_files);
        assert!(caps.can_write_files);
        assert!(caps.can_execute_commands);
        assert!(caps.can_spawn_subagents);
        assert_eq!(caps.max_iterations, None);
        assert_eq!(caps.temperature, None);
    }

    #[test]
    fn test_role_capabilities_custom() {
        let caps = RoleCapabilities {
            can_read_files: true,
            can_write_files: false,
            can_execute_commands: false,
            can_spawn_subagents: false,
            max_iterations: Some(10),
            temperature: Some(0.7),
        };
        assert!(caps.can_read_files);
        assert!(!caps.can_write_files);
        assert_eq!(caps.max_iterations, Some(10));
        assert_eq!(caps.temperature, Some(0.7));
    }

    #[tokio::test]
    async fn test_agent_role_trait() {
        let role = TestRole {
            name: "test".to_string(),
            description: "Test role".to_string(),
            system_prompt: "You are a test agent.".to_string(),
            allowed_tools: vec!["read_file".to_string()],
            capabilities: RoleCapabilities::default(),
        };

        assert_eq!(role.name(), "test");
        assert_eq!(role.description(), "Test role");
        assert_eq!(role.system_prompt(), "You are a test agent.");
        assert_eq!(role.allowed_tools(), vec!["read_file"]);
        assert!(role.before_task("test task").await.is_ok());
        assert!(role.after_task("test result").await.is_ok());
    }
}
