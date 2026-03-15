//! Agent role definitions for multi-agent collaboration
//!
//! This module provides role-based specialization for agents, allowing
//! different agents to have different capabilities, tools, and prompts.

pub mod architect;
pub mod base;
pub mod developer;
pub mod reviewer;
pub mod tester;

pub use architect::ArchitectRole;
pub use base::{AgentRole, RoleCapabilities};
pub use developer::DeveloperRole;
pub use reviewer::ReviewerRole;
pub use tester::TesterRole;

use std::collections::HashMap;
use std::sync::Arc;

/// Registry for managing available agent roles
pub struct RoleRegistry {
    roles: HashMap<String, Arc<dyn AgentRole>>,
}

impl RoleRegistry {
    /// Create a new role registry with built-in roles
    pub fn new() -> Self {
        let mut registry = Self {
            roles: HashMap::new(),
        };
        registry.register_builtin_roles();
        registry
    }

    /// Register a role
    pub fn register_role(&mut self, role: Arc<dyn AgentRole>) {
        self.roles.insert(role.name().to_string(), role);
    }

    /// Get a role by name
    pub fn get_role(&self, name: &str) -> Option<Arc<dyn AgentRole>> {
        self.roles.get(name).cloned()
    }

    /// List all registered role names
    pub fn list_roles(&self) -> Vec<String> {
        self.roles.keys().cloned().collect()
    }

    /// Check if a role exists
    pub fn has_role(&self, name: &str) -> bool {
        self.roles.contains_key(name)
    }

    /// Register all built-in roles
    fn register_builtin_roles(&mut self) {
        self.register_role(Arc::new(ArchitectRole::new()));
        self.register_role(Arc::new(DeveloperRole::new()));
        self.register_role(Arc::new(ReviewerRole::new()));
        self.register_role(Arc::new(TesterRole::new()));
    }
}

impl Default for RoleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_registry_creation() {
        let registry = RoleRegistry::new();
        assert!(registry.has_role("architect"));
        assert!(registry.has_role("developer"));
        assert!(registry.has_role("reviewer"));
        assert!(registry.has_role("tester"));
    }

    #[test]
    fn test_role_registry_get_role() {
        let registry = RoleRegistry::new();
        let architect = registry.get_role("architect");
        assert!(architect.is_some());
        assert_eq!(architect.unwrap().name(), "architect");
    }

    #[test]
    fn test_role_registry_list_roles() {
        let registry = RoleRegistry::new();
        let roles = registry.list_roles();
        assert!(roles.contains(&"architect".to_string()));
        assert!(roles.contains(&"developer".to_string()));
        assert!(roles.contains(&"reviewer".to_string()));
        assert!(roles.contains(&"tester".to_string()));
    }
}
