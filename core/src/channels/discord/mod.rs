//! discord channel using serenity and poise

pub mod components;

use super::base::Channel;
use std::collections::HashMap;
use crate::bus::MessageBus;
use crate::config::DiscordConfig;
use crate::error::{ChannelError, Result};
use crate::get_workspace_path;
use crate::messages::InboundMessage;
use crate::permissions::{PermissionLevel, PermissionManager};
use crate::rbac::{RbacManager, Role};
use async_trait::async_trait;
use poise::serenity_prelude as serenity;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// poise data
pub struct Data {
    /// message bus for agent communication
    pub bus: MessageBus,
    /// discord configuration
    pub config: DiscordConfig,
    /// active mofa-effect sessions keyed by channel id
    pub mofa_sessions: Arc<RwLock<HashMap<u64, Arc<MofaSession>>>>,
}

/// discord channel using serenity and poise
pub struct DiscordChannel {
    config: DiscordConfig,
    bus: MessageBus,
    running: Arc<RwLock<bool>>,
    http: Arc<RwLock<Option<Arc<serenity::Http>>>>,
    permissions: PermissionManager,
    rbac_manager: Option<Arc<RbacManager>>,
}

/// A running mofa-effect session (Python process) attached to a Discord channel.
/// It holds a handle to the child stdin so Discord messages can be forwarded.
pub struct MofaSession {
    pub stdin: Mutex<tokio::process::ChildStdin>,
}

/// poise error type
pub type DiscordError = serenity::Error;

/// issue action enum
#[derive(Debug, poise::ChoiceParameter)]
pub enum IssueAction {
    #[name = "create"]
    Create,
    #[name = "list"]
    List,
    #[name = "view"]
    View,
    #[name = "close"]
    Close,
    #[name = "comment"]
    Comment,
    #[name = "assign"]
    Assign,
}

/// pr action enum
#[derive(Debug, poise::ChoiceParameter)]
pub enum PrAction {
    #[name = "create"]
    Create,
    #[name = "list"]
    List,
    #[name = "view"]
    View,
    #[name = "merge"]
    Merge,
    #[name = "close"]
    Close,
    #[name = "comment"]
    Comment,
    #[name = "review"]
    Review,
}

/// pr review action enum
#[derive(Debug, poise::ChoiceParameter)]
pub enum PrReviewAction {
    #[name = "approve"]
    Approve,
    #[name = "reject"]
    Reject,
}

impl DiscordChannel {
    /// create new discord channel
    pub fn new(config: DiscordConfig, bus: MessageBus) -> Result<Self> {
        Self::with_rbac(config, bus, None)
    }

    /// create new discord channel with RBAC manager
    pub fn with_rbac(
        config: DiscordConfig,
        bus: MessageBus,
        rbac_manager: Option<Arc<RbacManager>>,
    ) -> Result<Self> {
        if config.token.is_empty() {
            return Err(ChannelError::NotConfigured("Discord".to_string()).into());
        }

        let permissions = PermissionManager::from_discord_config(&config);

        Ok(Self {
            config,
            bus,
            running: Arc::new(RwLock::new(false)),
            http: Arc::new(RwLock::new(None)),
            permissions,
            rbac_manager,
        })
    }

    /// check if a user/role is allowed
    fn is_allowed(&self, user_id: &str, roles: &[String]) -> bool {
        if self.config.allow_from.is_empty() {
            return true;
        }
        if self.config.allow_from.contains(&user_id.to_string()) {
            return true;
        }
        roles
            .iter()
            .any(|role| self.config.allow_from.contains(role))
    }

    /// check if user has admin role
    fn is_admin(&self, roles: &[String]) -> bool {
        self.permissions.has_level(roles, PermissionLevel::Admin)
    }

    /// check if user has member role (or admin role)
    fn is_member(&self, roles: &[String]) -> bool {
        self.permissions.has_level(roles, PermissionLevel::Member)
    }

    /// check if user is guest (anyone can be a guest, but not restricted)
    fn is_guest(&self, _roles: &[String]) -> bool {
        // guests have no restrictions - everyone is a guest by default
        true
    }

    /// get user roles from discord context
    async fn get_user_roles(
        guild_id: Option<serenity::GuildId>,
        http: &serenity::Http,
        user_id: serenity::UserId,
    ) -> Vec<String> {
        if let Some(gid) = guild_id
            && let Ok(member) = gid.member(http, user_id).await
        {
            return member.roles.iter().map(|r| r.to_string()).collect();
        }
        Vec::new()
    }

    /// create channel instance from poise context
    fn from_context(ctx: &poise::Context<'_, Data, DiscordError>) -> Self {
        let config = ctx.data().config.clone();
        let permissions = PermissionManager::from_discord_config(&config);

        Self {
            config,
            bus: ctx.data().bus.clone(),
            running: Arc::new(RwLock::new(true)),
            http: Arc::new(RwLock::new(None)),
            permissions,
            rbac_manager: None, // not used in this context
        }
    }

    /// Resolve user role using RBAC if enabled, otherwise fallback to old system
    fn resolve_user_role(&self, user_id: &str, discord_roles: &[String]) -> Role {
        if let Some(ref rbac) = self.rbac_manager {
            rbac.get_role_from_discord(user_id, discord_roles)
        } else {
            // Fallback to old permission system
            let level = self.permissions.level_for(discord_roles);
            Role::from(level)
        }
    }
}

/// issue management command
#[poise::command(slash_command)]
async fn issue(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "action type"] action: IssueAction,
    #[description = "issue title"] title: Option<String>,
    #[description = "issue body"] body: Option<String>,
    #[description = "issue number"] number: Option<u32>,
    #[description = "comment body"] comment: Option<String>,
    #[description = "github username to assign"] assignee: Option<String>,
    #[description = "label filter"] label: Option<String>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let roles = DiscordChannel::get_user_roles(ctx.guild_id(), ctx.http(), ctx.author().id).await;
    let channel = DiscordChannel::from_context(&ctx);

    let user_role = channel.resolve_user_role(&user_id, &roles);

    let request = match action {
        IssueAction::Create => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "issue.create") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_member(&roles) {
                    ctx.say("error: create operation requires member role")
                        .await?;
                    return Ok(());
                }
            }
            if let Some(t) = title {
                let mut req = format!("create a github issue with title: {}", t);
                if let Some(b) = body {
                    req.push_str(&format!(" and body: {}", b));
                }
                req
            } else {
                ctx.say("issue title required for create action").await?;
                return Ok(());
            }
        }
        IssueAction::List => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "issue.list") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_guest(&roles) {
                    ctx.say("error: list operation requires guest permission")
                        .await?;
                    return Ok(());
                }
            }
            let mut req = "list all github issues".to_string();
            if let Some(l) = label {
                req.push_str(&format!(" with label: {}", l));
            }
            req
        }
        IssueAction::View => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "issue.view") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_guest(&roles) {
                    ctx.say("error: view operation requires guest permission")
                        .await?;
                    return Ok(());
                }
            }
            if let Some(n) = number {
                format!("view github issue #{}", n)
            } else {
                ctx.say("issue number required for view action").await?;
                return Ok(());
            }
        }
        IssueAction::Close => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "issue.close") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_member(&roles) {
                    ctx.say("error: close operation requires member role")
                        .await?;
                    return Ok(());
                }
            }
            if let Some(n) = number {
                format!("close github issue #{}", n)
            } else {
                ctx.say("issue number required for close action").await?;
                return Ok(());
            }
        }
        IssueAction::Comment => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "issue.comment") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_member(&roles) {
                    ctx.say("error: comment operation requires member role")
                        .await?;
                    return Ok(());
                }
            }
            if let Some(n) = number {
                if let Some(c) = comment {
                    format!("comment on github issue #{} with body: {}", n, c)
                } else {
                    ctx.say("comment body required for comment action").await?;
                    return Ok(());
                }
            } else {
                ctx.say("issue number required for comment action").await?;
                return Ok(());
            }
        }
        IssueAction::Assign => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "issue.assign") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_admin(&roles) {
                    ctx.say("error: assign operation requires admin role")
                        .await?;
                    return Ok(());
                }
            }
            if let Some(n) = number {
                if let Some(github_username) = assignee {
                    if github_username.is_empty() {
                        ctx.say("error: github username cannot be empty").await?;
                        return Ok(());
                    }
                    format!(
                        "assign github issue #{} to github user: {}",
                        n, github_username
                    )
                } else {
                    ctx.say("error: github username required for assign action")
                        .await?;
                    return Ok(());
                }
            } else {
                ctx.say("issue number required for assign action").await?;
                return Ok(());
            }
        }
    };

    // send formatted embed response
    let embed = serenity::CreateEmbed::default()
        .title("GitHub Issue")
        .description(format!("Processing: {}", request))
        .color(0x5865F2) // Discord blurple
        .timestamp(serenity::Timestamp::now());

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish issue command to bus: {}", e);

        let error_embed = serenity::CreateEmbed::default()
            .title("Error")
            .description(format!("Failed to process issue request: {}", e))
            .color(0xED4245) // Discord red
            .timestamp(serenity::Timestamp::now());

        ctx.send(poise::CreateReply::default().embed(error_embed))
            .await?;
    }

    Ok(())
}

/// Run a mofa-effect terminal session and mirror its I/O in Discord.
#[poise::command(slash_command, rename = "mofa-effect")]
async fn mofa_effect(
    ctx: poise::Context<'_, Data, DiscordError>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let channel_id = ctx.channel_id();
    let channel_key = channel_id.get();

    // Check if a session is already running in this channel
    {
        let sessions_guard = ctx.data().mofa_sessions.read().await;
        if sessions_guard.contains_key(&channel_key) {
            ctx.send(
                poise::CreateReply::default()
                    .content("A mofa-effect session is already running in this channel. Finish it before starting a new one.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    }

    ctx.send(
        poise::CreateReply::default()
            .content("Starting mofa-effect session...\nAll messages you send in this channel will be forwarded to the session until it exits.")
            .ephemeral(true),
    )
    .await?;

    // Determine project root (where Cargo.toml lives)
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mofa_comp_dir = project_root.join("mofa-effect").join("comp");

    // Spawn python main.py in mofa-effect/comp
    let mut cmd = TokioCommand::new("python");
    cmd.arg("-u")
        .arg("main.py")
        .env("PYTHONUNBUFFERED", "1")
        .current_dir(&mofa_comp_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            error!("failed to spawn mofa-effect process: {}", e);
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("Failed to start mofa-effect: {}", e))
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let stdin = child.stdin.take().ok_or_else(|| {
        serenity::Error::Other("failed to capture mofa-effect stdin (no handle)")
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        serenity::Error::Other("failed to capture mofa-effect stdout (no handle)")
    })?;
    let stderr = child.stderr.take();

    let session = Arc::new(MofaSession {
        stdin: Mutex::new(stdin),
    });

    {
        let mut sessions_guard = ctx.data().mofa_sessions.write().await;
        sessions_guard.insert(channel_key, session.clone());
    }

    let http = ctx.serenity_context().http.clone();
    let sessions_map = ctx.data().mofa_sessions.clone();

    // Path to render_report.json we will watch after the Python process exits
    let report_path = mofa_comp_dir.join("output").join("render_report.json");
    let root_for_http = project_root.join("mofa-effect");

    // Background task: stream stdout/stderr to Discord and then watch for final video
    tokio::spawn(async move {
        // Stream stdout in chunks rather than one line per message.
        let mut reader = BufReader::new(stdout).lines();
        let mut buffer = String::new();
        let mut line_count: usize = 0;
        while let Ok(Some(line)) = reader.next_line().await {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }

            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(trimmed);
            line_count += 1;

            // Heuristics to decide when to flush:
            // - buffer is getting large
            // - we've accumulated enough lines
            // - this line looks like a prompt that expects user input
            let is_prompt = trimmed.ends_with(':')
                || trimmed.contains("Comp file path")
                || trimmed.contains("Make changes? (yes/no)")
                || trimmed.contains("New text (Enter to keep)")
                || trimmed.contains("New (e.g. WELCOME / TO / MOFA")
                || trimmed.contains("Your file path (or type 'None' for placeholder)")
                || trimmed.contains("New hex color, e.g. #ff3300")
                || trimmed.contains("Enter number (1-")
                || trimmed.contains("Choose the font to use for all text in this video");

            let too_big = buffer.len() > 1700 || line_count >= 12;

            if is_prompt || too_big {
                let content = format!("```text\n{}\n```", buffer);
                if let Err(e) = channel_id.say(&http, content).await {
                    error!("failed to send mofa-effect stdout to discord: {}", e);
                    break;
                }
                buffer.clear();
                line_count = 0;
            }
        }

        if !buffer.is_empty() {
            let content = format!("```text\n{}\n```", buffer);
            let _ = channel_id.say(&http, content).await;
        }

        // Optionally stream stderr (as a single block when done)
        if let Some(err) = stderr {
            let mut err_reader = BufReader::new(err);
            let mut buf = Vec::new();
            if err_reader.read_to_end(&mut buf).await.is_ok() && !buf.is_empty() {
                let text = String::from_utf8_lossy(&buf);
                let content = format!("```text\n[stderr]\n{}\n```", text);
                let _ = channel_id.say(&http, content).await;
            }
        }

        // Wait for child process to exit
        match child.wait().await {
            Ok(status) => {
                info!("mofa-effect process exited with status: {}", status);
            }
            Err(e) => {
                error!("failed to wait for mofa-effect process: {}", e);
            }
        }

        // Remove session mapping
        {
            let mut sessions_guard = sessions_map.write().await;
            sessions_guard.remove(&channel_key);
        }

        // Let the user know that setup is done and they should run the MoFA_Render script.
        let _ = channel_id
            .say(
                &http,
                "MoFA setup complete. In DaVinci Resolve, go to `Workspace → Scripts → MoFA_Render` to start rendering.\n\
I will watch for the rendered video file and post a link here once it is ready.",
            )
            .await;

        // Try to read render_report.json to find expected output path
        if let Ok(text) = tokio::fs::read_to_string(&report_path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(expected) = json
                    .get("expected_output")
                    .and_then(|v| v.as_str())
                    .map(|s| PathBuf::from(s))
                {
                    // Start python -m http.server in background (root_for_http as cwd)
                    let mut http_cmd = TokioCommand::new("python");
                    http_cmd
                        .arg("-m")
                        .arg("http.server")
                        .arg("8000")
                        .current_dir(&root_for_http)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null());
                    if let Err(e) = http_cmd.spawn() {
                        // Likely already running; log and continue
                        warn!("failed to start http.server (maybe already running): {}", e);
                    }

                    // Compute initial expected video path and parent directory
                    let mut video_path = expected;
                    let output_dir = video_path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| root_for_http.join("comp").join("output"));

                    // Wait for video file to appear. If the exact expected path never
                    // shows up (e.g. Resolve wrote .mov instead of .mp4), fall back
                    // to using the most recent *_mofa_* video in the output directory.
                    let max_wait = std::time::Duration::from_secs(60 * 60);
                    let start = std::time::Instant::now();
                    loop {
                        if start.elapsed() > max_wait {
                            let _ = channel_id
                                .say(
                                    &http,
                                    "Timed out waiting for rendered video file to appear.",
                                )
                                .await;
                            break;
                        }

                        // 1) Check the originally expected path.
                        let mut found_path: Option<PathBuf> = None;
                        if tokio::fs::metadata(&video_path)
                            .await
                            .map(|m| m.len() > 0)
                            .unwrap_or(false)
                        {
                            found_path = Some(video_path.clone());
                        } else {
                            // 2) Fallback: scan output_dir for the newest *_mofa_* .mp4/.mov.
                            if let Ok(mut dir) = tokio::fs::read_dir(&output_dir).await {
                                let mut newest: Option<(PathBuf, std::time::SystemTime)> = None;
                                while let Ok(Some(entry)) = dir.next_entry().await {
                                    let path: PathBuf = entry.path();
                                    if !path.is_file() {
                                        continue;
                                    }
                                    let name_opt = path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .map(|s| s.to_string());
                                    let name: String = match name_opt {
                                        Some(n) => n,
                                        None => continue,
                                    };
                                    if !name.contains("_mofa_") {
                                        continue;
                                    }
                                    let ext = path
                                        .extension()
                                        .and_then(|e| e.to_str())
                                        .unwrap_or("")
                                        .to_lowercase();
                                    if ext != "mp4" && ext != "mov" {
                                        continue;
                                    }
                                    if let Ok(meta) = tokio::fs::metadata(&path).await {
                                        if meta.len() == 0 {
                                            continue;
                                        }
                                        if let Ok(modified) = meta.modified() {
                                            match newest {
                                                Some((_, ts)) if modified <= ts => {}
                                                _ => {
                                                    newest = Some((path.clone(), modified));
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some((path, _)) = newest {
                                    found_path = Some(path);
                                }
                            }
                        }

                        if let Some(final_path) = found_path {
                            video_path = final_path;
                            let rel = match video_path.strip_prefix(&root_for_http) {
                                Ok(p) => p.to_path_buf(),
                                Err(_) => video_path.clone(),
                            };
                            let url = format!(
                                "http://localhost:8000/{}",
                                rel.to_string_lossy().replace('\\', "/")
                            );
                            let msg =
                                format!("Here is your video link:\n{}", url);
                            let _ = channel_id.say(&http, msg).await;
                            break;
                        }

                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        }
    });

    Ok(())
}

/// pr management command
#[poise::command(slash_command)]
async fn pr(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "action type"] action: PrAction,
    #[description = "pr title"] title: Option<String>,
    #[description = "base branch"] base: Option<String>,
    #[description = "head branch"] head: Option<String>,
    #[description = "pr number"] number: Option<u32>,
    #[description = "state (open/closed/all)"] state: Option<String>,
    #[description = "comment body"] comment: Option<String>,
    #[description = "review action"] review_action: Option<PrReviewAction>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let roles = DiscordChannel::get_user_roles(ctx.guild_id(), ctx.http(), ctx.author().id).await;
    let channel = DiscordChannel::from_context(&ctx);
    let user_role = channel.resolve_user_role(&user_id, &roles);

    let request = match action {
        PrAction::Create => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "pr.create") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_member(&roles) {
                    ctx.say("error: create operation requires member role")
                        .await?;
                    return Ok(());
                }
            }
            if let Some(t) = title {
                let mut req = format!("create a pull request with title: {}", t);
                if let Some(b) = base {
                    req.push_str(&format!(" from base: {}", b));
                }
                if let Some(h) = head {
                    req.push_str(&format!(" to head: {}", h));
                }
                req
            } else {
                ctx.say("pr title required for create action").await?;
                return Ok(());
            }
        }
        PrAction::List => {
            // List requires Guest permission (everyone can list)
            if !channel.is_guest(&roles) {
                ctx.say("error: list operation requires guest permission")
                    .await?;
                return Ok(());
            }
            let mut req = "list all pull requests".to_string();
            if let Some(s) = state {
                req.push_str(&format!(" with state: {}", s));
            }
            req
        }
        PrAction::View => {
            // View requires Guest permission (everyone can view)
            if !channel.is_guest(&roles) {
                ctx.say("error: view operation requires guest permission")
                    .await?;
                return Ok(());
            }
            if let Some(n) = number {
                format!("view pull request #{}", n)
            } else {
                ctx.say("pr number required for view action").await?;
                return Ok(());
            }
        }
        PrAction::Merge => {
            // Check permission using RBAC if enabled
            if let Some(ref rbac) = channel.rbac_manager {
                match rbac.check_permission(user_role, "skills.github", "pr.merge") {
                    crate::rbac::manager::PermissionResult::Allowed => {}
                    crate::rbac::manager::PermissionResult::Denied(reason) => {
                        ctx.say(format!("error: {}", reason)).await?;
                        return Ok(());
                    }
                }
            } else {
                // Fallback to old permission check
                if !channel.is_admin(&roles) {
                    ctx.say("error: merge operation requires admin role")
                        .await?;
                    return Ok(());
                }
            }
            if let Some(n) = number {
                // Interactive confirmation before destructive merge (closes #27)
                let prompt = format!(
                    "Merge PR #{n} into main? CI status will be checked first. This cannot be undone."
                );
                match components::confirm(&ctx, &prompt).await {
                    Ok(true) => {}
                    Ok(false) => return Ok(()),
                    Err(e) => {
                        ctx.say(format!("error: confirmation failed: {}", e))
                            .await?;
                        return Ok(());
                    }
                }
                // Check CI status before merge
                format!(
                    "check ci status for pr #{} then merge pull request #{}",
                    n, n
                )
            } else {
                ctx.say("pr number required for merge action").await?;
                return Ok(());
            }
        }
        PrAction::Close => {
            // Close requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: close operation requires member role")
                    .await?;
                return Ok(());
            }
            if let Some(n) = number {
                format!("close pull request #{}", n)
            } else {
                ctx.say("pr number required for close action").await?;
                return Ok(());
            }
        }
        PrAction::Comment => {
            // Comment requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: comment operation requires member role")
                    .await?;
                return Ok(());
            }
            if let Some(n) = number {
                if let Some(c) = comment {
                    format!("comment on pull request #{} with body: {}", n, c)
                } else {
                    ctx.say("comment body required for comment action").await?;
                    return Ok(());
                }
            } else {
                ctx.say("pr number required for comment action").await?;
                return Ok(());
            }
        }
        PrAction::Review => {
            // Review requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: review operation requires member role")
                    .await?;
                return Ok(());
            }
            if let Some(n) = number {
                if let Some(review) = review_action {
                    match review {
                        PrReviewAction::Approve => {
                            format!("approve pull request #{}", n)
                        }
                        PrReviewAction::Reject => {
                            format!("reject pull request #{}", n)
                        }
                    }
                } else {
                    ctx.say("review action (approve/reject) required for review action")
                        .await?;
                    return Ok(());
                }
            } else {
                ctx.say("pr number required for review action").await?;
                return Ok(());
            }
        }
    };

    // send formatted embed response
    let embed = serenity::CreateEmbed::default()
        .title("GitHub Pull Request")
        .description(format!("Processing: {}", request))
        .color(0x5865F2) // Discord blurple
        .timestamp(serenity::Timestamp::now());

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish pr command to bus: {}", e);

        let error_embed = serenity::CreateEmbed::default()
            .title("Error")
            .description(format!("Failed to process pr request: {}", e))
            .color(0xED4245) // Discord red
            .timestamp(serenity::Timestamp::now());

        ctx.send(poise::CreateReply::default().embed(error_embed))
            .await?;
    }

    Ok(())
}

/// view ci status command
#[poise::command(slash_command)]
async fn status(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "workflow name"] workflow: Option<String>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let request = if let Some(w) = workflow {
        format!("check ci status for workflow: {}", w)
    } else {
        "check overall ci status".to_string()
    };

    ctx.say("checking ci status...").await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish status command to bus: {}", e);
        ctx.say(format!("error: failed to check ci status: {}", e))
            .await?;
    }

    Ok(())
}

/// release management command
#[poise::command(slash_command)]
async fn release(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "release version"] version: Option<String>,
    #[description = "create or view"] action: Option<String>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let roles = DiscordChannel::get_user_roles(ctx.guild_id(), ctx.http(), ctx.author().id).await;
    let channel = DiscordChannel::from_context(&ctx);

    let request = match action.as_deref() {
        Some("create") => {
            if !channel.is_admin(&roles) {
                ctx.say("error: create release operation requires admin role")
                    .await?;
                return Ok(());
            }
            if let Some(v) = version {
                format!("create a github release with version: {}", v)
            } else {
                ctx.say("version required for create action").await?;
                return Ok(());
            }
        }
        Some("delete") => {
            // Delete requires Admin + interactive confirmation (closes #27)
            if !channel.is_admin(&roles) {
                ctx.say("error: delete release operation requires admin role")
                    .await?;
                return Ok(());
            }
            if let Some(v) = &version {
                let prompt = format!(
                    "Delete release **{}**? This is irreversible and cannot be undone.",
                    v
                );
                match components::confirm(&ctx, &prompt).await {
                    Ok(true) => {}
                    Ok(false) => return Ok(()),
                    Err(e) => {
                        ctx.say(format!("error: confirmation failed: {}", e))
                            .await?;
                        return Ok(());
                    }
                }
                format!("delete github release: {}", v)
            } else {
                ctx.say("version required for delete action").await?;
                return Ok(());
            }
        }
        _ => {
            if let Some(v) = version {
                format!("view github release: {}", v)
            } else {
                "list all github releases".to_string()
            }
        }
    };

    ctx.say("processing release request...").await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish release command to bus: {}", e);
        ctx.say(format!("error: failed to process release request: {}", e))
            .await?;
    }

    Ok(())
}

/// summarize command
#[poise::command(slash_command)]
async fn summarize(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "url or file path to summarize"] target: String,
    #[description = "summary length"] length: Option<String>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let mut request = format!("use summarize to summarize: {}", target);
    if let Some(l) = length {
        request.push_str(&format!(" with length: {}", l));
    }

    ctx.say("summarizing...").await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish summarize command to bus: {}", e);
        ctx.say(format!("error: failed to summarize: {}", e))
            .await?;
    }

    Ok(())
}

/// weather command
#[poise::command(slash_command)]
async fn weather(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "location (city, airport code, etc.)"] location: String,
    #[description = "format: compact, full, or current"] _format: Option<String>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let location_encoded = location.replace(' ', "+");
    let format_param = match _format.as_deref() {
        Some("compact") => "?format=3",
        Some("current") => "?0",
        Some("full") | None => "?T",
        _ => "?T",
    };

    let request = format!(
        "use the weather skill to get weather for {}. use curl to call wttr.in: curl -s \"wttr.in/{}?{}\" --max-time 10",
        location,
        location_encoded,
        format_param.trim_start_matches('?')
    );

    ctx.say("fetching weather...").await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish weather command to bus: {}", e);
        ctx.say(format!("error: failed to get weather: {}", e))
            .await?;
    }

    Ok(())
}

/// tmux action enum
#[derive(Debug, poise::ChoiceParameter)]
pub enum TmuxAction {
    #[name = "create"]
    Create,
    #[name = "list"]
    List,
    #[name = "attach"]
    Attach,
    #[name = "send"]
    Send,
    #[name = "capture"]
    Capture,
}

/// tmux command
#[poise::command(slash_command)]
async fn tmux(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "action type"] action: TmuxAction,
    #[description = "session name"] session: Option<String>,
    #[description = "command or keys to send"] command: Option<String>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let request = match action {
        TmuxAction::Create => {
            if let Some(s) = session {
                format!("use tmux skill to create a new tmux session named: {}", s)
            } else {
                ctx.say("session name required for create action").await?;
                return Ok(());
            }
        }
        TmuxAction::List => "use tmux skill to list all tmux sessions".to_string(),
        TmuxAction::Attach => {
            if let Some(s) = session {
                format!("use tmux skill to attach to session: {}", s)
            } else {
                ctx.say("session name required for attach action").await?;
                return Ok(());
            }
        }
        TmuxAction::Send => {
            if let Some(s) = session {
                if let Some(c) = command {
                    format!(
                        "use tmux skill to send command '{}' to tmux session: {}",
                        c, s
                    )
                } else {
                    ctx.say("command required for send action").await?;
                    return Ok(());
                }
            } else {
                ctx.say("session name required for send action").await?;
                return Ok(());
            }
        }
        TmuxAction::Capture => {
            if let Some(s) = session {
                format!("use tmux skill to capture output from session: {}", s)
            } else {
                ctx.say("session name required for capture action").await?;
                return Ok(());
            }
        }
    };

    ctx.say("processing tmux request...").await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish tmux command to bus: {}", e);
        ctx.say(format!("error: failed to process tmux request: {}", e))
            .await?;
    }

    Ok(())
}

/// skill action enum
#[derive(Debug, poise::ChoiceParameter)]
pub enum SkillAction {
    #[name = "create"]
    Create,
    #[name = "update"]
    Update,
    #[name = "list"]
    List,
    #[name = "view"]
    View,
}

/// skill command (skill-creator)
#[poise::command(slash_command)]
async fn skill(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "action type"] action: SkillAction,
    #[description = "skill name"] name: Option<String>,
    #[description = "skill description"] description: Option<String>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let user_id = ctx.author().id.to_string();
    let username = ctx.author().name.clone();
    let sender_id = format!("{}|{}", user_id, username);
    let chat_id = ctx.channel_id().to_string();

    let roles = DiscordChannel::get_user_roles(ctx.guild_id(), ctx.http(), ctx.author().id).await;
    let channel = DiscordChannel::from_context(&ctx);

    let request = match action {
        SkillAction::Create => {
            if !channel.is_admin(&roles) {
                ctx.say("error: create skill operation requires admin role")
                    .await?;
                return Ok(());
            }
            if let Some(n) = name {
                if let Some(d) = description {
                    format!(
                        "use skill-creator to create a new skill named '{}' with description: {}",
                        n, d
                    )
                } else {
                    format!("use skill-creator to create a new skill named: {}", n)
                }
            } else {
                ctx.say("skill name required for create action").await?;
                return Ok(());
            }
        }
        SkillAction::Update => {
            if !channel.is_admin(&roles) {
                ctx.say("error: update skill operation requires admin role")
                    .await?;
                return Ok(());
            }
            if let Some(n) = name {
                format!("use skill-creator to update skill: {}", n)
            } else {
                ctx.say("skill name required for update action").await?;
                return Ok(());
            }
        }
        SkillAction::List => "use skill-creator to list all available skills".to_string(),
        SkillAction::View => {
            if let Some(n) = name {
                format!("use skill-creator to view details of skill: {}", n)
            } else {
                ctx.say("skill name required for view action").await?;
                return Ok(());
            }
        }
    };

    ctx.say("processing skill request...").await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish skill command to bus: {}", e);
        ctx.say(format!("error: failed to process skill request: {}", e))
            .await?;
    }

    Ok(())
}

/// workspace file enum
#[derive(Debug, poise::ChoiceParameter)]
pub enum WorkspaceFile {
    #[name = "soul"]
    Soul,
    #[name = "user"]
    User,
    #[name = "agents"]
    Agents,
    #[name = "tools"]
    Tools,
    #[name = "heartbeat"]
    Heartbeat,
}

/// workspace intent from keyword matching
enum WorkspaceIntent {
    ViewFile(WorkspaceFile),
    HeartbeatList,
    MemoryView,
    None,
}

/// general command intent from keyword matching
enum CommandIntent {
    Workspace(WorkspaceIntent),
    Issue(String),
    Pr(String),
    Status(String),
    Release(String),
    Summarize(String),
    Weather(String),
    Tmux(String),
    Skill(String),
    None,
}

/// match workspace keywords to intent
fn match_workspace_keywords(content: &str) -> WorkspaceIntent {
    let lower = content.to_lowercase();

    // workspace view file patterns
    if lower.contains("show workspace")
        || lower.contains("view workspace")
        || lower.contains("workspace")
    {
        if lower.contains("soul") {
            return WorkspaceIntent::ViewFile(WorkspaceFile::Soul);
        }
        if lower.contains("user") {
            return WorkspaceIntent::ViewFile(WorkspaceFile::User);
        }
        if lower.contains("agent") {
            return WorkspaceIntent::ViewFile(WorkspaceFile::Agents);
        }
        if lower.contains("tool") {
            return WorkspaceIntent::ViewFile(WorkspaceFile::Tools);
        }
        if lower.contains("heartbeat") {
            return WorkspaceIntent::ViewFile(WorkspaceFile::Heartbeat);
        }
    }

    // heartbeat list patterns
    if lower.contains("heartbeat")
        && (lower.contains("list") || lower.contains("task") || lower.contains("show"))
    {
        return WorkspaceIntent::HeartbeatList;
    }

    // memory view patterns
    if lower.contains("memory") || lower.contains("memories") {
        return WorkspaceIntent::MemoryView;
    }

    WorkspaceIntent::None
}

/// match general command keywords to intent
fn match_command_keywords(content: &str) -> CommandIntent {
    let lower = content.to_lowercase();

    // workspace commands (handled separately)
    let workspace_intent = match_workspace_keywords(content);
    if !matches!(workspace_intent, WorkspaceIntent::None) {
        return CommandIntent::Workspace(workspace_intent);
    }

    // github issue patterns
    if lower.contains("issue") {
        if lower.contains("create") || lower.contains("new") {
            // extract title after "create issue" or "new issue"
            let title =
                extract_after_keywords(&lower, &["create issue", "new issue", "issue create"]);
            if !title.is_empty() {
                return CommandIntent::Issue(format!(
                    "create a github issue with title: {}",
                    title
                ));
            }
            return CommandIntent::Issue("create a github issue".to_string());
        }
        if lower.contains("list") || lower.contains("show all") {
            let label = extract_after_keywords(
                &lower,
                &["list issues", "list issue", "issue list", "show issues"],
            );
            if !label.is_empty() {
                return CommandIntent::Issue(format!(
                    "list all github issues with label: {}",
                    label
                ));
            }
            return CommandIntent::Issue("list all github issues".to_string());
        }
        if lower.contains("view") || lower.contains("show") {
            // extract issue number
            if let Some(num) = extract_number(&lower) {
                return CommandIntent::Issue(format!("view github issue #{}", num));
            }
        }
        if lower.contains("close") {
            if let Some(num) = extract_number(&lower) {
                return CommandIntent::Issue(format!("close github issue #{}", num));
            }
        }
        if lower.contains("comment") {
            if let Some(num) = extract_number(&lower) {
                let comment_body = extract_after_keywords(
                    &lower,
                    &["comment on issue", "comment issue", "issue comment"],
                );
                if !comment_body.is_empty() {
                    return CommandIntent::Issue(format!(
                        "comment on github issue #{} with body: {}",
                        num, comment_body
                    ));
                }
                return CommandIntent::Issue(format!("comment on github issue #{}", num));
            }
        }
        if lower.contains("assign") {
            if let Some(num) = extract_number(&lower) {
                let user =
                    extract_after_keywords(&lower, &["assign issue", "issue assign", "assign to"]);
                if !user.is_empty() {
                    return CommandIntent::Issue(format!(
                        "assign github issue #{} to user: {}",
                        num, user
                    ));
                }
                return CommandIntent::Issue(format!("assign github issue #{}", num));
            }
        }
    }

    // github pr patterns
    if lower.contains("pr") || lower.contains("pull request") {
        if lower.contains("create") || lower.contains("new") {
            let title = extract_after_keywords(
                &lower,
                &["create pr", "new pr", "pr create", "create pull request"],
            );
            if !title.is_empty() {
                return CommandIntent::Pr(format!("create a github pr with title: {}", title));
            }
            return CommandIntent::Pr("create a github pr".to_string());
        }
        if lower.contains("list") || lower.contains("show all") {
            let state =
                extract_after_keywords(&lower, &["list pr", "pr list", "list pull request"]);
            if !state.is_empty() {
                return CommandIntent::Pr(format!("list all github prs with state: {}", state));
            }
            return CommandIntent::Pr("list all github prs".to_string());
        }
        if lower.contains("view") || lower.contains("show") {
            if let Some(num) = extract_number(&lower) {
                return CommandIntent::Pr(format!("view github pr #{}", num));
            }
        }
        if lower.contains("merge") {
            if let Some(num) = extract_number(&lower) {
                return CommandIntent::Pr(format!(
                    "check ci status for pr #{} then merge github pr #{}",
                    num, num
                ));
            }
        }
        if lower.contains("close") {
            if let Some(num) = extract_number(&lower) {
                return CommandIntent::Pr(format!("close github pr #{}", num));
            }
        }
        if lower.contains("comment") {
            if let Some(num) = extract_number(&lower) {
                let comment_body =
                    extract_after_keywords(&lower, &["comment on pr", "comment pr", "pr comment"]);
                if !comment_body.is_empty() {
                    return CommandIntent::Pr(format!(
                        "comment on github pr #{} with body: {}",
                        num, comment_body
                    ));
                }
                return CommandIntent::Pr(format!("comment on github pr #{}", num));
            }
        }
        if lower.contains("review") || lower.contains("approve") || lower.contains("reject") {
            if let Some(num) = extract_number(&lower) {
                if lower.contains("approve") {
                    return CommandIntent::Pr(format!("approve pull request #{}", num));
                } else if lower.contains("reject") {
                    return CommandIntent::Pr(format!("reject pull request #{}", num));
                }
                return CommandIntent::Pr(format!("review pull request #{}", num));
            }
        }
    }

    // ci status patterns
    if lower.contains("ci status") || lower.contains("status") || lower.contains("ci") {
        let workflow = extract_after_keywords(&lower, &["status", "ci status"]);
        if !workflow.is_empty() {
            return CommandIntent::Status(format!("check ci status for workflow: {}", workflow));
        }
        return CommandIntent::Status("check overall ci status".to_string());
    }

    // release patterns
    if lower.contains("release") {
        if lower.contains("create") || lower.contains("new") {
            let version = extract_after_keywords(
                &lower,
                &["create release", "new release", "release create"],
            );
            if !version.is_empty() {
                return CommandIntent::Release(format!("create release {}", version));
            }
            return CommandIntent::Release("create release".to_string());
        }
        if lower.contains("list") {
            return CommandIntent::Release("list all releases".to_string());
        }
    }

    // summarize patterns
    if lower.contains("summarize") || lower.contains("summary") {
        let target = extract_after_keywords(&lower, &["summarize", "summary"]);
        if !target.is_empty() {
            return CommandIntent::Summarize(format!("summarize {}", target));
        }
        return CommandIntent::Summarize("summarize".to_string());
    }

    // weather patterns
    if lower.contains("weather") || lower.contains("temperature") || lower.contains("forecast") {
        let location = extract_after_keywords(
            &lower,
            &[
                "weather in",
                "weather for",
                "weather",
                "temperature in",
                "forecast for",
            ],
        );
        if !location.is_empty() {
            let location_encoded = location.replace(' ', "+");
            return CommandIntent::Weather(format!(
                "use the weather skill to get weather for {}. use curl to call wttr.in: curl -s \"wttr.in/{}?T\" --max-time 10",
                location, location_encoded
            ));
        }
    }

    // tmux patterns
    if lower.contains("tmux") {
        if lower.contains("create") || lower.contains("new") {
            let session =
                extract_after_keywords(&lower, &["create tmux", "new tmux", "tmux create"]);
            if !session.is_empty() {
                return CommandIntent::Tmux(format!(
                    "use tmux skill to create session: {}",
                    session
                ));
            }
        }
        if lower.contains("list") {
            return CommandIntent::Tmux("use tmux skill to list all sessions".to_string());
        }
        if lower.contains("attach") {
            let session = extract_after_keywords(&lower, &["attach", "attach to"]);
            if !session.is_empty() {
                return CommandIntent::Tmux(format!(
                    "use tmux skill to attach to session: {}",
                    session
                ));
            }
        }
    }

    // skill patterns
    if lower.contains("skill") {
        if lower.contains("create") || lower.contains("new") {
            let name =
                extract_after_keywords(&lower, &["create skill", "new skill", "skill create"]);
            if !name.is_empty() {
                return CommandIntent::Skill(format!(
                    "use skill-creator to create skill: {}",
                    name
                ));
            }
        }
        if lower.contains("list") {
            return CommandIntent::Skill("use skill-creator to list all skills".to_string());
        }
        if lower.contains("view") || lower.contains("show") {
            let name = extract_after_keywords(&lower, &["view skill", "show skill", "skill view"]);
            if !name.is_empty() {
                return CommandIntent::Skill(format!("use skill-creator to view skill: {}", name));
            }
        }
    }

    CommandIntent::None
}

/// extract text after keywords
fn extract_after_keywords(text: &str, keywords: &[&str]) -> String {
    for keyword in keywords {
        if let Some(pos) = text.find(keyword) {
            let after = &text[pos + keyword.len()..];
            let trimmed = after.trim();
            if !trimmed.is_empty() {
                // take up to next keyword or end
                let end = keywords
                    .iter()
                    .filter_map(|k| after.find(k))
                    .min()
                    .unwrap_or(after.len());
                return after[..end].trim().to_string();
            }
        }
    }
    String::new()
}

/// extract number from text (e.g., "issue #123" -> Some(123))
fn extract_number(text: &str) -> Option<u32> {
    // look for # followed by digits
    if let Some(pos) = text.find('#') {
        let after = &text[pos + 1..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    // look for standalone numbers
    let words: Vec<&str> = text.split_whitespace().collect();
    for word in words {
        if let Ok(num) = word.parse::<u32>() {
            if num > 0 && num < 100000 {
                return Some(num);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_workspace_soul() {
        let intent = match_command_keywords("show workspace soul file");
        match intent {
            CommandIntent::Workspace(WorkspaceIntent::ViewFile(WorkspaceFile::Soul)) => {}
            _ => panic!("unexpected intent for workspace soul"),
        }
    }

    #[test]
    fn test_match_issue_create_with_title() {
        let intent = match_command_keywords("please create issue login bug");
        match intent {
            CommandIntent::Issue(s) => {
                assert_eq!(s, "create a github issue with title: login bug");
            }
            _ => panic!("expected issue intent"),
        }
    }

    #[test]
    fn test_match_pr_list_with_state() {
        let intent = match_command_keywords("can you list pr open");
        match intent {
            CommandIntent::Pr(s) => {
                assert_eq!(s, "list all github prs with state: open");
            }
            _ => panic!("expected pr intent"),
        }
    }

    #[test]
    fn test_match_weather_location() {
        let intent = match_command_keywords("what's the weather in New York");
        match intent {
            CommandIntent::Weather(s) => {
                assert!(s.contains("weather for new york"));
            }
            _ => panic!("expected weather intent"),
        }
    }

    #[test]
    fn test_extract_number_from_hash() {
        assert_eq!(extract_number("please check issue #123"), Some(123));
    }

    #[test]
    fn test_extract_number_from_standalone() {
        assert_eq!(extract_number("view pr 42 please"), Some(42));
    }
}

/// execute workspace view file (for natural language)
async fn execute_workspace_view(
    http: &serenity::Http,
    channel_id: serenity::ChannelId,
    file: WorkspaceFile,
) -> std::result::Result<(), serenity::Error> {
    let workspace = get_workspace_path();
    let file_path = match file {
        WorkspaceFile::Soul => workspace.join("SOUL.md"),
        WorkspaceFile::User => workspace.join("USER.md"),
        WorkspaceFile::Agents => workspace.join("AGENTS.md"),
        WorkspaceFile::Tools => workspace.join("TOOLS.md"),
        WorkspaceFile::Heartbeat => workspace.join("HEARTBEAT.md"),
    };

    match tokio::fs::read_to_string(&file_path).await {
        Ok(content) => {
            let preview = if content.len() > 1900 {
                format!(
                    "{}...\n\n*(truncated, file is {} chars)*",
                    &content[..1900],
                    content.len()
                )
            } else {
                content
            };
            channel_id
                .send_message(
                    http,
                    serenity::CreateMessage::new().content(format!(
                        "**{}**\n```\n{}\n```",
                        file_path.file_name().unwrap_or_default().to_string_lossy(),
                        preview
                    )),
                )
                .await?;
        }
        Err(e) => {
            channel_id
                .send_message(
                    http,
                    serenity::CreateMessage::new().content(format!("error reading file: {}", e)),
                )
                .await?;
        }
    }
    Ok(())
}

/// execute workspace heartbeat list (for natural language)
async fn execute_workspace_heartbeat_list(
    http: &serenity::Http,
    channel_id: serenity::ChannelId,
) -> std::result::Result<(), serenity::Error> {
    let workspace = get_workspace_path();
    let file_path = workspace.join("HEARTBEAT.md");

    match tokio::fs::read_to_string(&file_path).await {
        Ok(content) => {
            let tasks: Vec<&str> = content
                .lines()
                .filter(|line| line.trim_start().starts_with("- [ ]"))
                .collect();

            if tasks.is_empty() {
                channel_id
                    .send_message(
                        http,
                        serenity::CreateMessage::new().content("no active heartbeat tasks"),
                    )
                    .await?;
            } else {
                let task_list = tasks.join("\n");
                let preview = if task_list.len() > 1900 {
                    format!("{}...\n\n*(truncated)*", &task_list[..1900])
                } else {
                    task_list
                };
                channel_id
                    .send_message(
                        http,
                        serenity::CreateMessage::new()
                            .content(format!("**Active Heartbeat Tasks**\n```\n{}\n```", preview)),
                    )
                    .await?;
            }
        }
        Err(e) => {
            channel_id
                .send_message(
                    http,
                    serenity::CreateMessage::new()
                        .content(format!("error reading heartbeat: {}", e)),
                )
                .await?;
        }
    }
    Ok(())
}

/// execute workspace memory view (for natural language)
async fn execute_workspace_memory_view(
    http: &serenity::Http,
    channel_id: serenity::ChannelId,
) -> std::result::Result<(), serenity::Error> {
    let workspace = get_workspace_path();
    let memory_dir = workspace.join("memory");
    let memory_file = memory_dir.join("MEMORY.md");

    match tokio::fs::read_to_string(&memory_file).await {
        Ok(content) => {
            let preview = if content.len() > 1900 {
                format!(
                    "{}...\n\n*(truncated, file is {} chars)*",
                    &content[..1900],
                    content.len()
                )
            } else {
                content
            };
            channel_id
                .send_message(
                    http,
                    serenity::CreateMessage::new()
                        .content(format!("**Memory**\n```\n{}\n```", preview)),
                )
                .await?;
        }
        Err(e) => {
            channel_id
                .send_message(
                    http,
                    serenity::CreateMessage::new().content(format!(
                        "error reading memory: {} (file may not exist yet)",
                        e
                    )),
                )
                .await?;
        }
    }
    Ok(())
}

/// workspace view command
#[poise::command(slash_command)]
async fn workspace_view(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "file to view"] file: WorkspaceFile,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let workspace = get_workspace_path();
    let file_path = match file {
        WorkspaceFile::Soul => workspace.join("SOUL.md"),
        WorkspaceFile::User => workspace.join("USER.md"),
        WorkspaceFile::Agents => workspace.join("AGENTS.md"),
        WorkspaceFile::Tools => workspace.join("TOOLS.md"),
        WorkspaceFile::Heartbeat => workspace.join("HEARTBEAT.md"),
    };

    match tokio::fs::read_to_string(&file_path).await {
        Ok(content) => {
            let preview = if content.len() > 1900 {
                format!(
                    "{}...\n\n*(truncated, file is {} chars)*",
                    &content[..1900],
                    content.len()
                )
            } else {
                content
            };
            ctx.send(
                poise::CreateReply::default()
                    .content(format!(
                        "**{}**\n```\n{}\n```",
                        file_path.file_name().unwrap_or_default().to_string_lossy(),
                        preview
                    ))
                    .ephemeral(true),
            )
            .await?;
        }
        Err(e) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("error reading file: {}", e))
                    .ephemeral(true),
            )
            .await?;
        }
    }

    Ok(())
}

/// workspace heartbeat list command
#[poise::command(slash_command)]
async fn workspace_heartbeat_list(
    ctx: poise::Context<'_, Data, DiscordError>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let workspace = get_workspace_path();
    let file_path = workspace.join("HEARTBEAT.md");

    match tokio::fs::read_to_string(&file_path).await {
        Ok(content) => {
            let tasks: Vec<&str> = content
                .lines()
                .filter(|line| line.trim_start().starts_with("- [ ]"))
                .collect();

            if tasks.is_empty() {
                ctx.send(
                    poise::CreateReply::default()
                        .content("no active heartbeat tasks")
                        .ephemeral(true),
                )
                .await?;
            } else {
                let task_list = tasks.join("\n");
                let preview = if task_list.len() > 1900 {
                    format!("{}...\n\n*(truncated)*", &task_list[..1900])
                } else {
                    task_list
                };
                ctx.send(
                    poise::CreateReply::default()
                        .content(format!("**Active Heartbeat Tasks**\n```\n{}\n```", preview))
                        .ephemeral(true),
                )
                .await?;
            }
        }
        Err(e) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("error reading heartbeat: {}", e))
                    .ephemeral(true),
            )
            .await?;
        }
    }

    Ok(())
}

/// workspace memory view command
#[poise::command(slash_command)]
async fn workspace_memory_view(
    ctx: poise::Context<'_, Data, DiscordError>,
) -> std::result::Result<(), DiscordError> {
    // show typing indicator
    let _ = ctx.channel_id().start_typing(&ctx.serenity_context().http);

    let workspace = get_workspace_path();
    let memory_dir = workspace.join("memory");
    let memory_file = memory_dir.join("MEMORY.md");

    match tokio::fs::read_to_string(&memory_file).await {
        Ok(content) => {
            let preview = if content.len() > 1900 {
                format!(
                    "{}...\n\n*(truncated, file is {} chars)*",
                    &content[..1900],
                    content.len()
                )
            } else {
                content
            };
            ctx.send(
                poise::CreateReply::default()
                    .content(format!("**Memory**\n```\n{}\n```", preview))
                    .ephemeral(true),
            )
            .await?;
        }
        Err(e) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(format!(
                        "error reading memory: {} (file may not exist yet)",
                        e
                    ))
                    .ephemeral(true),
            )
            .await?;
        }
    }

    Ok(())
}

/// help command
#[poise::command(slash_command)]
async fn help(
    ctx: poise::Context<'_, Data, DiscordError>,
    #[description = "command name"] command: Option<String>,
) -> std::result::Result<(), DiscordError> {
    if let Some(cmd) = command {
        let help_text = match cmd.as_str() {
            "issue" => {
                "**Issue Management**\n`/issue create <title> [body]` - create new issue (member)\n`/issue list [label]` - list all issues (guest)\n`/issue view <number>` - view issue details (guest)\n`/issue close <number>` - close issue (member)\n`/issue comment <number> <body>` - comment on issue (member)\n`/issue assign <number> <github_username>` - assign issue to github user (admin)"
            }
            "pr" => {
                "**PR Management**\n`/pr create <title> [base] [head]` - create new pr (member)\n`/pr list [state]` - list prs (guest)\n`/pr view <number>` - view pr details (guest)\n`/pr merge <number>` - merge pr (admin only, checks CI)\n`/pr comment <number> <body>` - comment on pr (member)\n`/pr review <number> <approve|reject>` - code review (member)\n`/pr close <number>` - close pr (member)"
            }
            "status" => {
                "**CI Status**\n`/status [workflow]` - check ci status for workflow or overall"
            }
            "release" => {
                "**Release Management**\n`/release [version] create` - create release (admin only)\n`/release [version]` - view release\n`/release` - list all releases"
            }
            "summarize" => {
                "**Summarize**\n`/summarize <url|file> [length]` - summarize url or file\nlength: short|medium|long|xl|xxl"
            }
            "weather" => {
                "**Weather**\n`/weather <location> [format]` - get weather info\nformat: compact|full|current"
            }
            "tmux" => {
                "**Tmux**\n`/tmux create <session>` - create session\n`/tmux list` - list sessions\n`/tmux attach <session>` - attach to session\n`/tmux send <session> <command>` - send command\n`/tmux capture <session>` - capture output"
            }
            "skill" => {
                "**Skill Management**\n`/skill create <name> [description]` - create skill (admin only)\n`/skill update <name>` - update skill (admin only)\n`/skill list` - list all skills\n`/skill view <name>` - view skill details"
            }
            "workspace" => {
                "**Workspace Management**\n`/workspace view <file>` - view workspace file (soul|user|agents|tools|heartbeat)\n`/workspace heartbeat list` - list heartbeat tasks\n`/workspace memory view` - view memory"
            }
            _ => "unknown command. use `/help` to see all commands",
        };
        ctx.send(
            poise::CreateReply::default()
                .content(help_text)
                .ephemeral(true),
        )
        .await?;
    } else {
        let help_text = r#"**Help**

**GitHub:**
`/issue` | `/pr` | `/status` | `/release`

**Tools:**
`/summarize` | `/weather` | `/tmux` | `/skill`

**Workspace:**
`/workspace` | `/workspace heartbeat` | `/workspace memory`

**Info:**
`/help [command]`

More: `/help <command>` for detailed usage"#;
        ctx.send(
            poise::CreateReply::default()
                .content(help_text)
                .ephemeral(true),
        )
        .await?;
    }
    Ok(())
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            info!("discord channel disabled");
            return Ok(());
        }

        if self.config.token.is_empty() {
            return Err(ChannelError::NotConfigured("Discord".to_string()).into());
        }

        *self.running.write().await = true;

        let config = self.config.clone();
        let bus = self.bus.clone();
        let running = Arc::clone(&self.running);
        let http_ref = Arc::clone(&self.http);

        info!("starting discord bot");

        let bus_clone = bus.clone();
        let config_clone = config.clone();
        let http_ref_setup = http_ref.clone();
        let bus_message = bus.clone();
        let config_message = config.clone();

        let intents = serenity::GatewayIntents::non_privileged()
            | serenity::GatewayIntents::MESSAGE_CONTENT
            | serenity::GatewayIntents::GUILD_MESSAGES
            | serenity::GatewayIntents::DIRECT_MESSAGES;

        struct MessageHandler {
            bus: MessageBus,
            config: DiscordConfig,
            mofa_sessions: Arc<RwLock<HashMap<u64, Arc<MofaSession>>>>,
        }

        #[serenity::async_trait]
        impl serenity::EventHandler for MessageHandler {
            async fn message(&self, ctx: serenity::Context, new_message: serenity::Message) {
                if new_message.author.bot {
                    return;
                }

                // If a mofa-effect session is active for this channel, forward input to it
                let channel_key = new_message.channel_id.get();
                {
                    let sessions_guard = self.mofa_sessions.read().await;
                    if let Some(session) = sessions_guard.get(&channel_key) {
                        let mut stdin = session.stdin.lock().await;
                        if let Err(e) = stdin
                            .write_all(format!("{}\n", new_message.content).as_bytes())
                            .await
                        {
                            error!("failed to write to mofa-effect stdin: {}", e);
                        }
                        return;
                    }
                }

                let in_guild = new_message.guild_id.is_some();
                if in_guild {
                    let bot_id = match ctx.http.get_current_user().await {
                        Ok(user) => user.id,
                        Err(e) => {
                            error!("failed to get current discord user: {}", e);
                            return;
                        }
                    };

                    let mentioned = new_message.mentions.iter().any(|user| user.id == bot_id);
                    if !mentioned {
                        debug!("skipping message without mention in guild channel");
                        return;
                    }
                }

                let user_id = new_message.author.id.to_string();
                let username = new_message.author.name.clone();
                let sender_id = format!("{}|{}", user_id, username);
                let chat_id = new_message.channel_id.to_string();

                let roles = if let Some(guild_id) = new_message.guild_id {
                    if let Ok(member) = guild_id.member(&ctx.http, new_message.author.id).await {
                        member
                            .roles
                            .iter()
                            .map(|r| r.to_string())
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

                let channel = DiscordChannel::new(self.config.clone(), self.bus.clone()).unwrap_or(
                    DiscordChannel {
                        config: self.config.clone(),
                        bus: self.bus.clone(),
                        running: Arc::new(RwLock::new(true)),
                        http: Arc::new(RwLock::new(None)),
                        permissions: PermissionManager::from_discord_config(&self.config),
                        rbac_manager: None,
                    },
                );

                if !channel.is_allowed(&user_id, &roles) {
                    debug!("message from unauthorized user: {}", user_id);
                    return;
                }

                let content = new_message.content.clone();
                if content.trim().is_empty() {
                    debug!("empty or whitespace-only message from {}", sender_id);
                    return;
                }

                debug!("discord message from {}: {}", sender_id, content);

                // show typing indicator
                let _ = new_message.channel_id.start_typing(&ctx.http);

                // check for keyword matches (intent recognition via keyword matching)
                match match_command_keywords(&content) {
                    CommandIntent::Workspace(WorkspaceIntent::ViewFile(file)) => {
                        debug!("matched workspace view file keyword: {:?}", file);
                        if let Err(e) =
                            execute_workspace_view(ctx.http.as_ref(), new_message.channel_id, file)
                                .await
                        {
                            error!("failed to execute workspace view: {}", e);
                            let _ = new_message
                                .channel_id
                                .send_message(
                                    ctx.http.as_ref(),
                                    serenity::CreateMessage::new().content(format!("error: {}", e)),
                                )
                                .await;
                        }
                        return;
                    }
                    CommandIntent::Workspace(WorkspaceIntent::HeartbeatList) => {
                        debug!("matched workspace heartbeat list keyword");
                        if let Err(e) = execute_workspace_heartbeat_list(
                            ctx.http.as_ref(),
                            new_message.channel_id,
                        )
                        .await
                        {
                            error!("failed to execute workspace heartbeat list: {}", e);
                            let _ = new_message
                                .channel_id
                                .send_message(
                                    ctx.http.as_ref(),
                                    serenity::CreateMessage::new().content(format!("error: {}", e)),
                                )
                                .await;
                        }
                        return;
                    }
                    CommandIntent::Workspace(WorkspaceIntent::MemoryView) => {
                        debug!("matched workspace memory view keyword");
                        if let Err(e) =
                            execute_workspace_memory_view(ctx.http.as_ref(), new_message.channel_id)
                                .await
                        {
                            error!("failed to execute workspace memory view: {}", e);
                            let _ = new_message
                                .channel_id
                                .send_message(
                                    ctx.http.as_ref(),
                                    serenity::CreateMessage::new().content(format!("error: {}", e)),
                                )
                                .await;
                        }
                        return;
                    }
                    CommandIntent::Workspace(WorkspaceIntent::None) | CommandIntent::None => {
                        // no keyword match, fall back to llm intent recognition
                        debug!("no keyword match, using llm intent recognition");
                    }
                    CommandIntent::Issue(request) => {
                        debug!("matched issue keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish issue command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Pr(request) => {
                        debug!("matched pr keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish pr command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Status(request) => {
                        debug!("matched status keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish status command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Release(request) => {
                        debug!("matched release keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish release command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Summarize(request) => {
                        debug!("matched summarize keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish summarize command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Weather(request) => {
                        debug!("matched weather keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish weather command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Tmux(request) => {
                        debug!("matched tmux keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish tmux command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Skill(request) => {
                        debug!("matched skill keyword, sending to agent: {}", request);
                        let inbound_msg =
                            InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish skill command to bus: {}", e);
                        }
                        return;
                    }
                }

                // fall back to llm-based intent recognition for unmatched messages
                let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &content);

                if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                    error!("failed to publish message to bus: {}", e);
                }
            }

            async fn ready(&self, _ctx: serenity::Context, ready: serenity::Ready) {
                info!("discord bot connected as {}", ready.user.name);
            }
        }

        let mofa_sessions: Arc<RwLock<HashMap<u64, Arc<MofaSession>>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let mofa_sessions_for_setup = mofa_sessions.clone();

        let framework = poise::Framework::builder()
            .options(poise::FrameworkOptions {
                commands: vec![
                    issue(),
                    pr(),
                    status(),
                    release(),
                    summarize(),
                    weather(),
                    tmux(),
                    skill(),
                    mofa_effect(),
                    workspace_view(),
                    workspace_heartbeat_list(),
                    workspace_memory_view(),
                    help(),
                ],
                prefix_options: poise::PrefixFrameworkOptions {
                    prefix: None, // disable prefix commands - we only use slash commands
                    ..Default::default()
                },
                ..Default::default()
            })
            .setup(move |ctx, _ready, framework| {
                let http_ref = http_ref_setup.clone();
                let bus = bus_clone.clone();
                let config = config_clone.clone();
                let mofa_sessions = mofa_sessions_for_setup.clone();
                Box::pin(async move {
                    info!("discord bot connected as {}", _ready.user.name);
                    *http_ref.write().await = Some(ctx.http.clone());

                    // always register globally (for DMs)
                    match poise::builtins::register_globally(ctx, &framework.options().commands).await {
                        Ok(_) => {
                            info!("registered global discord slash commands (available in DMs)");
                            info!("note: global commands can take up to 1 hour to appear in DMs");
                        }
                        Err(e) => {
                            error!("failed to register global discord commands: {}", e);
                            error!("this may be due to:");
                            error!("  - bot not verified (unverified bots have limited global command access)");
                            error!("  - rate limiting");
                            error!("  - invalid application id or permissions");
                        }
                    }

                    // also register in guild if specified (for instant updates in that guild)
                    if let Some(guild_id) = config.guild_id {
                        let gid = serenity::GuildId::new(guild_id);
                        if let Err(e) = poise::builtins::register_in_guild(
                            ctx,
                            &framework.options().commands,
                            gid,
                        )
                        .await
                        {
                            error!("failed to register discord commands in guild {}: {}", guild_id, e);
                        } else {
                            info!("registered discord slash commands in guild {} (instant)", guild_id);
                        }
                    }

                    Ok(Data {
                        bus,
                        config,
                        mofa_sessions,
                    })
                })
            })
            .build();

        let message_handler = MessageHandler {
            bus: bus_message.clone(),
            config: config_message.clone(),
            mofa_sessions,
        };

        let mut client = serenity::ClientBuilder::new(&config.token, intents)
            .framework(framework)
            .event_handler(message_handler)
            .await
            .map_err(|e| ChannelError::SendFailed(format!("discord client error: {}", e)))?;

        {
            let mut http_guard = http_ref.write().await;
            *http_guard = Some(client.http.clone());
        }

        let mut outbound_rx = self.bus.subscribe_outbound();
        let running_clone = Arc::clone(&running);
        let http_ref_clone = Arc::clone(&self.http);

        tokio::spawn(async move {
            while *running_clone.read().await {
                match outbound_rx.recv().await {
                    Ok(msg) if msg.channel == "discord" => {
                        debug!(
                            "📤 sending outbound message to discord - chat_id: {}",
                            msg.chat_id
                        );

                        let http_guard = http_ref_clone.read().await;
                        let http = match http_guard.as_ref() {
                            Some(h) => h.clone(),
                            None => {
                                warn!("http client not available yet, skipping message");
                                continue;
                            }
                        };
                        drop(http_guard);

                        let channel_id = match msg.chat_id.parse::<u64>() {
                            Ok(id) => serenity::ChannelId::new(id),
                            Err(e) => {
                                error!("invalid discord channel id: {} - {}", msg.chat_id, e);
                                continue;
                            }
                        };

                        // Discord has a 2000 character limit per message, so we need to chunk long messages
                        const MAX_MESSAGE_LENGTH: usize = 2000;
                        let content = &msg.content;

                        if content.len() <= MAX_MESSAGE_LENGTH {
                            // Single message, send directly
                            match channel_id.say(&*http, content).await {
                                Ok(_) => {
                                    debug!("sent message to discord channel {}", msg.chat_id);
                                }
                                Err(e) => {
                                    error!("failed to send message to discord: {}", e);
                                }
                            }
                        } else {
                            // Split into chunks, trying to split on newlines for better readability
                            let mut chunks = Vec::new();
                            let mut current_chunk = String::new();

                            for line in content.lines() {
                                // If adding this line would exceed the limit, start a new chunk
                                let line_with_newline = if current_chunk.is_empty() {
                                    line.len()
                                } else {
                                    line.len() + 1 // +1 for newline
                                };

                                if current_chunk.len() + line_with_newline > MAX_MESSAGE_LENGTH {
                                    if !current_chunk.is_empty() {
                                        chunks.push(current_chunk.clone());
                                        current_chunk.clear();
                                    }
                                    // If a single line is too long, split it by character
                                    if line.len() > MAX_MESSAGE_LENGTH {
                                        for chunk in line
                                            .chars()
                                            .collect::<Vec<_>>()
                                            .chunks(MAX_MESSAGE_LENGTH)
                                        {
                                            chunks.push(chunk.iter().collect());
                                        }
                                    } else {
                                        current_chunk = line.to_string();
                                    }
                                } else {
                                    if !current_chunk.is_empty() {
                                        current_chunk.push('\n');
                                    }
                                    current_chunk.push_str(line);
                                }
                            }
                            if !current_chunk.is_empty() {
                                chunks.push(current_chunk);
                            }

                            // Send all chunks
                            for (i, chunk) in chunks.iter().enumerate() {
                                let chunk_content = if chunks.len() > 1 {
                                    // Add part indicator, but keep it under the limit
                                    let indicator =
                                        format!("*Part {} of {}*\n\n", i + 1, chunks.len());
                                    let available_space =
                                        MAX_MESSAGE_LENGTH.saturating_sub(indicator.len());
                                    if chunk.len() > available_space {
                                        // If chunk is still too long with indicator, truncate chunk
                                        format!(
                                            "{}*Part {} of {}*\n\n{}",
                                            &chunk[..available_space.min(chunk.len())],
                                            i + 1,
                                            chunks.len(),
                                            if chunk.len() > available_space {
                                                "..."
                                            } else {
                                                ""
                                            }
                                        )
                                    } else {
                                        format!("{}{}", indicator, chunk)
                                    }
                                } else {
                                    chunk.clone()
                                };

                                // Ensure final content doesn't exceed limit
                                let final_content = if chunk_content.len() > MAX_MESSAGE_LENGTH {
                                    chunk_content
                                        .chars()
                                        .take(MAX_MESSAGE_LENGTH)
                                        .collect::<String>()
                                } else {
                                    chunk_content
                                };

                                match channel_id.say(&*http, &final_content).await {
                                    Ok(_) => {
                                        debug!(
                                            "sent message chunk {}/{} to discord channel {}",
                                            i + 1,
                                            chunks.len(),
                                            msg.chat_id
                                        );
                                    }
                                    Err(e) => {
                                        // Check for specific HTTP error status codes in error message
                                        let error_str = e.to_string();
                                        let error_msg = if error_str.contains("401")
                                            || error_str.contains("Unauthorized")
                                        {
                                            error!(
                                                "discord authentication failed (401): token may be expired or invalid - {}",
                                                error_str
                                            );
                                            "error: bot authentication failed. please check bot token."
                                        } else if error_str.contains("403")
                                            || error_str.contains("Forbidden")
                                        {
                                            error!(
                                                "discord permission denied (403): bot lacks permissions in channel {} - {}",
                                                msg.chat_id, error_str
                                            );
                                            "error: bot doesn't have permission to send messages in this channel."
                                        } else if error_str.contains("404")
                                            || error_str.contains("Not Found")
                                        {
                                            error!(
                                                "discord channel not found (404): channel {} may have been deleted - {}",
                                                msg.chat_id, error_str
                                            );
                                            "error: channel not found. it may have been deleted."
                                        } else {
                                            error!(
                                                "failed to send message chunk {}/{} to discord: {}",
                                                i + 1,
                                                chunks.len(),
                                                e
                                            );
                                            "error: failed to send message."
                                        };

                                        // Try to send error message to user (if possible)
                                        if i == 0 {
                                            // Only try to send error on first chunk to avoid spam
                                            let _ = channel_id.say(&*http, error_msg).await;
                                        }

                                        break; // Stop sending remaining chunks on error
                                    }
                                }

                                // Small delay between chunks to avoid rate limiting
                                if i < chunks.len() - 1 {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(500))
                                        .await;
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("outbound message receiver lagged, missed {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("outbound message bus closed");
                        break;
                    }
                }
            }
        });

        info!("discord bot started, waiting for messages...");
        client.start().await.map_err(|e| {
            error!("discord client error: {}", e);
            ChannelError::SendFailed(format!("discord client error: {}", e))
        })?;

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        *self.running.write().await = false;
        info!("stopping discord bot");
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled && !self.config.token.is_empty()
    }
}
