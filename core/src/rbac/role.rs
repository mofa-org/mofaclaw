//! Role definitions and conversions

use crate::permissions::PermissionLevel;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Role levels in increasing order of privilege
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Guest - read-only, non-sensitive operations only
    Guest = 0,
    /// Member - standard operations, limited write access
    Member = 1,
    /// Admin - full access to most operations
    Admin = 2,
    /// Super Admin - unrestricted system-level access
    SuperAdmin = 3,
}

impl Role {
    /// Get role level as integer
    pub fn level(&self) -> u8 {
        *self as u8
    }

    /// Parse role from string (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "guest" => Some(Role::Guest),
            "member" => Some(Role::Member),
            "admin" => Some(Role::Admin),
            "superadmin" | "super_admin" => Some(Role::SuperAdmin),
            _ => None,
        }
    }

    /// Get role name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Guest => "guest",
            Role::Member => "member",
            Role::Admin => "admin",
            Role::SuperAdmin => "superadmin",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Convert from old PermissionLevel for backward compatibility
impl From<PermissionLevel> for Role {
    fn from(level: PermissionLevel) -> Self {
        match level {
            PermissionLevel::Guest => Role::Guest,
            PermissionLevel::Member => Role::Member,
            PermissionLevel::Admin => Role::Admin,
        }
    }
}

/// Convert to old PermissionLevel for backward compatibility
impl From<Role> for PermissionLevel {
    fn from(role: Role) -> Self {
        match role {
            Role::Guest => PermissionLevel::Guest,
            Role::Member => PermissionLevel::Member,
            Role::Admin | Role::SuperAdmin => PermissionLevel::Admin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_ordering() {
        assert!(Role::Guest < Role::Member);
        assert!(Role::Member < Role::Admin);
        assert!(Role::Admin < Role::SuperAdmin);
        assert!(Role::Guest <= Role::Guest);
        assert!(Role::SuperAdmin >= Role::Guest);
    }

    #[test]
    fn test_role_from_str() {
        assert_eq!(Role::from_str("guest"), Some(Role::Guest));
        assert_eq!(Role::from_str("GUEST"), Some(Role::Guest));
        assert_eq!(Role::from_str("member"), Some(Role::Member));
        assert_eq!(Role::from_str("admin"), Some(Role::Admin));
        assert_eq!(Role::from_str("superadmin"), Some(Role::SuperAdmin));
        assert_eq!(Role::from_str("super_admin"), Some(Role::SuperAdmin));
        assert_eq!(Role::from_str("invalid"), None);
    }

    #[test]
    fn test_role_conversion() {
        assert_eq!(Role::from(PermissionLevel::Guest), Role::Guest);
        assert_eq!(Role::from(PermissionLevel::Member), Role::Member);
        assert_eq!(Role::from(PermissionLevel::Admin), Role::Admin);
    }

    #[test]
    fn test_role_display() {
        assert_eq!(format!("{}", Role::Guest), "guest");
        assert_eq!(format!("{}", Role::SuperAdmin), "superadmin");
    }
}
