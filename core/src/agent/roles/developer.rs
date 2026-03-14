//! Developer role implementation
//!
//! The Developer role focuses on implementation, coding, and debugging.
//! Developers translate designs into working code, write tests, and fix bugs.

use super::{AgentRole, RoleCapabilities};
use async_trait::async_trait;

pub struct DeveloperRole;

impl DeveloperRole {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentRole for DeveloperRole {
    fn name(&self) -> &str {
        "developer"
    }

    fn description(&self) -> &str {
        "Implements features, writes code, and debugs issues"
    }

    fn system_prompt(&self) -> String {
        r#"You are a Developer AI agent specializing in code implementation and debugging.

## Your Role
You focus on translating designs into working code, implementing features,
writing tests, and fixing bugs. You write clean, maintainable, and efficient code.

## Your Responsibilities
- Implement features according to specifications
- Write clean, readable, and maintainable code
- Follow coding standards and best practices
- Write unit tests and integration tests
- Debug and fix issues
- Refactor code for better quality
- Document code with comments

## Your Approach
- Write code that is clear and easy to understand
- Follow the project's coding standards
- Test your code thoroughly
- Consider edge cases and error handling
- Optimize for readability first, performance second
- Write self-documenting code with clear variable names

## Tools Available
You have access to:
- read_file: Read existing code and documentation
- write_file: Create new files
- edit_file: Modify existing files
- list_dir: Explore project structure
- exec: Execute shell commands (for testing, building, etc.)
- message: Communicate with other team members

## Guidelines
- Read and understand the requirements before coding
- Ask for clarification if specifications are unclear
- Write tests alongside your code
- Test your code before submitting
- Follow the existing code style and patterns
- Document complex logic and algorithms
- Consider performance implications
- Handle errors gracefully"#
            .to_string()
    }

    fn allowed_tools(&self) -> Vec<String> {
        vec![
            "read_file".to_string(),
            "write_file".to_string(),
            "edit_file".to_string(),
            "list_dir".to_string(),
            "exec".to_string(),
            "message".to_string(),
        ]
    }

    fn capabilities(&self) -> RoleCapabilities {
        RoleCapabilities {
            can_read_files: true,
            can_write_files: true,
            can_execute_commands: true, // For testing and building
            can_spawn_subagents: false,
            max_iterations: Some(30), // More iterations for complex implementations
            temperature: Some(0.3),   // Lower temperature for more deterministic code
        }
    }
}

impl Default for DeveloperRole {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_developer_role_name() {
        let role = DeveloperRole::new();
        assert_eq!(role.name(), "developer");
    }

    #[test]
    fn test_developer_role_description() {
        let role = DeveloperRole::new();
        assert!(!role.description().is_empty());
        assert!(role.description().contains("code"));
    }

    #[test]
    fn test_developer_role_system_prompt() {
        let role = DeveloperRole::new();
        let prompt = role.system_prompt();
        assert!(prompt.contains("Developer"));
        assert!(prompt.contains("code"));
        assert!(prompt.contains("implement"));
    }

    #[test]
    fn test_developer_role_tools() {
        let role = DeveloperRole::new();
        let tools = role.allowed_tools();
        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"write_file".to_string()));
        assert!(tools.contains(&"edit_file".to_string()));
        assert!(tools.contains(&"exec".to_string()));
    }

    #[test]
    fn test_developer_role_capabilities() {
        let role = DeveloperRole::new();
        let caps = role.capabilities();
        assert!(caps.can_read_files);
        assert!(caps.can_write_files);
        assert!(caps.can_execute_commands);
        assert!(!caps.can_spawn_subagents);
        assert_eq!(caps.max_iterations, Some(30));
        assert_eq!(caps.temperature, Some(0.3));
    }
}
