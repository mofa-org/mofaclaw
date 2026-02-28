//! Example: How to use RBAC in your code
//!
//! This example demonstrates how to integrate RBAC checks into your application.

use mofaclaw_core::{
    rbac::{RbacManager, Role},
    Config,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Example: Check permission before executing a GitHub operation
async fn example_github_operation_check(
    rbac_manager: &Arc<RbacManager>,
    user_role: Role,
    operation: &str,
) -> Result<(), String> {
    match rbac_manager.check_permission(user_role, "skills.github", operation) {
        mofaclaw_core::rbac::manager::PermissionResult::Allowed => {
            println!("Permission granted for operation: {}", operation);
            Ok(())
        }
        mofaclaw_core::rbac::manager::PermissionResult::Denied(reason) => {
            Err(format!("Permission denied: {}", reason))
        }
    }
}

/// Example: Check filesystem access before file operations
async fn example_filesystem_check(
    rbac_manager: &Arc<RbacManager>,
    user_role: Role,
    path: &PathBuf,
    operation: &str,
) -> Result<(), String> {
    match rbac_manager.check_path_access(user_role, operation, path) {
        mofaclaw_core::rbac::manager::PermissionResult::Allowed => {
            println!("Access granted for {} on {}", operation, path.display());
            Ok(())
        }
        mofaclaw_core::rbac::manager::PermissionResult::Denied(reason) => {
            Err(format!("Access denied: {}", reason))
        }
    }
}

/// Example: Check shell command access
async fn example_shell_command_check(
    rbac_manager: &Arc<RbacManager>,
    user_role: Role,
    command: &str,
) -> Result<(), String> {
    match rbac_manager.check_command_access(user_role, command) {
        mofaclaw_core::rbac::manager::PermissionResult::Allowed => {
            println!("Command execution allowed: {}", command);
            Ok(())
        }
        mofaclaw_core::rbac::manager::PermissionResult::Denied(reason) => {
            Err(format!("Command execution denied: {}", reason))
        }
    }
}

/// Example: Resolve user role from Discord
fn example_discord_role_resolution(
    rbac_manager: &Arc<RbacManager>,
    user_id: &str,
    discord_roles: &[String],
) -> Role {
    let role = rbac_manager.get_role_from_discord(user_id, discord_roles);
    println!(
        "User {} with Discord roles {:?} resolved to mofa role: {}",
        user_id, discord_roles, role
    );
    role
}

/// Example: Initialize RBAC from config
fn example_initialize_rbac(config: &Config) -> Option<Arc<RbacManager>> {
    if let Ok(Some(rbac_config)) = config.get_rbac_config() {
        if rbac_config.enabled {
            let workspace = config.workspace_path();
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            let manager = Arc::new(RbacManager::new(rbac_config, workspace, home));
            println!("RBAC initialized and enabled");
            Some(manager)
        } else {
            println!("RBAC is disabled in config");
            None
        }
    } else {
        println!("No RBAC configuration found");
        None
    }
}

#[cfg(test)]
mod example_tests {
    use super::*;
    use mofaclaw_core::rbac::config::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_example_github_operation() {
        // Create a test RBAC manager
        let mut config = RbacConfig::default();
        config.enabled = true;
        config.default_role = "guest".to_string();

        let mut github_ops = HashMap::new();
        github_ops.insert(
            "issue.create".to_string(),
            OperationPermission {
                min_role: "member".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );

        let mut skills = HashMap::new();
        skills.insert(
            "github".to_string(),
            SkillPermissionConfig {
                operations: github_ops,
            },
        );

        config.permissions = PermissionConfig {
            skills,
            tools: HashMap::new(),
        };

        let manager = Arc::new(RbacManager::new(
            config,
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        ));

        // Test: Member can create issues
        assert!(example_github_operation_check(&manager, Role::Member, "issue.create")
            .await
            .is_ok());

        // Test: Guest cannot create issues
        assert!(example_github_operation_check(&manager, Role::Guest, "issue.create")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_example_filesystem_check() {
        let mut config = RbacConfig::default();
        config.enabled = true;
        config.default_role = "guest".to_string();

        let mut fs_write_whitelist = HashMap::new();
        fs_write_whitelist.insert("member".to_string(), vec!["${workspace}/**".to_string()]);

        let mut fs_ops = HashMap::new();
        fs_ops.insert(
            "write".to_string(),
            OperationPermission {
                min_role: "member".to_string(),
                path_whitelist: fs_write_whitelist,
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );

        let mut tools = HashMap::new();
        tools.insert(
            "filesystem".to_string(),
            ToolPermissionConfig { operations: fs_ops },
        );

        config.permissions = PermissionConfig {
            skills: HashMap::new(),
            tools,
        };

        let manager = Arc::new(RbacManager::new(
            config,
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        ));

        let workspace_path = PathBuf::from("/workspace/test.txt");

        // Test: Member can write to workspace
        assert!(example_filesystem_check(&manager, Role::Member, &workspace_path, "write")
            .await
            .is_ok());

        // Test: Guest cannot write
        assert!(example_filesystem_check(&manager, Role::Guest, &workspace_path, "write")
            .await
            .is_err());
    }
}
