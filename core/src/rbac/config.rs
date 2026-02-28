//! RBAC configuration structures

use crate::rbac::role::Role;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// RBAC configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RbacConfig {
    /// Whether RBAC is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Default role for users without explicit role mapping
    #[serde(default = "default_role")]
    pub default_role: String,
    /// Role definitions
    #[serde(default)]
    pub roles: HashMap<String, RoleDefinition>,
    /// Channel-specific role mappings
    #[serde(default)]
    pub role_mappings: HashMap<String, ChannelRoleMapping>,
    /// Permission definitions
    #[serde(default)]
    pub permissions: PermissionConfig,
}

impl Default for RbacConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_role: default_role(),
            roles: HashMap::new(),
            role_mappings: HashMap::new(),
            permissions: PermissionConfig::default(),
        }
    }
}

fn default_role() -> String {
    "guest".to_string()
}

/// Role definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// Role level (0-3)
    pub level: u8,
    /// Human-readable description
    pub description: String,
}

/// Channel-specific role mapping
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelRoleMapping {
    /// Super admin role names/IDs
    #[serde(default)]
    pub superadmin_roles: Vec<String>,
    /// Admin role names/IDs
    #[serde(default)]
    pub admin_roles: Vec<String>,
    /// Member role names/IDs
    #[serde(default)]
    pub member_roles: Vec<String>,
    /// Guest role names/IDs
    #[serde(default)]
    pub guest_roles: Vec<String>,
    /// User-specific role overrides (user_id -> role_name)
    #[serde(default)]
    pub user_overrides: HashMap<String, String>,
}

/// Permission configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionConfig {
    /// Skill permissions
    #[serde(default)]
    pub skills: HashMap<String, SkillPermissionConfig>,
    /// Tool permissions
    #[serde(default)]
    pub tools: HashMap<String, ToolPermissionConfig>,
}

/// Skill permission configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillPermissionConfig {
    /// Operation-level permissions
    #[serde(default)]
    pub operations: HashMap<String, OperationPermission>,
}

/// Tool permission configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolPermissionConfig {
    /// Operation-level permissions (e.g., "read", "write", "delete", "execute")
    #[serde(flatten)]
    pub operations: HashMap<String, OperationPermission>,
}

/// Operation permission definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationPermission {
    /// Minimum required role
    pub min_role: String,
    /// Path whitelist (role -> patterns)
    #[serde(default)]
    pub path_whitelist: HashMap<String, Vec<String>>,
    /// Path blacklist (applies to all roles)
    #[serde(default)]
    pub path_blacklist: Vec<String>,
    /// Allowed commands (for shell operations)
    #[serde(default)]
    pub allowed: Vec<String>,
}

impl RbacConfig {
    /// Get default role
    pub fn default_role(&self) -> Role {
        Role::from_str(&self.default_role).unwrap_or(Role::Guest)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate default role
        if Role::from_str(&self.default_role).is_none() {
            return Err(format!("Invalid default_role: {}", self.default_role));
        }

        // Validate role definitions
        for (name, def) in &self.roles {
            if def.level > 3 {
                return Err(format!("Role '{}' has invalid level: {}", name, def.level));
            }
            if Role::from_str(name).is_none() {
                return Err(format!("Role '{}' is not a valid role name", name));
            }
        }

        // Validate permissions
        for (skill_name, skill_config) in &self.permissions.skills {
            for (op_name, op_perm) in &skill_config.operations {
                if Role::from_str(&op_perm.min_role).is_none() {
                    return Err(format!(
                        "Invalid min_role '{}' for skill.{}.{}",
                        op_perm.min_role, skill_name, op_name
                    ));
                }
            }
        }

        for (tool_name, tool_config) in &self.permissions.tools {
            for (op_name, op_perm) in &tool_config.operations {
                if Role::from_str(&op_perm.min_role).is_none() {
                    return Err(format!(
                        "Invalid min_role '{}' for tool.{}.{}",
                        op_perm.min_role, tool_name, op_name
                    ));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RbacConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.default_role, "guest");
    }

    #[test]
    fn test_config_validation() {
        let mut config = RbacConfig::default();
        config.enabled = true;
        config.default_role = "invalid".to_string();
        assert!(config.validate().is_err());

        config.default_role = "guest".to_string();
        assert!(config.validate().is_ok());
    }
}
