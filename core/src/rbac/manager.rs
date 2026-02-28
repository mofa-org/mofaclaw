//! RBAC Manager - core permission checking logic

use crate::rbac::config::RbacConfig;
use crate::rbac::path_matcher::PathMatcher;
use crate::rbac::role::Role;
use regex;
use std::path::Path;
use tracing::{debug, warn};

/// Result of a permission check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResult {
    /// Permission granted
    Allowed,
    /// Permission denied with reason
    Denied(String),
}

/// RBAC Manager for permission checking
pub struct RbacManager {
    config: RbacConfig,
    path_matcher: PathMatcher,
}

impl RbacManager {
    /// Create a new RBAC manager
    pub fn new(config: RbacConfig, workspace: std::path::PathBuf, home: std::path::PathBuf) -> Self {
        let path_matcher = PathMatcher::new(workspace, home);
        Self {
            config,
            path_matcher,
        }
    }

    /// Check if RBAC is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Check permission for a resource and operation
    pub fn check_permission(
        &self,
        user_role: Role,
        resource: &str,
        operation: &str,
    ) -> PermissionResult {
        if !self.config.enabled {
            return PermissionResult::Allowed;
        }

        // Parse resource (e.g., "skills.github" or "tools.filesystem")
        let parts: Vec<&str> = resource.split('.').collect();
        if parts.len() < 2 {
            warn!("Invalid resource format: {}", resource);
            return PermissionResult::Allowed; // Default to allow if format is invalid
        }

        let category = parts[0];
        let name = parts[1];

        match category {
            "skills" => self.check_skill_permission(user_role, name, operation),
            "tools" => self.check_tool_permission(user_role, name, operation),
            _ => {
                warn!("Unknown permission category: {}", category);
                PermissionResult::Allowed // Default to allow
            }
        }
    }

    /// Check skill permission
    fn check_skill_permission(
        &self,
        user_role: Role,
        skill_name: &str,
        operation: &str,
    ) -> PermissionResult {
        let skill_config = match self.config.permissions.skills.get(skill_name) {
            Some(config) => config,
            None => {
                debug!(
                    "No permission config for skill '{}', operation '{}' - allowing",
                    skill_name, operation
                );
                return PermissionResult::Allowed;
            }
        };

        let op_perm = match skill_config.operations.get(operation) {
            Some(perm) => perm,
            None => {
                debug!(
                    "No permission config for skill '{}', operation '{}' - allowing",
                    skill_name, operation
                );
                return PermissionResult::Allowed;
            }
        };

        let min_role = match Role::from_str(&op_perm.min_role) {
            Some(role) => role,
            None => {
                warn!("Invalid min_role '{}' for skill.{}.{}", op_perm.min_role, skill_name, operation);
                return PermissionResult::Allowed;
            }
        };

        if user_role >= min_role {
            PermissionResult::Allowed
        } else {
            PermissionResult::Denied(format!(
                "Operation '{}' on skill '{}' requires role '{}' or higher, but user has role '{}'",
                operation, skill_name, min_role.as_str(), user_role.as_str()
            ))
        }
    }

    /// Check tool permission
    fn check_tool_permission(
        &self,
        user_role: Role,
        tool_name: &str,
        operation: &str,
    ) -> PermissionResult {
        let tool_config = match self.config.permissions.tools.get(tool_name) {
            Some(config) => config,
            None => {
                debug!(
                    "No permission config for tool '{}', operation '{}' - allowing",
                    tool_name, operation
                );
                return PermissionResult::Allowed;
            }
        };

        let op_perm = match tool_config.operations.get(operation) {
            Some(perm) => perm,
            None => {
                debug!(
                    "No permission config for tool '{}', operation '{}' - allowing",
                    tool_name, operation
                );
                return PermissionResult::Allowed;
            }
        };

        let min_role = match Role::from_str(&op_perm.min_role) {
            Some(role) => role,
            None => {
                warn!("Invalid min_role '{}' for tool.{}.{}", op_perm.min_role, tool_name, operation);
                return PermissionResult::Allowed;
            }
        };

        if user_role < min_role {
            return PermissionResult::Denied(format!(
                "Operation '{}' on tool '{}' requires role '{}' or higher, but user has role '{}'",
                operation, tool_name, min_role.as_str(), user_role.as_str()
            ));
        }

        PermissionResult::Allowed
    }

    /// Check path access for filesystem operations
    pub fn check_path_access(
        &self,
        role: Role,
        operation: &str,
        path: &Path,
    ) -> PermissionResult {
        if !self.config.enabled {
            return PermissionResult::Allowed;
        }

        let tool_config = match self.config.permissions.tools.get("filesystem") {
            Some(config) => config,
            None => return PermissionResult::Allowed,
        };

        let op_perm = match tool_config.operations.get(operation) {
            Some(perm) => perm,
            None => return PermissionResult::Allowed,
        };

        // Check blacklist first
        if self.path_matcher.is_blacklisted(path, &op_perm.path_blacklist) {
            return PermissionResult::Denied(format!(
                "Path '{}' is blacklisted for operation '{}'",
                path.display(),
                operation
            ));
        }

        // Check whitelist
        let role_str = role.as_str();
        if let Some(whitelist) = op_perm.path_whitelist.get(role_str) {
            if !self.path_matcher.is_whitelisted(path, whitelist) {
                return PermissionResult::Denied(format!(
                    "Path '{}' is not whitelisted for role '{}' and operation '{}'",
                    path.display(),
                    role_str,
                    operation
                ));
            }
        } else if !op_perm.path_whitelist.is_empty() {
            // If whitelist exists but no entry for this role, deny
            return PermissionResult::Denied(format!(
                "No path whitelist configured for role '{}' and operation '{}'",
                role_str, operation
            ));
        }

        PermissionResult::Allowed
    }

    /// Check command access for shell operations
    pub fn check_command_access(&self, role: Role, command: &str) -> PermissionResult {
        if !self.config.enabled {
            return PermissionResult::Allowed;
        }

        // SuperAdmin has full access
        if role == Role::SuperAdmin {
            return PermissionResult::Allowed;
        }

        let tool_config = match self.config.permissions.tools.get("shell") {
            Some(config) => config,
            None => return PermissionResult::Allowed,
        };

        // Check for full_access permission
        if let Some(op_perm) = tool_config.operations.get("full_access") {
            let min_role = match Role::from_str(&op_perm.min_role) {
                Some(role) => role,
                None => return PermissionResult::Allowed,
            };
            if role >= min_role {
                return PermissionResult::Allowed;
            }
        }

        // Check safe_commands
        if let Some(op_perm) = tool_config.operations.get("safe_commands") {
            let min_role = match Role::from_str(&op_perm.min_role) {
                Some(role) => role,
                None => return PermissionResult::Denied("Invalid safe_commands configuration".to_string()),
            };

            if role < min_role {
                return PermissionResult::Denied(format!(
                    "Command execution requires role '{}' or higher",
                    min_role.as_str()
                ));
            }

            // Check if command matches any allowed pattern
            let command_lower = command.to_lowercase();
            for pattern in &op_perm.allowed {
                if self.matches_command_pattern(&command_lower, pattern) {
                    return PermissionResult::Allowed;
                }
            }

            return PermissionResult::Denied(format!(
                "Command '{}' is not in the allowed list for role '{}'",
                command, role.as_str()
            ));
        }

        PermissionResult::Allowed
    }

    /// Check if command matches a pattern (supports wildcards)
    fn matches_command_pattern(&self, command: &str, pattern: &str) -> bool {
        // Simple wildcard matching
        if pattern.contains('*') {
            let regex_pattern = pattern
                .replace(".", "\\.")
                .replace("*", ".*");
            if let Ok(re) = regex::Regex::new(&format!("^{}$", regex_pattern)) {
                return re.is_match(command);
            }
        }
        command.starts_with(pattern)
    }

    /// Get role from Discord user ID and roles
    pub fn get_role_from_discord(
        &self,
        user_id: &str,
        discord_roles: &[String],
    ) -> Role {
        if !self.config.enabled {
            // Fallback to old behavior
            return self.config.default_role();
        }

        let mapping = match self.config.role_mappings.get("discord") {
            Some(m) => m,
            None => return self.config.default_role(),
        };

        // Check user-specific overrides first (highest priority)
        if let Some(role_str) = mapping.user_overrides.get(user_id) {
            if let Some(role) = Role::from_str(role_str) {
                return role;
            }
        }

        // Map Discord roles to mofa roles (highest role wins)
        let mut max_role = self.config.default_role();

        for discord_role in discord_roles {
            // Check superadmin roles
            if mapping.superadmin_roles.iter().any(|r| r == discord_role) {
                return Role::SuperAdmin;
            }
            // Check admin roles
            if mapping.admin_roles.iter().any(|r| r == discord_role) {
                if max_role < Role::Admin {
                    max_role = Role::Admin;
                }
            }
            // Check member roles
            if mapping.member_roles.iter().any(|r| r == discord_role) {
                if max_role < Role::Member {
                    max_role = Role::Member;
                }
            }
            // Guest roles are default, no need to check
        }

        max_role
    }

    /// Get role from DingTalk user ID and tags
    pub fn get_role_from_dingtalk(
        &self,
        user_id: &str,
        tags: &[String],
    ) -> Role {
        if !self.config.enabled {
            return self.config.default_role();
        }

        let mapping = match self.config.role_mappings.get("dingtalk") {
            Some(m) => m,
            None => return self.config.default_role(),
        };

        // Check user-specific overrides first
        if let Some(role_str) = mapping.user_overrides.get(user_id) {
            if let Some(role) = Role::from_str(role_str) {
                return role;
            }
        }

        // Map tags to roles (highest role wins)
        let mut max_role = self.config.default_role();

        for tag in tags {
            if mapping.superadmin_roles.iter().any(|r| r == tag) {
                return Role::SuperAdmin;
            }
            if mapping.admin_roles.iter().any(|r| r == tag) {
                if max_role < Role::Admin {
                    max_role = Role::Admin;
                }
            }
            if mapping.member_roles.iter().any(|r| r == tag) {
                if max_role < Role::Member {
                    max_role = Role::Member;
                }
            }
        }

        max_role
    }

    /// Get role from Feishu user ID and tags
    pub fn get_role_from_feishu(
        &self,
        user_id: &str,
        tags: &[String],
    ) -> Role {
        // Same logic as DingTalk
        self.get_role_from_dingtalk(user_id, tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbac::config::{ChannelRoleMapping, OperationPermission, PermissionConfig, RbacConfig, RoleDefinition};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn create_test_manager() -> RbacManager {
        let mut config = RbacConfig::default();
        config.enabled = true;
        config.default_role = "guest".to_string();

        // Add role definitions
        config.roles.insert(
            "guest".to_string(),
            RoleDefinition {
                level: 0,
                description: "Guest".to_string(),
            },
        );

        // Add GitHub skill permissions
        let mut github_ops = HashMap::new();
        github_ops.insert(
            "repo.view".to_string(),
            OperationPermission {
                min_role: "guest".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );
        github_ops.insert(
            "repo.create".to_string(),
            OperationPermission {
                min_role: "superadmin".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );

        let mut skills = HashMap::new();
        skills.insert(
            "github".to_string(),
            crate::rbac::config::SkillPermissionConfig {
                operations: github_ops,
            },
        );

        config.permissions = PermissionConfig { skills, tools: HashMap::new() };

        RbacManager::new(config, PathBuf::from("/workspace"), PathBuf::from("/home/user"))
    }

    #[test]
    fn test_check_permission_allowed() {
        let manager = create_test_manager();
        assert_eq!(
            manager.check_permission(Role::Guest, "skills.github", "repo.view"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_check_permission_denied() {
        let manager = create_test_manager();
        match manager.check_permission(Role::Guest, "skills.github", "repo.create") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied"),
        }
    }

    #[test]
    fn test_get_role_from_discord() {
        let mut config = RbacConfig::default();
        config.enabled = true;

        let mut mapping = ChannelRoleMapping::default();
        mapping.admin_roles.push("Admin".to_string());
        mapping.member_roles.push("Member".to_string());
        mapping.user_overrides.insert("123".to_string(), "superadmin".to_string());

        config.role_mappings.insert("discord".to_string(), mapping);

        let manager = RbacManager::new(config, PathBuf::from("/workspace"), PathBuf::from("/home/user"));

        // Test user override
        assert_eq!(
            manager.get_role_from_discord("123", &[]),
            Role::SuperAdmin
        );

        // Test role mapping
        assert_eq!(
            manager.get_role_from_discord("456", &["Admin".to_string()]),
            Role::Admin
        );
    }
}
