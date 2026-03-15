//! Multi-agent collaboration primitives
//!
//! This module provides team management, workflow orchestration, and shared workspace
//! functionality for coordinating multiple agents.

pub mod team;
pub mod workflow;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use team::{AgentTeam, MemberStatus, TeamManager, TeamMember, TeamStatus};
pub use workflow::{
    Workflow, WorkflowEngine, WorkflowResult, WorkflowStatus, WorkflowStep,
    create_code_review_workflow, create_design_workflow,
};
pub use workspace::{Artifact, ArtifactContent, ArtifactType, ConflictStrategy, SharedWorkspace};
