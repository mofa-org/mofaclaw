//! Shell execution tool

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::rbac::{RbacManager, Role};
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::warn;

/// Tool to execute shell commands
pub struct ExecTool {
    /// Timeout in seconds
    timeout: u64,
    /// Default working directory
    working_dir: Option<String>,
    /// RBAC manager for permission checks
    rbac_manager: Option<Arc<RbacManager>>,
    /// User role for permission checks
    user_role: Option<Role>,
}

impl ExecTool {
    /// Create a new ExecTool with default settings
    pub fn new() -> Self {
        Self {
            timeout: 60,
            working_dir: None,
            rbac_manager: None,
            user_role: None,
        }
    }

    /// Create a new ExecTool with custom timeout
    pub fn with_timeout(timeout: u64) -> Self {
        Self {
            timeout,
            working_dir: None,
            rbac_manager: None,
            user_role: None,
        }
    }

    /// Create a new ExecTool with a working directory
    pub fn with_working_dir(working_dir: impl Into<String>) -> Self {
        Self {
            timeout: 60,
            working_dir: Some(working_dir.into()),
            rbac_manager: None,
            user_role: None,
        }
    }

    /// Create with RBAC manager and user role
    pub fn with_rbac(rbac_manager: Arc<RbacManager>, user_role: Role) -> Self {
        Self {
            timeout: 60,
            working_dir: None,
            rbac_manager: Some(rbac_manager),
            user_role: Some(user_role),
        }
    }

    /// Set the working directory
    pub fn set_working_dir(&mut self, dir: impl Into<String>) {
        self.working_dir = Some(dir.into());
    }
}

impl Default for ExecTool {
    fn default() -> Self {
        Self {
            timeout: 60,
            working_dir: None,
            rbac_manager: None,
            user_role: None,
        }
    }
}

#[async_trait]
impl SimpleTool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output. Use with caution."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Optional working directory for the command"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let command = match input.get_str("command") {
            Some(c) => c,
            None => return ToolResult::failure("Missing 'command' parameter"),
        };

        let working_dir = input.get_str("working_dir").or(self.working_dir.as_deref());

        // Check permissions if RBAC is enabled
        if let (Some(rbac), Some(role)) = (&self.rbac_manager, &self.user_role) {
            match rbac.check_command_access(*role, command) {
                crate::rbac::manager::PermissionResult::Allowed => {}
                crate::rbac::manager::PermissionResult::Denied(reason) => {
                    return ToolResult::failure(format!("Permission denied: {}", reason));
                }
            }
        } else {
            // Fallback to basic security check if RBAC not enabled
            if is_dangerous_command(command) {
                return ToolResult::failure("Error: Command blocked for security reasons.");
            }
        }

        let cwd = if let Some(dir) = working_dir {
            let path = Path::new(dir);
            if path.exists() && path.is_dir() {
                Some(path)
            } else {
                None
            }
        } else {
            None
        };

        // Execute with timeout
        let result = timeout(Duration::from_secs(self.timeout), async {
            execute_command(command, cwd).await
        })
        .await;

        match result {
            Ok(Ok(output)) => ToolResult::success_text(output),
            Ok(Err(e)) => ToolResult::failure(format!("Error: {}", e)),
            Err(_) => ToolResult::failure(format!(
                "Error: Command timed out after {} seconds",
                self.timeout
            )),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    fn metadata(&self) -> mofa_sdk::kernel::ToolMetadata {
        mofa_sdk::kernel::ToolMetadata::new()
            .with_category("shell")
            .dangerous()
    }
}

/// Execute a command safely without invoking a shell.
///
/// The command string is parsed into argv tokens using POSIX quoting rules
/// (`shell-words`), then executed directly via `Command::new(program).args(...)`.
/// The shell interpreter is never involved, eliminating the entire class of
/// shell metacharacter injection attacks.
async fn execute_command(command: &str, cwd: Option<&Path>) -> std::io::Result<String> {
    // Parse command into argv using POSIX quoting rules.
    let argv = shell_words::split(command).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Failed to parse command: {}", e),
        )
    })?;

    if argv.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Empty command",
        ));
    }

    let mut cmd = Command::new(&argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd.output().await?;

    let mut result_parts = Vec::new();

    if !output.stdout.is_empty() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        result_parts.push(stdout.to_string());
    }

    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            result_parts.push(format!("STDERR:\n{}", stderr));
        }
    }

    if !output.status.success() {
        result_parts.push(format!("\nExit code: {:?}", output.status.code()));
    }

    let result = if result_parts.is_empty() {
        "(no output)".to_string()
    } else {
        result_parts.join("\n")
    };

    // Truncate very long output
    const MAX_LEN: usize = 10000;
    if result.len() > MAX_LEN {
        Ok(format!(
            "{}\n... (truncated, {} more chars)",
            &result[..MAX_LEN],
            result.len() - MAX_LEN
        ))
    } else {
        Ok(result)
    }
}

/// Fallback safety check used when RBAC is disabled.
///
/// Blocks shell metacharacters (prevents injection) and rejects commands
/// whose program name is a well-known high-risk binary.
/// This is a defence-in-depth measure — prefer enabling RBAC for full control.
fn is_dangerous_command(command: &str) -> bool {
    // 1. Reject shell metacharacters outright.
    const SHELL_METACHARS: &[char] = &[
        ';', '&', '|', '`', '$', '<', '>', '(', ')', '{', '}', '\n', '\r', '%', '!', '^',
    ];
    if command.chars().any(|c| SHELL_METACHARS.contains(&c)) {
        warn!("Fallback safety check: command rejected due to shell metacharacters");
        return true;
    }

    // 2. Reject commands whose first token is a high-risk binary.
    const HIGH_RISK_BINS: &[&str] = &[
        "rm", "mkfs", "dd", "chmod", "chown", "shred", "curl",
        "wget", // exfiltration / download
        "nc", "ncat", "netcat", // reverse shells
        "bash", "sh", "zsh", "fish", "dash", "ksh", // shell spawning
        "python", "python3", "ruby", "perl", "node", // interpreter spawning
        "sudo", "su", // privilege escalation
    ];

    let parsed_args = match shell_words::split(command) {
        Ok(args) => args,
        Err(_) => {
            warn!("Fallback safety check: command rejected due to unparseable quotes");
            return true;
        }
    };

    if let Some(first_token) = parsed_args.first() {
        let first_token_lower = first_token.to_lowercase();
        // Strip any leading path (e.g. /usr/bin/rm → rm)
        let file_name = std::path::Path::new(&first_token_lower)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&first_token_lower);

        // Strip common extensions (e.g. curl.exe → curl)
        let bin = std::path::Path::new(file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(file_name);

        if HIGH_RISK_BINS.contains(&bin) {
            warn!("Fallback safety check: high-risk binary '{}' rejected", bin);
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_command() {
        let tool = ExecTool::new();
        let input = ToolInput::from_json(json!({"command": "echo hello"}));

        let result = tool.execute(input).await;
        assert!(result.success);
        assert!(result.as_text().unwrap().contains("hello"));
    }

    #[test]
    fn test_dangerous_command_detection() {
        assert!(is_dangerous_command("rm -rf /"));
        assert!(!is_dangerous_command("echo hello"));
    }
}
