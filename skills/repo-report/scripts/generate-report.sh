#!/usr/bin/env bash
# generate-report.sh — One-shot repository analytics report
# Usage: bash generate-report.sh [--days N] [--repo owner/repo] [--json]
# Outputs a full Markdown report to stdout (or JSON when --json is set).
set -euo pipefail

# ── Defaults ─────────────────────────────────────────────────────────────────
DAYS=30
REPO=""
OUTPUT_FORMAT="markdown"
TMPFILE=""
REPO_REPORT_CACHE_ENABLED="1"
REPO_REPORT_CACHE_TTL_SECONDS="${REPO_REPORT_CACHE_TTL_SECONDS:-600}"
REPO_REPORT_CACHE_DIR="${REPO_REPORT_CACHE_DIR:-$HOME/.mofaclaw/cache/repo-report}"

# ── Argument parsing ──────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --days)  DAYS="$2";  shift 2 ;;
    --repo)  REPO="$2";  shift 2 ;;
    --json)  OUTPUT_FORMAT="json"; shift 1 ;;
    --no-cache) REPO_REPORT_CACHE_ENABLED="0"; shift 1 ;;
    *)       echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

# ── Dependency check ──────────────────────────────────────────────────────────
for bin in gh git jq; do
  if ! command -v "$bin" &>/dev/null; then
    echo "Error: '$bin' not found. Install it and try again." >&2
    exit 1
  fi
done

# ── Auto-detect repo ──────────────────────────────────────────────────────────
if [[ -z "$REPO" ]]; then
  REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)
  if [[ -z "$REPO" ]]; then
    echo "Error: Could not detect repo. Run inside a git repo or pass --repo owner/repo" >&2
    exit 1
  fi
fi

# ── Date helpers (macOS + Linux) ──────────────────────────────────────────────
if date -v-1d +%Y &>/dev/null 2>&1; then
  # macOS (BSD date)
  SINCE_ISO=$(date -v-${DAYS}d +%Y-%m-%dT%H:%M:%SZ)
  SINCE_DATE=$(date -v-${DAYS}d +%Y-%m-%d)
  TODAY=$(date +%Y-%m-%d)
else
  # Linux (GNU date)
  SINCE_ISO=$(date -d "-${DAYS} days" +%Y-%m-%dT%H:%M:%SZ)
  SINCE_DATE=$(date -d "-${DAYS} days" +%Y-%m-%d)
  TODAY=$(date +%Y-%m-%d)
fi
GENERATED_AT=$(date +"%Y-%m-%d %H:%M %Z")

# ── JSON mode: capture markdown output then wrap it as JSON ────────────────
# In --json mode we still generate the markdown report, but redirect stdout
# so the final output is a single JSON object.
exec 3>&1
if [[ "$OUTPUT_FORMAT" == "json" ]]; then
  TMPFILE="$(mktemp /tmp/repo-report.XXXXXX.md)"
  exec 1>"$TMPFILE"
fi

# ── Helper: print a section header ────────────────────────────────────────────
section() { echo -e "\n---\n\n## $1\n"; }

# ─────────────────────────────────────────────────────────────────────────────
echo "# 📊 Repository Health Report: $REPO"
echo ""
echo "**Period:** Last ${DAYS} days — ${SINCE_DATE} to ${TODAY}"
echo "**Generated:** ${GENERATED_AT}"

# ── Cache config (optional) ────────────────────────────────────────────────
TTL_MIN=$(( (REPO_REPORT_CACHE_TTL_SECONDS + 59) / 60 ))
cache_repo="${REPO//\//_}"
mkdir -p "$REPO_REPORT_CACHE_DIR"
PR_CACHE_FILE="$REPO_REPORT_CACHE_DIR/pr-${cache_repo}-${DAYS}-${SINCE_DATE}.json"
ISSUE_CACHE_FILE="$REPO_REPORT_CACHE_DIR/issue-${cache_repo}-${DAYS}-${SINCE_DATE}.json"
RUN_CACHE_FILE="$REPO_REPORT_CACHE_DIR/run-${cache_repo}-${DAYS}-${SINCE_DATE}.json"

# ─────────────────────────────────────────────────────────────────────────────
section "1. Commit Activity"

TOTAL_COMMITS=$(git log --since="$DAYS days ago" --oneline --no-merges | wc -l | tr -d ' ')
echo "**Total commits (no merges):** $TOTAL_COMMITS over $DAYS days"
echo ""
echo "### Top Contributors"
echo ""
echo "| Commits | Author |"
echo "|---------|--------|"
git shortlog -sn --since="$DAYS days ago" --no-merges HEAD | head -10 \
  | while read -r count name; do echo "| $count | $name |"; done
echo ""

echo "### Daily Commit Frequency"
echo ""
echo "| Date | Commits |"
echo "|------|---------|"
git log --since="$DAYS days ago" --format="%ad" --date=short --no-merges \
  | sort | uniq -c \
  | while read -r count date; do echo "| $date | $count |"; done
echo ""

# Weekly + monthly frequency (required for manager-friendly health snapshots)
declare -i IS_MAC=0
if date -v-1d +%Y &>/dev/null 2>&1; then
  IS_MAC=1
fi

week_key_from_date() {
  if [[ "$IS_MAC" == "1" ]]; then
    # iso week-year + week number (e.g. 2026-W11)
    date -j -f "%Y-%m-%d" "$1" '+%G-W%V'
  else
    date -d "$1" '+%G-W%V'
  fi
}

month_key_from_date() {
  if [[ "$IS_MAC" == "1" ]]; then
    date -j -f "%Y-%m-%d" "$1" '+%Y-%m'
  else
    date -d "$1" '+%Y-%m'
  fi
}

echo "### Weekly Commit Frequency"
echo ""
echo "| Week | Commits |"
echo "|------|---------|"
git log --since="$DAYS days ago" --format="%ad" --date=short --no-merges \
  | sort | uniq -c \
  | while read -r count date; do
      wk="$(week_key_from_date "$date")"
      echo "$wk $count"
    done \
  | awk '{c[$1]+=$2} END{for(k in c) print k "\t" c[k]}' \
  | sort -k2,2gr \
  | while IFS=$'\t' read -r wk cnt; do echo "| $wk | $cnt |"; done
echo ""

echo "### Monthly Commit Frequency"
echo ""
echo "| Month | Commits |"
echo "|-------|---------|"
git log --since="$DAYS days ago" --format="%ad" --date=short --no-merges \
  | sort | uniq -c \
  | while read -r count date; do
      mo="$(month_key_from_date "$date")"
      echo "$mo $count"
    done \
  | awk '{c[$1]+=$2} END{for(k in c) print k "\t" c[k]}' \
  | sort -k2,2gr \
  | while IFS=$'\t' read -r mo cnt; do echo "| $mo | $cnt |"; done
echo ""

# Conventional commits check
TOTAL_MSG=$(git log --since="$DAYS days ago" --format="%s" --no-merges | wc -l | tr -d ' ')
if [[ "$TOTAL_MSG" -gt 0 ]]; then
  CC_NONCOMPLIANT=$(git log --since="$DAYS days ago" --format="%s" --no-merges \
    | grep -Ecv "^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\(.+\))?: .+" || true)
  CC_COMPLIANT=$(( TOTAL_MSG - CC_NONCOMPLIANT ))
  CC_RATE=$(( CC_COMPLIANT * 100 / TOTAL_MSG ))
  echo "### Conventional Commits Compliance"
  echo ""
  echo "| Metric | Value |"
  echo "|--------|-------|"
  echo "| Compliant | $CC_COMPLIANT |"
  echo "| Non-compliant | $CC_NONCOMPLIANT |"
  echo "| Compliance rate | ${CC_RATE}% |"
  echo ""
fi

# ─────────────────────────────────────────────────────────────────────────────
section "2. Pull Request Metrics"

if [[ "$REPO_REPORT_CACHE_ENABLED" == "1" ]] && [[ -f "$PR_CACHE_FILE" ]] && [[ "$TTL_MIN" -gt 0 ]] && find "$PR_CACHE_FILE" -mmin -"${TTL_MIN}" | grep -q .; then
  PR_JSON="$(cat "$PR_CACHE_FILE")"
else
  PR_JSON=$(gh pr list --repo "$REPO" --state all --limit 1000 \
    --json number,state,createdAt,mergedAt,additions,deletions 2>/dev/null || echo "[]")
  if [[ "$REPO_REPORT_CACHE_ENABLED" == "1" ]]; then
    echo "$PR_JSON" > "$PR_CACHE_FILE"
  fi
fi

PR_STATS=$(echo "$PR_JSON" | jq --arg since "$SINCE_ISO" '
  [.[] | select(.createdAt >= $since)] as $prs |
  {
    opened:  ($prs | length),
    merged:  ($prs | map(select(.state == "MERGED")) | length),
    closed:  ($prs | map(select(.state == "CLOSED")) | length),
    open:    ($prs | map(select(.state == "OPEN"))   | length),
    avg_merge_hours: (
      [$prs[] | select(.mergedAt != null)
        | ((.mergedAt|fromdateiso8601) - (.createdAt|fromdateiso8601)) / 3600]
      | if length > 0 then (add/length * 10 | round / 10) else 0 end
    ),
    small:   ($prs | map(select((.additions + .deletions) < 100))   | length),
    medium:  ($prs | map(select((.additions + .deletions) >= 100 and (.additions + .deletions) < 500)) | length),
    large:   ($prs | map(select((.additions + .deletions) >= 500))  | length)
  }
')

echo "| Status | Count |"
echo "|--------|-------|"
echo "| Opened | $(echo "$PR_STATS" | jq -r .opened) |"
echo "| Merged | $(echo "$PR_STATS" | jq -r .merged) |"
echo "| Closed (unmerged) | $(echo "$PR_STATS" | jq -r .closed) |"
echo "| Still open | $(echo "$PR_STATS" | jq -r .open) |"
echo ""
AVG_MERGE=$(echo "$PR_STATS" | jq -r .avg_merge_hours)
AVG_MERGE_DAYS=$(echo "$AVG_MERGE" | awk '{printf "%.1f", $1/24}')
echo "**Average time to merge:** ${AVG_MERGE} hours (~${AVG_MERGE_DAYS} days)"
echo ""
echo "### PR Size Distribution"
echo ""
echo "| Size | Lines Changed | Count |"
echo "|------|--------------|-------|"
echo "| Small  | < 100   | $(echo "$PR_STATS" | jq -r .small) |"
echo "| Medium | 100–500 | $(echo "$PR_STATS" | jq -r .medium) |"
echo "| Large  | > 500   | $(echo "$PR_STATS" | jq -r .large) |"
echo ""

# ─────────────────────────────────────────────────────────────────────────────
section "3. Issue Statistics"

if [[ "$REPO_REPORT_CACHE_ENABLED" == "1" ]] && [[ -f "$ISSUE_CACHE_FILE" ]] && [[ "$TTL_MIN" -gt 0 ]] && find "$ISSUE_CACHE_FILE" -mmin -"${TTL_MIN}" | grep -q .; then
  ISSUE_JSON="$(cat "$ISSUE_CACHE_FILE")"
else
  ISSUE_JSON=$(gh issue list --repo "$REPO" --state all --limit 1000 \
    --json number,state,createdAt,closedAt,labels 2>/dev/null || echo "[]")
  if [[ "$REPO_REPORT_CACHE_ENABLED" == "1" ]]; then
    echo "$ISSUE_JSON" > "$ISSUE_CACHE_FILE"
  fi
fi

ISSUE_STATS=$(echo "$ISSUE_JSON" | jq --arg since "$SINCE_ISO" '
  [.[] | select(.createdAt >= $since)] as $issues |
  {
    opened: ($issues | length),
    closed: ($issues | map(select(.state == "CLOSED")) | length),
    open:   ($issues | map(select(.state == "OPEN"))   | length),
    avg_close_hours: (
      [$issues[] | select(.closedAt != null)
        | ((.closedAt|fromdateiso8601) - (.createdAt|fromdateiso8601)) / 3600]
      | if length > 0 then (add/length * 10 | round / 10) else 0 end
    )
  }
')

echo "| Status | Count |"
echo "|--------|-------|"
echo "| Opened  | $(echo "$ISSUE_STATS" | jq -r .opened) |"
echo "| Closed  | $(echo "$ISSUE_STATS" | jq -r .closed) |"
echo "| Still open | $(echo "$ISSUE_STATS" | jq -r .open) |"
echo ""
AVG_CLOSE=$(echo "$ISSUE_STATS" | jq -r .avg_close_hours)
AVG_CLOSE_DAYS=$(echo "$AVG_CLOSE" | awk '{printf "%.1f", $1/24}')
echo "**Average time to close:** ${AVG_CLOSE} hours (~${AVG_CLOSE_DAYS} days)"
echo ""

echo "### Issues by Label"
echo ""
echo "| Label | Count |"
echo "|-------|-------|"
echo "$ISSUE_JSON" | jq --arg since "$SINCE_ISO" -r '
  [.[] | select(.createdAt >= $since) | .labels[].name]
  | group_by(.) | sort_by(-length) | .[]
  | "\(.[0]) | \(length)"
' | while IFS='|' read -r label count; do
    echo "| $(echo "$label" | xargs) | $(echo "$count" | xargs) |"
  done
echo ""

# ─────────────────────────────────────────────────────────────────────────────
section "4. CI/CD Health"

if [[ "$REPO_REPORT_CACHE_ENABLED" == "1" ]] && [[ -f "$RUN_CACHE_FILE" ]] && [[ "$TTL_MIN" -gt 0 ]] && find "$RUN_CACHE_FILE" -mmin -"${TTL_MIN}" | grep -q .; then
  RUN_JSON="$(cat "$RUN_CACHE_FILE")"
else
  RUN_JSON=$(gh run list --repo "$REPO" --limit 1000 \
    --json status,conclusion,createdAt,updatedAt,name,headBranch,databaseId 2>/dev/null || echo "[]")
  if [[ "$REPO_REPORT_CACHE_ENABLED" == "1" ]]; then
    echo "$RUN_JSON" > "$RUN_CACHE_FILE"
  fi
fi

CI_STATS=$(echo "$RUN_JSON" | jq --arg since "$SINCE_ISO" '
  [.[] | select(.createdAt >= $since and .status == "completed")] as $runs |
  {
    total:     ($runs | length),
    success:   ($runs | map(select(.conclusion == "success"))   | length),
    failure:   ($runs | map(select(.conclusion == "failure"))   | length),
    cancelled: ($runs | map(select(.conclusion == "cancelled")) | length),
    success_rate: (
      if ($runs | length) > 0
      then (($runs | map(select(.conclusion == "success")) | length)
            / ($runs | length) * 100 * 10 | round / 10)
      else 0 end
    ),
    avg_build_min: (
      [$runs[] | select(.conclusion == "success")
        | ((.updatedAt|fromdateiso8601) - (.createdAt|fromdateiso8601)) / 60]
      | if length > 0 then (add/length * 10 | round / 10) else 0 end
    )
  }
')

echo "| Metric | Value |"
echo "|--------|-------|"
echo "| Total runs | $(echo "$CI_STATS" | jq -r .total) |"
echo "| Successful | $(echo "$CI_STATS" | jq -r .success) |"
echo "| Failed | $(echo "$CI_STATS" | jq -r .failure) |"
echo "| Cancelled | $(echo "$CI_STATS" | jq -r .cancelled) |"
echo "| Success rate | $(echo "$CI_STATS" | jq -r .success_rate)% |"
echo "| Avg build time | $(echo "$CI_STATS" | jq -r .avg_build_min) min |"
echo ""

FAILURES=$(echo "$RUN_JSON" | jq --arg since "$SINCE_ISO" -r '
  [.[] | select(.createdAt >= $since and .conclusion == "failure")]
  | sort_by(.createdAt) | reverse | .[:5][]
  | "| \(.databaseId) | \(.name) | \(.headBranch) | \(.createdAt[:10]) |"
')
if [[ -n "$FAILURES" ]]; then
  echo "### Recent Failures (last 5)"
  echo ""
  echo "| Run ID | Workflow | Branch | Date |"
  echo "|--------|----------|--------|------|"
  echo "$FAILURES"
  echo ""
fi

# ─────────────────────────────────────────────────────────────────────────────
section "5. Code Change Statistics"

NUMSTAT=$(git log --since="$DAYS days ago" --pretty=tformat: --numstat --no-merges 2>/dev/null || true)
if [[ -n "$NUMSTAT" ]]; then
  LINES_ADDED=$(echo "$NUMSTAT" | awk 'NF==3 {add += $1} END {print add+0}')
  LINES_REMOVED=$(echo "$NUMSTAT" | awk 'NF==3 {del += $2} END {print del+0}')
  echo "**Total changes:** +${LINES_ADDED} lines added, -${LINES_REMOVED} lines removed"
  echo ""
fi

echo "### Most Frequently Changed Files (Top 15)"
echo ""
echo "| Changes | File |"
echo "|---------|------|"
git log --since="$DAYS days ago" --pretty=format: --name-only --no-merges \
  | grep -v '^$' | sort | uniq -c | sort -rg | head -15 \
  | while read -r count file; do echo "| $count | \`$file\` |"; done
echo ""

echo "### Most Active Directories (Top 10)"
echo ""
echo "| File changes | Directory |"
echo "|--------------|-----------|"
git log --since="$DAYS days ago" --pretty=format: --name-only --no-merges \
  | grep -v '^$' | sed 's|/[^/]*$||' | sort | uniq -c | sort -rg | head -10 \
  | while read -r count dir; do echo "| $count | \`$dir\` |"; done
echo ""

# ─────────────────────────────────────────────────────────────────────────────
echo "---"
echo ""
echo "*Report generated by [mofaclaw](https://github.com/mofa-org/mofaclaw) \`repo-report\` skill.*"

if [[ "$OUTPUT_FORMAT" == "json" ]]; then
  exec 1>&3
  jq -n \
    --arg repo "$REPO" \
    --argjson days "$DAYS" \
    --arg generated_at "$GENERATED_AT" \
    --arg since_date "$SINCE_DATE" \
    --arg today "$TODAY" \
    --rawfile markdown "$TMPFILE" \
    '{
      format: "repo-report",
      repo: $repo,
      days: $days,
      period: { since: $since_date, until: $today },
      generated_at: $generated_at,
      report_markdown: $markdown
    }'
  rm -f "$TMPFILE"
fi
