//! Example: Creating and managing a multi-agent team
//!
//! This example demonstrates how to:
//! 1. Create a team with multiple agents
//! 2. Configure agents with different roles
//! 3. Use the team for collaborative tasks

use mofaclaw_core::{
    MessageBus, SessionManager, agent::collaboration::team::TeamManager, load_config,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = Arc::new(load_config().await?);
    let user_bus = MessageBus::new();
    let sessions = Arc::new(SessionManager::new(&config));

    // Create team manager
    let team_manager = Arc::new(TeamManager::new(config, user_bus, sessions));

    println!("Creating a code review team...");

    // Create a team with architect, developer, reviewer, and tester
    let roles = vec![
        ("architect".to_string(), "arch-1".to_string()),
        ("developer".to_string(), "dev-1".to_string()),
        ("reviewer".to_string(), "rev-1".to_string()),
        ("tester".to_string(), "test-1".to_string()),
    ];

    match team_manager
        .create_team("code-review-team", "Code Review Team", roles)
        .await
    {
        Ok(team) => {
            println!("✓ Team '{}' created successfully!", team.name);
            println!("  Team ID: {}", team.id);
            println!("  Members: {}", team.member_count());
            println!("\nTeam members:");
            for (instance_id, member) in &team.members {
                println!(
                    "  - {}: {} ({:?})",
                    instance_id, member.agent_id.role, member.status
                );
            }

            // List all teams
            println!("\nAll teams:");
            let teams = team_manager.list_teams().await;
            for team_id in teams {
                if let Some(t) = team_manager.get_team(&team_id).await {
                    println!("  - {} ({})", t.id, t.name);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to create team: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}
