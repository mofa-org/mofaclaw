//! Permission-based tool execution layer
//!
//! Centralizes permission enforcement for all tool executions by integrating
//! the tool registry with the RBAC system. Each tool declares a minimum
//! permission requirement; the registry checks the caller's role before
//! executing.

use crate::error::{Result, ToolError};
use crate::rbac::manager::PermissionResult;
use crate::rbac::role::Role;
use crate::rbac::{AuditLogger, RbacManager};
use crate::tools::registry::ToolRegistry;
use mofa_sdk::llm::Tool as MofaTool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Permission requirement metadata
// ---------------------------------------------------------------------------

/// The minimum role required to execute a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionRequirement {
    /// Minimum role required for execution
    pub min_role: Role,
    /// Whether the tool performs dangerous/destructive operations
    pub dangerous: bool,
}

impl ToolPermissionRequirement {
    pub fn new(min_role: Role, dangerous: bool) -> Self {
        Self {
            min_role,
            dangerous,
        }
    }
}

/// Return the built-in default permission mapping for all known tools.
///
/// These defaults follow the principle of least privilege:
/// - Read-only tools → Guest
/// - Standard write tools → Member
/// - Destructive / system-level tools → Admin
pub fn default_tool_permissions() -> HashMap<String, ToolPermissionRequirement> {
    let mut m = HashMap::new();

    // Filesystem — read
    m.insert(
        "read_file".to_string(),
        ToolPermissionRequirement::new(Role::Guest, false),
    );
    m.insert(
        "list_dir".to_string(),
        ToolPermissionRequirement::new(Role::Guest, false),
    );

    // Filesystem — write
    m.insert(
        "write_file".to_string(),
        ToolPermissionRequirement::new(Role::Member, false),
    );
    m.insert(
        "edit_file".to_string(),
        ToolPermissionRequirement::new(Role::Member, false),
    );

    // Shell
    m.insert(
        "exec".to_string(),
        ToolPermissionRequirement::new(Role::Admin, true),
    );

    // Web
    m.insert(
        "web_search".to_string(),
        ToolPermissionRequirement::new(Role::Member, false),
    );
    m.insert(
        "web_fetch".to_string(),
        ToolPermissionRequirement::new(Role::Member, false),
    );

    // Spawn subagent
    m.insert(
        "spawn".to_string(),
        ToolPermissionRequirement::new(Role::Admin, true),
    );

    // Message
    m.insert(
        "message".to_string(),
        ToolPermissionRequirement::new(Role::Member, false),
    );

    m
}

// ---------------------------------------------------------------------------
// Permission-aware registry wrapper
// ---------------------------------------------------------------------------

/// A wrapper around `ToolRegistry` that enforces RBAC permission checks
/// before every tool execution.
///
/// When RBAC is disabled (or no `RbacManager` is provided) all calls
/// pass through without restriction.
pub struct PermissionAwareRegistry {
    inner: Arc<RwLock<ToolRegistry>>,
    rbac_manager: Option<Arc<RbacManager>>,
    audit_logger: Option<Arc<AuditLogger>>,
    user_role: Role,
    user_id: String,
    /// Per-tool minimum role requirements.
    requirements: HashMap<String, ToolPermissionRequirement>,
}

impl PermissionAwareRegistry {
    /// Create a new permission-aware registry wrapping an existing registry.
    pub fn new(
        inner: Arc<RwLock<ToolRegistry>>,
        rbac_manager: Option<Arc<RbacManager>>,
        audit_logger: Option<Arc<AuditLogger>>,
        user_role: Role,
        user_id: String,
    ) -> Self {
        Self {
            inner,
            rbac_manager,
            audit_logger,
            user_role,
            user_id,
            requirements: default_tool_permissions(),
        }
    }

    /// Override or extend the default permission requirements.
    pub fn with_requirements(mut self, reqs: HashMap<String, ToolPermissionRequirement>) -> Self {
        for (k, v) in reqs {
            self.requirements.insert(k, v);
        }
        self
    }

    /// Check whether the current user may execute the given tool.
    ///
    /// Returns `Ok(())` on success, `Err(ToolError::PermissionDenied)` on failure.
    pub fn check_permission(&self, tool_name: &str) -> Result<()> {
        // 1. If the RBAC manager says "check_tool_permission" for a
        //    specific operation config, honour that first.
        if let Some(ref rbac) = self.rbac_manager {
            let result =
                rbac.check_permission(self.user_role, &format!("tools.{}", tool_name), "execute");

            // Audit if logger is present
            if let Some(ref logger) = self.audit_logger {
                logger.log(
                    &self.user_id,
                    self.user_role,
                    &format!("tools.{}", tool_name),
                    "execute",
                    &result,
                );
            }

            match result {
                PermissionResult::Allowed => return Ok(()),
                PermissionResult::Denied(reason) => {
                    return Err(ToolError::PermissionDenied(reason).into());
                }
            }
        }

        // 2. Fallback to built-in default requirements.
        if let Some(req) = self.requirements.get(tool_name) {
            if self.user_role >= req.min_role {
                debug!(
                    "Tool '{}' allowed for role '{}' (min: '{}')",
                    tool_name,
                    self.user_role.as_str(),
                    req.min_role.as_str()
                );
                return Ok(());
            }
            let reason = format!(
                "Tool '{}' requires role '{}' or higher, but user '{}' has role '{}'",
                tool_name,
                req.min_role.as_str(),
                self.user_id,
                self.user_role.as_str()
            );
            warn!("{}", reason);
            return Err(ToolError::PermissionDenied(reason).into());
        }

        // 3. Unknown tools default to allowed (with a debug message).
        debug!(
            "No permission requirement configured for tool '{}' — allowing",
            tool_name
        );
        Ok(())
    }

    /// Execute a tool by name, enforcing permission checks first.
    pub async fn execute(&self, name: &str, params: &HashMap<String, Value>) -> Result<String> {
        self.check_permission(name)?;

        let registry = self.inner.read().await;
        registry.execute(name, params).await
    }

    /// Get tool definitions, filtered to only include tools the user can access.
    pub async fn get_permitted_definitions(&self) -> Vec<Value> {
        let registry = self.inner.read().await;
        registry
            .get_definitions()
            .into_iter()
            .filter(|def| {
                let name = def
                    .pointer("/function/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                self.check_permission(name).is_ok()
            })
            .collect()
    }

    /// Get tool names that the user is permitted to use.
    pub async fn get_permitted_tool_names(&self) -> Vec<String> {
        let registry = self.inner.read().await;
        registry
            .tool_names()
            .into_iter()
            .filter(|name| self.check_permission(name).is_ok())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// ToolExecutor implementation for PermissionAwareRegistry
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl mofa_sdk::llm::ToolExecutor for PermissionAwareRegistry {
    async fn execute(&self, name: &str, arguments: &str) -> mofa_sdk::llm::LLMResult<String> {
        // Permission check
        self.check_permission(name)
            .map_err(|e| mofa_sdk::llm::LLMError::Other(format!("Permission denied: {}", e)))?;

        let registry = self.inner.read().await;

        let value: Value =
            serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!({}));

        let params: HashMap<String, Value> = value
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        registry
            .execute(name, &params)
            .await
            .map_err(|e| mofa_sdk::llm::LLMError::Other(format!("Tool execution failed: {}", e)))
    }

    async fn available_tools(&self) -> mofa_sdk::llm::LLMResult<Vec<MofaTool>> {
        let registry = self.inner.read().await;
        let all_tools = mofa_sdk::agent::ToolRegistry::list(registry.inner());

        let permitted: Vec<MofaTool> = all_tools
            .iter()
            .filter(|t| self.check_permission(&t.name).is_ok())
            .map(|t| MofaTool::function(&t.name, &t.description, t.parameters_schema.clone()))
            .collect();

        Ok(permitted)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbac::config::*;
    use crate::tools::registry::ToolRegistry;
    use std::path::PathBuf;

    /// Helper: build an RbacManager with known tool permission configs.
    fn make_rbac_manager(tool_configs: HashMap<String, ToolPermissionConfig>) -> Arc<RbacManager> {
        let mut config = RbacConfig::default();
        config.enabled = true;
        config.default_role = "guest".to_string();
        config.permissions = PermissionConfig {
            skills: HashMap::new(),
            tools: tool_configs,
        };
        Arc::new(RbacManager::new(
            config,
            PathBuf::from("/workspace"),
            PathBuf::from("/home/user"),
        ))
    }

    fn empty_registry() -> Arc<RwLock<ToolRegistry>> {
        Arc::new(RwLock::new(ToolRegistry::new()))
    }

    // -- Default permission map tests --

    #[test]
    fn test_default_permissions_exist_for_all_tools() {
        let map = default_tool_permissions();
        let expected = [
            "read_file",
            "list_dir",
            "write_file",
            "edit_file",
            "exec",
            "web_search",
            "web_fetch",
            "spawn",
            "message",
        ];
        for name in &expected {
            assert!(map.contains_key(*name), "Missing default for '{}'", name);
        }
    }

    #[test]
    fn test_default_permissions_least_privilege() {
        let map = default_tool_permissions();
        assert_eq!(map["read_file"].min_role, Role::Guest);
        assert_eq!(map["list_dir"].min_role, Role::Guest);
        assert_eq!(map["write_file"].min_role, Role::Member);
        assert_eq!(map["edit_file"].min_role, Role::Member);
        assert_eq!(map["exec"].min_role, Role::Admin);
        assert_eq!(map["spawn"].min_role, Role::Admin);
        assert_eq!(map["web_search"].min_role, Role::Member);
        assert_eq!(map["web_fetch"].min_role, Role::Member);
        assert_eq!(map["message"].min_role, Role::Member);
    }

    #[test]
    fn test_dangerous_flags() {
        let map = default_tool_permissions();
        assert!(!map["read_file"].dangerous);
        assert!(!map["write_file"].dangerous);
        assert!(map["exec"].dangerous);
        assert!(map["spawn"].dangerous);
    }

    // -- PermissionAwareRegistry (no RBAC manager — fallback) --

    #[test]
    fn test_fallback_guest_allowed_read() {
        let reg =
            PermissionAwareRegistry::new(empty_registry(), None, None, Role::Guest, "user1".into());
        assert!(reg.check_permission("read_file").is_ok());
        assert!(reg.check_permission("list_dir").is_ok());
    }

    #[test]
    fn test_fallback_guest_denied_write() {
        let reg =
            PermissionAwareRegistry::new(empty_registry(), None, None, Role::Guest, "user1".into());
        assert!(reg.check_permission("write_file").is_err());
        assert!(reg.check_permission("edit_file").is_err());
        assert!(reg.check_permission("exec").is_err());
    }

    #[test]
    fn test_fallback_member_allowed_write() {
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            None,
            None,
            Role::Member,
            "user2".into(),
        );
        assert!(reg.check_permission("write_file").is_ok());
        assert!(reg.check_permission("web_search").is_ok());
        assert!(reg.check_permission("message").is_ok());
    }

    #[test]
    fn test_fallback_member_denied_admin_tools() {
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            None,
            None,
            Role::Member,
            "user2".into(),
        );
        assert!(reg.check_permission("exec").is_err());
        assert!(reg.check_permission("spawn").is_err());
    }

    #[test]
    fn test_fallback_admin_allowed_all_defaults() {
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            None,
            None,
            Role::Admin,
            "admin1".into(),
        );
        for name in &[
            "read_file",
            "list_dir",
            "write_file",
            "edit_file",
            "exec",
            "web_search",
            "web_fetch",
            "spawn",
            "message",
        ] {
            assert!(
                reg.check_permission(name).is_ok(),
                "admin denied '{}'",
                name
            );
        }
    }

    #[test]
    fn test_unknown_tool_allowed() {
        let reg =
            PermissionAwareRegistry::new(empty_registry(), None, None, Role::Guest, "user1".into());
        // Tools without any requirement default to allowed
        assert!(reg.check_permission("my_custom_tool").is_ok());
    }

    // -- With RbacManager --

    #[test]
    fn test_rbac_exec_permission_denied_via_config() {
        // RBAC config says "exec" execute requires admin
        let mut exec_ops: HashMap<String, OperationPermission> = HashMap::new();
        exec_ops.insert(
            "execute".to_string(),
            OperationPermission {
                min_role: "admin".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );
        let mut tool_configs = HashMap::new();
        tool_configs.insert(
            "exec".to_string(),
            ToolPermissionConfig {
                operations: exec_ops,
            },
        );

        let rbac = make_rbac_manager(tool_configs);
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            Some(rbac),
            None,
            Role::Guest,
            "guest1".into(),
        );

        assert!(reg.check_permission("exec").is_err());
    }

    #[test]
    fn test_rbac_exec_permission_allowed_for_admin() {
        let mut exec_ops: HashMap<String, OperationPermission> = HashMap::new();
        exec_ops.insert(
            "execute".to_string(),
            OperationPermission {
                min_role: "admin".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );
        let mut tool_configs = HashMap::new();
        tool_configs.insert(
            "exec".to_string(),
            ToolPermissionConfig {
                operations: exec_ops,
            },
        );

        let rbac = make_rbac_manager(tool_configs);
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            Some(rbac),
            None,
            Role::Admin,
            "admin1".into(),
        );

        assert!(reg.check_permission("exec").is_ok());
    }

    #[test]
    fn test_rbac_unconfigured_tool_allows() {
        // RBAC is enabled but has no config for "read_file"
        let rbac = make_rbac_manager(HashMap::new());
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            Some(rbac),
            None,
            Role::Guest,
            "guest1".into(),
        );

        // RbacManager returns Allowed for unconfigured tools
        assert!(reg.check_permission("read_file").is_ok());
    }

    // -- Custom requirements override --

    #[test]
    fn test_custom_requirements_override() {
        let mut custom = HashMap::new();
        custom.insert(
            "read_file".to_string(),
            ToolPermissionRequirement::new(Role::Admin, false),
        );

        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            None,
            None,
            Role::Member,
            "user1".into(),
        )
        .with_requirements(custom);

        // read_file now requires Admin
        assert!(reg.check_permission("read_file").is_err());
    }

    // -- Filtered tool listing --

    #[tokio::test]
    async fn test_get_permitted_tool_names() {
        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        {
            let mut guard = registry.write().await;
            guard.register(crate::tools::filesystem::ReadFileTool::new());
            guard.register(crate::tools::shell::ExecTool::new());
        }

        let reg = PermissionAwareRegistry::new(registry, None, None, Role::Guest, "guest1".into());

        let names = reg.get_permitted_tool_names().await;
        assert!(names.contains(&"read_file".to_string()));
        assert!(!names.contains(&"exec".to_string()));
    }

    #[tokio::test]
    async fn test_get_permitted_definitions_filters() {
        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        {
            let mut guard = registry.write().await;
            guard.register(crate::tools::filesystem::ReadFileTool::new());
            guard.register(crate::tools::shell::ExecTool::new());
        }

        let reg = PermissionAwareRegistry::new(registry, None, None, Role::Guest, "guest1".into());

        let defs = reg.get_permitted_definitions().await;
        let names: Vec<&str> = defs
            .iter()
            .filter_map(|d| d.pointer("/function/name").and_then(|v| v.as_str()))
            .collect();

        assert!(names.contains(&"read_file"));
        assert!(!names.contains(&"exec"));
    }

    // -- Audit logging --

    #[test]
    fn test_audit_logging_on_permission_check() {
        let mut exec_ops: HashMap<String, OperationPermission> = HashMap::new();
        exec_ops.insert(
            "execute".to_string(),
            OperationPermission {
                min_role: "admin".to_string(),
                path_whitelist: HashMap::new(),
                path_blacklist: Vec::new(),
                allowed: Vec::new(),
            },
        );
        let mut tool_configs = HashMap::new();
        tool_configs.insert(
            "exec".to_string(),
            ToolPermissionConfig {
                operations: exec_ops,
            },
        );

        let rbac = make_rbac_manager(tool_configs);
        let (audit, mut rx) = AuditLogger::new();

        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            Some(rbac),
            Some(Arc::new(audit)),
            Role::Guest,
            "guest1".into(),
        );

        let _ = reg.check_permission("exec");

        // Verify an audit entry was recorded
        let entry = rx.try_recv().expect("Expected audit log entry");
        assert_eq!(entry.user_id, "guest1");
        assert_eq!(entry.resource, "tools.exec");
        assert_eq!(entry.operation, "execute");
        assert_eq!(entry.result, "denied");
    }

    #[test]
    fn test_permission_denied_error_message() {
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            None,
            None,
            Role::Guest,
            "guest1".into(),
        );

        let err = reg.check_permission("exec").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Permission denied"), "got: {}", msg);
        assert!(msg.contains("exec"), "got: {}", msg);
        assert!(msg.contains("admin"), "got: {}", msg);
    }

    // -- Role hierarchy tests --

    #[test]
    fn test_superadmin_has_access_to_everything() {
        let reg = PermissionAwareRegistry::new(
            empty_registry(),
            None,
            None,
            Role::SuperAdmin,
            "superadmin1".into(),
        );
        for name in &[
            "read_file",
            "list_dir",
            "write_file",
            "edit_file",
            "exec",
            "web_search",
            "web_fetch",
            "spawn",
            "message",
        ] {
            assert!(
                reg.check_permission(name).is_ok(),
                "SuperAdmin denied '{}'",
                name,
            );
        }
    }

    // -- Integration: execute with permission --

    #[tokio::test]
    async fn test_execute_denied_returns_error() {
        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        {
            let mut guard = registry.write().await;
            guard.register(crate::tools::shell::ExecTool::new());
        }

        let reg = PermissionAwareRegistry::new(registry, None, None, Role::Guest, "guest1".into());

        let mut params = HashMap::new();
        params.insert("command".to_string(), serde_json::json!("echo hello"));

        let result = reg.execute("exec", &params).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Permission denied")
        );
    }
}
