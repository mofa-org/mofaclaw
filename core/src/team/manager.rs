//! Orchestrates the creation and lifecycle of Agent Teams

use super::team::AgentTeam;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

pub struct TeamManager {
    active_teams: RwLock<HashMap<String, Arc<AgentTeam>>>,
}

impl TeamManager {
    pub fn new() -> Self {
        Self {
            active_teams: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_team(&self, id: String, workspace_path: Option<std::path::PathBuf>) -> Arc<AgentTeam> {
        let team = Arc::new(AgentTeam::new(id.clone(), workspace_path));
        let mut map = self.active_teams.write().await;
        map.insert(id, team.clone());
        team
    }
    
    pub async fn get_team(&self, id: &str) -> Option<Arc<AgentTeam>> {
        let map = self.active_teams.read().await;
        map.get(id).cloned()
    }
}

impl Default for TeamManager {
    fn default() -> Self {
        Self::new()
    }
}
