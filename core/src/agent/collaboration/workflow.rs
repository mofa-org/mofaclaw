//! Workflow orchestration for multi-agent collaboration
//!
//! Provides structures and functionality for defining and executing multi-step
//! workflows with agent handoffs and approval gates.

use crate::agent::collaboration::team::AgentTeam;
use crate::agent::communication::AgentId;
use crate::error::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

/// Status of a workflow execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowStatus {
    /// Workflow is pending execution
    Pending,
    /// Workflow is currently running
    Running,
    /// Workflow is waiting for approval
    WaitingApproval,
    /// Workflow completed successfully
    Completed,
    /// Workflow failed
    Failed,
    /// Workflow was cancelled
    Cancelled,
}

/// A single step in a workflow
#[derive(Debug, Clone)]
pub struct WorkflowStep {
    /// Unique step identifier
    pub id: String,
    /// Step name/description
    pub name: String,
    /// Role required for this step
    pub role: String,
    /// Task prompt/description for the agent
    pub task_prompt: String,
    /// Advisory metadata listing artifact IDs that this step conceptually
    /// depends on. The workflow engine does not currently enforce the
    /// existence of these artifacts before executing the step.
    pub required_artifacts: Vec<String>,
    /// Advisory metadata listing artifact IDs that this step is expected
    /// to produce. The workflow engine does not automatically persist or
    /// attach these artifacts to any workspace.
    pub produces_artifacts: Vec<String>,
    /// Whether approval is required before proceeding
    pub approval_required: bool,
    /// Role that can approve this step (if approval_required is true)
    pub approver_role: Option<String>,
    /// Timeout for this step (None = no timeout)
    pub timeout: Option<Duration>,
}

/// A workflow definition
#[derive(Debug, Clone)]
pub struct Workflow {
    /// Unique workflow identifier
    pub id: String,
    /// Workflow name
    pub name: String,
    /// Steps in the workflow (executed in order)
    pub steps: Vec<WorkflowStep>,
    /// Current step index (0-based)
    pub current_step: usize,
    /// Current workflow status
    pub status: WorkflowStatus,
    /// When the workflow started
    pub started_at: Option<DateTime<Utc>>,
    /// When the workflow completed
    pub completed_at: Option<DateTime<Utc>>,
    /// Workflow context (shared data between steps)
    pub context: HashMap<String, serde_json::Value>,
    /// Step results (step_id -> result)
    pub step_results: HashMap<String, StepResult>,
}

impl Workflow {
    /// Create a new workflow
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

    /// Get the current step
    pub fn current_step(&self) -> Option<&WorkflowStep> {
        self.steps.get(self.current_step)
    }

    /// Check if workflow is complete
    pub fn is_complete(&self) -> bool {
        self.status == WorkflowStatus::Completed
            || self.status == WorkflowStatus::Failed
            || self.status == WorkflowStatus::Cancelled
    }

    /// Check if workflow has more steps
    pub fn has_next_step(&self) -> bool {
        self.current_step < self.steps.len()
    }

    /// Move to the next step
    pub fn advance_step(&mut self) {
        if self.current_step < self.steps.len() {
            self.current_step += 1;
        }
    }
}

/// Result of executing a workflow step
#[derive(Debug, Clone)]
pub struct StepResult {
    /// Step ID
    pub step_id: String,
    /// Whether the step succeeded
    pub success: bool,
    /// Step output/result
    pub output: String,
    /// Artifacts produced by this step
    pub artifacts: Vec<String>,
    /// When the step completed
    pub completed_at: DateTime<Utc>,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Workflow execution engine
pub struct WorkflowEngine {
    /// Active workflows
    workflows: Arc<RwLock<HashMap<String, Workflow>>>,
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowEngine {
    /// Create a new workflow engine
    pub fn new() -> Self {
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Execute a workflow with a team
    ///
    /// This will:
    /// 1. Start the workflow
    /// 2. Execute each step in order
    /// 3. Handle handoffs between agents
    /// 4. Process approval gates
    /// 5. Complete the workflow
    pub async fn execute_workflow(
        &self,
        mut workflow: Workflow,
        team: Arc<AgentTeam>,
        initial_context: HashMap<String, serde_json::Value>,
    ) -> Result<WorkflowResult> {
        info!("Starting workflow {} with team {}", workflow.id, team.id);

        // Initialize workflow
        workflow.status = WorkflowStatus::Running;
        workflow.started_at = Some(Utc::now());
        workflow.context = initial_context;

        // Store workflow
        let workflow_id = workflow.id.clone();
        {
            let mut workflows = self.workflows.write().await;
            workflows.insert(workflow_id.clone(), workflow.clone());
        }

        // Execute steps
        while workflow.has_next_step() && !workflow.is_complete() {
            let step = workflow
                .current_step()
                .ok_or_else(|| crate::error::MofaclawError::Other("No current step".to_string()))?
                .clone();

            info!(
                "Executing step {} ({}) of workflow {}",
                step.id, step.name, workflow.id
            );

            // Execute the step with retry logic
            let mut step_result = None;
            let max_retries = 3;
            let mut retry_count = 0;

            while step_result.is_none() && retry_count < max_retries {
                match self.execute_step(&step, team.as_ref(), &workflow.context).await {
                    Ok(result) => {
                        if result.success {
                            step_result = Some(result);
                        } else {
                            retry_count += 1;
                            if retry_count < max_retries {
                                warn!(
                                    "Step {} failed, retrying ({}/{})",
                                    step.id, retry_count, max_retries
                                );
                                tokio::time::sleep(Duration::from_secs(2)).await;
                            } else {
                                // Final failure after retries
                                step_result = Some(result);
                            }
                        }
                    }
                    Err(e) => {
                        retry_count += 1;
                        if retry_count < max_retries {
                            warn!(
                                "Step {} error: {}, retrying ({}/{})",
                                step.id, e, retry_count, max_retries
                            );
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        } else {
                            // Final error after retries
                            step_result = Some(StepResult {
                                step_id: step.id.clone(),
                                success: false,
                                output: String::new(),
                                artifacts: vec![],
                                completed_at: Utc::now(),
                                error: Some(e.to_string()),
                            });
                        }
                    }
                }
            }

            let result = step_result.expect("Step result should be set after retries");

            // Store step result
            workflow
                .step_results
                .insert(step.id.clone(), result.clone());

            // Add step output to workflow context for next steps
            // Use step_id as the key, or a more descriptive name if available
            workflow.context.insert(
                format!("step_{}_output", step.id),
                serde_json::json!(result.output),
            );
            // Also add a generic "last_step_output" for convenience
            workflow.context.insert(
                "last_step_output".to_string(),
                serde_json::json!(result.output),
            );
            // Add step name to context
            workflow.context.insert(
                format!("step_{}_name", step.id),
                serde_json::json!(step.name),
            );

            // Check if step failed after retries
            if !result.success {
                warn!("Step {} failed after {} retries", step.id, max_retries);
                workflow.status = WorkflowStatus::Failed;
                workflow.completed_at = Some(Utc::now());

                // Update stored workflow
                let mut workflows = self.workflows.write().await;
                workflows.insert(workflow_id.clone(), workflow.clone());
                drop(workflows);

                return Ok(WorkflowResult {
                    workflow_id: workflow_id.clone(),
                    success: false,
                    completed_steps: workflow.current_step,
                    error: result.error.clone(),
                });
            }

            // Check if approval is required
            if step.approval_required {
                workflow.status = WorkflowStatus::WaitingApproval;
                info!(
                    "Step {} requires approval from role {:?}",
                    step.id, step.approver_role
                );
                
                // Wait for approval via agent message
                if let Some(ref approver_role) = step.approver_role {
                    match self.wait_for_approval(&step, team.as_ref(), approver_role).await {
                        Ok(approved) => {
                            if approved {
                                info!("Step {} approved, continuing workflow", step.id);
                                workflow.status = WorkflowStatus::Running;
                            } else {
                                warn!("Step {} was rejected", step.id);
                                workflow.status = WorkflowStatus::Failed;
                                workflow.completed_at = Some(Utc::now());
                                
                                let mut workflows = self.workflows.write().await;
                                workflows.insert(workflow_id.clone(), workflow.clone());
                                drop(workflows);
                                
                                return Ok(WorkflowResult {
                                    workflow_id: workflow_id.clone(),
                                    success: false,
                                    completed_steps: workflow.current_step,
                                    error: Some("Step was rejected during approval".to_string()),
                                });
                            }
                        }
                        Err(e) => {
                            warn!("Approval process failed: {}, auto-approving", e);
                            // Auto-approve on error to not block workflow
                            workflow.status = WorkflowStatus::Running;
                        }
                    }
                } else {
                    warn!("Approval required but no approver role specified, auto-approving");
                    workflow.status = WorkflowStatus::Running;
                }
            }

            // Advance to next step
            workflow.advance_step();

            // Update stored workflow
            let mut workflows = self.workflows.write().await;
            workflows.insert(workflow_id.clone(), workflow.clone());
            drop(workflows);
        }

        // Mark workflow as completed
        workflow.status = WorkflowStatus::Completed;
        workflow.completed_at = Some(Utc::now());

        // Update stored workflow
        {
            let mut workflows = self.workflows.write().await;
            workflows.insert(workflow_id.clone(), workflow.clone());
        }

        info!("Workflow {} completed successfully", workflow_id);

        Ok(WorkflowResult {
            workflow_id,
            success: true,
            completed_steps: workflow.steps.len(),
            error: None,
        })
    }

    /// Execute a single workflow step
    async fn execute_step(
        &self,
        step: &WorkflowStep,
        team: &AgentTeam,
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<StepResult> {
        // Find an agent with the required role
        let members = team.get_members_by_role(&step.role);
        let member = members.first().ok_or_else(|| {
            crate::error::MofaclawError::Other(format!(
                "No agent with role '{}' found in team",
                step.role
            ))
        })?;

        info!(
            "Assigning step '{}' to agent {}",
            step.name, member.agent_id
        );

        // Build task prompt with context
        let task_prompt = self.build_task_prompt(step, context);

        // Execute the task using the agent
        let session_key = format!("workflow:{}:{}", team.id, step.id);
        match member.agent_loop.process_direct(&task_prompt, &session_key).await {
            Ok(output) => {
                info!("Step '{}' completed successfully", step.name);
                Ok(StepResult {
                    step_id: step.id.clone(),
                    success: true,
                    output,
                    artifacts: step.produces_artifacts.clone(),
                    completed_at: Utc::now(),
                    error: None,
                })
            }
            Err(e) => {
                warn!("Step '{}' failed: {}", step.name, e);
                Ok(StepResult {
                    step_id: step.id.clone(),
                    success: false,
                    output: String::new(),
                    artifacts: vec![],
                    completed_at: Utc::now(),
                    error: Some(e.to_string()),
                })
            }
        }
    }

    /// Wait for approval from an approver agent
    async fn wait_for_approval(
        &self,
        step: &WorkflowStep,
        team: &AgentTeam,
        approver_role: &str,
    ) -> Result<bool> {
        // Find approver agent
        let approvers = team.get_members_by_role(approver_role);
        let approver = approvers.first().ok_or_else(|| {
            crate::error::MofaclawError::Other(format!(
                "No approver agent with role '{}' found",
                approver_role
            ))
        })?;

        // Send approval request via agent message bus
        use crate::agent::communication::{AgentMessage, AgentMessageType, RequestType};
        let context_id = format!("approval_{}", step.id);
        let approval_request = AgentMessage::request(
            AgentId::new(&team.id, "workflow", "engine"),
            approver.agent_id.clone(),
            RequestType::RequestApproval,
            serde_json::json!({
                "step_id": step.id,
                "step_name": step.name,
                "task_prompt": step.task_prompt,
                "message": format!("Please review and approve step '{}'", step.name)
            }),
            Some(context_id.clone()),
        );

        team.broadcast(approval_request).await?;

        // Wait for response message with matching context_id
        let timeout = step.timeout.unwrap_or(Duration::from_secs(300)); // 5 minutes default
        let start_time = std::time::Instant::now();
        let mut receiver = team.message_bus.subscribe_agent(&approver.agent_id);

        loop {
            // Check timeout
            if start_time.elapsed() > timeout {
                warn!("Approval timeout for step {}", step.id);
                return Ok(false);
            }

            // Wait for message with timeout
            let wait_duration = timeout.saturating_sub(start_time.elapsed());
            match tokio::time::timeout(wait_duration, receiver.recv()).await {
                Ok(Ok(message)) => {
                    // Check if this is a response to our approval request
                    // Match by correlation_id (which should match our context_id)
                    if let Some(ref msg_correlation_id) = message.correlation_id
                        && msg_correlation_id == &context_id
                        && let AgentMessageType::Response { success, payload: _ } = &message.message_type
                    {
                        info!("Received approval response for step {}: {}", step.id, success);
                        return Ok(*success);
                    }
                    // Continue waiting if not the right message
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    // Lagged messages, continue waiting
                    continue;
                }
                Ok(Err(_)) => {
                    // Channel closed
                    warn!("Message channel closed while waiting for approval");
                    return Ok(false);
                }
                Err(_) => {
                    // Timeout
                    warn!("Timeout waiting for approval response");
                    return Ok(false);
                }
            }
        }
    }

    /// Build task prompt with context variables
    fn build_task_prompt(
        &self,
        step: &WorkflowStep,
        context: &HashMap<String, serde_json::Value>,
    ) -> String {
        let mut prompt = step.task_prompt.clone();

        // Replace context variables (format: {variable_name})
        for (key, value) in context {
            let placeholder = format!("{{{}}}", key);
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            prompt = prompt.replace(&placeholder, &value_str);
        }

        prompt
    }

    /// Get workflow status
    pub async fn get_workflow(&self, workflow_id: &str) -> Option<Workflow> {
        let workflows: tokio::sync::RwLockReadGuard<'_, HashMap<String, Workflow>> =
            self.workflows.read().await;
        workflows.get(workflow_id).cloned()
    }

    /// List all workflows
    pub async fn list_workflows(&self) -> Vec<String> {
        let workflows: tokio::sync::RwLockReadGuard<'_, HashMap<String, Workflow>> =
            self.workflows.read().await;
        workflows.keys().cloned().collect()
    }
}

/// Result of workflow execution
#[derive(Debug, Clone)]
pub struct WorkflowResult {
    /// Workflow ID
    pub workflow_id: String,
    /// Whether the workflow succeeded
    pub success: bool,
    /// Number of steps completed
    pub completed_steps: usize,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Create a code review workflow
pub fn create_code_review_workflow() -> Workflow {
    Workflow::new(
        "code_review",
        "Code Review Workflow",
        vec![
            WorkflowStep {
                id: "implement".to_string(),
                name: "Implement Feature".to_string(),
                role: "developer".to_string(),
                task_prompt: "Implement the feature: {user_request}".to_string(),
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
                task_prompt: "Review the implementation for code quality, bugs, and best practices"
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
                task_prompt: "Write comprehensive tests for the implementation".to_string(),
                required_artifacts: vec!["implementation".to_string()],
                produces_artifacts: vec!["tests".to_string()],
                approval_required: false,
                approver_role: None,
                timeout: None,
            },
            WorkflowStep {
                id: "final_approval".to_string(),
                name: "Final Approval".to_string(),
                role: "reviewer".to_string(),
                task_prompt: "Perform final review and approval".to_string(),
                required_artifacts: vec!["implementation".to_string(), "tests".to_string()],
                produces_artifacts: vec!["approval".to_string()],
                approval_required: true,
                approver_role: Some("reviewer".to_string()),
                timeout: None,
            },
        ],
    )
}

/// Create a design workflow
pub fn create_design_workflow() -> Workflow {
    Workflow::new(
        "design",
        "Design Workflow",
        vec![
            WorkflowStep {
                id: "design".to_string(),
                name: "Design System".to_string(),
                role: "architect".to_string(),
                task_prompt: "Design the system architecture: {user_request}".to_string(),
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
                task_prompt: "Review the design for feasibility and implementation concerns"
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
                task_prompt: "Refine the design based on feasibility feedback".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_creation() {
        let workflow = Workflow::new(
            "test_workflow",
            "Test Workflow",
            vec![WorkflowStep {
                id: "step1".to_string(),
                name: "Step 1".to_string(),
                role: "developer".to_string(),
                task_prompt: "Do something".to_string(),
                required_artifacts: vec![],
                produces_artifacts: vec![],
                approval_required: false,
                approver_role: None,
                timeout: None,
            }],
        );

        assert_eq!(workflow.id, "test_workflow");
        assert_eq!(workflow.name, "Test Workflow");
        assert_eq!(workflow.steps.len(), 1);
        assert_eq!(workflow.status, WorkflowStatus::Pending);
    }

    #[test]
    fn test_workflow_current_step() {
        let workflow = create_code_review_workflow();
        assert!(workflow.current_step().is_some());
        assert_eq!(workflow.current_step().unwrap().id, "implement");
    }

    #[test]
    fn test_workflow_advance() {
        let mut workflow = create_code_review_workflow();
        assert_eq!(workflow.current_step, 0);
        workflow.advance_step();
        assert_eq!(workflow.current_step, 1);
    }

    #[test]
    fn test_builtin_workflows() {
        let code_review = create_code_review_workflow();
        assert_eq!(code_review.steps.len(), 4);

        let design = create_design_workflow();
        assert_eq!(design.steps.len(), 3);
    }
}
