---
name: pr-review
description: "Analyze pull requests and provide structured code review suggestions. Use when the user asks to review a PR, check a PR, analyze pull request changes, inspect CI status, summarize review findings, verify PR template compliance, or generate review comments."
metadata:
  mofaclaw:
    emoji: "🔍"
    requires:
      bins: ["gh", "git"]
    install:
      - id: brew
        kind: brew
        formula: gh
        bins: ["gh"]
        label: "Install GitHub CLI (brew)"
      - id: apt
        kind: apt
        package: gh
        bins: ["gh"]
        label: "Install GitHub CLI (apt)"
---

# PR Review Skill

Use the `gh` CLI to analyze pull requests and provide structured code review suggestions. Focus on reviewer assistance, not autopilot approval. Prefer concrete findings, reproducible commands, and links back to the exact failing checks or changed code.

When possible, collect facts in this order:
1. PR metadata and body
2. Changed-file summary
3. Targeted diff inspection
4. CI status and logs
5. Final review comment

## When to Use (Trigger Phrases)

Use this skill immediately when the user asks any of:
- "review this PR"
- "review pr"
- "check this pr"
- "pr review for"
- "analyze pull request"
- "what changed in this PR"
- "review code changes"
- "check CI status on PR"
- "generate review comment for PR"
- "is this PR ready to merge"
- "summarize the review findings on this PR"

## Workflow

### Step 1: PR Overview

Fetch PR metadata and summary. Start with structured JSON before reading raw patch output:

```bash
# Get PR details with files, additions, deletions
gh pr view <PR_NUMBER> --repo <OWNER/REPO> --json title,body,author,state,baseRefName,headRefName,additions,deletions,changedFiles,files

# Example for mofaclaw repo
gh pr view 78 --repo mofa-org/mofaclaw --json title,body,author,baseRefName,headRefName,files,additions,deletions,changedFiles

# Get commit messages for change intent and breaking-change clues
gh api repos/<OWNER>/<REPO>/pulls/<PR_NUMBER>/commits --jq '.[].commit.message'
```

Review checklist:
- Confirm title, author, base branch, and head branch make sense.
- Summarize scope from changed files before diving into patch details.
- Note whether the PR is unusually large and may need splitting.

### Step 1.5: PR Template Compliance

Check whether the PR description follows the repository template or at least covers expected review sections:

```bash
gh pr view <PR_NUMBER> --repo <OWNER/REPO> --json body --jq .body
```

Look for expected sections such as:
- summary or motivation
- linked issue
- testing notes
- rollout or risk notes when applicable

If the body is empty or missing key sections, call it out as a reviewer suggestion rather than inventing compliance.

### Step 2: Code Change Analysis

Analyze changed files by type:

```bash
# List all changed files
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only

# Get additions/deletions summary (use --json with pr view)
gh pr view <PR_NUMBER> --repo <OWNER/REPO> --json additions,deletions,changedFiles

# Get detailed file changes with patch (Unix/Mac)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --patch | head -100

# Get detailed file changes with patch (PowerShell)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --patch | Select-Object -First 100

# Inspect one risky file more closely
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --patch -- <PATH>
```

**File type categorization:**
- `src/` - Source code
- `test/` or `tests/` - Test files
- `config/` or `.github/` - Configuration
- `docs/` or `*.md` - Documentation

**Flag large diffs:**
- >10 files or >500 lines: Requires careful review
- >20 files or >1000 lines: Consider splitting

**Breaking-change checks:**
- Compare changed public APIs, config schema, CLI flags, env vars, and default behavior.
- Read commit messages for words like `breaking`, `rename`, `remove`, `deprecate`, or `migrate`.
- Treat changes in `core/src/lib.rs`, public exports, config structs, API routes, and external command behavior as higher risk.
- If a breaking change is likely, verify the PR body documents migration impact.

### Step 3: Security Checks

Check for sensitive file changes and potential vulnerabilities:

```bash
# Check for sensitive files (Unix/Mac)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only | grep -iE '(password|secret|key|token|credentials|\.env|config\.json)'

# Check for sensitive files (Windows)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only | findstr /i "password secret key token credentials .env config.json"

# Check for potential injection patterns in diff (Unix/Mac)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> | grep -iE '(exec\(|system\(|eval\(|shell_exec\(|subprocess\()'

# Check for potential injection patterns (Windows)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> | findstr /i "exec( system( eval( shell_exec( subprocess("

# Check for SQL injection patterns (Unix/Mac)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> | grep -iE '(SELECT|INSERT|UPDATE|DELETE).*\+.*'

# Check for changed workflow, deployment, or secret-related files
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only | findstr /i ".github/workflows docker compose .env secret token key"
```

**Security checklist:**
- [ ] No credentials/secrets exposed
- [ ] No hardcoded API keys or tokens
- [ ] Input validation present
- [ ] No dangerous functions (exec, eval, system)
- [ ] No SQL injection vulnerabilities
- [ ] No path traversal issues
- [ ] Dependencies are secure

### Step 4: Quality Suggestions

Check for test coverage and documentation:

```bash
# Get list of changed source files (Unix/Mac)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only | grep -E '^core/|^src/|^lib/'

# Get list of changed source files (Windows)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only | findstr "core/ src/ lib/"

# Check if corresponding tests exist
# For each .rs file, check if test_*.rs or *_test.rs exists

# Check for documentation changes (Unix/Mac)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only | grep -E '\.md$|docs/'

# Check for documentation changes (Windows)
gh pr diff <PR_NUMBER> --repo <OWNER/REPO> --name-only | findstr /i ".md docs/"
```

**Quality checklist:**
- [ ] Modified source files have corresponding tests
- [ ] Public APIs are documented
- [ ] Breaking changes are documented
- [ ] Code follows project style guidelines

Reviewer guidance:
- Prefer concrete missing-test observations over generic "needs more tests."
- If public exports or config change, check for docs, examples, or migration notes.
- When a file is risky but hard to test, ask for manual verification steps in the PR body.

### Step 5: CI Status Check

Verify CI/CD pipeline status:

```bash
# Check CI status
gh pr checks <PR_NUMBER> --repo <OWNER/REPO>

# Get detailed status in JSON
gh pr view <PR_NUMBER> --repo <OWNER/REPO> --json statusCheckRollup

# List recent runs tied to the PR branch
gh run list --repo <OWNER/REPO> --branch <HEAD_BRANCH> --limit 10

# Inspect a specific failed run
gh run view <RUN_ID> --repo <OWNER/REPO>

# Show logs for only failed steps
gh run view <RUN_ID> --repo <OWNER/REPO> --log-failed
```

**CI checklist:**
- [ ] All CI checks pass
- [ ] No failed checks
- [ ] Required checks are included

CI review guidance:
- Summarize failed checks by name, status, and what stage failed.
- Include the `gh run view` URL or the Actions URL when reporting failures.
- If all checks pass, say so briefly instead of over-explaining.
- If checks are missing, state that clearly instead of assuming they passed.

### Step 6: Review Comment Generation

Generate structured review comments based on findings.

Preferred output order:
1. Blocking concerns
2. Medium-risk suggestions
3. Nits
4. Short summary / recommendation

Do not pad the review with generic praise if there are actionable issues.

Minimum review output:
- PR scope summary
- File/risk summary
- CI summary
- Findings grouped as blocking, suggestions, and nits
- Explicit recommendation: approve, comment, or request changes

## Example Commands (Tested on mofaclaw)

### Example 1: Large PR Analysis (PR #78)

```bash
# PR Overview
gh pr view 78 --repo mofa-org/mofaclaw --json title,body,author,files,additions,deletions,changedFiles
# Result: 755 additions, 4 deletions, 5 files changed

# List changed files
gh pr diff 78 --repo mofa-org/mofaclaw --name-only
# Result: core/src/agent/loop_.rs, core/src/error.rs, core/src/lib.rs, core/src/tools/mod.rs, core/src/tools/permissions.rs

# Check CI status
gh pr checks 78 --repo mofa-org/mofaclaw

# Check commit messages
gh api repos/mofa-org/mofaclaw/pulls/78/commits --jq '.[].commit.message'
```

### Example 2: Small Documentation PR (PR #89)

```bash
# PR Overview
gh pr view 89 --repo mofa-org/mofaclaw --json title,body,author,files,additions,deletions,changedFiles
# Result: 32 additions, 1 deletion, 2 files changed

# List changed files
gh pr diff 89 --repo mofa-org/mofaclaw --name-only
# Result: TUTORIAL.md, TUTORIAL_CN.md

# Check CI status
gh pr checks 89 --repo mofa-org/mofaclaw
```

### Example 3: Review CI Failure Details

```bash
# Find branch name first
gh pr view 78 --repo mofa-org/mofaclaw --json headRefName --jq .headRefName

# Find recent runs for that branch
gh run list --repo mofa-org/mofaclaw --branch feat/permission-based-tool-registry --limit 5

# View one run in detail
gh run view <RUN_ID> --repo mofa-org/mofaclaw
```

## Review Comment Template

Use this template for structured review comments:

```markdown
## Code Review Summary

### PR Overview
- **Title:** <PR title>
- **Author:** <author>
- **Files Changed:** <count> files, <additions> additions, <deletions> deletions
- **CI Status:** ✅ Passing / ❌ Failing

### Findings

#### ✅ Things Look Good
- <positive observation 1>
- <positive observation 2>

#### 🔧 Suggestions
- <suggestion 1>
- <suggestion 2>

#### ⚠️ Concerns (Blocking)
- <blocking issue 1>
- <blocking issue 2>

#### 📝 Nits
- <minor issue 1>
- <minor issue 2>

### Security Check
- [ ] No credentials exposed
- [ ] Input validation present
- [ ] No dangerous functions
- [ ] Dependencies secure

### Quality Check
- [ ] Tests included
- [ ] Documentation updated
- [ ] Breaking changes documented

---

**Recommendation:** ✅ Approve / 🛡️ Request Changes / 💬 Comment
```

## Important Notes

- Focus on assisting reviewers, not replacing them
- Provide actionable suggestions, not just observations
- Include escalation paths for security concerns
- Consider rate limits when fetching large PRs
- Always verify commands work as expected before including in output
- Test against real PRs in the repository before finalizing review
- If you did not inspect CI logs, say that explicitly instead of implying you did
- If you infer a breaking change, label it as an inference and cite the changed surface

## Error Handling

If commands fail:
- Check if PR number is valid
- Verify repository exists and is accessible
- Ensure `gh` is authenticated (`gh auth status`)
- For large PRs, consider fetching just the summary first
