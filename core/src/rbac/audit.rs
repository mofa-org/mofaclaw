//! Audit logging for RBAC permission checks

use crate::rbac::role::Role;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// Timestamp of the check
    pub timestamp: DateTime<Utc>,
    /// User ID
    pub user_id: String,
    /// User role
    pub role: String,
    /// Resource (e.g., "skills.github", "tools.filesystem")
    pub resource: String,
    /// Operation (e.g., "issue.create", "read", "write")
    pub operation: String,
    /// Result: "allowed" or "denied"
    pub result: String,
    /// Optional reason for denial
    pub reason: Option<String>,
}

/// Audit logger for RBAC operations
pub struct AuditLogger {
    sender: mpsc::UnboundedSender<AuditLogEntry>,
}

impl AuditLogger {
    /// Create a new audit logger
    pub fn new() -> (Self, mpsc::UnboundedReceiver<AuditLogEntry>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, receiver)
    }

    /// Log a permission check
    pub fn log(
        &self,
        user_id: &str,
        role: Role,
        resource: &str,
        operation: &str,
        result: &crate::rbac::manager::PermissionResult,
    ) {
        let entry = AuditLogEntry {
            timestamp: Utc::now(),
            user_id: user_id.to_string(),
            role: role.as_str().to_string(),
            resource: resource.to_string(),
            operation: operation.to_string(),
            result: match result {
                crate::rbac::manager::PermissionResult::Allowed => "allowed".to_string(),
                crate::rbac::manager::PermissionResult::Denied(_) => "denied".to_string(),
            },
            reason: match result {
                crate::rbac::manager::PermissionResult::Allowed => None,
                crate::rbac::manager::PermissionResult::Denied(reason) => Some(reason.clone()),
            },
        };

        if let Err(e) = self.sender.send(entry.clone()) {
            error!("Failed to send audit log entry: {}", e);
        } else {
            debug!(
                "Audit: user={} role={} resource={} operation={} result={}",
                entry.user_id, entry.role, entry.resource, entry.operation, entry.result
            );
        }
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        let (sender, _) = mpsc::unbounded_channel();
        Self { sender }
    }
}

/// Background task to process audit logs
pub async fn process_audit_logs(mut receiver: mpsc::UnboundedReceiver<AuditLogEntry>) {
    // For now, just log to tracing
    // In the future, this could write to a file or database
    while let Some(entry) = receiver.recv().await {
        tracing::info!(
            "RBAC Audit: user={} role={} resource={} operation={} result={} reason={:?}",
            entry.user_id,
            entry.role,
            entry.resource,
            entry.operation,
            entry.result,
            entry.reason
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audit_logger() {
        let (logger, mut receiver) = AuditLogger::new();
        let role = Role::Member;

        logger.log(
            "user123",
            role,
            "tools.filesystem",
            "write",
            &crate::rbac::manager::PermissionResult::Allowed,
        );

        let entry = receiver.recv().await.unwrap();
        assert_eq!(entry.user_id, "user123");
        assert_eq!(entry.role, "member");
        assert_eq!(entry.resource, "tools.filesystem");
        assert_eq!(entry.operation, "write");
        assert_eq!(entry.result, "allowed");
    }
}
