# Builtin Skills

This directory contains the builtin skills for mofaclaw. These skills are automatically available to all mofaclaw workspaces.

## Available Skills

- **weather** - Get current weather and forecasts using wttr.in (no API key required)
- **github** - Interact with GitHub using the `gh` CLI
- **summarize** - Summarize or extract text/transcripts from URLs, podcasts, and local files
- **tmux** - Remote-control tmux sessions for interactive CLIs
- **skill-creator** - Meta-skill for creating new skills

## Skill Structure

Each skill directory contains:
- `SKILL.md` - The main skill file with YAML frontmatter and markdown instructions
- Optional: `scripts/`, `references/`, `assets/` directories for bundled resources

## Workspace Override

Workspace-specific skills in `<workspace>/skills/` take precedence over builtin skills with the same name.
