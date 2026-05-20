//! Defines an AgentTeam and valid internal Roles

use super::bus::AgentMessageBus;
use super::workspace::SharedWorkspace;

/// Common roles within a collaborative developer team
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Role {
    DevAgent,
    RevAgent,
    ArchAgent,
    Custom(String),
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::DevAgent => write!(f, "DevAgent"),
            Role::RevAgent => write!(f, "RevAgent"),
            Role::ArchAgent => write!(f, "ArchAgent"),
            Role::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Represents a running team of agents
#[derive(Clone)]
pub struct AgentTeam {
    pub id: String,
    pub bus: AgentMessageBus,
    pub workspace: SharedWorkspace,
}

impl AgentTeam {
    pub fn new(id: String, base_path: Option<std::path::PathBuf>) -> Self {
        Self {
            id,
            bus: AgentMessageBus::new(),
            workspace: SharedWorkspace::new(base_path),
        }
    }
}
