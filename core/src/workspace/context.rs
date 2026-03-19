//! Context store – shared project knowledge persisted as JSON files
//!
//! Manages `context/decisions.json`, `context/constraints.json`, and
//! `context/glossary.json`.

use crate::error::Result;
use crate::workspace::fs::{ExclusiveLock, atomic_write_json};
use crate::workspace::types::{AgentId, Constraint, Decision, GlossaryEntry};
use chrono::Utc;
use std::path::PathBuf;
use tokio::fs;
use uuid::Uuid;

/// Manages the shared context directory.
pub struct ContextStore {
    root: PathBuf,
}

impl ContextStore {
    pub async fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root).await?;

        // Initialise files if they don't exist yet
        for name in &["decisions.json", "constraints.json", "glossary.json"] {
            let p = root.join(name);
            if !p.exists() {
                fs::write(&p, b"[]").await?;
            }
        }

        Ok(Self { root })
    }

    // ── Decisions ─────────────────────────────────────────────────────

    fn decisions_path(&self) -> PathBuf {
        self.root.join("decisions.json")
    }

    pub async fn list_decisions(&self) -> Result<Vec<Decision>> {
        let data = fs::read_to_string(self.decisions_path()).await?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn add_decision(
        &self,
        title: String,
        rationale: String,
        agent: AgentId,
    ) -> Result<Decision> {
        let _guard = ExclusiveLock::acquire(self.decisions_path().with_extension("json.lock")).await?;
        let mut decisions = self.list_decisions().await?;
        let decision = Decision {
            id: Uuid::new_v4(),
            title,
            rationale,
            decided_by: agent,
            decided_at: Utc::now(),
        };
        decisions.push(decision.clone());
        atomic_write_json(&self.decisions_path(), &decisions).await?;
        Ok(decision)
    }

    pub async fn remove_decision(&self, id: Uuid) -> Result<bool> {
        let _guard = ExclusiveLock::acquire(self.decisions_path().with_extension("json.lock")).await?;
        let mut decisions = self.list_decisions().await?;
        let before = decisions.len();
        decisions.retain(|d| d.id != id);
        if decisions.len() == before {
            return Ok(false);
        }
        atomic_write_json(&self.decisions_path(), &decisions).await?;
        Ok(true)
    }

    // ── Constraints ───────────────────────────────────────────────────

    fn constraints_path(&self) -> PathBuf {
        self.root.join("constraints.json")
    }

    pub async fn list_constraints(&self) -> Result<Vec<Constraint>> {
        let data = fs::read_to_string(self.constraints_path()).await?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn add_constraint(
        &self,
        name: String,
        description: String,
        agent: AgentId,
    ) -> Result<Constraint> {
        let _guard = ExclusiveLock::acquire(self.constraints_path().with_extension("json.lock")).await?;
        let mut constraints = self.list_constraints().await?;
        let constraint = Constraint {
            id: Uuid::new_v4(),
            name,
            description,
            added_by: agent,
            added_at: Utc::now(),
        };
        constraints.push(constraint.clone());
        atomic_write_json(&self.constraints_path(), &constraints).await?;
        Ok(constraint)
    }

    pub async fn remove_constraint(&self, id: Uuid) -> Result<bool> {
        let _guard = ExclusiveLock::acquire(self.constraints_path().with_extension("json.lock")).await?;
        let mut constraints = self.list_constraints().await?;
        let before = constraints.len();
        constraints.retain(|c| c.id != id);
        if constraints.len() == before {
            return Ok(false);
        }
        atomic_write_json(&self.constraints_path(), &constraints).await?;
        Ok(true)
    }

    // ── Glossary ──────────────────────────────────────────────────────

    fn glossary_path(&self) -> PathBuf {
        self.root.join("glossary.json")
    }

    pub async fn list_glossary(&self) -> Result<Vec<GlossaryEntry>> {
        let data = fs::read_to_string(self.glossary_path()).await?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn add_glossary_entry(
        &self,
        term: String,
        definition: String,
        agent: AgentId,
    ) -> Result<GlossaryEntry> {
        let _guard = ExclusiveLock::acquire(self.glossary_path().with_extension("json.lock")).await?;
        let mut entries = self.list_glossary().await?;
        let entry = GlossaryEntry {
            term,
            definition,
            added_by: agent,
            added_at: Utc::now(),
        };
        entries.push(entry.clone());
        atomic_write_json(&self.glossary_path(), &entries).await?;
        Ok(entry)
    }

    pub async fn remove_glossary_entry(&self, term: &str) -> Result<bool> {
        let _guard = ExclusiveLock::acquire(self.glossary_path().with_extension("json.lock")).await?;
        let mut entries = self.list_glossary().await?;
        let before = entries.len();
        entries.retain(|e| e.term != term);
        if entries.len() == before {
            return Ok(false);
        }
        atomic_write_json(&self.glossary_path(), &entries).await?;
        Ok(true)
    }
}
