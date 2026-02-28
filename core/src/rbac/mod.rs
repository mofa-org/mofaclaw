//! Role-Based Access Control (RBAC) system for mofaclaw
//!
//! This module provides fine-grained permission control for Skills, Tools, and filesystem access.

pub mod audit;
pub mod config;
pub mod manager;
pub mod path_matcher;
pub mod role;

#[cfg(test)]
mod tests;

pub use audit::{AuditLogger, AuditLogEntry};
pub use config::*;
pub use manager::RbacManager;
pub use path_matcher::PathMatcher;
pub use role::Role;
