//! Sandbox configurations

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Global sandbox configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxConfig {
    #[serde(default)]
    pub resource_limits: Option<ResourceLimitsConfig>,
    #[serde(default)]
    pub role_limits: Option<HashMap<String, RoleLimitConfig>>,
}

impl SandboxConfig {
    pub fn build_limiter(&self) -> Option<super::resource::ResourceLimiter> {
        self.resource_limits
            .as_ref()
            .map(|rc| super::resource::ResourceLimiter {
                limits: super::resource::ResourceLimits {
                    command_timeout_seconds: rc.timeouts.command_seconds,
                    file_operation_timeout_seconds: rc.timeouts.file_operation_seconds,
                    web_request_timeout_seconds: rc.timeouts.web_request_seconds,

                    max_command_output_bytes: rc
                        .sizes
                        .max_command_output_mb
                        .saturating_mul(1024 * 1024),
                    max_file_read_bytes: rc.sizes.max_file_read_mb.saturating_mul(1024 * 1024),
                    max_web_response_bytes: rc
                        .sizes
                        .max_web_response_mb
                        .saturating_mul(1024 * 1024),

                    max_commands_per_minute: rc.rates.commands_per_minute,
                    max_file_ops_per_minute: rc.rates.file_ops_per_minute,
                    max_web_requests_per_minute: rc.rates.web_requests_per_minute,

                    max_concurrent_commands: rc.concurrency.max_concurrent_commands,
                    max_concurrent_subagents: rc.concurrency.max_concurrent_subagents,
                    role_limits: self.role_limits.clone().unwrap_or_default(),
                },
                usage: super::resource::ResourceUsageTracker::new(),
            })
    }
}

/// Resource Limits configuration wrapper
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimitsConfig {
    #[serde(default)]
    pub timeouts: TimeoutsConfig,
    #[serde(default)]
    pub sizes: SizesConfig,
    #[serde(default)]
    pub rates: RatesConfig,
    #[serde(default)]
    pub concurrency: ConcurrencyConfig,
}

/// Time limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutsConfig {
    pub command_seconds: u64,
    pub file_operation_seconds: u64,
    pub web_request_seconds: u64,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            command_seconds: 60,
            file_operation_seconds: 30,
            web_request_seconds: 30,
        }
    }
}

/// Size limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizesConfig {
    pub max_command_output_mb: usize,
    pub max_file_read_mb: usize,
    pub max_web_response_mb: usize,
}

impl Default for SizesConfig {
    fn default() -> Self {
        Self {
            max_command_output_mb: 1,
            max_file_read_mb: 10,
            max_web_response_mb: 1,
        }
    }
}

/// Rate limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatesConfig {
    pub commands_per_minute: u32,
    pub file_ops_per_minute: u32,
    pub web_requests_per_minute: u32,
}

impl Default for RatesConfig {
    fn default() -> Self {
        Self {
            commands_per_minute: 30,
            file_ops_per_minute: 60,
            web_requests_per_minute: 30,
        }
    }
}

/// Concurrency limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    pub max_concurrent_commands: u32,
    pub max_concurrent_subagents: u32,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            max_concurrent_commands: 3,
            max_concurrent_subagents: 5,
        }
    }
}

/// Per-Role limits configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleLimitConfig {
    pub commands_per_minute: Option<u32>,
    pub max_concurrent_commands: Option<u32>,
    pub file_ops_per_minute: Option<u32>,
    pub web_requests_per_minute: Option<u32>,
    pub max_concurrent_subagents: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_limiter_deserializes_and_converts_units_correctly() {
        let json = r#"{
            "resource_limits": {
                "timeouts": {
                    "command_seconds": 30,
                    "file_operation_seconds": 45,
                    "web_request_seconds": 60
                },
                "sizes": {
                    "max_command_output_mb": 2,
                    "max_file_read_mb": 3,
                    "max_web_response_mb": 4
                },
                "rates": {
                    "commands_per_minute": 10,
                    "file_ops_per_minute": 20,
                    "web_requests_per_minute": 30
                },
                "concurrency": {
                    "max_concurrent_commands": 5,
                    "max_concurrent_subagents": 6
                }
            },
            "role_limits": {
                "default": {}
            }
        }"#;

        let config: SandboxConfig = serde_json::from_str(json).unwrap();
        let limiter = config.build_limiter().unwrap();
        let limits = limiter.limits;

        assert_eq!(limits.command_timeout_seconds, 30);
        assert_eq!(limits.file_operation_timeout_seconds, 45);
        assert_eq!(limits.web_request_timeout_seconds, 60);

        const MB: usize = 1024 * 1024;
        assert_eq!(limits.max_command_output_bytes, 2 * MB);
        assert_eq!(limits.max_file_read_bytes, 3 * MB);
        assert_eq!(limits.max_web_response_bytes, 4 * MB);

        assert_eq!(limits.max_commands_per_minute, 10);
        assert_eq!(limits.max_file_ops_per_minute, 20);
        assert_eq!(limits.max_web_requests_per_minute, 30);

        assert_eq!(limits.max_concurrent_commands, 5);
        assert_eq!(limits.max_concurrent_subagents, 6);

        assert_eq!(limits.role_limits.len(), 1);
        assert!(limits.role_limits.contains_key("default"));
    }
}
