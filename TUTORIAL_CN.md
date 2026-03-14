# mofaclaw 教程：贡献者入门指南

> **声明**：本教程主要由 [Claude Code](https://claude.ai/claude-code) 生成，待 MoFAClaw 架构师 [@lijingrs](https://github.com/lijingrs) 审阅。内容可能随项目发展而更新。

欢迎！本教程面向 **GSoC 学生**及新贡献者，帮助你深入理解 mofaclaw 的架构并学习如何扩展系统。学完本教程后，你将全面了解系统的工作原理，并掌握如何构建新的技能（Skill）、工具（Tool）和频道（Channel）。

**目录**

- [前置条件](#前置条件)
- [构建与运行](#构建与运行)
- [架构概览](#架构概览)
  - [消息流转](#消息流转)
  - [核心模块](#核心模块)
  - [深入了解：MessageBus](#深入了解messagebus)
  - [深入了解：AgentLoop](#深入了解agentloop)
  - [深入了解：ContextBuilder 与 Skills](#深入了解contextbuilder-与-skills)
  - [深入了解：会话管理](#深入了解会话管理)
  - [深入了解：配置系统](#深入了解配置系统)
  - [深入了解：错误处理](#深入了解错误处理)
- [工作区](#工作区)
- [实战：添加一个 Skill](#实战添加一个-skill)
- [实战：添加一个 Tool](#实战添加一个-tool)
- [实战：添加一个 Channel](#实战添加一个-channel)
- [网关：系统的启动流程](#网关系统的启动流程)
- [心跳与定时任务系统](#心跳与定时任务系统)
- [开发工作流](#开发工作流)
- [GSoC 项目创意](#gsoc-项目创意)
- [贡献指南](#贡献指南)

---

## 前置条件

开始之前，请确保你已安装：

- **Rust 1.85+** — [安装 Rust](https://www.rust-lang.org/tools/install)（中国用户推荐使用 [rsproxy.cn](https://rsproxy.cn/) 镜像加速）
- **Git** — 用于克隆仓库
- **API 密钥** — 推荐使用 [OpenRouter](https://openrouter.ai/keys)（一个密钥即可访问多种模型）
- 代码编辑器（推荐 VS Code + rust-analyzer 插件）

验证环境：

```bash
rustc --version   # 应为 1.85.0 或更高版本
cargo --version
git --version
```

## 构建与运行

### 1. 克隆并构建

```bash
git clone https://github.com/mofaclaw/mofaclaw.git
cd mofaclaw
cargo build --release
```

### 2. 初始化

```bash
cargo run --release -- onboard
```

此命令会创建 `~/.mofaclaw/config.json` 和工作区 `~/.mofaclaw/workspace/`，包含模板文件（`AGENTS.md`、`SOUL.md`、`USER.md`）、长期记忆文件 `memory/MEMORY.md`，并将内置技能复制到工作区。

### 3. 配置 API 密钥

编辑 `~/.mofaclaw/config.json`：

```json
{
  "providers": {
    "openrouter": {
      "apiKey": "sk-or-v1-你的密钥"
    }
  },
  "agents": {
    "defaults": {
      "model": "anthropic/claude-opus-4-5"
    }
  }
}
```

### 4. 开始对话

```bash
# 单条消息模式
cargo run --release -- agent -m "你好！你能做什么？"

# 交互模式
cargo run --release -- agent

# 指定会话名称（对话历史会持久化）
cargo run --release -- agent -s my-project
```

### 5. CLI 命令概览

| 命令 | 说明 |
|------|------|
| `mofaclaw onboard` | 初始化配置和工作区 |
| `mofaclaw agent -m "..."` | 单条消息对话 |
| `mofaclaw agent` | 交互式对话（标准输入） |
| `mofaclaw agent -s KEY` | 使用指定会话名称对话 |
| `mofaclaw gateway` | 启动完整网关（频道 + 代理 + 心跳） |
| `mofaclaw gateway --port 9000` | 指定端口启动网关 |
| `mofaclaw status` | 显示配置、工作区、模型和 API 密钥状态 |
| `mofaclaw session list` | 列出所有对话会话 |
| `mofaclaw session show KEY` | 显示会话内容 |
| `mofaclaw session delete KEY` | 删除会话 |
| `mofaclaw cron add --name "daily" --message "Hello" --cron "0 9 * * *"` | 添加定时任务 |
| `mofaclaw cron list` | 列出定时任务 |
| `mofaclaw channels status` | 显示已启用的频道和桥接 URL |

---

## 架构概览

### 消息流转

以下是用户消息在整个系统中的流转过程：

```
用户输入
    │
    ▼
┌──────────────┐     ┌────────────┐     ┌──────────────────────────────────┐
│  CLI / 聊天  │────▶│ MessageBus │────▶│           AgentLoop              │
│  频道        │     │ (tokio     │     │                                  │
│ (钉钉,      │     │  broadcast)│     │  1. SessionManager               │
│  Telegram,   │     └────────────┘     │     └─ 加载/创建会话             │
│  飞书,       │                        │     └─ 获取对话历史              │
│  WhatsApp)   │                        │                                  │
└──────────────┘                        │  2. ContextBuilder               │
       ▲                                │     └─ 引导文件                  │
       │                                │     └─ 技能元数据                │
       │                                │     └─ 记忆（通过 mofa-sdk）     │
       │                                │                                  │
       │                                │  3. LLM Provider (mofa-sdk)      │
       │                                │     └─ 发送提示词 + 历史         │
       │                                │     └─ 接收响应                  │
       │                                │                                  │
       │                                │  4. 工具执行循环                 │
       │                                │     └─ ToolRegistry 分发         │
       │                                │     └─ 结果反馈给 LLM           │
       │                                │     └─ 重复直到完成              │
       │                                │                                  │
       │                                │  5. 保存会话                     │
       │     ┌────────────┐             │  6. 发布 OutboundMessage         │
       └─────│ MessageBus │◀────────────┤                                  │
             │ (出站)     │             └──────────────────────────────────┘
             └────────────┘
```

### 核心模块

| 模块 | 路径 | 用途 |
|------|------|------|
| **Agent** | `core/src/agent/` | 代理循环、上下文构建、子代理管理 |
| **Tools** | `core/src/tools/` | 内置工具（文件系统、Shell、网页、子代理、消息） |
| **Channels** | `core/src/channels/` | 聊天平台集成（钉钉、Telegram、飞书、WhatsApp） |
| **Bus** | `core/src/bus/` | 基于 tokio broadcast 的异步消息路由 |
| **Session** | `core/src/session/` | 对话持久化（磁盘上的 JSONL 文件） |
| **Config** | `core/src/config.rs` | 基于 JSON 的配置（提供商、频道、工具） |
| **Provider** | `core/src/provider/` | 重新导出 mofa-sdk 的 LLM 提供商（兼容 OpenAI） |
| **Skills** | `skills/` | 扩展代理能力的内置技能（基于 Markdown） |
| **Heartbeat** | `core/src/heartbeat/` | 主动式周期性任务运行器（每 30 分钟） |
| **Cron** | `core/src/cron/` | 支持 cron 表达式的定时任务系统 |
| **Messages** | `core/src/messages.rs` | `InboundMessage` 和 `OutboundMessage` 类型 |
| **Types** | `core/src/types.rs` | 核心类型：`Message`、`MessageContent`、`MessageRole` |
| **Error** | `core/src/error.rs` | 完整的错误层次结构 |
| **CLI** | `cli/src/main.rs` | 命令行界面和网关启动 |

### 深入了解：MessageBus

`MessageBus`（`core/src/bus/`）是系统的中枢神经——它使用 tokio 的异步 broadcast 通道将频道与代理解耦。

```rust
pub struct MessageBus {
    inbound: broadcast::Sender<InboundMessage>,    // 频道 → 代理
    outbound: broadcast::Sender<OutboundMessage>,  // 代理 → 频道
    outbound_subscribers: Arc<RwLock<HashMap<String, Vec<OutboundCallback>>>>,
}
```

**核心 API：**

```rust
// 频道发布传入的用户消息
bus.publish_inbound(msg: InboundMessage).await;

// AgentLoop 订阅用户消息
let mut rx = bus.subscribe_inbound();

// AgentLoop 发布响应
bus.publish_outbound(msg: OutboundMessage).await;

// 频道订阅响应
let mut rx = bus.subscribe_outbound();

// 或注册特定频道的回调
bus.subscribe_outbound_channel("telegram", callback).await;
```

**消息类型**（`core/src/messages.rs`）：

```rust
pub struct InboundMessage {
    pub channel: String,       // "cli"、"telegram"、"dingtalk" 等
    pub sender_id: String,     // 发送者标识
    pub chat_id: String,       // 对话标识
    pub content: String,       // 实际消息文本
    pub timestamp: DateTime<Utc>,
    pub media: Vec<String>,    // 附带的图片/文件路径
    pub metadata: HashMap<String, Value>,
}

pub struct OutboundMessage {
    pub channel: String,       // 目标频道
    pub chat_id: String,       // 目标对话
    pub content: String,       // 响应文本
    pub reply_to: Option<String>,
    pub media: Vec<String>,
    pub metadata: HashMap<String, Value>,
}
```

会话键的格式为 `"{channel}:{chat_id}"` — 这是对话分组和持久化的依据。

### 深入了解：AgentLoop

`AgentLoop`（`core/src/agent/loop_.rs`）是核心处理引擎，将所有组件串联在一起：

```rust
pub struct AgentLoop {
    _agent: Arc<LLMAgent>,                    // mofa-sdk LLM 代理
    provider: Arc<dyn MofaLLMProvider>,       // LLM 提供商（OpenRouter 等）
    tools: Arc<RwLock<ToolRegistry>>,         // 已注册的工具
    bus: MessageBus,                          // 消息路由
    sessions: Arc<SessionManager>,            // 对话持久化
    context: ContextBuilder,                  // 系统提示词组装
    running: Arc<RwLock<bool>>,               // 运行状态
    task_orchestrator: Arc<TaskOrchestrator>,  // 子代理生成（mofa-sdk）
    max_iterations: usize,                    // 最大工具调用循环次数（默认：20）
    default_model: String,                    // 如 "anthropic/claude-opus-4-5"
    temperature: Option<f32>,                 // 默认：0.7
    max_tokens: Option<u32>,                  // 默认：8192
}
```

**单条消息的处理流程：**

1. `run()` — 主循环，监听 bus 上的 `InboundMessage`
2. `process_message(msg)` — 处理一条消息：
   - 确定响应频道和 chat_id
   - 通过 `SessionManager` 获取或创建会话
   - 加载对话历史（最近 50 条消息）
   - 通过 `ContextBuilder` 构建系统提示词
   - 调用 `run_agent_loop()` 传入上下文 + 用户消息
3. `run_agent_loop()` — 委托给 mofa-sdk 的 `AgentLoop`，处理：
   - LLM API 调用
   - 工具调用检测和执行（通过 `ToolRegistryExecutor`）
   - 迭代循环：LLM → 工具调用 → 结果 → LLM → ... 直到完成或达到 `max_iterations`
4. 保存更新后的会话（用户消息 + 助手响应）
5. 返回 `OutboundMessage` 发布到 bus

**`register_default_tools()` 中注册的默认工具：**

```rust
// 文件工具
registry.register(ReadFileTool::new());
registry.register(WriteFileTool::new());
registry.register(EditFileTool::new());
registry.register(ListDirTool::new());

// Shell 工具
registry.register(ExecTool::new());

// 网页工具
registry.register(WebSearchTool::new(brave_api_key));
registry.register(WebFetchTool::new());

// 消息工具（带 bus 回调用于发送消息）
registry.register(MessageTool::with_callback(...));
```

`SpawnTool` 在 `AgentLoop` 创建后单独注册，因为它需要引用回循环本身（用于生成子代理）。

### 深入了解：ContextBuilder 与 Skills

`ContextBuilder`（`core/src/agent/context.rs`）负责组装塑造代理行为的系统提示词。

```rust
pub struct ContextBuilder {
    skills: Arc<SkillsManager>,  // 来自 mofa-sdk
    workspace: PathBuf,          // ~/.mofaclaw/workspace
}
```

**系统提示词组装**（`build_system_prompt()`）：

1. **引导文件**：从工作区加载（由 mofa-sdk 的 `PromptContextBuilder` 处理）：
   - `AGENTS.md` — 代理指令和准则
   - `SOUL.md` — 性格和价值观
   - `USER.md` — 用户画像和偏好
   - `TOOLS.md` — 工具说明
   - `IDENTITY.md` — 身份信息

2. **记忆** — 来自 `memory/MEMORY.md` 的长期记忆，由 mofa-sdk 的 `PromptContext` 自动处理。

3. **技能** — 三级渐进式加载：
   - **始终加载**：标记为"始终启用"的技能（仅元数据，每个约 100 词）
   - **请求加载**：触发时加载完整的 SKILL.md 正文
   - **技能摘要**：注入所有可用技能的列表，让代理知道有哪些技能可用

**技能发现** — `SkillsManager` 扫描两个目录：
- 项目根目录下的 `skills/`（内置技能）
- `~/.mofaclaw/workspace/skills/`（用户创建的技能）

它还有一个回退链，会相对于可执行文件或 `CARGO_MANIFEST_DIR` 来查找内置技能。

### 深入了解：会话管理

`SessionManager`（`core/src/session/`）使用 JSONL 文件处理对话持久化。

```rust
pub struct SessionManager {
    inner: MofaSessionManager,  // mofa-sdk 的管理器
    sessions_dir: PathBuf,      // ~/.mofaclaw/sessions
}
```

**核心特性：**
- **存储**：每个会话是一个 JSONL 文件，位于 `~/.mofaclaw/sessions/{safe_key}.jsonl`
- **会话键**：格式为 `"{channel}:{chat_id}"`（如 `"telegram:12345"`、`"cli:default"`）
- **历史加载**：`get_history_as_messages(max)` 返回最近的消息用于 LLM 上下文
- **结构化内容**：视觉/多部分消息通过前缀编码（`"__mofaclaw_content__:"`）在会话间保留
- **元数据**：跟踪 `created_at`、`updated_at` 和 `schema_version`

**API：**

```rust
sessions.get_or_create("cli:default").await;  // 获取或创建
sessions.save(&session).await;                 // 持久化到磁盘
sessions.list_sessions().await;                // 列出所有及元数据
sessions.delete("cli:old").await;              // 删除会话
```

### 深入了解：配置系统

`Config` 结构体（`core/src/config.rs`）代表完整的 `~/.mofaclaw/config.json`：

```rust
pub struct Config {
    pub agents: AgentsConfig,       // 模型、token 数、温度、迭代次数
    pub channels: ChannelsConfig,   // Telegram、钉钉、飞书、WhatsApp
    pub providers: ProvidersConfig, // API 密钥和基础 URL
    pub gateway: GatewayConfig,     // 主机和端口
    pub tools: ToolsConfig,         // 网页搜索、语音转录
}
```

**代理默认值：**

| 设置 | 默认值 | 说明 |
|------|--------|------|
| `workspace` | `~/.mofaclaw/workspace` | 代理的工作目录 |
| `model` | `anthropic/claude-opus-4-5` | 默认 LLM 模型 |
| `max_tokens` | 8192 | 最大响应 token 数 |
| `temperature` | 0.7 | 采样温度 |
| `max_tool_iterations` | 20 | 最大工具调用循环次数 |

**支持的提供商**（每个都有 `api_key` 和可选的 `api_base`）：

| 提供商 | 用途 | 备注 |
|--------|------|------|
| `openrouter` | 访问所有模型 | 推荐，一个密钥即可用 Claude/GPT 等 |
| `anthropic` | Claude 直连 | |
| `openai` | GPT 直连 | |
| `gemini` | Gemini 直连 | |
| `groq` | LLM + 语音转录 | 免费 Whisper 转录 |
| `zhipu` | GLM 模型 | 默认地址：`https://open.bigmodel.cn/api/paas/v4` |
| `vllm` | 本地模型 | 任何兼容 OpenAI 的服务器 |

**API 密钥解析顺序**（`get_api_key()`）：OpenRouter → Anthropic → OpenAI → Gemini → Zhipu → Groq → vLLM（取第一个非空值）。

**频道配置** — 每个频道都有 `enabled: bool` 标志以及平台特定字段：
- **Telegram**：`token`、`allow_from`（用户 ID/用户名白名单）
- **钉钉**：`client_id`、`client_secret`、`robot_code`、`dm_policy`、`group_policy`、`debug`
- **飞书**：`app_id`、`app_secret`、`encrypt_key`、`verification_token`、`debug`
- **WhatsApp**：`bridge_url`（默认 `ws://localhost:3001`）、`allow_from`

### 深入了解：错误处理

mofaclaw 使用 `thiserror` 构建完整的错误层次结构（`core/src/error.rs`）：

```rust
pub type Result<T> = std::result::Result<T, MofaclawError>;

pub enum MofaclawError {
    Config(ConfigError),     // NotFound, Parse, Invalid, Missing
    Tool(ToolError),         // NotFound, ExecutionFailed, InvalidParameters, Timeout
    Provider(ProviderError), // RequestFailed, AuthenticationFailed, RateLimitExceeded
    Session(SessionError),   // LoadFailed, SaveFailed, NotFound
    Channel(ChannelError),   // NotConfigured, SendFailed, ConnectionFailed
    Agent(AgentError),       // Stopped, MaxIterationsExceeded, ContextFailed
    Io(std::io::Error),
    Other(String),
}
```

每个变体都携带上下文信息。例如，`ToolError::ExecutionFailed` 包含工具名称和错误消息。`ChannelError` 有 Python 相关的变体（`PythonNotInstalled`、`PythonVersionTooOld`、`PythonPackageInstallFailed`），用于钉钉/飞书的桥接。

---

## 工作区
---

## 安全与访问控制 (RBAC)

mofaclaw 包含一个强大的基于角色的访问控制 (RBAC) 系统，可以为 AI 的能力提供沙箱环境。这能够在 AI 使用执行 Shell 命令或访问文件系统等工具时，保护您的系统免受意外损坏或恶意提示词注入的攻击。

### 角色 (Roles)
代理的权限由 `config.json` 中 `rbac.default_role` 指定的默认角色，以及各渠道通过 `rbac.role_mappings` / `user_overrides` 解析得到的实际角色共同决定。
- **Guest (访客)**: 权限严格受限。仅对特定文件夹拥有只读访问权限。无法执行 Shell 命令。
- **Member (成员)** (默认): 标准权限。可以读写工作区，并能运行安全的已列入白名单的命令。
- **Admin (管理员)**: 拥有管理系统的扩展权限。
- **SuperAdmin (超级管理员)**: 无限制访问权限（绕过所有沙箱限制）。

### 🛡️ Shell 命令沙箱 (命令白名单)
当启用 RBAC 且配置了 `shell.safe_commands` 操作时，AI 只允许执行匹配白名单的命令；未列入白名单的命令（包括类似 `rm -rf /` 或 `sudo` 的危险命令）将被阻止。如果在启用 RBAC 的情况下未配置 `shell.safe_commands`，则默认允许命令执行；而当禁用 RBAC 时，系统将使用旧版的 `is_dangerous_command` 进行检查。您可以使用 `safe_commands` 白名单来配置精确的允许命令或模式。

```json
"rbac": {
  "permissions": {
    "tools": {
      "shell": {
        "safe_commands": {
          "min_role": "member",
          "allowed": [
            "ls *",
            "cat *",
            "git status",
            "gh issue *"
          ]
        }
      }
    }
  }
}
```

---


运行 `mofaclaw onboard` 后，会创建一个作为代理工作环境的工作区：

```
~/.mofaclaw/
├── config.json                 # 主配置文件
├── workspace/                  # 代理工作区
│   ├── AGENTS.md              # 代理指令和准则
│   ├── SOUL.md                # 性格和价值观定义
│   ├── USER.md                # 用户画像和偏好
│   ├── TOOLS.md               # 面向代理的工具文档
│   ├── IDENTITY.md            # 代理身份信息
│   ├── HEARTBEAT.md           # 周期性任务（每 30 分钟检查）
│   ├── memory/
│   │   └── MEMORY.md          # 长期事实和偏好
│   ├── skills/                # 用户创建的技能
│   └── cron_jobs.json         # 定时任务定义
├── sessions/                  # 对话历史（JSONL 文件）
│   ├── cli_default.jsonl
│   ├── telegram_12345.jsonl
│   └── ...
└── media/                     # 从频道下载的图片/文件
```

**引导文件**在代理每次处理消息时都会加载到系统提示词中。你可以自定义这些文件来改变代理的行为：

- **`AGENTS.md`** — 指令如"简洁回答"、"不明确时请求澄清"、"使用工具完成任务"
- **`SOUL.md`** — 性格："我是 mofaclaw，一个个人 AI 助手。乐于助人、简洁、好奇。"
- **`USER.md`** — 用户画像：姓名、时区、语言、偏好
- **`TOOLS.md`** — 从代理角度记录所有工具（每个工具的功能、参数）
- **`HEARTBEAT.md`** — 代理每 30 分钟检查的任务（在此添加任务清单）
- **`memory/MEMORY.md`** — 代理跨会话记住的持久化事实

---

## 实战：添加一个 Skill

技能是**扩展 mofaclaw 最简单的方式** — 它们只是 Markdown 文件，不需要写 Rust 代码！

### 什么是技能？

技能是一个包含 `SKILL.md` 文件的文件夹，该文件具有 YAML frontmatter 和 Markdown 指令。技能从两个位置自动发现：
- 项目根目录下的 `skills/`（内置技能）
- `~/.mofaclaw/workspace/skills/`（用户创建的技能）

`ContextBuilder` 使用 `SkillsManager` 扫描这些目录。每个技能的 `name` 和 `description` 会从 frontmatter 中提取并注入到系统提示词中，使代理知道有哪些技能可用。当技能被触发时，其完整的 Markdown 正文会被加载到上下文中。

### 技能的加载方式（渐进式披露）

技能使用三级加载系统来高效管理上下文：

1. **元数据**（name + description） — 始终在上下文中，每个技能约 100 词。这是代理判断相关性的依据。
2. **SKILL.md 正文** — 技能被触发时加载。建议控制在 500 行以内。
3. **捆绑资源**（scripts/、references/、assets/） — 代理决定需要时按需加载。

以下是 `ContextBuilder.build_system_prompt()` 中的相关代码：

```rust
// 技能摘要始终被包含 — 代理能看到所有可用技能
let skills_summary = self.skills.build_skills_summary().await;

// 摘要告诉代理："要使用某个技能，请使用 read_file 工具
// 读取其 SKILL.md 文件来了解如何使用它。"
```

### 示例：创建一个 `hello` 技能

创建文件 `skills/hello/SKILL.md`：

```markdown
---
name: hello
description: 热情地问候用户并分享有趣的事实。当有人说你好、嗨或请求问候时使用。
---

# 问候技能

当用户向你打招呼时，回复包含：

1. 一个温暖友好的问候
2. 一个关于今天日期的随机趣味知识
3. 一条鼓励的话

回复保持简洁（最多 2-3 句）。
```

就是这样！技能会在下次运行时自动被发现。你可以启动代理并询问"你有什么技能？"来验证。

### 示例：一个带资源的高级技能

```
my-research-skill/
├── SKILL.md
├── scripts/
│   └── search_papers.py       # 用于搜索的确定性脚本
├── references/
│   └── databases.md           # 学术数据库列表
└── assets/
    └── citation-template.txt  # 引用格式模板
```

SKILL.md 会引用这些资源：

```markdown
---
name: research-assistant
description: 帮助查找和总结学术论文。当用户询问研究、论文、引用或学术主题时使用。
---

# 研究助手

## 快速搜索
使用用户的查询运行 `scripts/search_papers.py`。

## 数据库参考
支持的数据库列表见 `references/databases.md`。

## 格式化引用
使用 `assets/citation-template.txt` 作为模板。
```

### 技能编写建议

- **描述是关键** — `description` 字段是主要的触发机制。同时包含技能的功能和使用场景。
- **保持 SKILL.md 精简** — 正文会加载到 LLM 的上下文窗口中。每个 token 都很重要。优先使用简洁的示例而非冗长的解释。
- **使用 references/ 存放详情** — 将大型文档移到 `references/` 文件中，并从 SKILL.md 引用。代理只在需要时才会加载它们。
- **不要重复已有知识** — LLM 本身已经很智能。只包含模型无法自行推断的领域特定流程、API 细节或工作流。

### 参考技能

查看现有技能获取灵感：
- `skills/weather/SKILL.md` — 使用 `curl` 命令和 wttr.in、Open-Meteo 获取天气数据，无需 API 密钥。简单独立技能的好示例。
- `skills/skill-creator/SKILL.md` — 指导创建新技能的元技能。展示了渐进式披露模式。
- `skills/summarize/` — 摘要技能
- `skills/github/` — GitHub 相关操作

---

## 实战：添加一个 Tool

工具赋予代理**执行操作**的能力 — 读取文件、运行命令、搜索网页等。工具使用 Rust 编写，实现 mofa-sdk 的 `SimpleTool` trait。

### `SimpleTool` trait

每个工具实现以下方法（定义在 mofa-sdk 中，通过 `core/src/tools/base.rs` 重新导出）：

```rust
use mofa_sdk::agent::{SimpleTool, ToolCategory};
use mofa_sdk::kernel::{ToolInput, ToolResult};

#[async_trait]
pub trait SimpleTool: Send + Sync {
    /// 工具的唯一名称（用于 LLM 工具调用）
    fn name(&self) -> &str;

    /// 人类可读的描述（展示给 LLM 以便它知道何时使用此工具）
    fn description(&self) -> &str;

    /// 工具参数的 JSON Schema（告诉 LLM 要传递什么参数）
    fn parameters_schema(&self) -> Value;

    /// 使用给定输入执行工具并返回结果
    async fn execute(&self, input: ToolInput) -> ToolResult;

    /// 可选：对工具进行分类（File、Shell、Web、Custom 等）
    fn category(&self) -> ToolCategory { ToolCategory::Custom }
}
```

**如何连接到 LLM**：`ToolRegistry` 将所有已注册的工具转换为 OpenAI 兼容的函数定义：

```json
{
  "type": "function",
  "function": {
    "name": "read_file",
    "description": "Read the contents of a file at the given path.",
    "parameters": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "description": "The file path to read" }
      },
      "required": ["path"]
    }
  }
}
```

当 LLM 决定使用工具时，它会返回一个工具调用。mofa-sdk 的 `AgentLoop` 拦截它，分发到 `ToolRegistry`，将结果反馈给 LLM，然后重复直到 LLM 完成。

### 示例：创建一个 `current_time` 工具

#### 步骤 1：创建文件

创建 `core/src/tools/time.rs`：

```rust
//! 时间工具：获取当前日期和时间

use super::base::{SimpleTool, ToolInput, ToolResult};
use async_trait::async_trait;
use serde_json::{Value, json};

/// 获取当前日期和时间的工具
pub struct CurrentTimeTool;

impl CurrentTimeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SimpleTool for CurrentTimeTool {
    fn name(&self) -> &str {
        "current_time"
    }

    fn description(&self) -> &str {
        "Get the current date and time. Returns the current UTC timestamp."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "description": "Output format: 'iso' for ISO 8601, 'human' for readable. Defaults to 'human'."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let format = input.get_str("format").unwrap_or("human");

        let now = chrono::Utc::now();

        let formatted = match format {
            "iso" => now.to_rfc3339(),
            _ => now.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        };

        ToolResult::success_text(format!("Current time: {}", formatted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_metadata() {
        let tool = CurrentTimeTool::new();
        assert_eq!(tool.name(), "current_time");
        assert!(!tool.description().is_empty());
        // 验证 schema 是有效的 JSON
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
    }

    #[tokio::test]
    async fn test_execute_default_format() {
        let tool = CurrentTimeTool::new();
        let input = ToolInput::from_json(json!({}));
        let result = tool.execute(input).await;
        assert!(result.success);
        assert!(result.to_string_output().contains("Current time:"));
    }

    #[tokio::test]
    async fn test_execute_iso_format() {
        let tool = CurrentTimeTool::new();
        let input = ToolInput::from_json(json!({"format": "iso"}));
        let result = tool.execute(input).await;
        assert!(result.success);
        // ISO 8601 包含 'T'
        assert!(result.to_string_output().contains("T"));
    }
}
```

#### 步骤 2：注册模块

将模块添加到 `core/src/tools/mod.rs`：

```rust
pub mod time;  // 添加此行

pub use time::CurrentTimeTool;  // 添加此行
```

#### 步骤 3：在代理循环中注册工具

在 `core/src/agent/loop_.rs` 中，找到 `register_default_tools` 方法并添加你的工具：

```rust
// 在 loop_.rs 顶部添加导入：
use crate::tools::time::CurrentTimeTool;

// 然后在 register_default_tools() 中：
fn register_default_tools(
    registry: &mut ToolRegistry,
    _workspace: &std::path::Path,
    brave_api_key: Option<String>,
    bus: MessageBus,
) {
    // ... 现有工具（文件、Shell、网页、消息）...

    // 时间工具
    registry.register(CurrentTimeTool::new());
}
```

#### 步骤 4：构建和测试

```bash
# 先运行单元测试
cargo test -p mofaclaw-core test_tool_metadata
cargo test -p mofaclaw-core test_execute

# 构建
cargo build --release

# 与代理一起测试
cargo run --release -- agent -m "现在几点了？"
```

### 参考：现有工具

研究这些文件了解模式和最佳实践：

| 文件 | 工具 | 学习要点 |
|------|------|----------|
| `core/src/tools/filesystem.rs` | `read_file`、`write_file`、`edit_file`、`list_dir` | 参数验证、错误处理、波浪号展开 |
| `core/src/tools/shell.rs` | `exec` | 60 秒超时的命令执行，危险命令阻止（`rm -rf /`、`mkfs`、fork 炸弹），输出截断至 10,000 字符，跨平台（Unix 上 `sh -c`，Windows 上 `cmd /C`） |
| `core/src/tools/web.rs` | `web_search`、`web_fetch` | 外部 API 集成（Brave Search），HTML 转 Markdown，内容截断，API 密钥环境变量回退 |
| `core/src/tools/message.rs` | `message` | 使用 `Arc<RwLock<>>` 的内部可变性，基于回调的工具设计，上下文注入（`set_context()`） |
| `core/src/tools/spawn.rs` | `spawn` | 子代理生成，`SubagentManager` trait，异步后台任务 |

### 工具实现的关键模式

**参数提取：**

```rust
// 字符串参数（返回 Option<&str>）
let path = input.get_str("path");

// 带验证
let path = match input.get_str("path") {
    Some(p) => p,
    None => return ToolResult::failure("Missing 'path' parameter"),
};
```

**成功和失败：**

```rust
// 成功返回文本
ToolResult::success_text("操作成功完成")

// 成功返回格式化输出
ToolResult::success_text(format!("从 {} 读取了 {} 字节", path, content.len()))

// 失败返回错误消息
ToolResult::failure("错误：文件未找到")
ToolResult::failure(format!("读取文件错误：{}", e))
```

**参数的 JSON Schema：**

```rust
fn parameters_schema(&self) -> Value {
    json!({
        "type": "object",
        "properties": {
            "required_param": {
                "type": "string",
                "description": "给 LLM 的清晰描述"
            },
            "optional_param": {
                "type": "integer",
                "description": "带默认值的可选数字"
            }
        },
        "required": ["required_param"]  // 只列出真正必需的参数
    })
}
```

**工具注册流程：**

```
SimpleTool 实现  →  registry.register(tool)  →  as_tool() 包装  →  Arc<dyn Tool>
                                                                          │
                                                                          ▼
                                  ToolRegistry.get_definitions()  ←  SimpleToolRegistry
                                            │
                                            ▼
                                  JSON 作为可用函数发送给 LLM
```

---

## 实战：添加一个 Channel

频道将 mofaclaw 连接到消息平台。每个频道实现 `Channel` trait，并通过 `MessageBus` 进行通信。

### `Channel` trait

定义在 `core/src/channels/base.rs`：

```rust
#[async_trait]
pub trait Channel: Send + Sync {
    /// 获取频道名称（如 "telegram"、"dingtalk"）
    fn name(&self) -> &str;

    /// 启动频道（开始接收消息）
    /// 由 ChannelManager 调用，应阻塞直到停止
    async fn start(&self) -> Result<()>;

    /// 优雅地停止频道
    async fn stop(&self) -> Result<()>;

    /// 检查频道是否在配置中启用
    fn is_enabled(&self) -> bool;
}
```

### 频道如何与系统交互

```
外部平台（如 Telegram API）
        │
        │  (webhook / 轮询 / WebSocket)
        ▼
┌─────────────────────┐
│   YourChannel       │
│                     │
│  fn start() {       │
│    // 监听          │
│    // 消息          │──────────┐
│  }                  │          │
│                     │          ▼
│  // 转换为          │   bus.publish_inbound(
│  // InboundMessage  │     InboundMessage::new(
│                     │       "your_channel",
│  // 订阅            │       sender_id,
│  // 出站消息        │       chat_id,
│  //                 │       content,
│  bus.subscribe_     │     )
│    outbound()       │   )
│       │             │
│       ▼             │
│  // 将响应发回      │
│  // 平台            │
└─────────────────────┘
```

**ChannelManager**（`core/src/channels/manager.rs`）编排所有频道：

```rust
pub struct ChannelManager {
    config: ChannelsConfig,
    channels: Arc<RwLock<Vec<Arc<dyn Channel>>>>,
    running: Arc<RwLock<bool>>,
}
```

它为每个已启用的频道在独立的 tokio 任务中运行，错误时自动重连（5 秒重试延迟）。

### 频道注册

频道在 `cli/src/main.rs` 的 gateway 命令中注册：

```rust
// 创建频道管理器
let channel_manager = ChannelManager::new(&config, bus.clone());

// 注册每个已启用的频道
if config.channels.dingtalk.enabled {
    let dingtalk = DingTalkChannel::new(config.channels.dingtalk.clone(), bus.clone());
    channel_manager.register_channel(Arc::new(dingtalk)).await;
}

if config.channels.telegram.enabled {
    let telegram = TelegramChannel::new(config.channels.telegram.clone(), bus.clone())?;
    channel_manager.register_channel(Arc::new(telegram)).await;
}

if config.channels.feishu.enabled {
    let feishu = FeishuChannel::new(config.channels.feishu.clone(), bus.clone());
    channel_manager.register_channel(Arc::new(feishu)).await;
}

// 并发启动所有已启用的频道
channel_manager.start_all().await?;
```

### 添加新频道：分步指南

让我们以添加一个假设的 **Slack** 频道为例。

#### 步骤 1：添加配置

在 `core/src/config.rs` 中添加配置结构体：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub app_token: String,
    pub allow_from: Vec<String>,  // Slack 用户 ID 白名单
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: String::new(),
            app_token: String::new(),
            allow_from: Vec::new(),
        }
    }
}
```

将其添加到 `ChannelsConfig`：

```rust
pub struct ChannelsConfig {
    pub telegram: TelegramConfig,
    pub dingtalk: DingTalkConfig,
    pub feishu: FeishuConfig,
    pub whatsapp: WhatsAppConfig,
    pub slack: SlackConfig,  // 添加此行
}
```

#### 步骤 2：实现频道

创建 `core/src/channels/slack.rs`：

```rust
use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::SlackConfig;
use crate::error::Result;
use crate::messages::{InboundMessage, OutboundMessage};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SlackChannel {
    config: SlackConfig,
    bus: MessageBus,
    running: Arc<RwLock<bool>>,
}

impl SlackChannel {
    pub fn new(config: SlackConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            running: Arc::new(RwLock::new(false)),
        }
    }
}

#[async_trait]
impl Channel for SlackChannel {
    fn name(&self) -> &str {
        "slack"
    }

    async fn start(&self) -> Result<()> {
        *self.running.write().await = true;

        // 订阅此频道的出站消息
        let mut outbound_rx = self.bus.subscribe_outbound();

        // 启动任务处理出站消息
        let running = self.running.clone();
        tokio::spawn(async move {
            while *running.read().await {
                if let Ok(msg) = outbound_rx.recv().await {
                    if msg.channel == "slack" {
                        // TODO: 通过 API 将 msg.content 发送到 Slack
                    }
                }
            }
        });

        // 主循环：监听传入的 Slack 消息
        while *self.running.read().await {
            // TODO: 连接 Slack Socket Mode API
            // 收到消息时：
            //   let inbound = InboundMessage::new(
            //       "slack",
            //       &user_id,
            //       &channel_id,
            //       &message_text,
            //   );
            //   self.bus.publish_inbound(inbound).await?;

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        *self.running.write().await = false;
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}
```

#### 步骤 3：注册模块

在 `core/src/channels/mod.rs` 中：

```rust
pub mod slack;
pub use slack::SlackChannel;
```

#### 步骤 4：在网关中注册

在 `cli/src/main.rs` 中添加注册逻辑：

```rust
if config.channels.slack.enabled {
    let slack = SlackChannel::new(config.channels.slack.clone(), bus.clone());
    channel_manager.register_channel(Arc::new(slack)).await;
    println!("Slack: enabled");
}
```

#### 步骤 5：更新 ChannelManager

在 `core/src/channels/manager.rs` 中，添加到 `enabled_channels()` 和 `has_enabled_channels()`：

```rust
if self.config.slack.enabled {
    enabled.push("slack".to_string());
}
```

### 参考实现

| 文件 | 频道 | 架构 |
|------|------|------|
| `core/src/channels/telegram.rs` | Telegram | 使用 `teloxide` crate 进行 bot 轮询。处理 Markdown→HTML 转换、语音转录（Groq/Whisper）、媒体下载到 `~/.mofaclaw/media/` |
| `core/src/channels/dingtalk.rs` | 钉钉 | 嵌入 Python 桥接脚本，使用 `dingtalk-stream` SDK，通过 WebSocket 在 `ws://localhost:3002` 通信 |
| `core/src/channels/feishu.rs` | 飞书/Lark | 类似钉钉 — 嵌入 Python 桥接，使用 `lark-oapi` SDK，WebSocket 在 `ws://localhost:3004` |
| `core/src/channels/whatsapp.rs` | WhatsApp | WebSocket 桥接在 `ws://localhost:3001` |

---

## 网关：系统的启动流程

`mofaclaw gateway` 命令是完整的生产模式。理解其启动序列有助于你看到所有组件如何连接：

```rust
async fn command_gateway(port: u16, verbose: bool) -> Result<()> {
    // 1. 从 ~/.mofaclaw/config.json 加载配置
    let config = load_config().await?;

    // 2. 创建 LLM 提供商（兼容 OpenAI 的 API）
    let openai_config = OpenAIConfig::new(&api_key)
        .with_model(&model)
        .with_base_url(api_base);  // 适用于 OpenRouter、vLLM 等
    let provider = Arc::new(OpenAIProvider::with_config(openai_config));

    // 3. 创建核心基础设施
    let bus = MessageBus::new();
    let sessions = Arc::new(SessionManager::new(&config));
    let tools = Arc::new(RwLock::new(ToolRegistry::new()));

    // 4. 注册工具
    AgentLoop::register_default_tools(&mut tools, &workspace, brave_api_key, bus.clone());

    // 5. 构建 LLMAgent（mofa-sdk）
    let llm_agent = LLMAgentBuilder::new()
        .with_id("mofaclaw-gateway")
        .with_name("Mofaclaw Gateway Agent")
        .with_provider(provider.clone())
        .with_system_prompt(system_prompt)
        .with_tool_executor(tool_executor)
        .build_async().await;

    // 6. 创建 AgentLoop
    let agent = AgentLoop::with_agent_and_tools(
        &config, llm_agent, provider, bus.clone(), sessions, tools
    ).await?;

    // 7. 注册 spawn 工具（需要代理引用）
    let subagent_manager = SubagentManager::new(agent.clone());
    agent.register_spawn_tool(subagent_manager).await;

    // 8. 注册频道
    let channel_manager = ChannelManager::new(&config, bus.clone());
    // ... 如果启用则注册 Telegram、钉钉、飞书、WhatsApp ...

    // 9. 启动心跳服务（每 30 分钟）
    let heartbeat = HeartbeatService::new(workspace, 30 * 60)
        .with_callback(|prompt| agent.process_direct(&prompt, "heartbeat"));

    // 10. 并发运行所有组件
    tokio::select! {
        result = agent.run() => result?,
        result = channel_manager.start_all() => result?,
        _ = tokio::signal::ctrl_c() => { /* 优雅关闭 */ }
    }
}
```

关键洞察：所有组件通过 `tokio::select!` 并发运行 — 代理循环、所有频道和心跳服务共享同一个 `MessageBus`。

---

## 心跳与定时任务系统

### 心跳服务

`HeartbeatService`（`core/src/heartbeat/`）每 30 分钟主动唤醒代理检查 `HEARTBEAT.md`：

```rust
pub struct HeartbeatService {
    workspace: PathBuf,
    interval_s: u64,       // 默认：1800（30 分钟）
    on_heartbeat: Option<HeartbeatCallback>,
    running: Arc<RwLock<bool>>,
}
```

**工作原理：**
1. 每 `interval_s` 秒，从工作区读取 `HEARTBEAT.md`。
2. 如果文件为空或只有标题/注释则跳过。
3. 如果有可执行内容，向代理发送提示："读取工作区中的 HEARTBEAT.md，执行其中列出的指令或任务。"
4. 代理像处理普通消息一样处理它，可以使用工具（读取文件、执行命令等）。
5. 如果没有需要注意的事项，代理回复 `HEARTBEAT_OK`。

**使用场景**：在 `HEARTBEAT.md` 中添加定期任务，如"检查服务器是否在线"或"总结今天的邮件"。

### 定时任务服务

`CronService`（`core/src/cron/`）处理更灵活的定时任务：

```rust
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: CronSchedule,
    pub payload: CronPayload,
    pub state: CronJobState,
}

pub enum CronSchedule {
    At { at_ms: Option<i64> },                           // 一次性定时
    Every { every_ms: Option<u64> },                     // 每 N 毫秒
    Cron { expr: Option<String>, tz: Option<String> },   // Cron 表达式
}
```

**示例：**

```bash
# 每天早上 9 点
mofaclaw cron add --name "morning" --message "早上好！" --cron "0 9 * * *"

# 每 2 小时
mofaclaw cron add --name "check" --message "检查服务器状态" --every 7200

# 一次性指定时间
mofaclaw cron add --name "meeting" --message "开会了！" --at "2025-01-31T15:00:00"
```

任务存储在 `~/.mofaclaw/workspace/cron_jobs.json` 中，可以选择性地将响应路由到频道。

---

## 开发工作流

### 构建和测试

```bash
# Debug 模式构建（编译更快）
cargo build

# Release 模式构建（优化后，用于测试性能）
cargo build --release

# 运行所有测试
cargo test

# 运行特定 crate 的测试
cargo test -p mofaclaw-core

# 按名称运行特定测试
cargo test test_channel_trait

# 运行测试并显示输出
cargo test -- --nocapture

# 检查编译器警告
cargo clippy

# 格式化代码
cargo fmt
```

### 调试

```bash
# 启用详细日志运行
RUST_LOG=debug cargo run -- agent -m "test"

# 或者网关模式带 trace 级别日志
RUST_LOG=trace cargo run -- gateway --verbose

# 只看 mofaclaw 日志（过滤依赖库）
RUST_LOG=mofaclaw_core=debug cargo run -- agent -m "test"
```

代码库使用 `tracing` crate 进行结构化日志：

```rust
use tracing::{info, debug, warn, error};

info!("AgentLoop started");
debug!("Processing message from {}", msg.channel);
warn!("Failed to publish outbound message: {}", e);
error!("Channel {} error: {}", name, e);
```

### 开发阶段的项目结构

```
mofaclaw/
├── core/                   # 核心库 crate（mofaclaw-core）
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # 公开 API 导出
│       ├── config.rs       # 配置类型和加载
│       ├── error.rs        # 错误层次结构（thiserror）
│       ├── messages.rs     # InboundMessage、OutboundMessage
│       ├── types.rs        # Message、MessageContent、MessageRole
│       ├── agent/
│       │   ├── mod.rs
│       │   ├── loop_.rs    # AgentLoop — 核心引擎
│       │   ├── context.rs  # ContextBuilder — 系统提示词组装
│       │   └── subagent.rs # SubagentManager
│       ├── tools/
│       │   ├── mod.rs      # 模块声明和重新导出
│       │   ├── base.rs     # 从 mofa-sdk 重新导出 SimpleTool trait
│       │   ├── registry.rs # ToolRegistry 和 ToolRegistryExecutor
│       │   ├── filesystem.rs # read_file、write_file、edit_file、list_dir
│       │   ├── shell.rs    # exec（Shell 命令执行）
│       │   ├── web.rs      # web_search、web_fetch
│       │   ├── message.rs  # message（发送给用户）
│       │   └── spawn.rs    # spawn（子代理创建）
│       ├── channels/
│       │   ├── mod.rs
│       │   ├── base.rs     # Channel trait
│       │   ├── manager.rs  # ChannelManager
│       │   ├── telegram.rs # Telegram（teloxide）
│       │   ├── dingtalk.rs # 钉钉（Python 桥接）
│       │   ├── feishu.rs   # 飞书/Lark（Python 桥接）
│       │   └── whatsapp.rs # WhatsApp（WebSocket 桥接）
│       ├── bus/
│       │   ├── mod.rs      # MessageBus（tokio broadcast）
│       │   └── queue.rs    # 队列工具
│       ├── session/
│       │   └── mod.rs      # SessionManager（JSONL 持久化）
│       ├── heartbeat/
│       │   └── service.rs  # HeartbeatService
│       └── cron/
│           ├── service.rs  # CronService
│           └── types.rs    # CronJob、CronSchedule、CronPayload
├── cli/                    # CLI 二进制 crate
│   ├── Cargo.toml
│   └── src/
│       └── main.rs         # 命令处理（clap）、网关启动
├── skills/                 # 内置技能（Markdown）
│   ├── weather/SKILL.md
│   ├── skill-creator/SKILL.md
│   ├── summarize/
│   ├── github/
│   └── tmux/
└── workspace/              # 默认工作区模板
    ├── AGENTS.md
    ├── SOUL.md
    ├── TOOLS.md
    └── ...
```

### 核心依赖

| Crate | 用途 |
|-------|------|
| `mofa-sdk` | LLM 框架：提供商、代理、工具、技能、会话 |
| `tokio` | 异步运行时 |
| `serde` / `serde_json` | 序列化 |
| `async-trait` | 异步 trait 实现 |
| `tracing` | 结构化日志 |
| `thiserror` / `anyhow` | 错误处理 |
| `reqwest` | HTTP 客户端（用于网页工具） |
| `teloxide` | Telegram bot 框架 |
| `clap` | CLI 参数解析 |
| `chrono` | 日期/时间处理 |
| `uuid` | 唯一标识符 |

---

## GSoC 项目创意

以下是 GSoC 贡献者的潜在项目方向，大致按复杂度排序：

### 入门级

1. **新技能** — 创建特定领域的技能（编程助手、语言学习导师、数据分析指南）。纯 Markdown，适合学习系统。

2. **新工具：计算器** — 实现一个精确计算的 `calculate` 工具。作为第一个 Rust 工具很合适 — 简单的 `SimpleTool` 实现，配合 `eval` 风格的解析。

3. **新工具：图片生成** — 集成 DALL-E 或 Stable Diffusion API。学习带外部 API 调用的工具开发。

### 中级

4. **新频道：Discord** — 使用 `serenity` crate 实现 Discord bot 频道。学习 Channel trait、MessageBus 集成和异步模式。

5. **新频道：Slack** — 使用 Socket Mode 实现 Slack bot。与 Discord 类似但 API 接口不同。

6. **增强会话管理** — 添加会话搜索、导出（JSON/Markdown）、会话分支（分叉对话）或跨频道共享会话。

7. **语音界面** — 改进语音转录（目前使用 Groq/Whisper）并通过 Telegram 频道添加文本转语音。

### 高级

8. **多代理协作** — 扩展 `TaskOrchestrator` 和 `SpawnTool`，支持复杂的多代理工作流：共享上下文、代理间通信、任务委托链。

9. **Web UI 频道** — 构建基于 Web 的聊天界面作为新频道，使用 WebSocket 与网关通信。包含消息历史、工具调用可视化和技能管理。

10. **插件系统** — 设计动态插件架构，允许在运行时加载工具和频道（如通过 WASM 或动态库），无需重新编译。

11. **RAG（检索增强生成）** — 添加向量存储工具用于文档嵌入和语义搜索，使代理能够处理大型知识库。

---

## 贡献指南

### 代码风格

- 每次提交前运行 `cargo fmt`。
- 运行 `cargo clippy` 并修复所有警告。
- 使用 `async_trait` 实现异步 trait。
- 保持工具自包含 — `core/src/tools/` 下每个工具一个文件。
- 编写文档注释（公开项用 `///`，模块级用 `//!`）。
- 使用 `tracing` crate 记录日志（`info!`、`debug!`、`warn!`、`error!`）。
- 遵循现有模式 — 编写新代码前先查看类似代码。

### 编写测试

每个新工具、频道或重要功能都应有测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // 同步测试用于元数据/配置
    #[test]
    fn test_tool_metadata() {
        let tool = MyTool::new();
        assert_eq!(tool.name(), "my_tool");
        assert!(!tool.description().is_empty());
    }

    // 异步测试用于工具执行
    #[tokio::test]
    async fn test_tool_execute() {
        let tool = MyTool::new();
        let input = ToolInput::from_json(json!({"param": "value"}));
        let result = tool.execute(input).await;
        assert!(result.success);
    }

    // Channel trait 测试
    #[tokio::test]
    async fn test_channel_name() {
        let config = MyConfig::default();
        let bus = MessageBus::new();
        let channel = MyChannel::new(config, bus);
        assert_eq!(channel.name(), "my_channel");
    }
}
```

```bash
# 运行所有测试
cargo test

# 只运行 core 的测试
cargo test -p mofaclaw-core

# 运行特定测试
cargo test test_tool_metadata

# 运行测试并显示输出
cargo test -- --nocapture
```

### 提交 PR

1. **Fork** 仓库并创建功能分支（`git checkout -b feature/my-feature`）。
2. **编写测试**覆盖新功能。
3. 提交前**运行** `cargo fmt && cargo clippy`。
4. **运行** `cargo test` 确保所有测试通过。
5. **保持 PR 聚焦** — 每个 PR 一个功能或修复。
6. 在 PR 描述中**说明**你的 PR 做了什么以及为什么。

### 获取帮助

- 在 GitHub 上开 issue 讨论 bug 或功能。
- 构建新内容前先查看现有技能和工具的模式。
- 阅读 `workspace/TOOLS.md` 了解工具从代理角度的文档化方式。
- 使用 `RUST_LOG=debug` 查看底层运行情况。
- mofa-sdk 文档涵盖了 LLM 提供商、代理和工具抽象。

祝编码愉快！
