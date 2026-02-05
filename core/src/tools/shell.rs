//! Shell execution tool

use super::base::{SimpleTool, ToolInput, ToolResult};
use crate::error::{Result, ToolError};
use async_trait::async_trait;
use mofa_sdk::kernel::ToolCategory;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Tool to execute shell commands
pub struct ExecTool {
    /// Timeout in seconds
    timeout: u64,
    /// Default working directory
    working_dir: Option<String>,
}

impl ExecTool {
    /// Create a new ExecTool with default settings
    pub fn new() -> Self {
        Self {
            timeout: 60,
            working_dir: None,
        }
    }

    /// Create a new ExecTool with custom timeout
    pub fn with_timeout(timeout: u64) -> Self {
        Self {
            timeout,
            working_dir: None,
        }
    }

    /// Create a new ExecTool with a working directory
    pub fn with_working_dir(working_dir: impl Into<String>) -> Self {
        Self {
            timeout: 60,
            working_dir: Some(working_dir.into()),
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

        // Check for dangerous commands (basic security)
        if is_dangerous_command(command) {
            return ToolResult::failure("Error: Command blocked for security reasons.");
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
            Err(_) => ToolResult::failure(format!("Error: Command timed out after {} seconds", self.timeout)),
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

/// Execute a command and return its output
async fn execute_command(command: &str, cwd: Option<&Path>) -> std::io::Result<String> {
    // Use shell on Unix, cmd on Windows
    #[cfg(unix)]
    let (shell, flag) = ("sh", "-c");
    #[cfg(windows)]
    let (shell, flag) = ("cmd", "/C");

    let mut cmd = Command::new(shell);
    cmd.arg(flag);
    cmd.arg(command);

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

/// Check for potentially dangerous commands
fn is_dangerous_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    let dangerous = [
        "rm -rf /",
        "rm -rf /*",
        "mkfs",
        "dd if=/dev/zero",
        ":(){ :|:& };:", // fork bomb
        "chmod 000",
    ];
    dangerous.iter().any(|&d| lower.contains(d))
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
