---
name: repo-report
description: "Generate comprehensive repository analytics reports for managers. Provides commit activity, PR/issue metrics, CI health, and code statistics. Supports time ranges like 'last N days/weeks/months'. Use when asked for: 'repo report', 'repository stats', 'project health', 'repository analytics', 'commit activity report', 'PR metrics', 'issue stats', 'CI health', 'code change statistics', or any request for a data-driven overview of a GitHub repository."
metadata: {"mofaclaw":{"emoji":"📊","requires":{"bins":["gh","git","jq"]},"install":[{"id":"brew","kind":"brew","formula":"gh","bins":["gh"],"label":"Install GitHub CLI (brew)"}]}}
---

# Repository Report Skill

## When to use (trigger phrases)
Use this skill when the user asks for a data-driven repository health overview, for example:
- "repo report"
- "repository report"
- "repository stats"
- "project health"
- "repository analytics"
- "commit activity report"
- "PR metrics"
- "issue stats"
- "CI health"
- "code change statistics"

---
## Example User Requests

- "repo report for mofa-org/mofaclaw"
- "repository stats for mofa-org/mofaclaw last 30 days"
- "project health for mofa-org/mofaclaw last 90 days (include commit frequency + CI health)"
- "ci health for owner/repo last 7 days"
- "issue stats for owner/repo last 30 days (export as json)"

---
## Example Commands to Document

# Generate full report for last 30 days
gh api repos/owner/repo/stats/participation
gh pr list --state all --limit 100 --json number,state,createdAt,mergedAt
gh issue list --state all --limit 100 --json number,state,createdAt,closedAt
gh run list --limit 50 --json status,conclusion

# Commit statistics
git shortlog -sn --since="30 days ago"
git log --since="30 days ago" --pretty=format: --name-only | sort | uniq -c | sort -rg | head -20

## Execution Strategy

**Preferred approach:** run the bundled generator script directly via the `exec` tool (do not try to re-implement the report by manually running each section).

Command shapes (use literal values, not shell variables):
- Markdown (exact path): `./skills/repo-report/scripts/generate-report.sh --days <DAYS> --repo <owner/repo>`
- JSON (exact path): `./skills/repo-report/scripts/generate-report.sh --days <DAYS> --repo <owner/repo> --json`

Never use `repo-report.sh` or any other filename; the script name is exactly `generate-report.sh`.

**Step 0 — always run first** (auto-detect, no user prompting needed):
```bash
DAYS=30  # default if user didn't specify a time range
# if user says:
# - "last N days" => DAYS=N
# - "last N weeks" => DAYS=N*7
# - "last N months" => DAYS=N*30
# never claim the skill "doesn't support DAYS"; instead, parse the user's time range here.
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null)
SINCE=$(date -v-${DAYS}d +%Y-%m-%d 2>/dev/null || date -d "-${DAYS} days" +%Y-%m-%d)
echo "Repo: $REPO  |  Since: $SINCE"
```

Time-range rule (must-follow):
- If the user specifies a time window like "last 7 days" / "last 2 weeks" / "last 3 months", you MUST convert it into a numeric `DAYS` and pass it to the generator via `--days <DAYS>`.
- Never respond that the skill "doesn't support" time ranges. If the user didn’t specify one, use the default `DAYS=30`.

If `REPO` is empty (not in a git directory), then ask the user for the `owner/repo`. Otherwise proceed immediately — **do not ask** the user for the repo if auto-detection works.

**Step 1 — generate + paste the report**
If the user asks for:
- Markdown report: run `./skills/repo-report/scripts/generate-report.sh --days <DAYS> --repo <owner/repo>` and paste stdout verbatim.
- JSON report: run `./skills/repo-report/scripts/generate-report.sh --days <DAYS> --repo <owner/repo> --json` and paste stdout verbatim (valid JSON only).

Then paste `stdout` to the user **verbatim** (do not summarize or rewrite). This preserves section headings like `Daily Commit Frequency`, `Weekly Commit Frequency`, and `Monthly Commit Frequency`.

Note: the detailed section commands below are reference material only; prefer the generator output for actual report generation.

---

## 1. Commit Activity

Top contributors by commit count:
```bash
git shortlog -sn --since="$DAYS days ago"
```

Recent commit frequency (daily / weekly / monthly):
```bash
# daily (and total) commit activity
git log --since="$DAYS days ago" --oneline | wc -l
git log --since="$DAYS days ago" --format="%ad" --date=short | sort | uniq -c

# weekly (iso week) commit frequency
git log --since="$DAYS days ago" --format="%ad" --date=short --no-merges \
  | sort | uniq -c \
  | while read -r count date; do
      week=$(date -j -f "%Y-%m-%d" "$date" '+%G-W%V' 2>/dev/null || date -d "$date" '+%G-W%V')
      echo "$week $count"
    done \
  | awk '{c[$1]+=$2} END{for(k in c) print k "\t" c[k]}' | sort -k2,2gr

# monthly commit frequency
git log --since="$DAYS days ago" --format="%ad" --date=short --no-merges \
  | sort | uniq -c \
  | while read -r count date; do
      month=$(date -j -f "%Y-%m-%d" "$date" '+%Y-%m' 2>/dev/null || date -d "$date" '+%Y-%m')
      echo "$month $count"
    done \
  | awk '{c[$1]+=$2} END{for(k in c) print k "\t" c[k]}' | sort -k2,2gr
```

Run each block above separately in the `exec` tool, collect outputs, then format as a table.

Conventional commits compliance (counts non-compliant messages):
```bash
git log --since="$DAYS days ago" --format="%s" \
  | grep -Ecv "^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?: .+"
```

---

## 2. Pull Request Metrics

Fetch all PRs in the time range:
```bash
gh pr list --repo "$REPO" --state all --limit 200 \
  --json number,state,createdAt,mergedAt,additions,deletions \
  --jq "[.[] | select(.createdAt >= \"$SINCE\")]"
```

Open / merged / closed counts:
```bash
gh pr list --repo "$REPO" --state all --limit 200 \
  --json state,createdAt \
  --jq "[.[] | select(.createdAt >= \"$SINCE\")] | group_by(.state) | map({(.[0].state): length}) | add"
```

Average time to merge (hours):
```bash
gh pr list --repo "$REPO" --state merged --limit 200 \
  --json createdAt,mergedAt \
  --jq "[.[] | select(.createdAt >= \"$SINCE\") | select(.mergedAt != null)
         | (((.mergedAt | fromdateiso8601) - (.createdAt | fromdateiso8601)) / 3600)]
        | if length > 0 then (add / length | . * 10 | round / 10) else 0 end"
```

PR size distribution (small <100 lines, medium 100–500, large >500):
```bash
gh pr list --repo "$REPO" --state all --limit 200 \
  --json additions,deletions,createdAt \
  --jq "[.[] | select(.createdAt >= \"$SINCE\")
         | {size: (if (.additions + .deletions) < 100 then \"small\"
                   elif (.additions + .deletions) < 500 then \"medium\"
                   else \"large\" end)}]
        | group_by(.size) | map({(.[0].size): length}) | add"
```

---

## 3. Issue Statistics

Open / closed counts:
```bash
gh issue list --repo "$REPO" --state all --limit 200 \
  --json number,state,createdAt,closedAt,labels \
  --jq "[.[] | select(.createdAt >= \"$SINCE\")] | group_by(.state) | map({(.[0].state): length}) | add"
```

Average time to close (hours):
```bash
gh issue list --repo "$REPO" --state closed --limit 200 \
  --json createdAt,closedAt \
  --jq "[.[] | select(.createdAt >= \"$SINCE\") | select(.closedAt != null)
         | (((.closedAt | fromdateiso8601) - (.createdAt | fromdateiso8601)) / 3600)]
        | if length > 0 then (add / length | . * 10 | round / 10) else 0 end"
```

Issues by label:
```bash
gh issue list --repo "$REPO" --state all --limit 200 \
  --json labels,createdAt \
  --jq "[.[] | select(.createdAt >= \"$SINCE\") | .labels[].name]
        | group_by(.) | map({(.[0]): length}) | add // {}"
```

---

## 4. CI/CD Health

Run success/failure summary:
```bash
gh run list --repo "$REPO" --limit 100 \
  --json conclusion,createdAt,status \
  --jq "[.[] | select(.createdAt >= \"$SINCE\") | select(.status == \"completed\")]
        | {total: length,
           success: (map(select(.conclusion == \"success\")) | length),
           failure: (map(select(.conclusion == \"failure\")) | length),
           success_rate: (if length > 0 then ((map(select(.conclusion == \"success\")) | length) / length * 100 | . * 10 | round / 10) else 0 end)}"
```

Average build time (minutes):
```bash
gh run list --repo "$REPO" --limit 100 \
  --json createdAt,updatedAt,conclusion \
  --jq "[.[] | select(.createdAt >= \"$SINCE\") | select(.conclusion == \"success\")
         | (((.updatedAt | fromdateiso8601) - (.createdAt | fromdateiso8601)) / 60)]
        | if length > 0 then (add / length | . * 10 | round / 10) else 0 end"
```

Recent failures summary:
```bash
gh run list --repo "$REPO" --limit 20 \
  --json databaseId,name,conclusion,createdAt,headBranch \
  --jq "[.[] | select(.conclusion == \"failure\") | {id: .databaseId, name: .name, branch: .headBranch, at: .createdAt}]"
```

---

## 5. Code Change Statistics

Lines added / removed in time range:
```bash
git log --since="$DAYS days ago" --pretty=tformat: --numstat \
  | awk '{add += $1; del += $2} END {print "+" add " lines added, -" del " lines removed"}'
```

Most frequently changed files:
```bash
git log --since="$DAYS days ago" --pretty=format: --name-only \
  | sort | uniq -c | sort -rg | head -20
```

Most active directories:
```bash
git log --since="$DAYS days ago" --pretty=format: --name-only \
  | sed 's|/[^/]*$||' | sort | uniq -c | sort -rg | head -10
```

---

## Time Range Examples

| Range | Variable | `--since` value |
|-------|----------|-----------------|
| Last 7 days | `DAYS=7` | `"7 days ago"` |
| Last 30 days | `DAYS=30` | `"30 days ago"` |
| Last 90 days | `DAYS=90` | `"90 days ago"` |

## Export Options

After generating the report as Markdown in chat, save it to a file:
```bash
cat <<'EOF' > repo-report-$(date +%Y-%m-%d).md
<paste the rendered markdown here>
EOF
```

Save report as JSON (recommended; uses `jq` to embed the markdown):
```bash
jq -n \
  --arg repo "${REPO:-}" \
  --arg generated_at "$(date +"%Y-%m-%d %H:%M %Z")" \
  --rawfile markdown repo-report-$(date +%Y-%m-%d).md \
  '{format:"repo-report", repo:$repo, generated_at:$generated_at, report_markdown:$markdown}'
```

---
## Implementation Notes (Caching, Pagination, Participation)

Caching (optional):
- The generator uses a simple on-disk cache for `gh pr list`, `gh issue list`, and `gh run list` results to avoid repeated calls on large repos.
- Configure via:
  - `REPO_REPORT_CACHE_ENABLED=1|0` (default `1`)
  - `REPO_REPORT_CACHE_TTL_SECONDS` (default `600`)
  - `REPO_REPORT_CACHE_DIR` (default `~/.mofaclaw/cache/repo-report`)
- Disable cache per run with: `--no-cache`

Hard pagination limits:
- The generator requests up to `1000` PRs/issues/runs via `gh` and then filters by the computed `$DAYS` window.
- If a repo has more than that in the time window, results may still be incomplete (this is a limitation of API/CLI pagination caps).

Participation API:
- `gh api repos/owner/repo/stats/participation` is included as a reference command, but the daily/weekly/monthly commit frequency tables are computed from `git log` for better granularity.

## Reference Files

- **[references/commands.md](references/commands.md)** — Full flag reference for every `gh`/`git` command used here. Read when you need additional flags, pagination, or `jq` filter customization.
- **[references/report-template.md](references/report-template.md)** — Formatted Markdown report template. Read when asked to produce a structured, human-readable manager report.
