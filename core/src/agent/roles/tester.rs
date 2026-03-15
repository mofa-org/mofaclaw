//! Tester role implementation
//!
//! The Tester role focuses on testing, validation, and verification.
//! Testers write test cases, run tests, and ensure code quality through testing.

use super::{AgentRole, RoleCapabilities};
use async_trait::async_trait;

pub struct TesterRole;

impl TesterRole {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentRole for TesterRole {
    fn name(&self) -> &str {
        "tester"
    }

    fn description(&self) -> &str {
        "Writes and runs tests to validate functionality and ensure quality"
    }

    fn system_prompt(&self) -> String {
        r#"You are a Tester AI agent specializing in testing, validation, and quality assurance.

## Your Role
You write comprehensive test cases, run tests, and validate that code works correctly.
You ensure adequate test coverage and catch bugs through systematic testing.

## Your Responsibilities
- Write unit tests, integration tests, and end-to-end tests
- Run test suites and analyze results
- Identify edge cases and boundary conditions
- Verify that code meets requirements
- Check test coverage and identify gaps
- Document test cases and test plans
- Report bugs and issues clearly
- Validate fixes and regressions

## Your Approach
- Think like a user and try to break things
- Test both happy paths and error cases
- Consider edge cases and boundary conditions
- Write clear, maintainable test code
- Document what you're testing and why
- Run tests systematically and report results accurately
- Verify fixes don't introduce regressions

## Tools Available
You have access to:
- read_file: Read code to understand what to test
- write_file: Create test files
- edit_file: Modify existing test files
- exec: Run test commands and scripts
- list_dir: Explore project structure
- message: Communicate test results and findings

## Guidelines
- Understand the code you're testing
- Write tests before or alongside code (TDD when possible)
- Test one thing at a time (unit tests)
- Use descriptive test names
- Test edge cases: empty inputs, null values, boundary conditions
- Test error handling and failure cases
- Keep tests independent and isolated
- Run tests frequently and fix failures immediately
- Document test requirements and expected behavior
- Report bugs with clear steps to reproduce"#
            .to_string()
    }

    fn allowed_tools(&self) -> Vec<String> {
        vec![
            "read_file".to_string(),
            "write_file".to_string(), // For test files
            "edit_file".to_string(),  // For modifying tests
            "exec".to_string(),       // For running tests
            "list_dir".to_string(),
            "message".to_string(),
        ]
    }

    fn capabilities(&self) -> RoleCapabilities {
        RoleCapabilities {
            can_read_files: true,
            can_write_files: true,      // For test files
            can_execute_commands: true, // For running tests
            can_spawn_subagents: false,
            max_iterations: Some(20), // Moderate iterations for test writing
            temperature: Some(0.4),   // Lower temperature for systematic testing
        }
    }
}

impl Default for TesterRole {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tester_role_name() {
        let role = TesterRole::new();
        assert_eq!(role.name(), "tester");
    }

    #[test]
    fn test_tester_role_description() {
        let role = TesterRole::new();
        assert!(!role.description().is_empty());
        assert!(role.description().contains("test"));
    }

    #[test]
    fn test_tester_role_system_prompt() {
        let role = TesterRole::new();
        let prompt = role.system_prompt();
        assert!(prompt.contains("Tester"));
        assert!(prompt.contains("test"));
        assert!(prompt.contains("validate"));
    }

    #[test]
    fn test_tester_role_tools() {
        let role = TesterRole::new();
        let tools = role.allowed_tools();
        assert!(tools.contains(&"read_file".to_string()));
        assert!(tools.contains(&"write_file".to_string()));
        assert!(tools.contains(&"exec".to_string()));
        assert!(tools.contains(&"message".to_string()));
    }

    #[test]
    fn test_tester_role_capabilities() {
        let role = TesterRole::new();
        let caps = role.capabilities();
        assert!(caps.can_read_files);
        assert!(caps.can_write_files);
        assert!(caps.can_execute_commands);
        assert!(!caps.can_spawn_subagents);
        assert_eq!(caps.max_iterations, Some(20));
        assert_eq!(caps.temperature, Some(0.4));
    }
}
