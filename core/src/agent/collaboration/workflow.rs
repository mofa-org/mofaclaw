//! Workflow orchestration for multi-agent collaboration
//!
//! Provides structures and functionality for defining and executing multi-step
//! workflows with agent handoffs and approval gates.
//!
//! Execution model:
//! - `start_async` spawns the workflow in a background task and returns the
//!   workflow_id immediately.  Callers poll `get_workflow` for status.
//! - Steps whose `required_artifacts` have no unsatisfied dependencies within
//!   the same workflow run are grouped into topological layers and executed
//!   concurrently inside each layer.
//! - Completed workflow state is persisted to `<data_dir>/workflows/` as JSON
//!   so that `get_workflow_status` survives process restarts.

use crate::agent::collaboration::team::AgentTeam;
use crate::agent::communication::AgentId;
use crate::error::Result;
use chrono::{DateTime, Utc};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Serde helpers
// ---------------------------------------------------------------------------

mod opt_duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(v: &Option<Duration>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        v.map(|d| d.as_secs()).serialize(s)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Option::<u64>::deserialize(d)?.map(Duration::from_secs))
    }
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Status of a workflow execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    Pending,
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

/// A single step in a workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub name: String,
    /// Role required for this step (must match a team member's role)
    pub role: String,
    /// Task prompt — supports `{variable}` substitution from workflow context
    pub task_prompt: String,
    /// Artifact IDs this step requires (from earlier steps' `produces_artifacts`)
    pub required_artifacts: Vec<String>,
    /// Artifact IDs this step is expected to produce
    pub produces_artifacts: Vec<String>,
    pub approval_required: bool,
    pub approver_role: Option<String>,
    #[serde(default, with = "opt_duration_secs")]
    pub timeout: Option<Duration>,
}

/// Result of executing a single workflow step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_id: String,
    pub success: bool,
    pub output: String,
    pub artifacts: Vec<String>,
    pub completed_at: DateTime<Utc>,
    pub error: Option<String>,
}

/// A workflow definition and its runtime state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub steps: Vec<WorkflowStep>,
    /// Index of the currently-executing step (best-effort with parallel layers)
    pub current_step: usize,
    pub status: WorkflowStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Shared context propagated between steps
    pub context: HashMap<String, serde_json::Value>,
    pub step_results: HashMap<String, StepResult>,
}

impl Workflow {
    pub fn new(id: impl Into<String>, name: impl Into<String>, steps: Vec<WorkflowStep>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            steps,
            current_step: 0,
            status: WorkflowStatus::Pending,
            started_at: None,
            completed_at: None,
            context: HashMap::new(),
            step_results: HashMap::new(),
        }
    }

    pub fn current_step(&self) -> Option<&WorkflowStep> {
        self.steps.get(self.current_step)
    }

    pub fn is_complete(&self) -> bool {
        matches!(
            self.status,
            WorkflowStatus::Completed | WorkflowStatus::Failed | WorkflowStatus::Cancelled
        )
    }

    pub fn has_next_step(&self) -> bool {
        self.current_step < self.steps.len()
    }

    pub fn advance_step(&mut self) {
        if self.current_step < self.steps.len() {
            self.current_step += 1;
        }
    }
}

/// Summary result returned after a workflow run
#[derive(Debug, Clone)]
pub struct WorkflowResult {
    pub workflow_id: String,
    pub success: bool,
    pub completed_steps: usize,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// WorkflowEngine
// ---------------------------------------------------------------------------

pub struct WorkflowEngine {
    workflows: Arc<RwLock<HashMap<String, Workflow>>>,
    data_dir: Option<PathBuf>,
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowEngine {
    /// Create an engine with no persistence (in-memory only).
    pub fn new() -> Self {
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            data_dir: None,
        }
    }

    /// Create an engine that persists workflow state to `data_dir/workflows/`.
    /// Previously-completed workflows are loaded on construction so
    /// `get_workflow_status` works after a restart.
    pub async fn with_persistence(data_dir: PathBuf) -> Self {
        let engine = Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            data_dir: Some(data_dir),
        };
        engine.load_persisted_workflows().await;
        engine
    }

    // ------------------------------------------------------------------
    // Persistence helpers
    // ------------------------------------------------------------------

    fn workflows_dir(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("workflows"))
    }

    async fn save_workflow_state(&self, workflow: &Workflow) {
        let Some(dir) = self.workflows_dir() else {
            return;
        };
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            warn!("Failed to create workflow dir: {}", e);
            return;
        }
        let path = dir.join(format!("{}.json", workflow.id));
        match serde_json::to_string_pretty(workflow) {
            Ok(json) => {
                if let Err(e) = tokio::fs::write(&path, json).await {
                    warn!("Failed to save workflow {}: {}", workflow.id, e);
                }
            }
            Err(e) => warn!("Failed to serialize workflow {}: {}", workflow.id, e),
        }
    }

    async fn load_persisted_workflows(&self) {
        let Some(dir) = self.workflows_dir() else {
            return;
        };
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut workflows = self.workflows.write().await;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(bytes) = tokio::fs::read(&path).await
                && let Ok(mut wf) = serde_json::from_slice::<Workflow>(&bytes)
            {
                // Workflows that were Running when the process died are now stale
                if matches!(
                    wf.status,
                    WorkflowStatus::Running | WorkflowStatus::WaitingApproval
                ) {
                    wf.status = WorkflowStatus::Failed;
                    wf.completed_at = Some(Utc::now());
                }
                workflows.insert(wf.id.clone(), wf);
            }
        }
    }

    // ------------------------------------------------------------------
    // Public API
    // ------------------------------------------------------------------

    /// Start a workflow in the background. Returns the unique workflow ID
    /// immediately — poll `get_workflow` to track progress.
    pub async fn start_async(
        self: &Arc<Self>,
        mut workflow: Workflow,
        team: Arc<AgentTeam>,
        initial_context: HashMap<String, serde_json::Value>,
    ) -> String {
        // Assign a unique run-specific ID so multiple runs of the same
        // workflow definition don't overwrite each other.
        let unique_id = format!("{}-{}", workflow.id, &uuid::Uuid::new_v4().to_string()[..8]);
        workflow.id = unique_id.clone();

        // Register as pending before spawning so callers can poll immediately.
        let pending = {
            let mut w = workflow.clone();
            w.status = WorkflowStatus::Pending;
            w
        };
        {
            let mut map = self.workflows.write().await;
            map.insert(unique_id.clone(), pending.clone());
        }
        self.save_workflow_state(&pending).await;

        let engine = self.clone();
        let wid = unique_id.clone();
        tokio::spawn(async move {
            if let Err(e) = engine
                .execute_workflow(workflow, team, initial_context)
                .await
            {
                tracing::error!("Workflow {} failed: {}", wid, e);
                let mut map = engine.workflows.write().await;
                if let Some(wf) = map.get_mut(&wid) {
                    wf.status = WorkflowStatus::Failed;
                    wf.completed_at = Some(Utc::now());
                    let snapshot = wf.clone();
                    drop(map);
                    engine.save_workflow_state(&snapshot).await;
                }
            }
        });

        unique_id
    }

    /// Get a workflow by ID (for status polling).
    pub async fn get_workflow(&self, workflow_id: &str) -> Option<Workflow> {
        self.workflows.read().await.get(workflow_id).cloned()
    }

    /// List all known workflow IDs (including completed/failed from disk).
    pub async fn list_workflows(&self) -> Vec<String> {
        self.workflows.read().await.keys().cloned().collect()
    }

    // ------------------------------------------------------------------
    // Execution internals
    // ------------------------------------------------------------------

    /// Execute a workflow sequentially through topological layers.
    /// Steps within the same layer (no inter-dependencies) run concurrently.
    pub async fn execute_workflow(
        &self,
        mut workflow: Workflow,
        team: Arc<AgentTeam>,
        initial_context: HashMap<String, serde_json::Value>,
    ) -> Result<WorkflowResult> {
        info!("Executing workflow {} with team {}", workflow.id, team.id);

        workflow.status = WorkflowStatus::Running;
        workflow.started_at = Some(Utc::now());
        workflow.context = initial_context;

        let workflow_id = workflow.id.clone();
        self.store_and_save(&workflow).await;

        let layers = Self::compute_execution_layers(&workflow.steps);
        let mut completed_steps = 0;

        for layer_indices in &layers {
            // Advertise which step we're on (first of layer)
            workflow.current_step = *layer_indices.first().unwrap_or(&completed_steps);
            self.store_and_save(&workflow).await;

            if layer_indices.len() == 1 {
                // Single step — run with retry
                let step = workflow.steps[layer_indices[0]].clone();
                let result =
                    run_step_with_retries(step.clone(), team.clone(), workflow.context.clone(), 3)
                        .await;

                let result = result.unwrap_or_else(|e| StepResult {
                    step_id: step.id.clone(),
                    success: false,
                    output: String::new(),
                    artifacts: vec![],
                    completed_at: Utc::now(),
                    error: Some(e.to_string()),
                });

                Self::apply_step_result(&mut workflow.context, &result);
                workflow
                    .step_results
                    .insert(step.id.clone(), result.clone());
                completed_steps += 1;

                if !result.success {
                    return self
                        .finish_workflow(
                            &mut workflow,
                            workflow_id,
                            false,
                            completed_steps,
                            result.error,
                        )
                        .await;
                }
            } else {
                // Multiple independent steps — run concurrently
                let futs: Vec<_> = layer_indices
                    .iter()
                    .map(|&idx| {
                        let step = workflow.steps[idx].clone();
                        let ctx = workflow.context.clone();
                        let team = team.clone();
                        async move { run_step_with_retries(step, team, ctx, 3).await }
                    })
                    .collect();

                let results = join_all(futs).await;

                let mut failed: Option<String> = None;
                for r in results {
                    let result = r.unwrap_or_else(|e| StepResult {
                        step_id: String::new(),
                        success: false,
                        output: String::new(),
                        artifacts: vec![],
                        completed_at: Utc::now(),
                        error: Some(e.to_string()),
                    });
                    if !result.success && failed.is_none() {
                        failed = Some(result.error.clone().unwrap_or_default());
                    }
                    Self::apply_step_result(&mut workflow.context, &result);
                    workflow.step_results.insert(result.step_id.clone(), result);
                    completed_steps += 1;
                }

                if let Some(err) = failed {
                    return self
                        .finish_workflow(
                            &mut workflow,
                            workflow_id,
                            false,
                            completed_steps,
                            Some(err),
                        )
                        .await;
                }
            }

            self.store_and_save(&workflow).await;
        }

        self.finish_workflow(&mut workflow, workflow_id, true, completed_steps, None)
            .await
    }

    /// Write step outputs into the shared context.
    fn apply_step_result(context: &mut HashMap<String, serde_json::Value>, result: &StepResult) {
        if !result.step_id.is_empty() {
            context.insert(
                format!("step_{}_output", result.step_id),
                serde_json::json!(result.output),
            );
            context.insert(
                "last_step_output".to_string(),
                serde_json::json!(result.output),
            );
        }
    }

    async fn finish_workflow(
        &self,
        workflow: &mut Workflow,
        workflow_id: String,
        success: bool,
        completed_steps: usize,
        error: Option<String>,
    ) -> Result<WorkflowResult> {
        workflow.status = if success {
            WorkflowStatus::Completed
        } else {
            WorkflowStatus::Failed
        };
        workflow.current_step = workflow.steps.len();
        workflow.completed_at = Some(Utc::now());
        self.store_and_save(workflow).await;

        if success {
            info!("Workflow {} completed successfully", workflow_id);
        } else {
            warn!("Workflow {} failed: {:?}", workflow_id, error);
        }

        Ok(WorkflowResult {
            workflow_id,
            success,
            completed_steps,
            error,
        })
    }

    async fn store_and_save(&self, workflow: &Workflow) {
        {
            let mut map = self.workflows.write().await;
            map.insert(workflow.id.clone(), workflow.clone());
        }
        self.save_workflow_state(workflow).await;
    }

    /// Group step indices into topological execution layers.
    /// Steps in the same layer have no intra-layer dependencies and can run
    /// concurrently.  Falls back to fully sequential on cycles.
    fn compute_execution_layers(steps: &[WorkflowStep]) -> Vec<Vec<usize>> {
        // artifact_id -> index of the step that produces it
        let mut artifact_producer: HashMap<&str, usize> = HashMap::new();
        for (idx, step) in steps.iter().enumerate() {
            for artifact in &step.produces_artifacts {
                artifact_producer.insert(artifact.as_str(), idx);
            }
        }

        // For each step, which other steps must complete before it can start
        let mut step_deps: Vec<HashSet<usize>> = vec![HashSet::new(); steps.len()];
        for (idx, step) in steps.iter().enumerate() {
            for required in &step.required_artifacts {
                if let Some(&producer) = artifact_producer.get(required.as_str())
                    && producer != idx
                {
                    step_deps[idx].insert(producer);
                }
            }
        }

        let mut layers: Vec<Vec<usize>> = Vec::new();
        let mut completed: HashSet<usize> = HashSet::new();
        let mut remaining: Vec<usize> = (0..steps.len()).collect();

        while !remaining.is_empty() {
            let layer: Vec<usize> = remaining
                .iter()
                .filter(|&&idx| step_deps[idx].iter().all(|dep| completed.contains(dep)))
                .copied()
                .collect();

            if layer.is_empty() {
                // Cycle — fall back to sequential remainder
                warn!("Cycle in workflow step dependencies, running remaining steps sequentially");
                layers.push(remaining.iter().flat_map(|&i| vec![i]).collect());
                break;
            }

            for &idx in &layer {
                completed.insert(idx);
            }
            remaining.retain(|idx| !completed.contains(idx));
            layers.push(layer);
        }

        layers
    }
}

// ---------------------------------------------------------------------------
// Step execution — free functions (no &self) so they can be used in join_all
// ---------------------------------------------------------------------------

/// Execute a step with up to `max_retries` attempts.
async fn run_step_with_retries(
    step: WorkflowStep,
    team: Arc<AgentTeam>,
    context: HashMap<String, serde_json::Value>,
    max_retries: usize,
) -> Result<StepResult> {
    for attempt in 0..max_retries {
        match run_workflow_step(step.clone(), team.clone(), context.clone()).await {
            Ok(result) if result.success => return Ok(result),
            Ok(result) if attempt + 1 >= max_retries => return Ok(result),
            Ok(_) => {
                warn!(
                    "Step '{}' failed, retrying ({}/{})",
                    step.id,
                    attempt + 1,
                    max_retries
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) if attempt + 1 >= max_retries => return Err(e),
            Err(e) => {
                warn!(
                    "Step '{}' error: {}, retrying ({}/{})",
                    step.id,
                    e,
                    attempt + 1,
                    max_retries
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    unreachable!("loop exhausted without returning")
}

/// Execute a single step: run the assigned agent, then handle any approval gate.
async fn run_workflow_step(
    step: WorkflowStep,
    team: Arc<AgentTeam>,
    context: HashMap<String, serde_json::Value>,
) -> Result<StepResult> {
    let members = team.get_members_by_role(&step.role);
    let member = members.into_iter().next().ok_or_else(|| {
        crate::error::MofaclawError::Other(format!(
            "No agent with role '{}' in team '{}'",
            step.role, team.id
        ))
    })?;

    // Substitute {variable} placeholders from the shared context
    let mut prompt = step.task_prompt.clone();
    for (key, value) in &context {
        let placeholder = format!("{{{}}}", key);
        let value_str = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        prompt = prompt.replace(&placeholder, &value_str);
    }

    let session_key = format!("workflow:{}:{}", team.id, step.id);
    let result = match member
        .agent_loop
        .process_direct(&prompt, &session_key)
        .await
    {
        Ok(output) => {
            info!("Step '{}' completed", step.name);
            StepResult {
                step_id: step.id.clone(),
                success: true,
                output,
                artifacts: step.produces_artifacts.clone(),
                completed_at: Utc::now(),
                error: None,
            }
        }
        Err(e) => {
            warn!("Step '{}' failed: {}", step.name, e);
            StepResult {
                step_id: step.id.clone(),
                success: false,
                output: String::new(),
                artifacts: vec![],
                completed_at: Utc::now(),
                error: Some(e.to_string()),
            }
        }
    };

    if !result.success {
        return Ok(result);
    }

    // Handle approval gate after a successful step run
    if step.approval_required
        && let Some(ref approver_role) = step.approver_role
    {
        let approved = wait_for_approval(&step, &team, approver_role)
            .await
            .unwrap_or(false);
        if !approved {
            return Ok(StepResult {
                step_id: step.id,
                success: false,
                output: result.output,
                artifacts: vec![],
                completed_at: Utc::now(),
                error: Some("Step rejected during approval".to_string()),
            });
        }
    }

    Ok(result)
}

/// Block until the approver agent sends an approval/rejection response or
/// the step's timeout elapses.
async fn wait_for_approval(
    step: &WorkflowStep,
    team: &AgentTeam,
    approver_role: &str,
) -> Result<bool> {
    use crate::agent::communication::{AgentMessage, AgentMessageType, RequestType};

    let approvers = team.get_members_by_role(approver_role);
    let approver = approvers.first().ok_or_else(|| {
        crate::error::MofaclawError::Other(format!(
            "No approver agent with role '{}'",
            approver_role
        ))
    })?;

    let context_id = format!("approval_{}", step.id);
    let request = AgentMessage::request(
        AgentId::new(&team.id, "workflow", "engine"),
        approver.agent_id.clone(),
        RequestType::RequestApproval,
        serde_json::json!({
            "step_id": step.id,
            "step_name": step.name,
            "task_prompt": step.task_prompt,
            "message": format!("Please review and approve step '{}'", step.name),
        }),
        Some(context_id.clone()),
    );
    team.broadcast(request).await?;

    let timeout = step.timeout.unwrap_or(Duration::from_secs(300));
    let start_time = std::time::Instant::now();
    let mut receiver = team.message_bus.subscribe_agent(&approver.agent_id);

    loop {
        if start_time.elapsed() > timeout {
            warn!("Approval timeout for step '{}'", step.id);
            return Ok(false);
        }
        let remaining = timeout.saturating_sub(start_time.elapsed());
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Ok(msg)) => {
                if let Some(ref cid) = msg.correlation_id
                    && cid == &context_id
                    && let AgentMessageType::Response { success, .. } = &msg.message_type
                {
                    info!("Approval response for step '{}': {}", step.id, success);
                    return Ok(*success);
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(_)) => {
                warn!("Approval channel closed for step '{}'", step.id);
                return Ok(false);
            }
            Err(_) => {
                warn!("Approval timed out for step '{}'", step.id);
                return Ok(false);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Built-in workflow definitions
// ---------------------------------------------------------------------------

/// Code review workflow: implement → (review ‖ test) → final approval
pub fn create_code_review_workflow() -> Workflow {
    Workflow::new(
        "code_review",
        "Code Review Workflow",
        vec![
            WorkflowStep {
                id: "implement".to_string(),
                name: "Implement Feature".to_string(),
                role: "developer".to_string(),
                task_prompt: "Implement the following: {user_request}".to_string(),
                required_artifacts: vec![],
                produces_artifacts: vec!["implementation".to_string()],
                approval_required: false,
                approver_role: None,
                timeout: None,
            },
            WorkflowStep {
                id: "review".to_string(),
                name: "Code Review".to_string(),
                role: "reviewer".to_string(),
                task_prompt:
                    "Review the implementation for correctness, code quality, and best practices. \
                     Implementation: {step_implement_output}"
                        .to_string(),
                required_artifacts: vec!["implementation".to_string()],
                produces_artifacts: vec!["review_comments".to_string()],
                approval_required: true,
                approver_role: Some("architect".to_string()),
                timeout: None,
            },
            WorkflowStep {
                id: "test".to_string(),
                name: "Write Tests".to_string(),
                role: "tester".to_string(),
                task_prompt: "Write comprehensive tests for the implementation. \
                     Implementation: {step_implement_output}"
                    .to_string(),
                required_artifacts: vec!["implementation".to_string()],
                produces_artifacts: vec!["test_results".to_string()],
                approval_required: false,
                approver_role: None,
                timeout: None,
            },
            WorkflowStep {
                id: "final_approval".to_string(),
                name: "Final Approval".to_string(),
                role: "reviewer".to_string(),
                task_prompt: "Perform final review and sign-off. \
                     Implementation: {step_implement_output}. \
                     Tests: {step_test_output}."
                    .to_string(),
                required_artifacts: vec!["implementation".to_string(), "test_results".to_string()],
                produces_artifacts: vec!["approval".to_string()],
                approval_required: true,
                approver_role: Some("reviewer".to_string()),
                timeout: None,
            },
        ],
    )
}

/// Design review workflow: design → feasibility review → refine design
pub fn create_design_workflow() -> Workflow {
    Workflow::new(
        "design",
        "Design Workflow",
        vec![
            WorkflowStep {
                id: "design".to_string(),
                name: "Design System".to_string(),
                role: "architect".to_string(),
                task_prompt: "Design the system architecture for: {user_request}".to_string(),
                required_artifacts: vec![],
                produces_artifacts: vec!["design_doc".to_string()],
                approval_required: false,
                approver_role: None,
                timeout: None,
            },
            WorkflowStep {
                id: "review_feasibility".to_string(),
                name: "Review Feasibility".to_string(),
                role: "developer".to_string(),
                task_prompt: "Review the design for implementation feasibility and concerns. \
                     Design: {step_design_output}"
                    .to_string(),
                required_artifacts: vec!["design_doc".to_string()],
                produces_artifacts: vec!["feasibility_review".to_string()],
                approval_required: false,
                approver_role: None,
                timeout: None,
            },
            WorkflowStep {
                id: "refine_design".to_string(),
                name: "Refine Design".to_string(),
                role: "architect".to_string(),
                task_prompt: "Refine the design based on feasibility feedback. \
                     Original design: {step_design_output}. \
                     Feasibility notes: {step_review_feasibility_output}."
                    .to_string(),
                required_artifacts: vec![
                    "design_doc".to_string(),
                    "feasibility_review".to_string(),
                ],
                produces_artifacts: vec!["refined_design".to_string()],
                approval_required: false,
                approver_role: None,
                timeout: None,
            },
        ],
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_creation() {
        let wf = create_code_review_workflow();
        assert_eq!(wf.steps.len(), 4);
        assert_eq!(wf.status, WorkflowStatus::Pending);
    }

    #[test]
    fn test_design_workflow() {
        let wf = create_design_workflow();
        assert_eq!(wf.steps.len(), 3);
    }

    #[test]
    fn test_compute_execution_layers_code_review() {
        let wf = create_code_review_workflow();
        let layers = WorkflowEngine::compute_execution_layers(&wf.steps);

        // Layer 0: implement (no deps)
        // Layer 1: review + test (both depend only on implement)
        // Layer 2: final_approval (depends on implement + test_results)
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].len(), 1); // implement
        assert_eq!(layers[1].len(), 2); // review + test in parallel
        assert_eq!(layers[2].len(), 1); // final_approval
    }

    #[test]
    fn test_compute_execution_layers_design() {
        let wf = create_design_workflow();
        let layers = WorkflowEngine::compute_execution_layers(&wf.steps);

        // design → review_feasibility → refine_design (all sequential)
        assert_eq!(layers.len(), 3);
        for layer in &layers {
            assert_eq!(layer.len(), 1);
        }
    }

    #[test]
    fn test_workflow_advance() {
        let mut wf = create_code_review_workflow();
        assert_eq!(wf.current_step, 0);
        wf.advance_step();
        assert_eq!(wf.current_step, 1);
    }

    #[test]
    fn test_workflow_serialization() {
        let wf = create_code_review_workflow();
        let json = serde_json::to_string(&wf).expect("serialization failed");
        let back: Workflow = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(back.id, wf.id);
        assert_eq!(back.steps.len(), wf.steps.len());
    }
}
