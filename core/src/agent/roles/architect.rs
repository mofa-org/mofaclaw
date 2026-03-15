//! Architect role implementation
//!
//! The Architect role focuses on high-level system design, architecture decisions,
//! and technical planning. Architects analyze requirements, design system structures,
//! and make strategic technical decisions.

use super::{AgentRole, RoleCapabilities};
use async_trait::async_trait;

pub struct ArchitectRole;

impl ArchitectRole {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentRole for ArchitectRole {
    fn name(&self) -> &str {
        "architect"
    }

    fn description(&self) -> &str {
        "Designs system architecture and makes high-level technical decisions"
    }

    fn system_prompt(&self) -> String {
        r#"You are an Architect AI agent specializing in system design and technical architecture.

## Your Role
You focus on high-level design, architecture patterns, and strategic technical decisions.
You analyze requirements, design system structures, and evaluate trade-offs.

## Your Responsibilities
- Analyze requirements and constraints
- Design system architecture and component structure
- Evaluate architectural patterns and trade-offs
- Make strategic technical decisions
- Document design decisions and rationale
- Review implementations for architectural compliance

## Your Approach
- Think in terms of systems, components, and interfaces
- Consider scalability, maintainability, and performance
- Evaluate multiple design options before deciding
- Document your reasoning and trade-offs
- Focus on the "what" and "why" rather than implementation details

## Tools Available
You have access to:
- read_file: Read and analyze existing code and documentation
- list_dir: Explore project structure
- message: Communicate with other team members

## Guidelines
- Ask clarifying questions when requirements are unclear
- Consider non-functional requirements (performance, security, scalability)
- Document your design decisions clearly
- Collaborate with developers to ensure feasibility
- Think long-term and consider maintainability"#
            .to_string()
    }

    fn allowed_tools(&self) -> Vec<String> {
        vec![
            "read_file".to_string(),
            "list_dir".to_string(),
            "message".to_string(),
        ]
    }

    fn capabilities(&self) -> RoleCapabilities {
        RoleCapabilities {
            can_read_files: true,
            can_write_files: false, // Architects don't write code
            can_execute_commands: false,
            can_spawn_subagents: false,
            max_iterations: Some(20), // More iterations for complex design work
            temperature: Some(0.7),   // Slightly higher for creative design thinking
        }
    }
}

impl Default for ArchitectRole {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_architect_role_name() {
        let role = ArchitectRole::new();
        assert_eq!(role.name(), "architect");
    }

    #[test]
    fn test_architect_role_description() {
        let role = ArchitectRole::new();
        assert!(!role.description().is_empty());
        assert!(role.description().contains("architecture"));
    }

    #[test]
    fn test_architect_role_system_prompt() {
        let role = ArchitectRole::new();
        let prompt = role.system_prompt();
        assert!(prompt.contains("Architect"));
        assert!(prompt.contains("design"));
        assert!(prompt.contains("architecture"));
    }

    #[test]
    fn test_architect_role_tools() {
        let role = ArchitectRole::new();
        let tools = role.allowed_tools();
        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"list_dir".to_string()));
        assert!(tools.contains(&"message".to_string()));
        assert!(!tools.contains(&"write_file".to_string()));
        assert!(!tools.contains(&"exec".to_string()));
    }

    #[test]
    fn test_architect_role_capabilities() {
        let role = ArchitectRole::new();
        let caps = role.capabilities();
        assert!(caps.can_read_files);
        assert!(!caps.can_write_files);
        assert!(!caps.can_execute_commands);
        assert!(!caps.can_spawn_subagents);
        assert_eq!(caps.max_iterations, Some(20));
        assert_eq!(caps.temperature, Some(0.7));
    }
}
