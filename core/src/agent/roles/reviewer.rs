//! Reviewer role implementation
//!
//! The Reviewer role focuses on code review, quality assurance, and providing
//! constructive feedback. Reviewers ensure code quality, catch bugs, and
//! suggest improvements.

use super::{AgentRole, RoleCapabilities};
use async_trait::async_trait;

pub struct ReviewerRole;

impl ReviewerRole {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentRole for ReviewerRole {
    fn name(&self) -> &str {
        "reviewer"
    }

    fn description(&self) -> &str {
        "Reviews code for quality, bugs, and improvements"
    }

    fn system_prompt(&self) -> String {
        r#"You are a Reviewer AI agent specializing in code review and quality assurance.

## Your Role
You review code for quality, correctness, maintainability, and adherence to standards.
You provide constructive feedback and catch potential issues before they reach production.

## Your Responsibilities
- Review code for correctness and logic errors
- Check adherence to coding standards and best practices
- Identify potential bugs and security issues
- Suggest improvements for readability and maintainability
- Verify that tests are adequate
- Check for performance issues
- Ensure code follows architectural guidelines

## Your Approach
- Be thorough and systematic in your review
- Be constructive and helpful in your feedback
- Focus on what matters: correctness, maintainability, security
- Explain the "why" behind your suggestions
- Acknowledge good practices and well-written code
- Prioritize critical issues over style preferences

## Tools Available
You have access to:
- read_file: Read code files to review
- list_dir: Explore project structure
- message: Communicate feedback to developers and other team members

## Guidelines
- Read the entire code change before commenting
- Understand the context and requirements
- Check for common issues:
  - Logic errors and edge cases
  - Security vulnerabilities
  - Performance problems
  - Code duplication
  - Missing error handling
  - Inadequate tests
- Provide specific, actionable feedback
- Be respectful and professional
- Ask questions if something is unclear
- Approve code that meets quality standards"#
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
            can_write_files: false, // Reviewers don't modify code
            can_execute_commands: false,
            can_spawn_subagents: false,
            max_iterations: Some(25), // More iterations for thorough reviews
            temperature: Some(0.5),   // Balanced for analytical review
        }
    }
}

impl Default for ReviewerRole {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reviewer_role_name() {
        let role = ReviewerRole::new();
        assert_eq!(role.name(), "reviewer");
    }

    #[test]
    fn test_reviewer_role_description() {
        let role = ReviewerRole::new();
        assert!(!role.description().is_empty());
        assert!(role.description().to_lowercase().contains("review"));
    }

    #[test]
    fn test_reviewer_role_system_prompt() {
        let role = ReviewerRole::new();
        let prompt = role.system_prompt();
        assert!(prompt.contains("Reviewer"));
        assert!(prompt.contains("review"));
        assert!(prompt.contains("quality"));
    }

    #[test]
    fn test_reviewer_role_tools() {
        let role = ReviewerRole::new();
        let tools = role.allowed_tools();
        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"list_dir".to_string()));
        assert!(tools.contains(&"message".to_string()));
        assert!(!tools.contains(&"write_file".to_string()));
        assert!(!tools.contains(&"exec".to_string()));
    }

    #[test]
    fn test_reviewer_role_capabilities() {
        let role = ReviewerRole::new();
        let caps = role.capabilities();
        assert!(caps.can_read_files);
        assert!(!caps.can_write_files);
        assert!(!caps.can_execute_commands);
        assert!(!caps.can_spawn_subagents);
        assert_eq!(caps.max_iterations, Some(25));
        assert_eq!(caps.temperature, Some(0.5));
    }
}
