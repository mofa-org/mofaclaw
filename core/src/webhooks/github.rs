//! GitHub webhook payload parsing and Discord notification formatting.
//!
//! Supports: pull_request, issues, push, workflow_run, release.

use crate::config::GitHubWebhookConfig;
use crate::messages::OutboundMessage;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::debug;

/// Verify GitHub webhook signature (X-Hub-Signature-256: sha256=...).
pub fn verify_signature(secret: &str, signature_header: Option<&str>, body: &[u8]) -> bool {
    let Some(sig) = signature_header else {
        return secret.is_empty();
    };
    let sig = sig.trim();
    if sig.len() < 7 || !sig.get(..7).map_or(false, |s| s.eq_ignore_ascii_case("sha256=")) {
        return false;
    }
    let expected = match hex::decode(sig.get(7..).unwrap_or("")) {
        Ok(b) => b,
        Err(_) => return false,
    };
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key size");
    mac.update(body);
    let result = mac.finalize();
    let computed = result.into_bytes();
    if computed.len() != expected.len() {
        return false;
    }
    computed
        .iter()
        .zip(expected.iter())
        .all(|(a, b)| a == b)
}

/// Minimal repo info present in all payloads
#[derive(Debug, Deserialize)]
pub struct RepoInfo {
    pub full_name: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub login: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestPayload {
    pub action: Option<String>,
    pub number: Option<u64>,
    pub pull_request: Option<PullRequestInner>,
    pub repository: Option<RepoInfo>,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestInner {
    pub title: Option<String>,
    pub html_url: Option<String>,
    pub state: Option<String>,
    pub merged: Option<bool>,
    pub user: Option<UserInfo>,
}

#[derive(Debug, Deserialize)]
pub struct IssuesPayload {
    pub action: Option<String>,
    pub issue: Option<IssueInner>,
    pub repository: Option<RepoInfo>,
}

#[derive(Debug, Deserialize)]
pub struct IssueInner {
    pub number: Option<u64>,
    pub title: Option<String>,
    pub html_url: Option<String>,
    pub state: Option<String>,
    pub user: Option<UserInfo>,
}

#[derive(Debug, Deserialize)]
pub struct PushPayload {
    pub r#ref: Option<String>,
    pub repository: Option<RepoInfo>,
    pub pusher: Option<PusherInfo>,
    pub compare: Option<String>,
    pub commits: Option<Vec<CommitInfo>>,
}

#[derive(Debug, Deserialize)]
pub struct PusherInfo {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommitInfo {
    pub message: Option<String>,
    pub id: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunPayload {
    pub action: Option<String>,
    pub workflow_run: Option<WorkflowRunInner>,
    pub repository: Option<RepoInfo>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunInner {
    pub name: Option<String>,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub html_url: Option<String>,
    pub repository: Option<RepoInfo>,
}

#[derive(Debug, Deserialize)]
pub struct ReleasePayload {
    pub action: Option<String>,
    pub release: Option<ReleaseInner>,
    pub repository: Option<RepoInfo>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseInner {
    pub name: Option<String>,
    pub tag_name: Option<String>,
    pub html_url: Option<String>,
    pub author: Option<UserInfo>,
}

/// Discord embed color constants (decimal)
const COLOR_PR_OPEN: u32 = 0x2ecc71;   // green
const COLOR_PR_MERGED: u32 = 0x9b59b6; // purple
const COLOR_PR_CLOSED: u32 = 0xe74c3c; // red
const COLOR_ISSUE: u32 = 0x3498db;    // blue
const COLOR_PUSH: u32 = 0x1abc9c;     // teal
const COLOR_CI_SUCCESS: u32 = 0x2ecc71;
const COLOR_CI_FAILURE: u32 = 0xe74c3c;
const COLOR_CI_NEUTRAL: u32 = 0x95a5a6;
const COLOR_RELEASE: u32 = 0xf1c40f;  // yellow

fn embed_metadata(title: String, description: String, url: Option<String>, color: u32) -> HashMap<String, serde_json::Value> {
    let mut m = HashMap::new();
    m.insert("embed_title".to_string(), serde_json::json!(title));
    m.insert("embed_description".to_string(), serde_json::json!(description));
    m.insert("embed_color".to_string(), serde_json::json!(color as i32));
    if let Some(u) = url {
        m.insert("embed_url".to_string(), serde_json::json!(u));
    }
    m
}

/// Build one or more OutboundMessages for Discord from a GitHub webhook event.
/// Returns empty vec if event is filtered out or parsing fails.
pub fn build_discord_notifications(
    config: &GitHubWebhookConfig,
    event_type: &str,
    body: &[u8],
) -> Vec<OutboundMessage> {
    if config.discord_channel_ids.is_empty() {
        return vec![];
    }
    if !config.events.is_empty() && !config.events.iter().any(|e| e == event_type) {
        debug!("GitHub webhook event {} filtered out by config", event_type);
        return vec![];
    }

    let (content, metadata) = match event_type {
        "pull_request" => format_pull_request(body),
        "issues" => format_issues(body),
        "push" => format_push(body),
        "workflow_run" => format_workflow_run(body),
        "release" => format_release(body),
        _ => {
            debug!("Unsupported GitHub webhook event: {}", event_type);
            return vec![];
        }
    };

    if content.is_empty() && metadata.get("embed_title").is_none() {
        return vec![];
    }

    let mut messages = Vec::with_capacity(config.discord_channel_ids.len());
    for channel_id in &config.discord_channel_ids {
        let mut msg = OutboundMessage::new("discord", channel_id.clone(), content.clone());
        if !metadata.is_empty() {
            msg.metadata = metadata.clone();
        }
        messages.push(msg);
    }
    messages
}

fn format_pull_request(body: &[u8]) -> (String, HashMap<String, serde_json::Value>) {
    let payload: PullRequestPayload = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse pull_request payload: {}", e);
            return (String::new(), HashMap::new());
        }
    };
    let action = payload.action.as_deref().unwrap_or("unknown");
    let repo = payload.repository.as_ref().and_then(|r| r.full_name.as_deref()).unwrap_or("unknown");
    let pr = match payload.pull_request.as_ref() {
        Some(p) => p,
        None => return (String::new(), HashMap::new()),
    };
    let title = pr.title.as_deref().unwrap_or("(no title)");
    let url = pr.html_url.clone();
    let user = pr.user.as_ref().and_then(|u| u.login.as_deref()).unwrap_or("?");
    let state = pr.state.as_deref().unwrap_or("");
    let merged = pr.merged.unwrap_or(false);

    let (action_label, color) = match (action, state, merged) {
        ("opened", _, _) => ("PR opened", COLOR_PR_OPEN),
        ("closed", _, true) => ("PR merged", COLOR_PR_MERGED),
        ("closed", _, _) => ("PR closed", COLOR_PR_CLOSED),
        ("reopened", _, _) => ("PR reopened", COLOR_PR_OPEN),
        _ => ("PR updated", COLOR_CI_NEUTRAL),
    };

    let description = format!("**{}**\nBy **{}** · [#{}]({})", title, user, payload.number.unwrap_or(0), url.as_deref().unwrap_or(""));
    let title_line = format!("{} · {}", repo, action_label);
    let metadata = embed_metadata(title_line, description, url, color);
    (String::new(), metadata)
}

fn format_issues(body: &[u8]) -> (String, HashMap<String, serde_json::Value>) {
    let payload: IssuesPayload = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse issues payload: {}", e);
            return (String::new(), HashMap::new());
        }
    };
    let action = payload.action.as_deref().unwrap_or("unknown");
    let repo = payload.repository.as_ref().and_then(|r| r.full_name.as_deref()).unwrap_or("unknown");
    let issue = match payload.issue.as_ref() {
        Some(i) => i,
        None => return (String::new(), HashMap::new()),
    };
    let title = issue.title.as_deref().unwrap_or("(no title)");
    let url = issue.html_url.clone();
    let user = issue.user.as_ref().and_then(|u| u.login.as_deref()).unwrap_or("?");

    let action_label = match action {
        "opened" => "Issue opened",
        "closed" => "Issue closed",
        "reopened" => "Issue reopened",
        _ => "Issue updated",
    };

    let description = format!("**{}**\nBy **{}** · [#{}]({})", title, user, issue.number.unwrap_or(0), url.as_deref().unwrap_or(""));
    let title_line = format!("{} · {}", repo, action_label);
    let metadata = embed_metadata(title_line, description, url, COLOR_ISSUE);
    (String::new(), metadata)
}

fn format_push(body: &[u8]) -> (String, HashMap<String, serde_json::Value>) {
    let payload: PushPayload = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse push payload: {}", e);
            return (String::new(), HashMap::new());
        }
    };
    let repo = payload.repository.as_ref().and_then(|r| r.full_name.as_deref()).unwrap_or("unknown");
    let r#ref = payload.r#ref.as_deref().unwrap_or("").strip_prefix("refs/heads/").unwrap_or("");
    let pusher = payload.pusher.as_ref().and_then(|p| p.name.as_deref()).unwrap_or("?");
    let compare = payload.compare.clone();
    let commits = payload.commits.as_deref().unwrap_or(&[]);
    let count = commits.len();
    let last_msg = commits.last().and_then(|c| c.message.as_deref()).unwrap_or("(no message)");
    let first_line = last_msg.lines().next().unwrap_or(last_msg);

    let title_line = format!("{} · Push to `{}`", repo, r#ref);
    let description = format!("**{}** pushed {} commit(s)\n\n{}", pusher, count, first_line);
    let metadata = embed_metadata(title_line, description, compare, COLOR_PUSH);
    (String::new(), metadata)
}

fn format_workflow_run(body: &[u8]) -> (String, HashMap<String, serde_json::Value>) {
    let payload: WorkflowRunPayload = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse workflow_run payload: {}", e);
            return (String::new(), HashMap::new());
        }
    };
    let repo = payload.repository.as_ref().and_then(|r| r.full_name.as_deref()).unwrap_or("unknown");
    let run = match payload.workflow_run.as_ref() {
        Some(r) => r,
        None => return (String::new(), HashMap::new()),
    };
    let name = run.name.as_deref().unwrap_or("Workflow");
    let status = run.status.as_deref().unwrap_or("?");
    let conclusion = run.conclusion.as_deref().unwrap_or("");
    let url = run.html_url.clone();

    let (color, conclusion_text) = match conclusion {
        "success" => (COLOR_CI_SUCCESS, "Success"),
        "failure" | "cancelled" => (COLOR_CI_FAILURE, conclusion),
        _ => (COLOR_CI_NEUTRAL, if conclusion.is_empty() { status } else { conclusion }),
    };

    let title_line = format!("{} · CI {}", repo, conclusion_text);
    let description = format!("**{}**\nStatus: {}", name, conclusion_text);
    let metadata = embed_metadata(title_line, description, url, color);
    (String::new(), metadata)
}

fn format_release(body: &[u8]) -> (String, HashMap<String, serde_json::Value>) {
    let payload: ReleasePayload = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse release payload: {}", e);
            return (String::new(), HashMap::new());
        }
    };
    let action = payload.action.as_deref().unwrap_or("published");
    let repo = payload.repository.as_ref().and_then(|r| r.full_name.as_deref()).unwrap_or("unknown");
    let release = match payload.release.as_ref() {
        Some(r) => r,
        None => return (String::new(), HashMap::new()),
    };
    let name = release.name.as_deref().or(release.tag_name.as_deref()).unwrap_or("Release");
    let url = release.html_url.clone();
    let author = release.author.as_ref().and_then(|a| a.login.as_deref()).unwrap_or("?");

    let title_line = format!("{} · Release {}", repo, action);
    let description = format!("**{}**\nBy **{}**", name, author);
    let metadata = embed_metadata(title_line, description, url, COLOR_RELEASE);
    (String::new(), metadata)
}
