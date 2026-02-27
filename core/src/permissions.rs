//! Role-based permission system for channels (currently Discord).
//!
//! This module centralizes permission tier logic so that role-to-permission
//! mapping and multi-role priority handling live in one place.

use crate::config::DiscordConfig;

/// Permission levels in increasing order of privilege.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionLevel {
    Guest,
    Member,
    Admin,
}

impl PermissionLevel {}

/// Alias used to mirror the `Action` parameter in the issue design.
///
/// For now, actions map directly to the minimum required `PermissionLevel`.
pub type Action = PermissionLevel;

/// Permission manager backed by configured admin and member role IDs.
#[derive(Debug, Clone)]
pub struct PermissionManager {
    admin_roles: Vec<String>,
    member_roles: Vec<String>,
}

impl PermissionManager {
    /// Create a new permission manager from explicit role lists.
    pub fn new(admin_roles: Vec<String>, member_roles: Vec<String>) -> Self {
        Self {
            admin_roles,
            member_roles,
        }
    }

    /// Create a permission manager from a `DiscordConfig`.
    pub fn from_discord_config(config: &DiscordConfig) -> Self {
        Self::new(config.admin_roles.clone(), config.member_roles.clone())
    }

    /// Compute the highest permission level for a user given their roles.
    ///
    /// - If the user has any admin role → `Admin`
    /// - Else if the user has any member role → `Member`
    /// - Else → `Guest`
    pub fn level_for(&self, user_roles: &[String]) -> PermissionLevel {
        if !self.admin_roles.is_empty()
            && user_roles
                .iter()
                .any(|role| self.admin_roles.contains(role))
        {
            return PermissionLevel::Admin;
        }

        if !self.member_roles.is_empty()
            && user_roles
                .iter()
                .any(|role| self.member_roles.contains(role))
        {
            return PermissionLevel::Member;
        }

        PermissionLevel::Guest
    }

    /// Check whether the user's roles satisfy the required permission level.
    pub fn has_level(&self, user_roles: &[String], required: PermissionLevel) -> bool {
        self.level_for(user_roles) >= required
    }

    /// Check permission for a given action (currently aliased to `PermissionLevel`).
    pub fn check_permission(&self, user_roles: &[String], action: Action) -> bool {
        self.has_level(user_roles, action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn level_for_no_roles_is_guest() {
        let mgr = PermissionManager::new(vec!["admin".into()], vec!["member".into()]);
        assert_eq!(mgr.level_for(&roles(&[])), PermissionLevel::Guest);
    }

    #[test]
    fn level_for_member_role_is_member() {
        let mgr = PermissionManager::new(vec!["admin".into()], vec!["member".into()]);
        assert_eq!(
            mgr.level_for(&roles(&["member"])),
            PermissionLevel::Member
        );
    }

    #[test]
    fn level_for_admin_role_is_admin() {
        let mgr = PermissionManager::new(vec!["admin".into()], vec!["member".into()]);
        assert_eq!(mgr.level_for(&roles(&["admin"])), PermissionLevel::Admin);
    }

    #[test]
    fn level_for_both_admin_and_member_prefers_admin() {
        let mgr = PermissionManager::new(vec!["admin".into()], vec!["member".into()]);
        assert_eq!(
            mgr.level_for(&roles(&["member", "admin"])),
            PermissionLevel::Admin
        );
    }

    #[test]
    fn check_permission_respects_hierarchy() {
        let mgr = PermissionManager::new(vec!["admin".into()], vec!["member".into()]);

        let guest = roles(&[]);
        let member = roles(&["member"]);
        let admin = roles(&["admin"]);

        // Guest
        assert!(mgr.check_permission(&guest, PermissionLevel::Guest));
        assert!(!mgr.check_permission(&guest, PermissionLevel::Member));
        assert!(!mgr.check_permission(&guest, PermissionLevel::Admin));

        // Member
        assert!(mgr.check_permission(&member, PermissionLevel::Guest));
        assert!(mgr.check_permission(&member, PermissionLevel::Member));
        assert!(!mgr.check_permission(&member, PermissionLevel::Admin));

        // Admin
        assert!(mgr.check_permission(&admin, PermissionLevel::Guest));
        assert!(mgr.check_permission(&admin, PermissionLevel::Member));
        assert!(mgr.check_permission(&admin, PermissionLevel::Admin));
    }
}


