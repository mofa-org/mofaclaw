//! Mofaclaw CLI - Command-line interface for the Mofaclaw AI assistant

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use console::Style;
use mofa_sdk::{llm::LLMAgentBuilder, skills::SkillsManager};
use mofaclaw_core::{
    channels::DiscordChannel, AgentLoop, ChannelManager, Config, ContextBuilder, DingTalkChannel,
    FeishuChannel, HeartbeatService, MessageBus, SessionManager, SubagentManager, TelegramChannel,
    load_config, provider::{OpenAIConfig, OpenAIProvider}, save_config,
    tools::{ToolRegistry, ToolRegistryExecutor},
};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::Level;
use tracing_subscriber::EnvFilter;

const MOFA_LOGO: &str = r#"
  __  __       ______
 |  \/  |     |  ____/\
 | \  / | ___ | |__ /  \
 | |\/| |/ _ \|  __/ /\ \
 | |  | | (_) | | / ____ \
 |_|  |_|\___/|_|/_/    \_\
"#;

/// Mofaclaw - Personal AI Assistant
#[derive(Parser, Debug)]
#[command(name = "mofaclaw")]
#[command(version = "0.1.3")]
#[command(about = "Mofaclaw - Personal AI Assistant", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize mofaclaw configuration and workspace
    Onboard,

    /// Start the mofaclaw gateway server
    Gateway {
        /// Gateway port
        #[arg(short, long, default_value_t = 18790)]
        port: u16,
        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Interact with the agent directly (CLI mode)
    Agent {
        /// Message to send
        #[arg(short, long)]
        message: Option<String>,
        /// Session ID
        #[arg(short, long, default_value = "cli:default")]
        session: String,
    },

    /// Show mofaclaw status
    Status,

    /// Manage channels
    Channels {
        #[command(subcommand)]
        channel_cmd: ChannelCommands,
    },

    /// Manage scheduled tasks
    Cron {
        #[command(subcommand)]
        cron_cmd: CronCommands,
    },

    /// Manage sessions
    Session {
        #[command(subcommand)]
        session_cmd: SessionCommands,
    },
}

#[derive(Subcommand, Debug)]
enum ChannelCommands {
    /// Show channel status
    Status,

    /// Link device via QR code (for WhatsApp)
    Login,
}

#[derive(Subcommand, Debug)]
enum SessionCommands {
    /// List all sessions
    List {
        /// Show detailed information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show session details
    Show {
        /// Session key
        key: String,
    },

    /// Delete a session
    Delete {
        /// Session key
        key: String,
        /// Confirm deletion without prompt
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CronCommands {
    /// List scheduled jobs
    List {
        /// Include disabled jobs
        #[arg(short, long)]
        all: bool,
    },

    /// Add a new scheduled job
    Add {
        /// Job name
        #[arg(short, long)]
        name: String,
        /// Message for agent
        #[arg(short, long)]
        message: String,
        /// Run every N seconds
        #[arg(short, long)]
        every: Option<u32>,
        /// Cron expression
        #[arg(short, long)]
        cron: Option<String>,
    },

    /// Remove a scheduled job
    Remove {
        /// Job ID to remove
        job_id: String,
    },

    /// Enable or disable a job
    Enable {
        /// Job ID
        job_id: String,
        /// Disable instead of enable
        #[arg(short, long)]
        disable: bool,
    },

    /// Manually run a job
    Run {
        /// Job ID to run
        job_id: String,
        /// Run even if disabled
        #[arg(short, long)]
        force: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Onboard => {
            command_onboard().await?;
        }
        Commands::Gateway { port, verbose } => {
            command_gateway(port, verbose).await?;
        }
        Commands::Agent { message, session } => {
            command_agent(message, session).await?;
        }
        Commands::Status => {
            command_status().await?;
        }
        Commands::Channels { channel_cmd } => {
            command_channels(channel_cmd).await?;
        }
        Commands::Cron { cron_cmd } => {
            command_cron(cron_cmd).await?;
        }
        Commands::Session { session_cmd } => {
            command_session(session_cmd).await?;
        }
    }

    Ok(())
}

/// Initialize mofaclaw configuration
async fn command_onboard() -> Result<()> {
    let green = Style::new().green();

    println!("{}{} MOFACLAW SETUP", MOFA_LOGO, green.apply_to(">>>"));

    let config_path = mofaclaw_core::get_config_path();

    if config_path.exists() {
        println!("\n⚠️  Config already exists at {}", config_path.display());
        println!("Proceed anyway? This will overwrite your existing config.");
        // In real implementation, would prompt for confirmation
    }

    // Create default config
    let config = Config::default();
    save_config(&config).await?;

    println!("\n✅ Created config at {}", config_path.display());

    // Create workspace
    let workspace = mofaclaw_core::get_workspace_path();
    tokio::fs::create_dir_all(&workspace).await?;
    println!("✅ Created workspace at {}", workspace.display());

    // Create default files
    create_workspace_templates(&workspace).await?;

    println!(
        "\n{}{} mofaclaw is ready!",
        MOFA_LOGO,
        green.apply_to(">>>")
    );
    println!("\nNext steps:");
    println!("  1. Add your API key to ~/.mofaclaw/config.json");
    println!("     Get one at: https://openrouter.ai/keys");
    println!("  2. Chat: mofaclaw agent -m \"Hello!\"");

    Ok(())
}

/// Start the mofaclaw gateway
async fn command_gateway(port: u16, verbose: bool) -> Result<()> {
    // Setup logging
    let filter = if verbose {
        EnvFilter::builder().parse("debug")?
    } else {
        EnvFilter::from_default_env().add_directive(Level::INFO.into())
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    println!(
        "{} Starting mofaclaw gateway on port {}...",
        MOFA_LOGO, port
    );

    let config = load_config().await?;

    // Get API key and base URL
    let api_key = config
        .get_api_key()
        .ok_or_else(|| anyhow::anyhow!("No API key configured"))?;

    let api_base = config.get_api_base();
    let model = config.agents.defaults.model.clone();

    // Create MoFA OpenAI provider directly
    let openai_config = OpenAIConfig::new(&api_key)
        .with_model(&model)
        .with_base_url(api_base.unwrap_or_else(|| {
            // Default to OpenRouter for Anthropic models
            if model.contains("anthropic") || model.contains("claude") {
                "https://openrouter.ai/api/v1".to_string()
            } else if model.contains("openai") || model.contains("gpt") {
                "https://openrouter.ai/api/v1".to_string()
            } else {
                "https://openrouter.ai/api/v1".to_string()
            }
        }));

    let mofa_provider = Arc::new(OpenAIProvider::with_config(openai_config));

    // Create components
    let bus = MessageBus::new();
    let sessions = std::sync::Arc::new(SessionManager::new(&config));

    // Create tool registry first (before creating LLMAgent)
    let tools = Arc::new(RwLock::new(ToolRegistry::new()));

    // Create context builder to get workspace and brave_api_key
    let context = ContextBuilder::new(&config);
    let workspace = config.workspace_path();
    let brave_api_key = config.get_brave_api_key();

    // Register default tools
    {
        let mut tools_guard = tools.write().await;
        AgentLoop::register_default_tools(&mut tools_guard, &workspace, brave_api_key, bus.clone());
    }

    // Create ToolRegistryExecutor for LLMAgentBuilder
    let tool_executor = Arc::new(ToolRegistryExecutor::new(tools.clone()));

    // Build system prompt
    let system_prompt = context.build_system_prompt(None).await.unwrap_or_else(|_| {
        "You are Mofaclaw, a helpful AI assistant with access to various tools.".to_string()
    });

    // Create LLMAgent with tools and executor
    let llm_agent = Arc::new(
        LLMAgentBuilder::new()
            .with_id("mofaclaw-gateway")
            .with_name("Mofaclaw Gateway Agent")
            .with_provider(mofa_provider.clone())
            .with_system_prompt(system_prompt)
            .with_tool_executor(tool_executor)
            .build_async()
            .await,
    );

    // Create AgentLoop with the pre-built agent AND tools
    let agent = Arc::new(
        AgentLoop::with_agent_and_tools(
            &config,
            llm_agent,
            mofa_provider,
            bus.clone(),
            sessions.clone(),
            tools.clone(),
        )
        .await?,
    );

    // Create subagent manager from agent loop
    let subagent_manager = std::sync::Arc::new(SubagentManager::new(agent.clone()));

    // re-register spawn tool with the real subagent manager
    agent.register_spawn_tool(subagent_manager.clone()).await;

    // create channel manager
    let channel_manager = ChannelManager::new(&config, bus.clone());

    // register dingtalk channel if enabled
    if config.channels.dingtalk.enabled {
        let dingtalk = DingTalkChannel::new(config.channels.dingtalk.clone(), bus.clone());
        channel_manager.register_channel(Arc::new(dingtalk)).await;
        println!("✅ DingTalk: enabled (via Python bridge on ws://localhost:3002)");
    }

    // register telegram channel if enabled
    if config.channels.telegram.enabled {
        match TelegramChannel::new(config.channels.telegram.clone(), bus.clone()) {
            Ok(telegram) => {
                channel_manager.register_channel(Arc::new(telegram)).await;
                println!("✅ Telegram: enabled");
            }
            Err(e) => {
                println!("⚠️  Telegram: failed to initialize: {}", e);
            }
        }
    }

    // register feishu channel if enabled
    if config.channels.feishu.enabled {
        let feishu = FeishuChannel::new(config.channels.feishu.clone(), bus.clone());
        channel_manager.register_channel(Arc::new(feishu)).await;
        println!("✅ Feishu: enabled (via Python bridge on ws://localhost:3004)");
    }

    // register discord channel if enabled
    if config.channels.discord.enabled {
        match DiscordChannel::new(config.channels.discord.clone(), bus.clone()) {
            Ok(discord) => {
                channel_manager.register_channel(Arc::new(discord)).await;
                println!("✅ Discord: enabled");
            }
            Err(e) => {
                println!("⚠️  Discord: failed to initialize: {}", e);
            }
        }
    }

    if channel_manager.has_enabled_channels() {
        let enabled = channel_manager.enabled_channels().join(", ");
        println!("✅ Channels: {}", enabled);
    } else {
        println!("⚠️  No channels enabled");
    }

    // Create heartbeat service
    let agent_for_heartbeat = Arc::clone(&agent);
    let heartbeat = HeartbeatService::new(
        workspace.clone(),
        30 * 60, // 30 minutes
    )
    .with_callback(Arc::new(move |prompt: String| {
        let agent = Arc::clone(&agent_for_heartbeat);
        Box::pin(async move { Ok(agent.process_direct(&prompt, "heartbeat").await?) })
    }));

    println!("✅ Heartbeat: every 30m");
    println!("✅ Ready!");

    // Start heartbeat
    heartbeat.start().await?;

    // Start everything
    tokio::select! {
        result = agent.run() => {
            result?;
        }
        result = channel_manager.start_all() => {
            result?;
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down...");
            heartbeat.stop().await;
            agent.stop().await;
            channel_manager.stop_all().await?;
        }
    }

    Ok(())
}

/// Interact with agent directly
async fn command_agent(message: Option<String>, session: String) -> Result<()> {
    let green = Style::new().green();

    let config = load_config().await?;

    let api_key = config
        .get_api_key()
        .ok_or_else(|| anyhow::anyhow!("No API key configured"))?;

    let api_base = config.get_api_base();
    let model = config.agents.defaults.model.clone();

    // Create MoFA OpenAI provider directly
    let openai_config = OpenAIConfig::new(&api_key)
        .with_model(&model)
        .with_base_url(api_base.unwrap_or_else(|| {
            // Default to OpenRouter
            "https://openrouter.ai/api/v1".to_string()
        }));

    let mofa_provider = Arc::new(OpenAIProvider::with_config(openai_config));

    let bus = MessageBus::new();
    let sessions = Arc::new(SessionManager::new(&config));

    // Create tool registry first (before creating LLMAgent)
    let tools = Arc::new(RwLock::new(ToolRegistry::new()));

    // Create context builder to get workspace and brave_api_key
    let context = ContextBuilder::new(&config);
    let workspace = config.workspace_path();
    let brave_api_key = config.get_brave_api_key();

    // Register default tools
    {
        let mut tools_guard = tools.write().await;
        AgentLoop::register_default_tools(&mut tools_guard, &workspace, brave_api_key, bus.clone());
    }

    // Create ToolRegistryExecutor for LLMAgentBuilder
    let tool_executor = Arc::new(ToolRegistryExecutor::new(tools.clone()));

    // Build system prompt
    let system_prompt = context.build_system_prompt(None).await.unwrap_or_else(|_| {
        "You are Mofaclaw, a helpful AI assistant with access to various tools.".to_string()
    });

    // Create LLMAgent with tools and executor
    let llm_agent = Arc::new(
        LLMAgentBuilder::new()
            .with_id("mofaclaw-cli")
            .with_name("Mofaclaw CLI Agent")
            .with_provider(mofa_provider.clone())
            .with_system_prompt(system_prompt)
            .with_tool_executor(tool_executor)
            .build_async()
            .await,
    );

    // Create AgentLoop with the pre-built agent AND tools
    let agent = Arc::new(
        AgentLoop::with_agent_and_tools(
            &config,
            llm_agent,
            mofa_provider,
            bus.clone(),
            sessions.clone(),
            tools.clone(),
        )
        .await?,
    );

    // Create subagent manager from agent loop
    let subagent_manager = Arc::new(SubagentManager::new(agent.clone()));

    // Re-register spawn tool with the real subagent manager
    agent.register_spawn_tool(subagent_manager.clone()).await;

    if let Some(msg) = message {
        // Single message mode
        let response = agent.process_direct(&msg, &session).await?;
        println!("\n{}{}\n", MOFA_LOGO, green.apply_to(response));
    } else {
        // Interactive mode
        println!("{} Interactive mode (Ctrl+C to exit)\n", MOFA_LOGO);

        loop {
            let user_input = console::Term::stdout().read_line()?;

            if user_input.trim().is_empty() {
                continue;
            }

            let response = agent.process_direct(&user_input, &session).await?;
            println!("\n{}{}\n", MOFA_LOGO, green.apply_to(response));
        }
    }

    Ok(())
}

/// Show status
async fn command_status() -> Result<()> {
    let green = Style::new().green();

    println!("{}\nmofaclaw Status\n", MOFA_LOGO);

    let config_path = mofaclaw_core::get_config_path();
    let workspace = mofaclaw_core::get_workspace_path();

    println!(
        "Config: {} {}",
        config_path.display(),
        if config_path.exists() {
            green.apply_to("✅")
        } else {
            console::style("❌").red()
        }
    );

    println!(
        "Workspace: {} {}",
        workspace.display(),
        if workspace.exists() {
            green.apply_to("✅")
        } else {
            console::style("❌").red()
        }
    );

    if config_path.exists() {
        let config = load_config().await?;
        println!("Model: {}", config.agents.defaults.model);

        let has_openrouter = !config.providers.openrouter.api_key.is_empty();
        let has_anthropic = !config.providers.anthropic.api_key.is_empty();
        let has_openai = !config.providers.openai.api_key.is_empty();

        println!(
            "OpenRouter API: {}",
            if has_openrouter {
                green.apply_to("✅")
            } else {
                console::style("not set").dim()
            }
        );
        println!(
            "Anthropic API: {}",
            if has_anthropic {
                green.apply_to("✅")
            } else {
                console::style("not set").dim()
            }
        );
        println!(
            "OpenAI API: {}",
            if has_openai {
                green.apply_to("✅")
            } else {
                console::style("not set").dim()
            }
        );
    }

    Ok(())
}

/// Channel commands
async fn command_channels(cmd: ChannelCommands) -> Result<()> {
    match cmd {
        ChannelCommands::Status => {
            let config = load_config().await?;

            println!("Channel Status\n");
            println!(
                "WhatsApp: {} - Bridge: {}",
                if config.channels.whatsapp.enabled {
                    "✅"
                } else {
                    "❌"
                },
                config.channels.whatsapp.bridge_url
            );
            println!(
                "Telegram: {}",
                if config.channels.telegram.enabled {
                    "✅"
                } else {
                    "❌"
                }
            );
        }
        ChannelCommands::Login => {
            command_channels_login().await?;
        }
    }

    Ok(())
}

/// Channel login command - setup and start WhatsApp bridge
async fn command_channels_login() -> Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    // User's bridge location
    let user_bridge = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join(".mofaclaw")
        .join("bridge");

    // Check if already built
    let dist_js = user_bridge.join("dist").join("index.js");
    if dist_js.exists() {
        println!(
            "{} Bridge already built at: {}",
            MOFA_LOGO,
            user_bridge.display()
        );
        println!("\nStarting bridge...");
        println!("Scan the QR code to connect.\n");

        let status = Command::new("npm")
            .arg("start")
            .current_dir(&user_bridge)
            .status()
            .context("Failed to start bridge")?;

        if !status.success() {
            return Err(anyhow!("Bridge exited with error"));
        }
        return Ok(());
    }

    // Check for npm
    let npm_exists = Command::new("npm")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !npm_exists {
        println!(
            "{}",
            yellow.apply_to("npm not found. Please install Node.js >= 18.")
        );
        return Err(anyhow!("npm not found"));
    }

    // Find source bridge
    let exe_path = std::env::current_exe()?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| anyhow!("Could not determine executable directory"))?;

    // Possible bridge locations
    let bridge_candidates = vec![
        exe_dir.join("bridge"),                // Installed with binary
        exe_dir.join("../bridge"),             // Development
        exe_dir.join("../../bridge"),          // Another dev layout
        PathBuf::from("/opt/mofaclaw/bridge"), // System installation
        PathBuf::from("./bridge"),             // Current directory
    ];

    let source = bridge_candidates
        .into_iter()
        .find(|p| p.join("package.json").exists());

    let source = match source {
        Some(s) => s,
        None => {
            println!("{}", yellow.apply_to("Bridge source not found."));
            println!("Please download from: https://github.com/lijingrs/mofaclaw");
            println!("Or install the Python package: pip install mofaclaw");
            return Err(anyhow!("Bridge source not found"));
        }
    };

    println!("{} Setting up bridge...", MOFA_LOGO);

    // Copy to user directory
    tokio::fs::create_dir_all(user_bridge.parent().unwrap())
        .await
        .context("Failed to create .mofaclaw directory")?;

    if user_bridge.exists() {
        tokio::fs::remove_dir_all(&user_bridge)
            .await
            .context("Failed to remove old bridge directory")?;
    }

    copy_dir_recursive(&source, &user_bridge)
        .await
        .context("Failed to copy bridge files")?;

    println!("  Installing dependencies...");

    let install_status = Command::new("npm")
        .arg("install")
        .current_dir(&user_bridge)
        .status()
        .context("Failed to run npm install")?;

    if !install_status.success() {
        return Err(anyhow!("npm install failed"));
    }

    println!("  Building...");

    let build_status = Command::new("npm")
        .arg("run")
        .arg("build")
        .current_dir(&user_bridge)
        .status()
        .context("Failed to run npm run build")?;

    if !build_status.success() {
        return Err(anyhow!("npm run build failed"));
    }

    println!("{} Bridge ready\n", green.apply_to("✅"));
    println!("Starting bridge...");
    println!("Scan the QR code to connect.\n");

    let start_status = Command::new("npm")
        .arg("start")
        .current_dir(&user_bridge)
        .status()
        .context("Failed to start bridge")?;

    if !start_status.success() {
        return Err(anyhow!("Bridge failed to start"));
    }

    Ok(())
}

/// Cron commands
async fn command_cron(cmd: CronCommands) -> Result<()> {
    use mofaclaw_core::cron::{CronSchedule, CronService};

    let config = load_config().await?;
    let workspace = config.workspace_path();
    let cron_store = workspace.join("cron_jobs.json");

    match cmd {
        CronCommands::List { all } => {
            let service = CronService::new(cron_store);
            let jobs = service.list_jobs(all).await;

            if jobs.is_empty() {
                println!("No scheduled jobs.");
            } else {
                println!("Scheduled Jobs:\n");
                for job in jobs {
                    let status = if job.enabled { "✓" } else { "✗" };
                    println!("  [{}] {} - {}", status, job.id, job.name);

                    // Show schedule info
                    match &job.schedule {
                        CronSchedule::Every { every_ms } => {
                            if let Some(ms) = every_ms {
                                println!("      Schedule: every {}s", ms / 1000);
                            }
                        }
                        CronSchedule::Cron { expr, .. } => {
                            if let Some(e) = expr {
                                println!("      Schedule: {}", e);
                            }
                        }
                        CronSchedule::At { at_ms } => {
                            if let Some(ms) = at_ms {
                                println!("      Schedule: at {} (timestamp)", ms);
                            }
                        }
                    }

                    // Show message
                    println!("      Message: {}", job.payload.message);

                    // Show next run time
                    if let Some(next) = job.state.next_run_at_ms {
                        let next_time =
                            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(next);
                        if let Some(dt) = next_time {
                            println!("      Next run: {}", dt.format("%Y-%m-%d %H:%M:%S UTC"));
                        }
                    }

                    println!();
                }
            }
        }
        CronCommands::Add {
            name,
            message,
            every,
            cron,
        } => {
            let schedule = if let Some(seconds) = every {
                CronSchedule::every(seconds as u64)
            } else if let Some(expr) = cron {
                CronSchedule::cron(expr)
            } else {
                return Err(anyhow!("Must specify either --every or --cron"));
            };

            let service = CronService::new(cron_store);
            let job = service
                .add_job(name, schedule, message, false, None, None)
                .await;

            println!("✅ Added job: {} ({})", job.name, job.id);
            println!("   Message: {}", job.payload.message);
        }
        CronCommands::Remove { job_id } => {
            let service = CronService::new(cron_store);
            let removed = service.remove_job(&job_id).await;

            if removed {
                println!("✅ Removed job: {}", job_id);
            } else {
                println!("⚠️  Job not found: {}", job_id);
            }
        }
        CronCommands::Enable { job_id, disable } => {
            let service = CronService::new(cron_store);
            let enabled = !disable;
            let result = service.enable_job(&job_id, enabled).await;

            if let Some(job) = result {
                let action = if enabled { "enabled" } else { "disabled" };
                println!("✅ {} job: {} ({})", action, job.name, job_id);
            } else {
                println!("⚠️  Job not found: {}", job_id);
            }
        }
        CronCommands::Run { job_id, force } => {
            let service = CronService::new(cron_store);
            let ran = service.run_job(&job_id, force).await;

            if ran {
                println!("✅ Job executed: {}", job_id);
            } else {
                println!(
                    "⚠️  Failed to run job: {} (job not found or disabled)",
                    job_id
                );
            }
        }
    }

    Ok(())
}

/// Create workspace template files
async fn create_workspace_templates(workspace: &PathBuf) -> Result<()> {
    let agents_md = r#"# Agent Instructions

You are a helpful AI assistant. Be concise, accurate, and friendly.

## Guidelines

- Always explain what you're doing before taking actions
- Ask for clarification when the request is ambiguous
- Use tools to help accomplish tasks
- Remember important information in your memory files
"#;

    let soul_md = r#"# Soul

I am mofaclaw, a lightweight AI assistant.

## Personality

- Helpful and friendly
- Concise and to the point
- Curious and eager to learn

## Values

- Accuracy over speed
- User privacy and safety
- Transparency in actions
"#;

    let user_md = r#"# User

Information about the user goes here.

## Preferences

- Communication style: (casual/formal)
- Timezone: (your timezone)
- Language: (your preferred language)
"#;

    // Create files
    tokio::fs::write(workspace.join("AGENTS.md"), agents_md).await?;
    tokio::fs::write(workspace.join("SOUL.md"), soul_md).await?;
    tokio::fs::write(workspace.join("USER.md"), user_md).await?;

    // Create memory directory
    let memory_dir = workspace.join("memory");
    tokio::fs::create_dir_all(&memory_dir).await?;

    let memory_md = r#"# Long-term Memory

This file stores important information that should persist across sessions.

## User Information

(Important facts about the user)

## Preferences

(User preferences learned over time)

## Important Notes

(Things to remember)
"#;

    tokio::fs::write(memory_dir.join("MEMORY.md"), memory_md).await?;

    // Copy builtin skills to workspace
    copy_builtin_skills(workspace).await?;

    Ok(())
}

/// Copy builtin skills from installation directory to workspace
async fn copy_builtin_skills(workspace: &PathBuf) -> Result<()> {
    let builtin_skills = SkillsManager::find_builtin_skills();

    if let Some(builtin_path) = builtin_skills {
        let workspace_skills = workspace.join("skills");

        // Create workspace skills directory
        tokio::fs::create_dir_all(&workspace_skills).await?;

        // Copy each skill directory
        let mut entries = tokio::fs::read_dir(&builtin_path).await?;
        let mut copied_count = 0;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let skill_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                let dest = workspace_skills.join(skill_name);

                // Skip if already exists
                if dest.exists() {
                    continue;
                }

                // Copy the skill directory
                copy_dir_recursive(&path, &dest).await?;
                copied_count += 1;
            }
        }

        if copied_count > 0 {
            println!(
                "✅ Copied {} builtin skills to {}",
                copied_count,
                workspace_skills.display()
            );
        }
    }

    Ok(())
}

/// Recursively copy a directory (boxed to avoid infinite future size)
async fn copy_dir_recursive(src: &PathBuf, dest: &PathBuf) -> Result<()> {
    tokio::fs::create_dir_all(dest).await?;

    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dest_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dest_path).await?;
        }
    }

    Ok(())
}

/// Session commands
async fn command_session(cmd: SessionCommands) -> Result<()> {
    let config = load_config().await?;
    let sessions = SessionManager::new(&config);

    match cmd {
        SessionCommands::List { verbose } => {
            let session_list = sessions.list_sessions().await?;

            if session_list.is_empty() {
                println!("No sessions found.");
            } else {
                println!("Sessions ({}):\n", session_list.len());

                for info in &session_list {
                    if verbose {
                        println!("  Key: {}", info.key);
                        println!(
                            "    Created: {}",
                            info.created_at
                                .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                .unwrap_or_else(|| "N/A".to_string())
                        );
                        println!(
                            "    Updated: {}",
                            info.updated_at
                                .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                .unwrap_or_else(|| "N/A".to_string())
                        );
                        println!("    Path: {}", info.path.display());
                        println!();
                    } else {
                        println!(
                            "  {} ({})",
                            info.key,
                            info.updated_at
                                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| "N/A".to_string())
                        );
                    }
                }
            }
        }
        SessionCommands::Show { key } => {
            let session = sessions.load(&key).await;

            match session {
                Ok(session) => {
                    println!("Session: {}\n", session.key);
                    println!(
                        "Created: {}",
                        session.created_at.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    println!(
                        "Updated: {}",
                        session.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                    println!("Messages: {}\n", session.len());

                    for msg in &session.messages {
                        let role_style = match msg.role.as_str() {
                            "user" => console::Style::new().cyan(),
                            "assistant" => console::Style::new().green(),
                            "system" => console::Style::new().dim(),
                            _ => console::Style::new(),
                        };

                        println!(
                            "{} [{}]",
                            role_style.apply_to(format!("{:?}", msg.role)),
                            msg.timestamp.format("%H:%M:%S")
                        );
                        println!("  {}\n", msg.content);
                    }
                }
                Err(e) => {
                    println!("Error loading session '{}': {}", key, e);
                }
            }
        }
        SessionCommands::Delete { key, force } => {
            if !force {
                println!("Are you sure you want to delete session '{}'?", key);
                println!("This action cannot be undone.");
                print!("Continue? [y/N]: ");

                std::io::stdout().flush().unwrap();

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                if !input.trim().to_lowercase().starts_with('y') {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            let deleted = sessions.delete(&key).await?;

            if deleted {
                println!("Deleted session: {}", key);
            } else {
                println!("Session not found: {}", key);
            }
        }
    }

    Ok(())
}
