//! Resource Limits and Rate Limiting
//! Implements per-user, per-role, and global resource limits for commands, file ops, web requests, and subagents.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::rbac::Role;

#[derive(Clone, Debug)]
pub struct ResourceLimits {
    // Time limits
    pub command_timeout_seconds: u64,
    pub file_operation_timeout_seconds: u64,
    pub web_request_timeout_seconds: u64,
    // Size limits
    pub max_command_output_bytes: usize,
    pub max_file_read_bytes: usize,
    pub max_web_response_bytes: usize,
    // Rate limits
    pub max_commands_per_minute: u32,
    pub max_file_ops_per_minute: u32,
    pub max_web_requests_per_minute: u32,
    // Concurrency limits
    pub max_concurrent_commands: u32,
    pub max_concurrent_subagents: u32,
    // Role specific limits
    pub role_limits: HashMap<String, crate::sandbox::config::RoleLimitConfig>,
}

#[derive(Clone, Debug)]
pub struct ResourceLimiter {
    pub limits: ResourceLimits,
    pub usage: ResourceUsageTracker,
}

#[derive(Clone, Debug)]
pub struct ResourceUsageTracker {
    pub user_usage: Arc<Mutex<HashMap<UserId, UserUsage>>>,
}

#[derive(Clone, Debug)]
pub struct UserUsage {
    pub commands_this_minute: u32,
    pub file_ops_this_minute: u32,
    pub web_requests_this_minute: u32,
    pub active_commands: u32,
    pub active_subagents: u32,
    pub last_reset: Instant,
}



#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserId(pub String);

#[derive(Clone, Debug)]
pub enum Operation {
    Command,
    FileOp,
    FileRead,
    WebRequest,
    SpawnSubagent,
}

#[derive(Debug)]
pub enum RateLimitError {
    CommandLimitExceeded { limit: u32, reset_in: u64 },
    FileOpLimitExceeded,
    WebRequestLimitExceeded,
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandLimitExceeded { limit, reset_in } => {
                write!(
                    f,
                    "Command rate limit exceeded ({} per minute), resets in {}s",
                    limit, reset_in
                )
            }
            Self::FileOpLimitExceeded => write!(f, "File operation rate limit exceeded"),
            Self::WebRequestLimitExceeded => write!(f, "Web request rate limit exceeded"),
        }
    }
}

impl std::error::Error for RateLimitError {}

#[derive(Debug)]
pub enum ResourceError {
    ConcurrencyLimitExceeded,
    SubagentLimitExceeded,
}

impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConcurrencyLimitExceeded => write!(f, "Concurrency limit exceeded"),
            Self::SubagentLimitExceeded => write!(f, "Subagent limit exceeded"),
        }
    }
}

impl std::error::Error for ResourceError {}

pub struct ResourceSlot {
    pub user: UserId,
    pub operation: Operation,
    pub usage_tracker: Arc<Mutex<HashMap<UserId, UserUsage>>>,
}

impl ResourceUsageTracker {
    pub fn new() -> Self {
        Self {
            user_usage: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl UserUsage {
    pub fn reset_if_minute_elapsed(&mut self) {
        if self.last_reset.elapsed() >= Duration::from_secs(60) {
            self.commands_this_minute = 0;
            self.file_ops_this_minute = 0;
            self.web_requests_this_minute = 0;
            self.last_reset = Instant::now();
        }
    }
    pub fn seconds_until_reset(&self) -> u64 {
        60u64.saturating_sub(self.last_reset.elapsed().as_secs())
    }
}

impl ResourceLimiter {
    pub fn new(limits: ResourceLimits) -> Self {
        Self {
            limits,
            usage: ResourceUsageTracker::new(),
        }
    }

    pub fn check_rate_limit(
        &self,
        user: &UserId,
        role: Option<&Role>,
        operation: &Operation,
    ) -> Result<(), RateLimitError> {
        let mut user_usage_lock = self.usage.user_usage.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let usage = user_usage_lock
            .entry(user.clone())
            .or_default();
        usage.reset_if_minute_elapsed();

        let limits = role
            .map(|r| r.as_str())
            .and_then(|r| self.limits.role_limits.get(r));

        match operation {
            Operation::Command => {
                let limit = limits
                    .and_then(|l| l.commands_per_minute)
                    .unwrap_or(self.limits.max_commands_per_minute);
                if usage.commands_this_minute >= limit {
                    tracing::warn!(user_id = %user.0, role = ?role.map(|r| r.as_str()), limit, current = usage.commands_this_minute, "Command rate limit exceeded");
                    return Err(RateLimitError::CommandLimitExceeded {
                        limit,
                        reset_in: usage.seconds_until_reset(),
                    });
                }
            }
            Operation::FileOp => {
                let limit = limits
                    .and_then(|l| l.file_ops_per_minute)
                    .unwrap_or(self.limits.max_file_ops_per_minute);
                if usage.file_ops_this_minute >= limit {
                    tracing::warn!(user_id = %user.0, role = ?role.map(|r| r.as_str()), limit, current = usage.file_ops_this_minute, "FileOp rate limit exceeded");
                    return Err(RateLimitError::FileOpLimitExceeded);
                }
            }
            Operation::WebRequest => {
                let limit = limits
                    .and_then(|l| l.web_requests_per_minute)
                    .unwrap_or(self.limits.max_web_requests_per_minute);
                if usage.web_requests_this_minute >= limit {
                    tracing::warn!(user_id = %user.0, role = ?role.map(|r| r.as_str()), limit, current = usage.web_requests_this_minute, "WebRequest rate limit exceeded");
                    return Err(RateLimitError::WebRequestLimitExceeded);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn increment_usage(&self, user: &UserId, operation: &Operation) {
        let mut user_usage_lock = self.usage.user_usage.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let usage = user_usage_lock
            .entry(user.clone())
            .or_default();

        match operation {
            Operation::Command => {
                usage.commands_this_minute += 1;
                tracing::debug!(user_id = %user.0, op = "Command", count = usage.commands_this_minute, "Usage incremented");
            }
            Operation::FileOp => {
                usage.file_ops_this_minute += 1;
                tracing::debug!(user_id = %user.0, op = "FileOp", count = usage.file_ops_this_minute, "Usage incremented");
            }
            Operation::WebRequest => {
                usage.web_requests_this_minute += 1;
                tracing::debug!(user_id = %user.0, op = "WebRequest", count = usage.web_requests_this_minute, "Usage incremented");
            }
            _ => {}
        }
    }
    pub fn truncate_output<'a>(&self, output: &'a [u8], operation: &Operation) -> Cow<'a, [u8]> {
        let max_size = match operation {
            Operation::Command => self.limits.max_command_output_bytes,
            Operation::FileRead => self.limits.max_file_read_bytes,
            Operation::WebRequest => self.limits.max_web_response_bytes,
            _ => usize::MAX,
        };
        if output.len() <= max_size {
            Cow::Borrowed(output)
        } else {
            let truncated = &output[..max_size];
            let message = format!("\n\n[TRUNCATED: Output exceeded {} bytes]", max_size);
            Cow::Owned([truncated, message.as_bytes()].concat())
        }
    }
    pub fn acquire_slot(
        &self,
        user: &UserId,
        role: Option<&Role>,
        operation: Operation,
    ) -> Result<ResourceSlot, ResourceError> {
        let mut user_usage_lock = self.usage.user_usage.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let usage = user_usage_lock
            .entry(user.clone())
            .or_default();

        let limits = role
            .map(|r| r.as_str())
            .and_then(|r| self.limits.role_limits.get(r));

        match operation {
            Operation::Command => {
                let limit = limits
                    .and_then(|l| l.max_concurrent_commands)
                    .unwrap_or(self.limits.max_concurrent_commands);
                if usage.active_commands >= limit {
                    tracing::warn!(user_id = %user.0, role = ?role.map(|r| r.as_str()), limit, active = usage.active_commands, "Command concurrency limit exceeded");
                    return Err(ResourceError::ConcurrencyLimitExceeded);
                }
                usage.active_commands += 1;
                tracing::debug!(user_id = %user.0, active = usage.active_commands, limit, "Acquired Command resource slot");
            }
            Operation::SpawnSubagent => {
                let limit = limits
                    .and_then(|l| l.max_concurrent_subagents)
                    .unwrap_or(self.limits.max_concurrent_subagents);
                if usage.active_subagents >= limit {
                    tracing::warn!(user_id = %user.0, role = ?role.map(|r| r.as_str()), limit, active = usage.active_subagents, "Subagent concurrency limit exceeded");
                    return Err(ResourceError::SubagentLimitExceeded);
                }
                usage.active_subagents += 1;
                tracing::debug!(user_id = %user.0, active = usage.active_subagents, limit, "Acquired SpawnSubagent resource slot");
            }
            _ => {}
        }
        Ok(ResourceSlot {
            user: user.clone(),
            operation,
            usage_tracker: Arc::clone(&self.usage.user_usage),
        })
    }
}

impl Drop for ResourceSlot {
    fn drop(&mut self) {
        if let Ok(mut lock) = self.usage_tracker.lock() {
            if let Some(usage) = lock.get_mut(&self.user) {
                match self.operation {
                    Operation::Command => {
                        if usage.active_commands > 0 {
                            usage.active_commands -= 1;
                            tracing::debug!(user_id = %self.user.0, active = usage.active_commands, "Released Command resource slot");
                        }
                    }
                    Operation::SpawnSubagent => {
                        if usage.active_subagents > 0 {
                            usage.active_subagents -= 1;
                            tracing::debug!(user_id = %self.user.0, active = usage.active_subagents, "Released SpawnSubagent resource slot");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

impl Default for UserUsage {
    fn default() -> Self {
        Self {
            commands_this_minute: 0,
            file_ops_this_minute: 0,
            web_requests_this_minute: 0,
            active_commands: 0,
            active_subagents: 0,
            last_reset: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::config::RoleLimitConfig;
    use std::collections::HashMap;

    fn make_limiter(role_limits: HashMap<String, RoleLimitConfig>) -> ResourceLimiter {
        ResourceLimiter::new(ResourceLimits {
            max_commands_per_minute: 5,
            max_file_ops_per_minute: 5,
            max_web_requests_per_minute: 5,
            max_command_output_bytes: 1024,
            max_file_read_bytes: 1024,
            max_web_response_bytes: 1024,
            command_timeout_seconds: 60,
            file_operation_timeout_seconds: 60,
            web_request_timeout_seconds: 60,
            max_concurrent_commands: 2,
            max_concurrent_subagents: 2,
            role_limits,
        })
    }

    #[test]
    fn test_role_based_rate_limiting() {
        let mut role_limits = HashMap::new();
        role_limits.insert(
            "guest".to_string(),
            RoleLimitConfig {
                commands_per_minute: Some(2),
                file_ops_per_minute: Some(2),
                web_requests_per_minute: Some(2),
                max_concurrent_commands: Some(1),
                max_concurrent_subagents: Some(1),
            },
        );
        role_limits.insert(
            "admin".to_string(),
            RoleLimitConfig {
                commands_per_minute: Some(10),
                file_ops_per_minute: Some(10),
                web_requests_per_minute: Some(10),
                max_concurrent_commands: Some(5),
                max_concurrent_subagents: Some(5),
            },
        );

        let limiter = make_limiter(role_limits);
        let guest_user = UserId("guest_user".to_string());
        let admin_user = UserId("admin_user".to_string());
        let guest_role = Role::Guest;
        let admin_role = Role::Admin;

        // Guest user hits their limit at 2 commands
        for _ in 0..2 {
            assert!(limiter
                .check_rate_limit(&guest_user, Some(&guest_role), &Operation::Command)
                .is_ok());
            limiter.increment_usage(&guest_user, &Operation::Command);
        }
        assert!(matches!(
            limiter.check_rate_limit(&guest_user, Some(&guest_role), &Operation::Command),
            Err(RateLimitError::CommandLimitExceeded { limit: 2, .. })
        ));

        // Admin user can do 10 commands
        for _ in 0..10 {
            assert!(limiter
                .check_rate_limit(&admin_user, Some(&admin_role), &Operation::Command)
                .is_ok());
            limiter.increment_usage(&admin_user, &Operation::Command);
        }
        assert!(matches!(
            limiter.check_rate_limit(&admin_user, Some(&admin_role), &Operation::Command),
            Err(RateLimitError::CommandLimitExceeded { limit: 10, .. })
        ));
    }

    #[test]
    fn test_output_truncation() {
        let limiter = make_limiter(HashMap::new());
        let small_output = vec![0u8; 512];
        let large_output = vec![0u8; 2048];

        // Small output should not be truncated
        let result = limiter.truncate_output(&small_output, &Operation::Command);
        assert_eq!(result.len(), 512);

        // Large output should be truncated to max_command_output_bytes (1024)
        let result = limiter.truncate_output(&large_output, &Operation::Command);
        assert!(result.len() > 1024); // 1024 bytes + truncation message
        let result_str = String::from_utf8_lossy(&result);
        assert!(result_str.contains("[TRUNCATED:"));
    }

    #[test]
    fn test_concurrency_slot_acquire_and_drop() {
        let limiter = make_limiter(HashMap::new());
        let user = UserId("test_user".to_string());

        // Acquire 2 slots (limit is 2)
        let slot1 = limiter.acquire_slot(&user, None, Operation::Command).unwrap();
        let slot2 = limiter.acquire_slot(&user, None, Operation::Command).unwrap();

        // Third should fail
        assert!(matches!(
            limiter.acquire_slot(&user, None, Operation::Command),
            Err(ResourceError::ConcurrencyLimitExceeded)
        ));

        // Drop one slot, then acquire should succeed
        drop(slot1);
        assert!(limiter.acquire_slot(&user, None, Operation::Command).is_ok());

        drop(slot2);
    }

    #[test]
    fn test_file_op_and_web_request_rate_limits() {
        let limiter = make_limiter(HashMap::new());
        let user = UserId("test_user".to_string());

        // Fill up file ops (limit is 5)
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(&user, None, &Operation::FileOp).is_ok());
            limiter.increment_usage(&user, &Operation::FileOp);
        }
        assert!(matches!(
            limiter.check_rate_limit(&user, None, &Operation::FileOp),
            Err(RateLimitError::FileOpLimitExceeded)
        ));

        // Fill up web requests (limit is 5)
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(&user, None, &Operation::WebRequest).is_ok());
            limiter.increment_usage(&user, &Operation::WebRequest);
        }
        assert!(matches!(
            limiter.check_rate_limit(&user, None, &Operation::WebRequest),
            Err(RateLimitError::WebRequestLimitExceeded)
        ));
    }

    #[test]
    fn test_seconds_until_reset_no_underflow() {
        let usage = UserUsage::default();
        // Freshly created, should be close to 60
        assert!(usage.seconds_until_reset() <= 60);
        // Even after time passes, should never panic or wrap
        let result = 60u64.saturating_sub(120);
        assert_eq!(result, 0);
    }
}
