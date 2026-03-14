---
name: github
description: "Interact with GitHub using the `gh` CLI. Use `gh issue`, `gh pr`, `gh run`, and `gh api` for issues, PRs, CI runs, and advanced queries."
metadata: {"mofaclaw":{"emoji":"🐙","requires":{"bins":["gh"]},"install":[{"id":"brew","kind":"brew","formula":"gh","bins":["gh"],"label":"Install GitHub CLI (brew)"},{"id":"apt","kind":"apt","package":"gh","bins":["gh"],"label":"Install GitHub CLI (apt)"}]}}
---

# GitHub Skill

Use the `gh` CLI to interact with GitHub. Always specify `--repo owner/repo` when not in a git directory, or use URLs directly.

## Pull Requests

Check CI status on a PR:
```bash
gh pr checks 55 --repo owner/repo
```

## CI/CD Status View

List recent workflow runs:
```bash
gh run list --repo owner/repo --limit 10
```

List runs filtered by workflow:
```bash
gh run list --repo owner/repo --limit 10 --workflow ci.yml
```

View a run and see which steps failed:
```bash
gh run view <run-id> --repo owner/repo
```

View logs for failed steps only:
```bash
gh run view <run-id> --repo owner/repo --log-failed
```

Monitor a run in real-time (blocks until complete):
```bash
gh run watch <run-id> --repo owner/repo --exit-status
```

View PR CI check results:
```bash
gh pr checks <pr-number> --repo owner/repo
```

### Status Indicators
Use these emoji when formatting CI/CD results:
- ✅ success / pass
- ❌ failure / fail
- 🟡 in_progress / pending
- ⏳ queued / waiting

### JSON output for CI runs
```bash
gh run list --repo owner/repo --json databaseId,status,conclusion,name,headBranch,event,createdAt --limit 10
```

## API for Advanced Queries

The `gh api` command is useful for accessing data not available through other subcommands.

Get PR with specific fields:
```bash
gh api repos/owner/repo/pulls/55 --jq '.title, .state, .user.login'
```

Get workflow runs via API:
```bash
gh api repos/owner/repo/actions/runs --jq '.workflow_runs[:5] | .[] | "\(.id) \(.status) \(.conclusion) \(.name)"'
```

## JSON Output

Most commands support `--json` for structured output.  You can use `--jq` to filter:

```bash
gh issue list --repo owner/repo --json number,title --jq '.[] | "\(.number): \(.title)"'
```
