//! Webhook receivers (e.g. GitHub → Discord notifications).

pub mod github;

pub use github::{build_discord_notifications, verify_signature};
