//! discord channel using serenity and poise

use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::DiscordConfig;
use crate::error::{ChannelError, Result};
use crate::messages::InboundMessage;
use crate::get_workspace_path;
use async_trait::async_trait;
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// poise data
pub struct Data {
    /// message bus for agent communication
    pub bus: MessageBus,
    /// discord configuration
    pub config: DiscordConfig,
}

/// discord channel using serenity and poise
pub struct DiscordChannel {
    config: DiscordConfig,
    bus: MessageBus,
    running: Arc<RwLock<bool>>,
    http: Arc<RwLock<Option<Arc<serenity::Http>>>>,
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
        if config.token.is_empty() {
            return Err(ChannelError::NotConfigured("Discord".to_string()).into());
        }

        Ok(Self {
            config,
            bus,
            running: Arc::new(RwLock::new(false)),
            http: Arc::new(RwLock::new(None)),
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
        roles.iter().any(|role| self.config.allow_from.contains(role))
    }

    /// check if user has admin role
    fn is_admin(&self, roles: &[String]) -> bool {
        !self.config.admin_roles.is_empty()
            && roles.iter().any(|role| self.config.admin_roles.contains(role))
    }

    /// check if user has member role (or admin role)
    fn is_member(&self, roles: &[String]) -> bool {
        // members include both member_roles and admin_roles
        self.is_admin(roles)
            || (!self.config.member_roles.is_empty()
                && roles.iter().any(|role| self.config.member_roles.contains(role)))
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
        Self {
            config: ctx.data().config.clone(),
            bus: ctx.data().bus.clone(),
            running: Arc::new(RwLock::new(true)),
            http: Arc::new(RwLock::new(None)),
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

    let request = match action {
        IssueAction::Create => {
            // Create requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: create operation requires member role").await?;
                return Ok(());
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
            // List requires Guest permission (everyone can list)
            if !channel.is_guest(&roles) {
                ctx.say("error: list operation requires guest permission").await?;
                return Ok(());
            }
            let mut req = "list all github issues".to_string();
            if let Some(l) = label {
                req.push_str(&format!(" with label: {}", l));
            }
            req
        }
        IssueAction::View => {
            // View requires Guest permission (everyone can view)
            if !channel.is_guest(&roles) {
                ctx.say("error: view operation requires guest permission").await?;
                return Ok(());
            }
            if let Some(n) = number {
                format!("view github issue #{}", n)
            } else {
                ctx.say("issue number required for view action").await?;
                return Ok(());
            }
        }
        IssueAction::Close => {
            // Close requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: close operation requires member role").await?;
                return Ok(());
            }
            if let Some(n) = number {
                format!("close github issue #{}", n)
            } else {
                ctx.say("issue number required for close action").await?;
                return Ok(());
            }
        }
        IssueAction::Comment => {
            // Comment requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: comment operation requires member role").await?;
                return Ok(());
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
            // Assign requires Admin permission
            if !channel.is_admin(&roles) {
                ctx.say("error: assign operation requires admin role").await?;
                return Ok(());
            }
            if let Some(n) = number {
                if let Some(github_username) = assignee {
                    if github_username.is_empty() {
                        ctx.say("error: github username cannot be empty").await?;
                        return Ok(());
                    }
                    format!("assign github issue #{} to github user: {}", n, github_username)
                } else {
                    ctx.say("error: github username required for assign action").await?;
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
        
        ctx.send(poise::CreateReply::default().embed(error_embed)).await?;
    }

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

    let request = match action {
        PrAction::Create => {
            // Create requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: create operation requires member role").await?;
                return Ok(());
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
                ctx.say("error: list operation requires guest permission").await?;
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
                ctx.say("error: view operation requires guest permission").await?;
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
            // Merge requires Admin permission
            if !channel.is_admin(&roles) {
                ctx.say("error: merge operation requires admin role").await?;
                return Ok(());
            }
            if let Some(n) = number {
                // Check CI status before merge
                format!("check ci status for pr #{} then merge pull request #{}", n, n)
            } else {
                ctx.say("pr number required for merge action").await?;
                return Ok(());
            }
        }
        PrAction::Close => {
            // Close requires Member permission
            if !channel.is_member(&roles) {
                ctx.say("error: close operation requires member role").await?;
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
                ctx.say("error: comment operation requires member role").await?;
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
                ctx.say("error: review operation requires member role").await?;
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
                    ctx.say("review action (approve/reject) required for review action").await?;
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
        
        ctx.send(poise::CreateReply::default().embed(error_embed)).await?;
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
        ctx.say(format!("error: failed to check ci status: {}", e)).await?;
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
                ctx.say("error: create release operation requires admin role").await?;
                return Ok(());
            }
            if let Some(v) = version {
                format!("create a github release with version: {}", v)
            } else {
                ctx.say("version required for create action").await?;
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
        ctx.say(format!("error: failed to process release request: {}", e)).await?;
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
        ctx.say(format!("error: failed to summarize: {}", e)).await?;
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
        location, location_encoded, format_param.trim_start_matches('?')
    );

    ctx.say("fetching weather...").await?;

    let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
    if let Err(e) = ctx.data().bus.publish_inbound(inbound_msg).await {
        error!("failed to publish weather command to bus: {}", e);
        ctx.say(format!("error: failed to get weather: {}", e)).await?;
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
                    format!("use tmux skill to send command '{}' to tmux session: {}", c, s)
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
        ctx.say(format!("error: failed to process tmux request: {}", e)).await?;
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
                ctx.say("error: create skill operation requires admin role").await?;
                return Ok(());
            }
            if let Some(n) = name {
                if let Some(d) = description {
                    format!("use skill-creator to create a new skill named '{}' with description: {}", n, d)
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
                ctx.say("error: update skill operation requires admin role").await?;
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
        ctx.say(format!("error: failed to process skill request: {}", e)).await?;
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
    if lower.contains("show workspace") || lower.contains("view workspace") || lower.contains("workspace") {
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
    if lower.contains("heartbeat") && (lower.contains("list") || lower.contains("task") || lower.contains("show")) {
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
            let title = extract_after_keywords(&lower, &["create issue", "new issue", "issue create"]);
            if !title.is_empty() {
                return CommandIntent::Issue(format!("create a github issue with title: {}", title));
            }
            return CommandIntent::Issue("create a github issue".to_string());
        }
        if lower.contains("list") || lower.contains("show all") {
            let label = extract_after_keywords(&lower, &["list issues", "list issue", "issue list", "show issues"]);
            if !label.is_empty() {
                return CommandIntent::Issue(format!("list all github issues with label: {}", label));
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
                let comment_body = extract_after_keywords(&lower, &["comment on issue", "comment issue", "issue comment"]);
                if !comment_body.is_empty() {
                    return CommandIntent::Issue(format!("comment on github issue #{} with body: {}", num, comment_body));
                }
                return CommandIntent::Issue(format!("comment on github issue #{}", num));
            }
        }
        if lower.contains("assign") {
            if let Some(num) = extract_number(&lower) {
                let user = extract_after_keywords(&lower, &["assign issue", "issue assign", "assign to"]);
                if !user.is_empty() {
                    return CommandIntent::Issue(format!("assign github issue #{} to user: {}", num, user));
                }
                return CommandIntent::Issue(format!("assign github issue #{}", num));
            }
        }
    }
    
    // github pr patterns
    if lower.contains("pr") || lower.contains("pull request") {
        if lower.contains("create") || lower.contains("new") {
            let title = extract_after_keywords(&lower, &["create pr", "new pr", "pr create", "create pull request"]);
            if !title.is_empty() {
                return CommandIntent::Pr(format!("create a github pr with title: {}", title));
            }
            return CommandIntent::Pr("create a github pr".to_string());
        }
        if lower.contains("list") || lower.contains("show all") {
            let state = extract_after_keywords(&lower, &["list pr", "pr list", "list pull request"]);
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
                return CommandIntent::Pr(format!("check ci status for pr #{} then merge github pr #{}", num, num));
            }
        }
        if lower.contains("close") {
            if let Some(num) = extract_number(&lower) {
                return CommandIntent::Pr(format!("close github pr #{}", num));
            }
        }
        if lower.contains("comment") {
            if let Some(num) = extract_number(&lower) {
                let comment_body = extract_after_keywords(&lower, &["comment on pr", "comment pr", "pr comment"]);
                if !comment_body.is_empty() {
                    return CommandIntent::Pr(format!("comment on github pr #{} with body: {}", num, comment_body));
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
            let version = extract_after_keywords(&lower, &["create release", "new release", "release create"]);
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
        let location = extract_after_keywords(&lower, &["weather in", "weather for", "weather", "temperature in", "forecast for"]);
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
            let session = extract_after_keywords(&lower, &["create tmux", "new tmux", "tmux create"]);
            if !session.is_empty() {
                return CommandIntent::Tmux(format!("use tmux skill to create session: {}", session));
            }
        }
        if lower.contains("list") {
            return CommandIntent::Tmux("use tmux skill to list all sessions".to_string());
        }
        if lower.contains("attach") {
            let session = extract_after_keywords(&lower, &["attach", "attach to"]);
            if !session.is_empty() {
                return CommandIntent::Tmux(format!("use tmux skill to attach to session: {}", session));
            }
        }
    }
    
    // skill patterns
    if lower.contains("skill") {
        if lower.contains("create") || lower.contains("new") {
            let name = extract_after_keywords(&lower, &["create skill", "new skill", "skill create"]);
            if !name.is_empty() {
                return CommandIntent::Skill(format!("use skill-creator to create skill: {}", name));
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
                let end = keywords.iter()
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
            channel_id.send_message(http, serenity::CreateMessage::new().content(format!(
                "**{}**\n```\n{}\n```",
                file_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
                preview
            ))).await?;
        }
        Err(e) => {
            channel_id.send_message(http, serenity::CreateMessage::new().content(format!("error reading file: {}", e))).await?;
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
                channel_id.send_message(http, serenity::CreateMessage::new().content("no active heartbeat tasks")).await?;
            } else {
                let task_list = tasks.join("\n");
                let preview = if task_list.len() > 1900 {
                    format!("{}...\n\n*(truncated)*", &task_list[..1900])
                } else {
                    task_list
                };
                channel_id.send_message(http, serenity::CreateMessage::new().content(format!("**Active Heartbeat Tasks**\n```\n{}\n```", preview))).await?;
            }
        }
        Err(e) => {
            channel_id.send_message(http, serenity::CreateMessage::new().content(format!("error reading heartbeat: {}", e))).await?;
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
                format!("{}...\n\n*(truncated, file is {} chars)*", &content[..1900], content.len())
            } else {
                content
            };
            channel_id.send_message(http, serenity::CreateMessage::new().content(format!("**Memory**\n```\n{}\n```", preview))).await?;
        }
        Err(e) => {
            channel_id.send_message(http, serenity::CreateMessage::new().content(format!("error reading memory: {} (file may not exist yet)", e))).await?;
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
                        file_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
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
                ctx.send(poise::CreateReply::default()
                    .content("no active heartbeat tasks")
                    .ephemeral(true))
                    .await?;
            } else {
                let task_list = tasks.join("\n");
                let preview = if task_list.len() > 1900 {
                    format!("{}...\n\n*(truncated)*", &task_list[..1900])
                } else {
                    task_list
                };
                ctx.send(poise::CreateReply::default()
                    .content(format!("**Active Heartbeat Tasks**\n```\n{}\n```", preview))
                    .ephemeral(true))
                    .await?;
            }
        }
        Err(e) => {
            ctx.send(poise::CreateReply::default()
                .content(format!("error reading heartbeat: {}", e))
                .ephemeral(true))
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
                format!("{}...\n\n*(truncated, file is {} chars)*", &content[..1900], content.len())
            } else {
                content
            };
            ctx.send(poise::CreateReply::default()
                .content(format!("**Memory**\n```\n{}\n```", preview))
                .ephemeral(true))
                .await?;
        }
        Err(e) => {
            ctx.send(poise::CreateReply::default()
                .content(format!("error reading memory: {} (file may not exist yet)", e))
                .ephemeral(true))
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
            "issue" => "**Issue Management**\n`/issue create <title> [body]` - create new issue (member)\n`/issue list [label]` - list all issues (guest)\n`/issue view <number>` - view issue details (guest)\n`/issue close <number>` - close issue (member)\n`/issue comment <number> <body>` - comment on issue (member)\n`/issue assign <number> <github_username>` - assign issue to github user (admin)",
            "pr" => "**PR Management**\n`/pr create <title> [base] [head]` - create new pr (member)\n`/pr list [state]` - list prs (guest)\n`/pr view <number>` - view pr details (guest)\n`/pr merge <number>` - merge pr (admin only, checks CI)\n`/pr comment <number> <body>` - comment on pr (member)\n`/pr review <number> <approve|reject>` - code review (member)\n`/pr close <number>` - close pr (member)",
            "status" => "**CI Status**\n`/status [workflow]` - check ci status for workflow or overall",
            "release" => "**Release Management**\n`/release [version] create` - create release (admin only)\n`/release [version]` - view release\n`/release` - list all releases",
            "summarize" => "**Summarize**\n`/summarize <url|file> [length]` - summarize url or file\nlength: short|medium|long|xl|xxl",
            "weather" => "**Weather**\n`/weather <location> [format]` - get weather info\nformat: compact|full|current",
            "tmux" => "**Tmux**\n`/tmux create <session>` - create session\n`/tmux list` - list sessions\n`/tmux attach <session>` - attach to session\n`/tmux send <session> <command>` - send command\n`/tmux capture <session>` - capture output",
            "skill" => "**Skill Management**\n`/skill create <name> [description]` - create skill (admin only)\n`/skill update <name>` - update skill (admin only)\n`/skill list` - list all skills\n`/skill view <name>` - view skill details",
            "workspace" => "**Workspace Management**\n`/workspace view <file>` - view workspace file (soul|user|agents|tools|heartbeat)\n`/workspace heartbeat list` - list heartbeat tasks\n`/workspace memory view` - view memory",
            _ => "unknown command. use `/help` to see all commands",
        };
        ctx.send(poise::CreateReply::default().content(help_text).ephemeral(true)).await?;
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
        ctx.send(poise::CreateReply::default().content(help_text).ephemeral(true)).await?;
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
        }

        #[serenity::async_trait]
        impl serenity::EventHandler for MessageHandler {
            async fn message(&self, ctx: serenity::Context, new_message: serenity::Message) {
                if new_message.author.bot {
                    return;
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

                let channel = DiscordChannel {
                    config: self.config.clone(),
                    bus: self.bus.clone(),
                    running: Arc::new(RwLock::new(true)),
                    http: Arc::new(RwLock::new(None)),
                };

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
                        if let Err(e) = execute_workspace_view(ctx.http.as_ref(), new_message.channel_id, file).await {
                            error!("failed to execute workspace view: {}", e);
                            let _ = new_message.channel_id.send_message(ctx.http.as_ref(), serenity::CreateMessage::new().content(format!("error: {}", e))).await;
                        }
                        return;
                    }
                    CommandIntent::Workspace(WorkspaceIntent::HeartbeatList) => {
                        debug!("matched workspace heartbeat list keyword");
                        if let Err(e) = execute_workspace_heartbeat_list(ctx.http.as_ref(), new_message.channel_id).await {
                            error!("failed to execute workspace heartbeat list: {}", e);
                            let _ = new_message.channel_id.send_message(ctx.http.as_ref(), serenity::CreateMessage::new().content(format!("error: {}", e))).await;
                        }
                        return;
                    }
                    CommandIntent::Workspace(WorkspaceIntent::MemoryView) => {
                        debug!("matched workspace memory view keyword");
                        if let Err(e) = execute_workspace_memory_view(ctx.http.as_ref(), new_message.channel_id).await {
                            error!("failed to execute workspace memory view: {}", e);
                            let _ = new_message.channel_id.send_message(ctx.http.as_ref(), serenity::CreateMessage::new().content(format!("error: {}", e))).await;
                        }
                        return;
                    }
                    CommandIntent::Workspace(WorkspaceIntent::None) | CommandIntent::None => {
                        // no keyword match, fall back to llm intent recognition
                        debug!("no keyword match, using llm intent recognition");
                    }
                    CommandIntent::Issue(request) => {
                        debug!("matched issue keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish issue command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Pr(request) => {
                        debug!("matched pr keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish pr command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Status(request) => {
                        debug!("matched status keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish status command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Release(request) => {
                        debug!("matched release keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish release command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Summarize(request) => {
                        debug!("matched summarize keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish summarize command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Weather(request) => {
                        debug!("matched weather keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish weather command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Tmux(request) => {
                        debug!("matched tmux keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
                        if let Err(e) = self.bus.publish_inbound(inbound_msg).await {
                            error!("failed to publish tmux command to bus: {}", e);
                        }
                        return;
                    }
                    CommandIntent::Skill(request) => {
                        debug!("matched skill keyword, sending to agent: {}", request);
                        let inbound_msg = InboundMessage::new("discord", &sender_id, &chat_id, &request);
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

                    Ok(Data { bus, config })
                })
            })
            .build();

        let message_handler = MessageHandler {
            bus: bus_message.clone(),
            config: config_message.clone(),
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
                        debug!("📤 sending outbound message to discord - chat_id: {}", msg.chat_id);

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
                                        for chunk in line.chars().collect::<Vec<_>>().chunks(MAX_MESSAGE_LENGTH) {
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
                                    let indicator = format!("*Part {} of {}*\n\n", i + 1, chunks.len());
                                    let available_space = MAX_MESSAGE_LENGTH.saturating_sub(indicator.len());
                                    if chunk.len() > available_space {
                                        // If chunk is still too long with indicator, truncate chunk
                                        format!("{}*Part {} of {}*\n\n{}", 
                                            &chunk[..available_space.min(chunk.len())], 
                                            i + 1, 
                                            chunks.len(),
                                            if chunk.len() > available_space { "..." } else { "" })
                                    } else {
                                        format!("{}{}", indicator, chunk)
                                    }
                                } else {
                                    chunk.clone()
                                };
                                
                                // Ensure final content doesn't exceed limit
                                let final_content = if chunk_content.len() > MAX_MESSAGE_LENGTH {
                                    chunk_content.chars().take(MAX_MESSAGE_LENGTH).collect::<String>()
                                } else {
                                    chunk_content
                                };
                                
                                match channel_id.say(&*http, &final_content).await {
                                    Ok(_) => {
                                        debug!("sent message chunk {}/{} to discord channel {}", i + 1, chunks.len(), msg.chat_id);
                                    }
                                    Err(e) => {
                                        // Check for specific HTTP error status codes in error message
                                        let error_str = e.to_string();
                                        let error_msg = if error_str.contains("401") || error_str.contains("Unauthorized") {
                                            error!("discord authentication failed (401): token may be expired or invalid - {}", error_str);
                                            "error: bot authentication failed. please check bot token."
                                        } else if error_str.contains("403") || error_str.contains("Forbidden") {
                                            error!("discord permission denied (403): bot lacks permissions in channel {} - {}", msg.chat_id, error_str);
                                            "error: bot doesn't have permission to send messages in this channel."
                                        } else if error_str.contains("404") || error_str.contains("Not Found") {
                                            error!("discord channel not found (404): channel {} may have been deleted - {}", msg.chat_id, error_str);
                                            "error: channel not found. it may have been deleted."
                                        } else {
                                            error!("failed to send message chunk {}/{} to discord: {}", i + 1, chunks.len(), e);
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
                                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
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
