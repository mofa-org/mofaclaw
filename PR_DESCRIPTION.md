### add discord channel integration with slash commands and natural language support

### problem statement

users needed a way to interact with mofaclaw agents directly from discord, but the platform lacked discord channel support. this created a significant gap in accessibility, as discord is one of the most popular communication platforms for developer communities and teams. users were unable to:

1. interact with agents through discord's native slash command interface
2. use natural language queries without memorizing exact command syntax
3. access workspace files and agent information from discord
4. receive real-time feedback during agent processing
5. use commands in both direct messages and guild channels

this limitation prevented teams from integrating mofaclaw into their existing discord workflows and made the platform less accessible to users who primarily communicate through discord.

### solution overview

this implementation provides a complete discord channel integration with comprehensive slash command support and intelligent natural language processing. the solution enables seamless interaction with mofaclaw agents directly from discord with a focus on user experience and accessibility.

#### discord slash commands

**core commands:**
- `/summarize` - summarize urls or files with customizable length options
- `/weather` - get weather information for any location with multiple format options
- `/tmux` - manage tmux sessions (create, list, attach, send commands, capture output)
- `/skill` - create, update, list, and view agent skills (admin-only for modifications)
- `/help` - comprehensive help system with categorized command listings
- `/workspace view` - view workspace files (soul, user, agents, tools, heartbeat)
- `/workspace heartbeat list` - list all heartbeat tasks
- `/workspace memory view` - view agent memory contents

**github integration commands:**
- `/issue` - complete issue management:
  - `/issue create <title> [body]` - create new issue (member)
  - `/issue list [label]` - list all issues with optional label filter (guest)
  - `/issue view <number>` - view issue details (guest)
  - `/issue close <number>` - close issue (member)
  - `/issue comment <number> <body>` - comment on issue (member)
  - `/issue assign <number> <user>` - assign issue to user (admin)
- `/pr` - complete pr management:
  - `/pr create <title> [base] [head]` - create new pr (member)
  - `/pr list [state]` - list prs with optional state filter (guest)
  - `/pr view <number>` - view pr details (guest)
  - `/pr merge <number>` - merge pr with ci status check (admin only)
  - `/pr comment <number> <body>` - comment on pr (member)
  - `/pr review <number> <approve|reject>` - code review (member)
  - `/pr close <number>` - close pr (member)
- `/status` - check ci/cd status for workflows
- `/release` - manage releases (create, view, list)

**user experience features:**
- ephemeral responses for help commands (private to user)
- typing indicators during agent processing for visual feedback
- automatic command registration in both dms and guilds
- graceful error handling with user-friendly messages
- formatted embed messages for github operations (issues and prs)
- granular permission system (guest/member/admin roles)

#### natural language processing

**keyword matching:**
- intelligent pattern recognition for common commands
- workspace command detection (e.g., "show me workspace soul", "view agents file")
- github command shortcuts (e.g., "create issue", "list prs", "close issue #123")
- weather query recognition (e.g., "weather in new york", "what's the weather")
- direct command execution without requiring exact slash command syntax

**intent recognition:**
- fallback to llm-based intent recognition for complex queries
- hybrid approach: fast keyword matching for common patterns, llm for nuanced requests
- seamless integration with existing agent message bus

**technical implementation:**
- efficient keyword matching with case-insensitive pattern detection
- number extraction for issue/pr references
- context-aware command routing
- automatic parameter extraction from natural language

### files changed

#### core implementation
- `core/src/channels/discord/mod.rs`: complete discord channel implementation (1712 lines)
  - full channel trait implementation
  - message handler for receiving messages
  - outbound message handler for sending replies
  - slash command handlers for all github operations
  - keyword matching for natural language processing
  - permission system with role-based access control
- `core/src/channels/manager.rs`: discord channel registration
- `core/src/channels/mod.rs`: channel module exports
- `core/src/config.rs`: discord configuration structure with member_roles support
- `cli/src/main.rs`: discord channel initialization in cli

#### workspace enhancements
- `workspace/AGENTS.md`: improved command execution instructions
- `workspace/TOOLS.md`: enhanced tool usage examples and context

#### configuration
- `Cargo.toml`: discord dependencies (serenity, poise)
- `core/Cargo.toml`: core discord dependencies
- `.gitignore`: rust build artifacts and temporary files

#### bug fixes
- `core/src/channels/telegram.rs`: fixed markdown formatting issues

### testing

#### manual testing
all commands tested manually with:
- real discord bot in development server
- direct message interactions
- guild channel interactions
- natural language queries
- slash command variations
- error scenarios (missing files, invalid inputs)
- typing indicator verification
- ephemeral message behavior

#### integration testing
- full message bus integration verified
- agent response routing confirmed
- command registration in both dms and guilds
- keyword matching accuracy validated
- llm fallback behavior tested

### usage examples

#### slash commands

```bash
# summarize a url
/summarize url:https://example.com/article length:medium

# get weather information
/weather location:New York format:compact

# manage tmux sessions
/tmux create session:my-session
/tmux list
/tmux attach session:my-session

# view workspace files
/workspace view file:soul
/workspace view file:agents

# get help
/help
/help weather
```

#### natural language queries

```bash
# workspace commands
"show me the workspace soul file"
"view the agents workspace"
"what's in the heartbeat list?"

# github commands
"create issue with title: bug in login"
"list all issues with label: bug"
"close issue #42"
"comment on issue #10 with body: fixed in pr #15"
"assign issue #5 to @user"
"show me pr #10"
"merge pr #20"
"review pr #15 and approve"
"list prs with state: open"

# weather queries
"weather in london"
"what's the weather in tokyo?"
```

### benefits

1. **accessibility**: discord integration makes mofaclaw accessible to teams already using discord for communication, reducing friction in adoption.

2. **user experience**: typing indicators and ephemeral messages provide clear feedback and maintain privacy for help commands.

3. **flexibility**: natural language processing allows users to interact without memorizing exact command syntax, making the platform more approachable.

4. **productivity**: direct access to workspace files and agent information from discord eliminates context switching between tools.

5. **developer-friendly**: comprehensive slash command interface follows discord conventions, making it intuitive for developers familiar with discord bots.

6. **scalability**: hybrid keyword matching + llm approach provides fast responses for common queries while maintaining flexibility for complex requests.

7. **privacy**: ephemeral help messages ensure command documentation is only visible to the requesting user.

### migration notes

no breaking changes. this is a new channel integration that works alongside existing channels (telegram, dingtalk, feishu). users can enable discord by adding configuration to `~/.mofaclaw/config.json`:

```json
{
  "channels": {
    "discord": {
      "enabled": true,
      "token": "YOUR_BOT_TOKEN",
      "application_id": 1234567890,
      "guild_id": 9876543210
    }
  }
}
```

### related issues

this implementation covers the following github issues:

- **[#19] discord channel implementation**: complete discord channel integration using serenity/poise framework with message receiving and sending capabilities, supporting both dm and guild channels
- **[#20] discord slash commands registration**: all commands registered globally and in guilds with proper parameter descriptions for auto-completion, including issue, pr, status, release, and help commands
- **[#22] github issue management**: full issue management with create, list, view, close, comment, and assign operations, including granular permissions (guest/member/admin) and formatted embed responses
- **[#23] github pr management**: complete pr management with create, list, view, merge, comment, review, and close operations, including ci status checks before merge, state filtering, and embed formatting

**additional features:**
- natural language processing for improved ux
- workspace access from discord
- keyword matching for common commands
- comprehensive permission system

### code quality

- all code follows rust idioms and project conventions
- `cargo fmt` applied
- `cargo clippy` passes with no warnings
- comprehensive error handling with proper error types
- async throughout using `tokio`
- proper resource cleanup and graceful shutdown
- follows serenity and poise best practices
