# Multi-Agent Architecture: Design & Evolution Plan

## Current Architecture

### What we have

```
User message
    ↓
AgentLoop (main agent)
    ↓ tool call: create_team
TeamManager ──creates──▶ AgentTeam
    │                       └─ TeamMember (AgentLoop × N)
    ↓ tool call: start_workflow
WorkflowEngine
    ↓ execute_workflow (background task)
    ├─ Layer 1: step A             (sequential layers,
    ├─ Layer 2: step B ‖ step C    parallel within each)
    └─ Layer 3: step D
```

### What the "single agent loop" concern means

The current model is **centralized and reactive**:

1. **Orchestrator-driven**: The main agent calls `start_workflow`, and `WorkflowEngine` drives every step — agents only respond when the engine calls `process_direct` on them.
2. **No agent initiative**: Team member agents cannot start tasks, send messages, or react to events on their own. They wake up only when assigned a step.
3. **No peer communication during execution**: While a step is running, that agent cannot send a message to another agent and wait for a response as part of its reasoning. The `send_agent_message` tool exists but agents in the middle of a workflow step have no mechanism to yield and wait.
4. **Centralized control**: If `WorkflowEngine` dies or the main agent loses context, the workflow stops. There is no self-organizing behavior.

This is not wrong for the current use case — it's simple and predictable. But it limits what kinds of collaboration are possible.

---

## Proposed Architecture: Event-Driven Autonomous Agents

### Core idea

Each agent runs its own persistent event loop. Instead of being called to execute a single task, agents subscribe to events and decide how to respond. The `WorkflowEngine` becomes a coordinator, not a controller.

```
                    MessageBus (AgentMessageBus)
                         │
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
  Developer Agent   Reviewer Agent   Architect Agent
  (event loop)      (event loop)     (event loop)
        │                │                │
        └────────────────┴────────────────┘
                         │
                  SharedWorkspace
               (artifact store, events)
```

### Key changes needed

#### 1. Agent event loop (`AgentEventLoop`)

A new type wrapping `AgentLoop` that runs persistently and processes events:

```rust
pub struct AgentEventLoop {
    agent_id: AgentId,
    agent_loop: Arc<AgentLoop>,
    event_bus: Arc<AgentMessageBus>,
    workspace: Arc<SharedWorkspace>,
}

impl AgentEventLoop {
    /// Run until shutdown signal received
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut events = self.event_bus.subscribe_agent(&self.agent_id);
        loop {
            tokio::select! {
                Ok(event) = events.recv() => {
                    self.handle_event(event).await;
                }
                _ = shutdown.changed() => break,
            }
        }
    }

    async fn handle_event(&self, event: AgentMessage) {
        // Agent decides what to do based on event type:
        // - WorkflowStepAssigned → execute the step
        // - ArtifactUpdated → review it if reviewer role
        // - AgentMessage → respond or forward
        // - ApprovalRequested → approve/reject based on role judgment
    }
}
```

#### 2. Workflow coordinator (not controller)

`WorkflowEngine` becomes a coordinator: it publishes `WorkflowStepAssigned` events and waits for `StepCompleted` events. It does not call agents directly.

```rust
// Instead of:
member.agent_loop.process_direct(&prompt, &session_key).await

// Publish an event and wait for completion:
engine.publish(WorkflowStepAssigned { step, assigned_to: agent_id });
engine.wait_for(StepCompleted { step_id }).await;
```

#### 3. Artifact-triggered behavior

Agents subscribe to workspace artifact events. A reviewer agent can automatically trigger a review when a new `implementation` artifact is created — without the workflow engine explicitly calling it.

```rust
// Reviewer agent's event handler:
AgentMessageType::ArtifactUpdate { artifact_id, artifact_type } => {
    if artifact_type == ArtifactType::CodeFile && self.role == "reviewer" {
        self.initiate_review(artifact_id).await;
    }
}
```

#### 4. Agent-initiated workflows

Agents can start workflows themselves. Example: a developer agent finishes an implementation and decides the code needs review — it calls `start_workflow("code_review", ...)` without waiting for the main agent to do it.

---

## When to implement

Do **not** implement this now. The right trigger is a concrete use case that the current model cannot handle. Examples of such use cases:

| Use case | Why current model fails | Why event-driven helps |
|---|---|---|
| Long-running background agent that watches a repo and reviews new PRs proactively | `WorkflowEngine` only runs when triggered by user | Reviewer agent can subscribe to webhook events and self-trigger |
| Agents that need to consult each other mid-step (e.g. developer asks architect a question during implementation) | `process_direct` is a single atomic call with no inter-agent yield | Event loop allows send-and-wait patterns during a step |
| Parallel agents that race to complete a task and the fastest result wins | Workflow layers are coordinated by engine | Agents can independently attempt and publish results |
| Persistent team that maintains context across user sessions without user re-creating it | `AgentLoop` sessions are ephemeral | Persistent event loops can accumulate shared context in workspace |

---

## Migration path (when the time comes)

1. **Phase 1**: Add `AgentEventLoop` as an opt-in wrapper. Existing `WorkflowEngine` unchanged.
2. **Phase 2**: Add `WorkflowStepAssigned` and `StepCompleted` event types to `AgentMessageBus`.
3. **Phase 3**: Change `WorkflowEngine.execute_step` to use event publishing instead of direct call.
4. **Phase 4**: Add artifact-change events to `SharedWorkspace`.
5. **Phase 5**: Enable agent-initiated task creation via tool (`initiate_workflow`).

Each phase is independently mergeable and backward compatible.

---

## What stays the same

- `ToolRegistry` / `ToolRegistryExecutor` — MoFA standard, unchanged
- `SharedWorkspace` / artifact model — extended, not replaced
- `AgentMessageBus` — extended with new event types
- Skill-triggered entry point — user still starts things via chat; autonomous behavior is additive

---

## Related files

- `core/src/agent/collaboration/team.rs` — `TeamManager`, `AgentTeam`, `TeamMember`
- `core/src/agent/collaboration/workflow.rs` — `WorkflowEngine`, step execution
- `core/src/agent/communication/` — `AgentMessageBus`, `AgentMessage`, `RequestType`
- `core/src/agent/collaboration/workspace.rs` — `SharedWorkspace`, artifact events
