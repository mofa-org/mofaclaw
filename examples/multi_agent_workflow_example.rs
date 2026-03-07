//! Example: Executing a multi-agent workflow
//!
//! This example demonstrates how to:
//! 1. Create a team
//! 2. Start a workflow
//! 3. Monitor workflow execution
//! 4. Check workflow results

use mofaclaw_core::{
    MessageBus, SessionManager,
    agent::collaboration::{
        team::TeamManager,
        workflow::{WorkflowEngine, create_code_review_workflow},
    },
    load_config,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = Arc::new(load_config().await?);
    let user_bus = MessageBus::new();
    let sessions = Arc::new(SessionManager::new(&config));

    // Create team manager and workflow engine
    let team_manager = Arc::new(TeamManager::new(
        config.clone(),
        user_bus.clone(),
        sessions.clone(),
    ));
    let workflow_engine = Arc::new(WorkflowEngine::new());

    println!("Setting up code review workflow...");

    // First, create a team
    let roles = vec![
        ("developer".to_string(), "dev-1".to_string()),
        ("reviewer".to_string(), "rev-1".to_string()),
        ("tester".to_string(), "test-1".to_string()),
    ];

    let team = match team_manager
        .create_team("workflow-team", "Workflow Team", roles)
        .await
    {
        Ok(t) => {
            println!("✓ Team created: {}", t.name);
            t
        }
        Err(e) => {
            eprintln!("Failed to create team: {}", e);
            return Err(e.into());
        }
    };

    // Create a code review workflow
    let workflow = create_code_review_workflow();
    println!("\n✓ Workflow created: {}", workflow.name);
    println!("  Steps: {}", workflow.steps.len());
    for (i, step) in workflow.steps.iter().enumerate() {
        println!("    {}. {} (role: {})", i + 1, step.name, step.role);
    }

    // Execute the workflow
    println!("\nExecuting workflow...");
    let mut initial_context: HashMap<String, Value> = HashMap::new();
    initial_context.insert(
        "user_request".to_string(),
        Value::String(
            "Implement a user authentication system with login and registration".to_string(),
        ),
    );

    match workflow_engine
        .execute_workflow(workflow, team.clone(), initial_context)
        .await
    {
        Ok(result) => {
            if result.success {
                println!("✓ Workflow completed successfully!");
                println!("  Completed steps: {}", result.completed_steps);
            } else {
                println!("✗ Workflow failed");
                if let Some(err) = result.error {
                    println!("  Error: {}", err);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to execute workflow: {}", e);
            return Err(e.into());
        }
    }

    // List all workflows
    println!("\nAll workflows:");
    let workflows = workflow_engine.list_workflows().await;
    for workflow_id in workflows {
        if let Some(wf) = workflow_engine.get_workflow(&workflow_id).await {
            println!("  - {} ({}) - {:?}", wf.id, wf.name, wf.status);
        }
    }

    Ok(())
}
