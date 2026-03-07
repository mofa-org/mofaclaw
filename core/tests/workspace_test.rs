//! Integration tests for the shared workspace module.

use mofaclaw_core::workspace::types::*;
use mofaclaw_core::workspace::SharedWorkspace;

/// Helper: create a workspace in a fresh temp directory.
async fn open_temp_workspace() -> (SharedWorkspace, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let ws = SharedWorkspace::open(dir.path().join("workspace"))
        .await
        .expect("open workspace");
    (ws, dir)
}

fn agent(name: &str) -> AgentId {
    AgentId::from(name)
}

// ────────────────────── directory structure ──────────────────────

#[tokio::test]
async fn test_workspace_creates_directory_tree() {
    let (ws, _dir) = open_temp_workspace().await;
    let root = ws.path();

    // artifacts sub-dirs
    for sub in &["designs", "code", "reviews", "tests", "other"] {
        assert!(root.join("artifacts").join(sub).is_dir(), "missing artifacts/{sub}");
    }
    // context files
    for f in &["decisions.json", "constraints.json", "glossary.json"] {
        assert!(root.join("context").join(f).is_file(), "missing context/{f}");
    }
    // state
    assert!(root.join("state/locks").is_dir());
    assert!(root.join("state/active_tasks.json").is_file());
    // history
    assert!(root.join("history/changes.log").is_file());
}

// ────────────────────── artifact CRUD ───────────────────────────

#[tokio::test]
async fn test_artifact_create_and_get() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact(
            "design.md".into(),
            ArtifactType::Design,
            agent("alice"),
            b"# Architecture".to_vec(),
        )
        .await
        .unwrap();

    assert_eq!(art.name, "design.md");
    assert_eq!(art.version, 1);
    assert_eq!(art.created_by, agent("alice"));
    assert_eq!(art.content, b"# Architecture");

    // Fetch by ID
    let fetched = ws.get_artifact(art.id).await.unwrap().unwrap();
    assert_eq!(fetched.id, art.id);
    assert_eq!(fetched.name, "design.md");
}

#[tokio::test]
async fn test_artifact_update_bumps_version() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact(
            "file.rs".into(),
            ArtifactType::Code,
            agent("bob"),
            b"fn main() {}".to_vec(),
        )
        .await
        .unwrap();

    let updated = ws
        .update_artifact(art.id, b"fn main() { println!(\"hi\"); }".to_vec(), agent("bob"))
        .await
        .unwrap();

    assert_eq!(updated.version, 2);
    assert_eq!(updated.content, b"fn main() { println!(\"hi\"); }");
    assert_eq!(updated.last_modified_by, agent("bob"));
}

#[tokio::test]
async fn test_artifact_delete() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact(
            "temp.txt".into(),
            ArtifactType::Other("misc".into()),
            agent("charlie"),
            b"temp data".to_vec(),
        )
        .await
        .unwrap();

    assert!(ws.get_artifact(art.id).await.unwrap().is_some());

    let deleted = ws.delete_artifact(art.id, agent("charlie")).await.unwrap();
    assert!(deleted);

    assert!(ws.get_artifact(art.id).await.unwrap().is_none());

    // Double-delete returns false
    let deleted2 = ws.delete_artifact(art.id, agent("charlie")).await.unwrap();
    assert!(!deleted2);
}

#[tokio::test]
async fn test_artifact_list_with_filters() {
    let (ws, _dir) = open_temp_workspace().await;

    ws.create_artifact("arch.md".into(), ArtifactType::Design, agent("a1"), b"d1".to_vec())
        .await
        .unwrap();
    ws.create_artifact("impl.rs".into(), ArtifactType::Code, agent("a2"), b"d2".to_vec())
        .await
        .unwrap();
    ws.create_artifact("arch2.md".into(), ArtifactType::Design, agent("a1"), b"d3".to_vec())
        .await
        .unwrap();

    // No filter → all 3
    let all = ws.list_artifacts(&ArtifactFilter::default()).await.unwrap();
    assert_eq!(all.len(), 3);

    // Filter by type
    let designs = ws
        .list_artifacts(&ArtifactFilter {
            artifact_type: Some(ArtifactType::Design),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(designs.len(), 2);

    // Filter by owner
    let a2 = ws
        .list_artifacts(&ArtifactFilter {
            owner: Some(agent("a2")),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(a2.len(), 1);
    assert_eq!(a2[0].name, "impl.rs");

    // Filter by name substring
    let arch = ws
        .list_artifacts(&ArtifactFilter {
            name_contains: Some("arch".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(arch.len(), 2);
}

// ────────────────────── version history & rollback ──────────────

#[tokio::test]
async fn test_version_history_and_rollback() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("doc.md".into(), ArtifactType::Design, agent("a"), b"v1".to_vec())
        .await
        .unwrap();

    ws.update_artifact(art.id, b"v2".to_vec(), agent("a")).await.unwrap();
    ws.update_artifact(art.id, b"v3".to_vec(), agent("a")).await.unwrap();

    let versions = ws.get_artifact_versions(art.id).await.unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[0].version, 1);
    assert_eq!(versions[1].version, 2);
    assert_eq!(versions[2].version, 3);

    // Rollback to v1 – creates v4 with v1 content
    let rolled = ws.rollback_artifact(art.id, 1, agent("a")).await.unwrap();
    assert_eq!(rolled.version, 4);
    assert_eq!(rolled.content, b"v1");
}

// ────────────────────── locking ─────────────────────────────────

#[tokio::test]
async fn test_lock_and_unlock() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("locked.rs".into(), ArtifactType::Code, agent("a"), b"x".to_vec())
        .await
        .unwrap();

    // Lock by agent a
    let lock = ws.lock_artifact(art.id, agent("a")).await.unwrap();
    assert_eq!(lock.agent, agent("a"));

    // Agent a can still update
    ws.update_artifact(art.id, b"y".to_vec(), agent("a")).await.unwrap();

    // Agent b cannot update (locked by a)
    let err = ws
        .update_artifact(art.id, b"z".to_vec(), agent("b"))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("locked"),
        "expected lock error, got: {err}"
    );

    // Agent b cannot lock either
    let err2 = ws.lock_artifact(art.id, agent("b")).await.unwrap_err();
    assert!(err2.to_string().contains("locked"));

    // Unlock by a
    let released = ws.unlock_artifact(art.id, &agent("a")).await.unwrap();
    assert!(released);

    // Now b can update
    ws.update_artifact(art.id, b"z".to_vec(), agent("b")).await.unwrap();
}

#[tokio::test]
async fn test_lock_idempotent_same_agent() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("f.txt".into(), ArtifactType::Test, agent("a"), b"t".to_vec())
        .await
        .unwrap();

    ws.lock_artifact(art.id, agent("a")).await.unwrap();
    // Re-locking by the same agent should succeed (idempotent)
    let lock2 = ws.lock_artifact(art.id, agent("a")).await.unwrap();
    assert_eq!(lock2.agent, agent("a"));
}

#[tokio::test]
async fn test_lock_nonexistent_artifact_fails() {
    let (ws, _dir) = open_temp_workspace().await;

    let fake_id = uuid::Uuid::new_v4();
    let err = ws.lock_artifact(fake_id, agent("a")).await.unwrap_err();
    assert!(err.to_string().contains("not found"), "got: {err}");
}

#[tokio::test]
async fn test_unlock_wrong_agent_fails() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("x.txt".into(), ArtifactType::Review, agent("a"), b"r".to_vec())
        .await
        .unwrap();

    ws.lock_artifact(art.id, agent("a")).await.unwrap();

    let err = ws.unlock_artifact(art.id, &agent("b")).await.unwrap_err();
    assert!(err.to_string().contains("held by"), "got: {err}");
}

// ────────────────────── context store ───────────────────────────

#[tokio::test]
async fn test_decisions_crud() {
    let (ws, _dir) = open_temp_workspace().await;

    let d = ws
        .add_decision("Use Rust".into(), "Performance matters".into(), agent("a"))
        .await
        .unwrap();

    let all = ws.list_decisions().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].title, "Use Rust");

    let removed = ws.remove_decision(d.id, agent("a")).await.unwrap();
    assert!(removed);

    let all = ws.list_decisions().await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn test_constraints_crud() {
    let (ws, _dir) = open_temp_workspace().await;

    let c = ws
        .add_constraint("Max 100ms latency".into(), "SLA requirement".into(), agent("b"))
        .await
        .unwrap();

    let all = ws.list_constraints().await.unwrap();
    assert_eq!(all.len(), 1);

    ws.remove_constraint(c.id, agent("b")).await.unwrap();
    assert!(ws.list_constraints().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_glossary_crud() {
    let (ws, _dir) = open_temp_workspace().await;

    ws.add_glossary_entry("Artifact".into(), "A shared work product".into(), agent("c"))
        .await
        .unwrap();

    let entries = ws.list_glossary().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].term, "Artifact");

    ws.remove_glossary_entry("Artifact", agent("c")).await.unwrap();
    assert!(ws.list_glossary().await.unwrap().is_empty());
}

// ────────────────────── change history ──────────────────────────

#[tokio::test]
async fn test_history_records_operations() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("h.txt".into(), ArtifactType::Test, agent("a"), b"1".to_vec())
        .await
        .unwrap();
    ws.update_artifact(art.id, b"2".to_vec(), agent("b")).await.unwrap();
    ws.delete_artifact(art.id, agent("a")).await.unwrap();

    let all = ws.read_history().await.unwrap();
    assert_eq!(all.len(), 3);

    // Check action types in order
    assert!(matches!(all[0].action, ChangeAction::Create));
    assert!(matches!(all[1].action, ChangeAction::Update));
    assert!(matches!(all[2].action, ChangeAction::Delete));

    let recent = ws.read_recent_history(2).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert!(matches!(recent[0].action, ChangeAction::Delete)); // newest first
}

// ────────────────────── active tasks ────────────────────────────

#[tokio::test]
async fn test_task_lifecycle() {
    let (ws, _dir) = open_temp_workspace().await;

    let task = ws
        .add_task(agent("worker"), "Implement feature X".into())
        .await
        .unwrap();
    assert_eq!(task.status, TaskStatus::Pending);

    let tasks = ws.list_tasks().await.unwrap();
    assert_eq!(tasks.len(), 1);

    ws.update_task_status(task.id, TaskStatus::InProgress).await.unwrap();
    let tasks = ws.list_tasks().await.unwrap();
    assert_eq!(tasks[0].status, TaskStatus::InProgress);

    ws.update_task_status(task.id, TaskStatus::Completed).await.unwrap();
    let tasks = ws.list_tasks().await.unwrap();
    assert_eq!(tasks[0].status, TaskStatus::Completed);

    let removed = ws.remove_task(task.id).await.unwrap();
    assert!(removed);
    assert!(ws.list_tasks().await.unwrap().is_empty());
}

// ────────────────────── re-open idempotency ─────────────────────

#[tokio::test]
async fn test_reopen_preserves_data() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("workspace");

    // Create and populate
    {
        let ws = SharedWorkspace::open(&root).await.unwrap();
        ws.create_artifact("persist.md".into(), ArtifactType::Design, agent("a"), b"data".to_vec())
            .await
            .unwrap();
        ws.add_decision("Keep it".into(), "Because".into(), agent("a"))
            .await
            .unwrap();
        ws.add_task(agent("a"), "do stuff".into()).await.unwrap();
    }

    // Re-open and verify
    {
        let ws = SharedWorkspace::open(&root).await.unwrap();

        let arts = ws.list_artifacts(&ArtifactFilter::default()).await.unwrap();
        assert_eq!(arts.len(), 1);
        assert_eq!(arts[0].name, "persist.md");

        let decs = ws.list_decisions().await.unwrap();
        assert_eq!(decs.len(), 1);

        let tasks = ws.list_tasks().await.unwrap();
        assert_eq!(tasks.len(), 1);

        let history = ws.read_history().await.unwrap();
        assert_eq!(history.len(), 3); // create + add_decision + add_task
    }
}

// ────────────────────── serde round-trip for types ──────────────

#[tokio::test]
async fn test_artifact_serde_roundtrip() {
    let (ws, _dir) = open_temp_workspace().await;

    // Use content with non-UTF8 bytes to verify base64 encoding
    let binary_content: Vec<u8> = (0..=255).collect();
    let art = ws
        .create_artifact(
            "binary.bin".into(),
            ArtifactType::Other("binary".into()),
            agent("a"),
            binary_content.clone(),
        )
        .await
        .unwrap();

    let fetched = ws.get_artifact(art.id).await.unwrap().unwrap();
    assert_eq!(fetched.content, binary_content);
}

// ────────────────────── conflict detection ──────────────────────

#[tokio::test]
async fn test_version_conflict_rejected() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("cf.txt".into(), ArtifactType::Code, agent("a"), b"v1".to_vec())
        .await
        .unwrap();
    assert_eq!(art.version, 1);

    // Update with correct expected version works
    let v2 = ws
        .update_artifact_if_version(art.id, b"v2".to_vec(), agent("a"), 1)
        .await
        .unwrap();
    assert_eq!(v2.version, 2);

    // Update with stale expected version is rejected
    let err = ws
        .update_artifact_if_version(art.id, b"stale".to_vec(), agent("b"), 1)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("version conflict"),
        "expected version conflict, got: {err}"
    );

    // Conflict should be recorded in history
    let history = ws.read_history().await.unwrap();
    let conflict_entries: Vec<_> = history
        .iter()
        .filter(|r| matches!(r.action, ChangeAction::Conflict))
        .collect();
    assert!(!conflict_entries.is_empty(), "conflict should be in history");
}

#[tokio::test]
async fn test_overwrite_strategy_accepts_stale() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("ow.txt".into(), ArtifactType::Code, agent("a"), b"v1".to_vec())
        .await
        .unwrap();

    ws.update_artifact(art.id, b"v2".to_vec(), agent("a")).await.unwrap();

    // With Overwrite strategy, stale expected version is accepted
    let v3 = ws
        .update_artifact_with_strategy(
            art.id,
            b"forced".to_vec(),
            agent("b"),
            Some(1),
            ConflictStrategy::Overwrite,
        )
        .await
        .unwrap();
    assert_eq!(v3.version, 3);
    assert_eq!(v3.content, b"forced");
}

// ────────────────────── delete respects locks ───────────────────

#[tokio::test]
async fn test_delete_blocked_by_lock() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("locked_del.txt".into(), ArtifactType::Code, agent("a"), b"x".to_vec())
        .await
        .unwrap();

    ws.lock_artifact(art.id, agent("a")).await.unwrap();

    // Agent b cannot delete (locked by a)
    let err = ws
        .delete_artifact(art.id, agent("b"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("locked"), "expected lock error, got: {err}");

    // Agent a can delete their own locked artifact
    let deleted = ws.delete_artifact(art.id, agent("a")).await.unwrap();
    assert!(deleted);
}

// ────────────────────── rollback respects locks ─────────────────

#[tokio::test]
async fn test_rollback_through_facade() {
    let (ws, _dir) = open_temp_workspace().await;

    let art = ws
        .create_artifact("rb.txt".into(), ArtifactType::Design, agent("a"), b"v1".to_vec())
        .await
        .unwrap();
    ws.update_artifact(art.id, b"v2".to_vec(), agent("a")).await.unwrap();
    ws.update_artifact(art.id, b"v3".to_vec(), agent("a")).await.unwrap();

    let rolled = ws.rollback_artifact(art.id, 1, agent("a")).await.unwrap();
    assert_eq!(rolled.version, 4);
    assert_eq!(rolled.content, b"v1");

    // History should contain the rollback entry
    let history = ws.read_history().await.unwrap();
    let rollback_entries: Vec<_> = history
        .iter()
        .filter(|r| r.description.contains("rolled back"))
        .collect();
    assert_eq!(rollback_entries.len(), 1);
}

// ────────────────────── context via facade with history ──────────

#[tokio::test]
async fn test_context_via_facade_records_history() {
    let (ws, _dir) = open_temp_workspace().await;

    // Decisions via facade
    let d = ws
        .add_decision("Use async".into(), "Better concurrency".into(), agent("a"))
        .await
        .unwrap();
    assert_eq!(d.title, "Use async");

    ws.remove_decision(d.id, agent("a")).await.unwrap();

    // Constraints via facade
    let c = ws
        .add_constraint("No panics".into(), "Must handle errors".into(), agent("b"))
        .await
        .unwrap();
    ws.remove_constraint(c.id, agent("b")).await.unwrap();

    // Glossary via facade
    let g = ws
        .add_glossary_entry("Agent".into(), "An autonomous worker".into(), agent("c"))
        .await
        .unwrap();
    assert_eq!(g.term, "Agent");
    ws.remove_glossary_entry("Agent", agent("c")).await.unwrap();

    // All context ops should be in history
    let history = ws.read_history().await.unwrap();
    let context_entries: Vec<_> = history
        .iter()
        .filter(|r| matches!(r.action, ChangeAction::ContextUpdate))
        .collect();
    assert_eq!(context_entries.len(), 6); // 3 adds + 3 removes
}

// ────────────────────── dashboard ───────────────────────────────

#[tokio::test]
async fn test_dashboard() {
    let (ws, _dir) = open_temp_workspace().await;

    ws.create_artifact("d1.txt".into(), ArtifactType::Code, agent("a"), b"x".to_vec())
        .await
        .unwrap();
    ws.create_artifact("d2.txt".into(), ArtifactType::Design, agent("b"), b"y".to_vec())
        .await
        .unwrap();
    ws.add_task(agent("a"), "task 1".into()).await.unwrap();
    ws.add_task_with_status(agent("b"), "task 2".into(), TaskStatus::Completed)
        .await
        .unwrap();

    let dash = ws.dashboard(10).await.unwrap();
    assert_eq!(dash.artifact_count, 2);
    // Only pending/in-progress tasks appear
    assert_eq!(dash.active_tasks.len(), 1);
    assert_eq!(dash.active_tasks[0].description, "task 1");
    assert!(dash.recent_changes.len() >= 4); // 2 creates + 2 task updates
}
