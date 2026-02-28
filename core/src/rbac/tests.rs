//! Comprehensive tests for RBAC system

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::rbac::config::*;
    use crate::rbac::manager::PermissionResult;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn create_test_rbac_config() -> RbacConfig {
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
        config.roles.insert(
            "member".to_string(),
            RoleDefinition {
                level: 1,
                description: "Member".to_string(),
            },
        );
        config.roles.insert(
            "admin".to_string(),
            RoleDefinition {
                level: 2,
                description: "Admin".to_string(),
            },
        );
        config.roles.insert(
            "superadmin".to_string(),
            RoleDefinition {
                level: 3,
                description: "SuperAdmin".to_string(),
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
        github_ops.insert(
            "issue.create".to_string(),
            OperationPermission {
                min_role: "member".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );
        github_ops.insert(
            "issue.close".to_string(),
            OperationPermission {
                min_role: "admin".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );
        github_ops.insert(
            "pr.merge".to_string(),
            OperationPermission {
                min_role: "admin".to_string(),
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

        // Add filesystem tool permissions
        let mut fs_read_whitelist = HashMap::new();
        fs_read_whitelist.insert("guest".to_string(), vec!["${workspace}/**".to_string()]);
        fs_read_whitelist.insert("member".to_string(), vec!["${workspace}/**".to_string(), "${home}/projects/**".to_string()]);

        let mut fs_write_whitelist = HashMap::new();
        fs_write_whitelist.insert("member".to_string(), vec!["${workspace}/**".to_string()]);

        let mut fs_ops = HashMap::new();
        fs_ops.insert(
            "read".to_string(),
            OperationPermission {
                min_role: "guest".to_string(),
                path_whitelist: fs_read_whitelist,
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );
        fs_ops.insert(
            "write".to_string(),
            OperationPermission {
                min_role: "member".to_string(),
                path_whitelist: fs_write_whitelist,
                path_blacklist: vec!["**/.env".to_string(), "**/credentials*".to_string()],
                allowed: Vec::new(),
            },
        );
        fs_ops.insert(
            "delete".to_string(),
            OperationPermission {
                min_role: "admin".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );

        let mut tools = HashMap::new();
        tools.insert(
            "filesystem".to_string(),
            ToolPermissionConfig {
                operations: fs_ops,
            },
        );

        // Add shell tool permissions
        let mut shell_ops = HashMap::new();
        shell_ops.insert(
            "safe_commands".to_string(),
            OperationPermission {
                min_role: "member".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: vec!["ls".to_string(), "cat".to_string(), "git status".to_string(), "gh * view".to_string()],
            },
        );
        shell_ops.insert(
            "full_access".to_string(),
            OperationPermission {
                min_role: "superadmin".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );

        tools.insert(
            "shell".to_string(),
            ToolPermissionConfig {
                operations: shell_ops,
            },
        );

        config.permissions = PermissionConfig { skills, tools };

        // Add Discord role mapping
        let mut discord_mapping = ChannelRoleMapping::default();
        discord_mapping.superadmin_roles.push("Owner".to_string());
        discord_mapping.superadmin_roles.push("Bot Admin".to_string());
        discord_mapping.admin_roles.push("Admin".to_string());
        discord_mapping.admin_roles.push("Moderator".to_string());
        discord_mapping.member_roles.push("Member".to_string());
        discord_mapping.member_roles.push("Contributor".to_string());
        discord_mapping.guest_roles.push("@everyone".to_string());
        discord_mapping.user_overrides.insert("123456789012345678".to_string(), "admin".to_string());
        discord_mapping.user_overrides.insert("987654321098765432".to_string(), "superadmin".to_string());

        config.role_mappings.insert("discord".to_string(), discord_mapping);

        config
    }

    fn create_test_manager() -> RbacManager {
        let config = create_test_rbac_config();
        RbacManager::new(
            config,
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        )
    }

    #[test]
    fn test_role_ordering() {
        assert!(Role::Guest < Role::Member);
        assert!(Role::Member < Role::Admin);
        assert!(Role::Admin < Role::SuperAdmin);
    }

    #[test]
    fn test_skill_permission_guest_allowed() {
        let manager = create_test_manager();
        assert_eq!(
            manager.check_permission(Role::Guest, "skills.github", "repo.view"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_skill_permission_guest_denied() {
        let manager = create_test_manager();
        match manager.check_permission(Role::Guest, "skills.github", "repo.create") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied"),
        }
    }

    #[test]
    fn test_skill_permission_member_allowed() {
        let manager = create_test_manager();
        assert_eq!(
            manager.check_permission(Role::Member, "skills.github", "issue.create"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_skill_permission_admin_allowed() {
        let manager = create_test_manager();
        assert_eq!(
            manager.check_permission(Role::Admin, "skills.github", "pr.merge"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_path_access_whitelist() {
        let manager = create_test_manager();
        let path = PathBuf::from("/workspace/test.txt");

        // Guest should be allowed to read from workspace
        assert_eq!(
            manager.check_path_access(Role::Guest, "read", &path),
            PermissionResult::Allowed
        );

        // Guest should be denied write
        match manager.check_path_access(Role::Guest, "write", &path) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied"),
        }
    }

    #[test]
    fn test_path_access_blacklist() {
        let manager = create_test_manager();
        let path = PathBuf::from("/workspace/.env");

        // Even member should be denied writing to .env
        match manager.check_path_access(Role::Member, "write", &path) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for blacklisted path"),
        }
    }

    #[test]
    fn test_command_access_safe_commands() {
        let manager = create_test_manager();

        // Member should be allowed safe commands
        assert_eq!(
            manager.check_command_access(Role::Member, "ls"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_access(Role::Member, "git status"),
            PermissionResult::Allowed
        );

        // Member should be denied unsafe commands
        match manager.check_command_access(Role::Member, "rm -rf /") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for unsafe command"),
        }
    }

    #[test]
    fn test_command_access_superadmin() {
        let manager = create_test_manager();

        // SuperAdmin should have full access
        assert_eq!(
            manager.check_command_access(Role::SuperAdmin, "rm -rf /"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_discord_role_resolution_user_override() {
        let manager = create_test_manager();

        // User override should take precedence
        assert_eq!(
            manager.get_role_from_discord("123456789012345678", &[]),
            Role::Admin
        );
        assert_eq!(
            manager.get_role_from_discord("987654321098765432", &[]),
            Role::SuperAdmin
        );
    }

    #[test]
    fn test_discord_role_resolution_role_mapping() {
        let manager = create_test_manager();

        // Role mapping should work
        assert_eq!(
            manager.get_role_from_discord("456", &["Admin".to_string()]),
            Role::Admin
        );
        assert_eq!(
            manager.get_role_from_discord("789", &["Member".to_string()]),
            Role::Member
        );
        assert_eq!(
            manager.get_role_from_discord("999", &["Owner".to_string()]),
            Role::SuperAdmin
        );
    }

    #[test]
    fn test_discord_role_resolution_highest_wins() {
        let manager = create_test_manager();

        // If user has multiple roles, highest should win
        assert_eq!(
            manager.get_role_from_discord("456", &["Member".to_string(), "Admin".to_string()]),
            Role::Admin
        );
    }

    #[test]
    fn test_config_validation() {
        let config = create_test_rbac_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_role() {
        let mut config = create_test_rbac_config();
        config.default_role = "invalid".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_path_matcher_expand_variables() {
        let matcher = PathMatcher::new(
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        );

        assert_eq!(
            matcher.expand_variables("${workspace}/test"),
            "/workspace/test"
        );
        assert_eq!(
            matcher.expand_variables("${home}/projects"),
            "/home/user/projects"
        );
    }

    #[test]
    fn test_path_matcher_glob_patterns() {
        let matcher = PathMatcher::new(
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        );

        let path = PathBuf::from("/workspace/test.txt");
        assert!(matcher.matches(&path, &["${workspace}/**".to_string()]));
        assert!(matcher.matches(&path, &["**/*.txt".to_string()]));
        assert!(!matcher.matches(&path, &["${workspace}/src/**".to_string()]));
    }

    #[test]
    fn test_rbac_disabled_allows_all() {
        let mut config = create_test_rbac_config();
        config.enabled = false;
        let manager = RbacManager::new(
            config,
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        );

        // When RBAC is disabled, all operations should be allowed
        assert_eq!(
            manager.check_permission(Role::Guest, "skills.github", "repo.create"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_path_access(Role::Guest, "write", &PathBuf::from("/any/path")),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_access(Role::Guest, "rm -rf /"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_missing_permission_config_allows() {
        let manager = create_test_manager();

        // Operations without config should default to allowed (with warning)
        assert_eq!(
            manager.check_permission(Role::Guest, "skills.unknown", "operation"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_permission(Role::Guest, "tools.unknown", "operation"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_path_blacklist_takes_precedence() {
        let manager = create_test_manager();
        let path = PathBuf::from("/workspace/.env");

        // Even though workspace is whitelisted, .env is blacklisted
        match manager.check_path_access(Role::Member, "write", &path) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for blacklisted path"),
        }
    }

    #[test]
    fn test_path_whitelist_role_specific() {
        let manager = create_test_manager();
        let workspace_path = PathBuf::from("/workspace/test.txt");
        let home_project_path = PathBuf::from("/home/user/projects/test.txt");

        // Guest can read from workspace
        assert_eq!(
            manager.check_path_access(Role::Guest, "read", &workspace_path),
            PermissionResult::Allowed
        );

        // Guest cannot read from home/projects
        match manager.check_path_access(Role::Guest, "read", &home_project_path) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for guest accessing home/projects"),
        }

        // Member can read from both
        assert_eq!(
            manager.check_path_access(Role::Member, "read", &workspace_path),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_path_access(Role::Member, "read", &home_project_path),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_command_pattern_matching() {
        let manager = create_test_manager();

        // Exact match
        assert_eq!(
            manager.check_command_access(Role::Member, "ls"),
            PermissionResult::Allowed
        );

        // Wildcard pattern matching
        assert_eq!(
            manager.check_command_access(Role::Member, "gh repo view"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_access(Role::Member, "gh issue view"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_access(Role::Member, "gh pr view"),
            PermissionResult::Allowed
        );

        // Command not in whitelist
        match manager.check_command_access(Role::Member, "rm -rf /") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for unsafe command"),
        }
    }

    #[test]
    fn test_command_case_insensitive() {
        let manager = create_test_manager();

        // Commands should be case-insensitive (lowercased before matching)
        assert_eq!(
            manager.check_command_access(Role::Member, "LS"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_access(Role::Member, "Git Status"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_role_hierarchy() {
        let manager = create_test_manager();

        // Admin should have access to member operations
        assert_eq!(
            manager.check_permission(Role::Admin, "skills.github", "issue.create"),
            PermissionResult::Allowed
        );

        // Admin should have access to admin operations
        assert_eq!(
            manager.check_permission(Role::Admin, "skills.github", "pr.merge"),
            PermissionResult::Allowed
        );

        // Member should not have access to admin operations
        match manager.check_permission(Role::Member, "skills.github", "pr.merge") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for member accessing admin operation"),
        }
    }

    #[test]
    fn test_discord_role_resolution_no_roles() {
        let manager = create_test_manager();

        // User with no roles should get default role
        assert_eq!(
            manager.get_role_from_discord("unknown", &[]),
            Role::Guest
        );
    }

    #[test]
    fn test_discord_role_resolution_multiple_roles() {
        let manager = create_test_manager();

        // User with multiple roles should get highest
        assert_eq!(
            manager.get_role_from_discord("456", &["Member".to_string(), "Admin".to_string(), "Contributor".to_string()]),
            Role::Admin
        );
    }

    #[test]
    fn test_discord_user_override_overrides_roles() {
        let manager = create_test_manager();

        // User override should take precedence even if user has lower roles
        assert_eq!(
            manager.get_role_from_discord("123456789012345678", &["Member".to_string()]),
            Role::Admin  // Override takes precedence
        );
    }

    #[test]
    fn test_path_matcher_various_paths() {
        let matcher = PathMatcher::new(
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        );

        // Test with absolute paths
        let abs_path = PathBuf::from("/workspace/test.txt");
        assert!(matcher.matches(&abs_path, &["${workspace}/**".to_string()]));
        
        // Test nested paths
        let nested_path = PathBuf::from("/workspace/src/main.rs");
        assert!(matcher.matches(&nested_path, &["${workspace}/**".to_string()]));
        
        // Test workspace root with exact match pattern
        let workspace_path = PathBuf::from("/workspace");
        assert!(matcher.matches(&workspace_path, &["${workspace}".to_string()]));
        
        // Test subdirectory
        let subdir_path = PathBuf::from("/workspace/subdir/file.txt");
        assert!(matcher.matches(&subdir_path, &["${workspace}/**".to_string()]));
    }

    #[test]
    fn test_path_matcher_multiple_patterns() {
        let matcher = PathMatcher::new(
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        );

        let path = PathBuf::from("/workspace/src/main.rs");
        let patterns = vec![
            "${workspace}/**".to_string(),
            "**/*.rs".to_string(),
            "/workspace/src/**".to_string(),
        ];

        // Should match if any pattern matches
        assert!(matcher.matches(&path, &patterns));
    }

    #[test]
    fn test_config_validation_missing_role_definition() {
        let mut config = create_test_rbac_config();
        // Remove a role definition but keep it in permissions - should still validate
        // (roles are optional, defaults are used if not defined)
        config.roles.remove("guest");
        // Config should still validate - missing role definitions use defaults
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_min_role() {
        let mut config = create_test_rbac_config();
        config.permissions.skills.get_mut("github").unwrap()
            .operations.get_mut("repo.view").unwrap()
            .min_role = "invalid_role".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_superadmin_bypasses_all_restrictions() {
        let manager = create_test_manager();

        // SuperAdmin should bypass command restrictions
        assert_eq!(
            manager.check_command_access(Role::SuperAdmin, "rm -rf /"),
            PermissionResult::Allowed
        );

        // SuperAdmin should have access to all skill operations
        assert_eq!(
            manager.check_permission(Role::SuperAdmin, "skills.github", "repo.create"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_permission(Role::SuperAdmin, "skills.github", "repo.delete"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_guest_restricted_operations() {
        let manager = create_test_manager();

        // Guest should be denied write operations
        match manager.check_path_access(Role::Guest, "write", &PathBuf::from("/workspace/test.txt")) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for guest write operation"),
        }

        // Guest should be denied shell execution
        match manager.check_command_access(Role::Guest, "ls") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for guest shell execution"),
        }
    }

    #[test]
    fn test_integration_github_workflow() {
        let manager = create_test_manager();

        // Simulate a typical GitHub workflow
        // 1. Guest views issues (allowed)
        assert_eq!(
            manager.check_permission(Role::Guest, "skills.github", "issue.list"),
            PermissionResult::Allowed
        );

        // 2. Member creates issue (allowed)
        assert_eq!(
            manager.check_permission(Role::Member, "skills.github", "issue.create"),
            PermissionResult::Allowed
        );

        // 3. Admin merges PR (allowed)
        assert_eq!(
            manager.check_permission(Role::Admin, "skills.github", "pr.merge"),
            PermissionResult::Allowed
        );

        // 4. Only SuperAdmin can delete repo (allowed)
        assert_eq!(
            manager.check_permission(Role::SuperAdmin, "skills.github", "repo.delete"),
            PermissionResult::Allowed
        );

        // 5. Member cannot merge PR (denied)
        match manager.check_permission(Role::Member, "skills.github", "pr.merge") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for member merging PR"),
        }
    }

    #[test]
    fn test_integration_filesystem_workflow() {
        let manager = create_test_manager();

        let safe_path = PathBuf::from("/workspace/src/main.rs");
        let sensitive_path = PathBuf::from("/workspace/.env");
        let restricted_path = PathBuf::from("/etc/passwd");

        // Guest can read from workspace
        assert_eq!(
            manager.check_path_access(Role::Guest, "read", &safe_path),
            PermissionResult::Allowed
        );

        // Guest cannot write
        match manager.check_path_access(Role::Guest, "write", &safe_path) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for guest write"),
        }

        // Member can write to workspace (but not sensitive files)
        assert_eq!(
            manager.check_path_access(Role::Member, "write", &safe_path),
            PermissionResult::Allowed
        );

        // Member cannot write to blacklisted files
        match manager.check_path_access(Role::Member, "write", &sensitive_path) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for blacklisted path"),
        }

        // Member cannot write to non-whitelisted paths
        match manager.check_path_access(Role::Member, "write", &restricted_path) {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for non-whitelisted path"),
        }
    }

    #[test]
    fn test_integration_shell_workflow() {
        let manager = create_test_manager();

        // Guest cannot execute any commands
        match manager.check_command_access(Role::Guest, "ls") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for guest command execution"),
        }

        // Member can execute safe commands
        assert_eq!(
            manager.check_command_access(Role::Member, "ls"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_access(Role::Member, "git status"),
            PermissionResult::Allowed
        );

        // Member cannot execute unsafe commands
        match manager.check_command_access(Role::Member, "rm -rf /") {
            PermissionResult::Denied(_) => {}
            _ => panic!("Expected denied for unsafe command"),
        }

        // SuperAdmin can execute anything
        assert_eq!(
            manager.check_command_access(Role::SuperAdmin, "rm -rf /"),
            PermissionResult::Allowed
        );
    }
}
