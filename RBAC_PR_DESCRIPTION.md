### fix for [P0] feature: enhanced rbac permission control for skills and tools (#43)

### problem statement

As mofaclaw grew more powerful, the original permission model (guest / member / admin) was only wired into Discord role checks and could not:

- express **operation‑level permissions** for GitHub skills (e.g. `issue.create` vs `pr.merge` vs `repo.delete`)
- restrict **filesystem**, **shell**, and **web** tools in a fine‑grained way
- support **multi‑tenant** or enterprise setups with different boundaries per role
- provide a **config‑driven** policy that can be versioned and audited

This made it hard to safely run mofaclaw against production repos, servers, and file systems.

### solution overview

#### core rbac framework

- Introduced a **4‑level role model**: `Guest`, `Member`, `Admin`, `SuperAdmin` with ordering and helpers.
- Added `RbacConfig` and `RbacManager`:
  - JSON‑driven roles, role mappings, and permission rules.
  - Central `check_permission(role, resource, operation)` API for skills and tools.
  - Helpers for **filesystem path** and **shell command** checks (`check_path_access`, `check_command_access`).
- RBAC is **off by default**; when enabled, missing entries default to “allow with warning” to keep backward compatibility.

#### skills and tools integration

- GitHub skill operations now go through RBAC, matching the matrix from the issue:
  - `repo.view`, `issue.list`, `issue.create`, `issue.close`, `pr.create`, `pr.merge`,
    `repo.create`, `repo.delete`, `workflow.run`, `secret.set`.
- Tool isolation is enforced via RBAC:
  - **Filesystem** tools (`read_file`, `write_file`, `edit_file`, `list_dir`) honor per‑role
    **whitelists** and global **blacklists** with `${workspace}` and `${home}` expansion.
  - **Shell** tool (`exec`) supports safe command lists vs full access, plus a few hard‑blocked patterns.
  - **Web** tools (`web_search`, `web_fetch`) check dedicated `tools.web` permissions.

#### channel role mapping and config

- Discord:
  - Role and user‑override mapping implemented via `RbacManager::get_role_from_discord`.
  - All GitHub‑related slash commands now call RBAC instead of ad‑hoc `is_admin` / `is_member`.
- RBAC configuration is loaded via the main `Config` and validated at startup:
  - Standard, production, relaxed, and strict example configs in `examples/`.
  - Additional **use‑case configs** (open source, enterprise, multi‑tenant SaaS, CI/CD bot).

### files changed (high level)

- **core rbac module**: `core/src/rbac/{mod.rs,role.rs,config.rs,manager.rs,path_matcher.rs,audit.rs,tests.rs}`
- **core integration**:
  - `core/src/lib.rs`, `core/src/config.rs` (expose and load RBAC)
  - `core/src/tools/{filesystem.rs,shell.rs,web.rs}` (enforce RBAC on tools)
  - `core/src/channels/discord/mod.rs` (RBAC for GitHub commands and role resolution)
  - `core/src/channels/{dingtalk.rs,feishu.rs}` (logging cleanups and prep for RBAC)
- **cli and workflow**:
  - `cli/src/main.rs` (RBAC initialization and channel wiring)
  - `.github/workflows/rbac-tests.yml` (RBAC‑focused CI job)
- **examples**:
  - `examples/rbac_config*.json` (standard/production/relaxed/strict)
  - `examples/rbac_use_case_*.json` (open source, enterprise, SaaS, CI/CD)
  - `examples/rbac_usage_example.rs` (code‑level usage demo)

### testing

- **unit and integration tests** (35 total in `rbac::tests`):
  - Role ordering and hierarchy.
  - Skill and tool permission checks (including SuperAdmin bypass).
  - Filesystem path matching (variables, globs, whitelist/blacklist precedence).
  - Shell command whitelisting and pattern matching.
  - Discord role resolution including user overrides and “highest role wins”.
  - End‑to‑end workflows for GitHub, filesystem, and shell.
- **example tests**:
  - `examples/rbac_usage_example.rs` includes executable examples of permission checks.
- **ci workflow**:

```bash
cargo check --package mofaclaw-core
cargo test --package mofaclaw-core --lib rbac::tests
```

### usage examples

#### minimal config

```json
{
  "rbac": {
    "enabled": true,
    "default_role": "guest"
  }
}
```

#### production‑style config (excerpt)

```json
{
  "rbac": {
    "enabled": true,
    "default_role": "guest",
    "permissions": {
      "tools": {
        "filesystem": {
          "write": {
            "min_role": "member",
            "path_whitelist": {
              "member": ["${workspace}/**"]
            },
            "path_blacklist": [
              "**/.env*",
              "**/credentials*",
              "**/secrets/**"
            ]
          }
        }
      }
    }
  }
}
```

#### code‑level usage

```rust
let manager = RbacManager::new(rbac_config, workspace_path, home_path);
let result = manager.check_permission(Role::Member, "skills.github", "issue.create");
```

### future work and follow‑ups

These are intentionally left for future patches to keep this change focused:

- **Skill metadata integration**: attach permission hints directly in `SKILL.md` and wire them into RBAC.
- **Generic `skill.invoke` middleware**: central gate for readonly vs write skills regardless of channel.
- **Full DingTalk/Feishu role mapping at runtime**: have the Python bridges surface tags/roles so the existing RBAC mapping can be enforced per user.
- **Rate limiting and temporary elevation (P1)**: add per‑role/operation rate limits and short‑lived elevation flows on top of the current RBAC model.

