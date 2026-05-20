//! Shared Workspace for the Agent Team
//!
//! Provides a shared in-memory and disk-based workspace where 
//! agents can collaboratively read and write artifacts.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A shared workspace for a team of agents
#[derive(Clone)]
pub struct SharedWorkspace {
    /// In-memory artifacts (like ongoing design docs, context snippets)
    artifacts: Arc<RwLock<HashMap<String, String>>>,
    /// Base path for disk operations if needed
    base_path: Option<std::path::PathBuf>,
}

impl SharedWorkspace {
    pub fn new(base_path: Option<std::path::PathBuf>) -> Self {
        Self {
            artifacts: Arc::new(RwLock::new(HashMap::new())),
            base_path,
        }
    }

    /// Store a string artifact in the shared memory space
    pub async fn put_artifact(&self, key: &str, content: &str) {
        let mut map = self.artifacts.write().await;
        map.insert(key.to_string(), content.to_string());
    }

    /// Retrieve an artifact by key
    pub async fn get_artifact(&self, key: &str) -> Option<String> {
        let map = self.artifacts.read().await;
        map.get(key).cloned()
    }

    /// List all available artifact keys in this workspace
    pub async fn list_artifacts(&self) -> Vec<String> {
        let map = self.artifacts.read().await;
        map.keys().cloned().collect()
    }
    
    pub fn base_path(&self) -> Option<&std::path::PathBuf> {
        self.base_path.as_ref()
    }
}
