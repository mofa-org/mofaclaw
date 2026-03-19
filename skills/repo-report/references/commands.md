# repo-report: Command Reference

Deep reference for every `gh` and `git` command used in this skill. Read this file when you need additional flags, pagination, or custom `jq` filters.

## Table of Contents

1. [gh pr list](#gh-pr-list)
2. [gh issue list](#gh-issue-list)
3. [gh run list](#gh-run-list)
4. [gh run view](#gh-run-view)
5. [gh api](#gh-api-advanced)
6. [git shortlog](#git-shortlog)
7. [git log](#git-log)
8. [Pagination & Limits](#pagination--limits)
9. [Date Handling](#date-handling)

---

## gh pr list

```bash
gh pr list \
  --repo owner/repo \          # target repo (omit when inside git dir)
  --state all|open|closed|merged \
  --limit 200 \                # max 1000; use pagination for more
  --base main \                # filter by base branch
  --author "@me" \             # filter by author
  --json <fields> \            # structured output (see fields below)
  --jq '<filter>'              # inline jq filter
```

### JSON fields available for `gh pr list`

| Field | Type | Description |
|-------|------|-------------|
| `number` | int | PR number |
| `title` | string | PR title |
| `state` | string | `OPEN`, `CLOSED`, `MERGED` |
| `createdAt` | ISO8601 | creation timestamp |
| `mergedAt` | ISO8601 or null | merge timestamp |
| `closedAt` | ISO8601 or null | close timestamp |
| `additions` | int | lines added |
| `deletions` | int | lines removed |
| `author.login` | string | author username |
| `labels[].name` | string | label names |
| `commits` | int | commit count |
| `reviewDecision` | string | `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED` |
| `isDraft` | bool | draft status |
| `headRefName` | string | source branch |
| `baseRefName` | string | target branch |

### Useful jq recipes for PRs

```bash
# Time-to-merge for each PR in hours
--jq '[.[] | select(.mergedAt != null)
       | {number: .number,
          hours: (((.mergedAt|fromdateiso8601)-(.createdAt|fromdateiso8601))/3600|round)}]'

# PRs by author
--jq 'group_by(.author.login) | map({(.[0].author.login): length}) | add'

# Draft vs non-draft
--jq 'group_by(.isDraft) | map({(.[0].isDraft|tostring): length}) | add'
```

---

## gh issue list

```bash
gh issue list \
  --repo owner/repo \
  --state all|open|closed \
  --limit 200 \
  --label "bug" \              # filter by label
  --assignee "@me" \
  --json <fields> \
  --jq '<filter>'
```

### JSON fields available for `gh issue list`

| Field | Type | Description |
|-------|------|-------------|
| `number` | int | issue number |
| `title` | string | issue title |
| `state` | string | `OPEN`, `CLOSED` |
| `createdAt` | ISO8601 | creation timestamp |
| `closedAt` | ISO8601 or null | close timestamp |
| `labels[].name` | string | label names |
| `author.login` | string | author username |
| `assignees[].login` | string | assignee usernames |
| `comments` | int | comment count |
| `milestone.title` | string | milestone name |

### Useful jq recipes for issues

```bash
# Issues closed in the time range
--jq '[.[] | select(.closedAt != null and .closedAt >= "2025-01-01")]'

# Label breakdown
--jq '[.[] | .labels[].name] | group_by(.) | map({(.[0]): length}) | add // {}'

# Issues with no labels
--jq '[.[] | select(.labels | length == 0)] | length'
```

---

## gh run list

```bash
gh run list \
  --repo owner/repo \
  --limit 100 \                # max 1000
  --workflow ci.yml \          # filter by workflow file name or ID
  --branch main \              # filter by branch
  --user "@me" \               # filter by triggering user
  --status success|failure|cancelled|in_progress \
  --json <fields> \
  --jq '<filter>'
```

### JSON fields available for `gh run list`

| Field | Type | Description |
|-------|------|-------------|
| `databaseId` | int | run ID |
| `name` | string | workflow name |
| `status` | string | `completed`, `in_progress`, `queued` |
| `conclusion` | string | `success`, `failure`, `cancelled`, `skipped`, null |
| `createdAt` | ISO8601 | start timestamp |
| `updatedAt` | ISO8601 | last update timestamp |
| `headBranch` | string | branch name |
| `headSha` | string | commit SHA |
| `event` | string | `push`, `pull_request`, `schedule`, etc. |
| `workflowName` | string | display name |
| `url` | string | web URL |

### Useful jq recipes for runs

```bash
# Success rate percentage
--jq '[.[] | select(.status=="completed")] as $all
      | ($all | map(select(.conclusion=="success")) | length) as $ok
      | {total: ($all|length), success: $ok,
         rate: (if ($all|length) > 0 then ($ok/($all|length)*100|.*10|round/10) else 0 end)}'

# Build time per workflow
--jq 'group_by(.workflowName)
      | map({name: .[0].workflowName,
             avg_minutes: ([.[] | select(.status=="completed")
               | ((.updatedAt|fromdateiso8601)-(.createdAt|fromdateiso8601))/60]
               | if length > 0 then (add/length|.*10|round/10) else 0 end)})'
```

---

## gh run view

```bash
# View a specific run
gh run view <run-id> --repo owner/repo

# Show only failed steps
gh run view <run-id> --repo owner/repo --log-failed

# JSON details of a run
gh run view <run-id> --repo owner/repo --json status,conclusion,name,jobs
```

---

## gh api (Advanced)

Use `gh api` for data not in `gh` subcommands, or for higher limits via pagination.

```bash
# Participation stats (52-week commit activity) — no auth for public repos
gh api repos/owner/repo/stats/participation \
  --jq '{total_last_4_weeks: (.all[-4:] | add), weeks: .all[-4:]}'

# Paginated PRs (past GitHub limit)
gh api "repos/owner/repo/pulls?state=all&per_page=100&page=1" --jq 'length'

# Contributors list
gh api repos/owner/repo/contributors --jq '[.[] | {login: .login, commits: .contributions}]'

# Commits per day via API
gh api "repos/owner/repo/commits?since=2025-01-01T00:00:00Z&per_page=100" \
  --jq '[.[] | .commit.author.date[:10]] | group_by(.) | map({(.[0]): length}) | add'

# Languages breakdown
gh api repos/owner/repo/languages
```

---

## git shortlog

```bash
git shortlog \
  -sn \                        # -s summary count, -n sort by number
  --since="30 days ago" \      # or --after="2025-01-01"
  --until="today" \
  --all \                      # include all branches
  HEAD                         # can also pass a branch name
```

---

## git log

```bash
git log \
  --since="30 days ago" \
  --until="today" \
  --format="%H %ad %an %s" \  # hash, date, author, subject
  --date=short \               # YYYY-MM-DD
  --name-only \                # list changed files (one per line)
  --pretty=tformat: \          # suppress commit header (for numstat)
  --numstat \                  # +lines -lines filename
  --no-merges \                # exclude merge commits
  HEAD
```

### Commit frequency (group by day)

```bash
git log --since="30 days ago" --format="%ad" --date=short \
  | sort | uniq -c | sort -k2
```

### Conventional commits compliance

```bash
# Count non-compliant commit subjects
git log --since="30 days ago" --format="%s" \
  | grep -Ecv "^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?: .+"

# Show non-compliant subjects
git log --since="30 days ago" --format="%s" \
  | grep -Ev "^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?: .+"
```

---

## Pagination & Limits

`gh pr list`, `gh issue list`, and `gh run list` all cap at `--limit 1000`. For larger repos:

```bash
# Use gh api with pagination loop
page=1
while true; do
  results=$(gh api "repos/$REPO/pulls?state=all&per_page=100&page=$page" \
            --jq 'if length == 0 then empty else . end' 2>/dev/null) || break
  echo "$results"
  ((page++))
done
```

---

## Date Handling

Cross-platform date arithmetic (macOS vs Linux):

```bash
# macOS (BSD date)
SINCE=$(date -v-${DAYS}d +%Y-%m-%dT%H:%M:%SZ)

# Linux (GNU date)
SINCE=$(date -d "-${DAYS} days" +%Y-%m-%dT%H:%M:%SZ)

# Compatible wrapper
if date -v-1d +%Y 2>/dev/null; then
  SINCE=$(date -v-${DAYS}d +%Y-%m-%dT%H:%M:%SZ)   # macOS
else
  SINCE=$(date -d "-${DAYS} days" +%Y-%m-%dT%H:%M:%SZ)  # Linux
fi
```

ISO 8601 comparison in `jq` works with string comparison because the format is sortable.
