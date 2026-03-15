---
name: multi-agent
description: Review code, review architecture, or delegate to a multi-agent team. Use for "review my code", "review this PR", "review this function", "review this architecture", "review this design", "have the team work on this", "use the team", "multi-agent review".
---

# Multi-Agent Skill

Route to the right MoFA workflow, create a team, and run it.

## 1. Pick a workflow

| Intent | Workflow | Roles |
|---|---|---|
| Review/implement code, check a PR, write tests | `code_review` | developer, reviewer, architect, tester |
| Review architecture, design a system | `design` | architect, developer |

Default to `code_review` if unsure.

## 2. Check for an existing team

```
list_teams()
```

Reuse a team that already has the right roles. If one exists, skip to step 4.

## 3. Create a team

**code_review:**
```
create_team(
  team_id: "code-review-<short-id>",
  team_name: "Code Review Team",
  roles: [
    { role: "developer",  instance_id: "dev-1" },
    { role: "reviewer",   instance_id: "rev-1" },
    { role: "architect",  instance_id: "arch-1" },
    { role: "tester",     instance_id: "test-1" }
  ]
)
```

**design:**
```
create_team(
  team_id: "design-<short-id>",
  team_name: "Design Review Team",
  roles: [
    { role: "architect", instance_id: "arch-1" },
    { role: "developer", instance_id: "dev-1" }
  ]
)
```

## 4. Start the workflow

```
start_workflow(
  workflow_name: "<chosen workflow>",
  team_id: "<team_id>",
  initial_context: { "user_request": "<user's request verbatim>" }
)
```

## 5. Monitor and present results

- Poll `get_workflow_status(workflow_id)` until complete.
- If a step is blocked on approval, call `respond_to_approval`.
- Fetch artifacts with `list_artifacts` + `get_artifact`.
- Summarize what each agent produced.

## Notes

- Pass the user's original request verbatim as `user_request` — the workflow injects it into each agent's prompt.
- Use `broadcast_to_team` to send additional context mid-workflow.
- `code_review` artifacts: `implementation`, `review_comments`, `test_results`.
