<div align="center">
  <img src="https://github.com/mofa-org/mofa/raw/main/documents/images/mofa-logo.png" alt="mofaclaw" width="500">
  <h1>mofaclaw: Ultra-Lightweight Personal AI Assistant</h1>
  <p>
    <img src="https://img.shields.io/badge/rust-1.85%2B-orange" alt="Rust">
    <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
  </p>
</div>

🐈 **mofaclaw** is an **ultra-lightweight** personal AI assistant inspired by [nanobot](https://github.com/HKUDS/nonobot)

⚡️ Written in **Rust** for maximum performance, safety, and efficiency — **99% smaller** than Clawdbot's 430k+ lines.

## 📢 News

- **2026-02-04** 🎉 mofaclaw inspired by nanobot, rewritten in Rust! Blazing fast performance with memory safety.

## Key Features of mofaclaw:

🦀 **Rust-Powered**: Memory-safe, zero-cost abstractions, and fearless concurrency for rock-solid stability.

🪶 **Ultra-Lightweight**: Minimal dependencies and clean architecture — easy to understand and modify.

🔬 **Research-Ready**: Clean, readable code that's easy to understand, modify, and extend for research.

⚡️ **Lightning Fast**: Compiled performance with instant startup, minimal memory footprint, and efficient execution.

💎 **Easy-to-Use**: Simple installation and setup — you're ready to go in minutes.

## 🏗️ Architecture

<p align="center">
  <img src="nanobot_arch.png" alt="mofaclaw architecture" width="800">
</p>

## 📦 Install

### Prerequisites

**Rust** (1.85+ required)

- For users in China, use the fast mirror: [https://rsproxy.cn/](https://rsproxy.cn/)
- Official installation: [https://www.rust-lang.org/tools/install](https://www.rust-lang.org/tools/install)

**Quick install (China mirror):**
```bash
# Visit https://rsproxy.cn/ for the latest installation command
curl --proto '=https' --tlsv1.2 -sSf https://rsproxy.cn/install.sh | sh
```

**Install from source** (recommended)

```bash
# Install the CLI binary
cargo install --path cli/

# Or build and run directly
cargo run --release -- onboard
```

**Build manually**

```bash
cargo build --release
# The binary will be at target/release/mofaclaw
```

**Install with cargo** (from crates.io, coming soon)

```bash
cargo install mofaclaw
```

## 🚀 Quick Start

> [!TIP]
> Set your API key in `~/.mofaclaw/config.json`.
> Get API keys: [OpenRouter](https://openrouter.ai/keys) (LLM) · [Brave Search](https://brave.com/search/api/) (optional, for web search)
> You can also change the model to `minimax/minimax-m2` for lower cost.

**1. Initialize**

```bash
mofaclaw onboard
```

**2. Configure** (`~/.mofaclaw/config.json`)

```json
{
  "providers": {
    "openrouter": {
      "apiKey": "sk-or-v1-xxx"
    }
  },
  "agents": {
    "defaults": {
      "model": "anthropic/claude-opus-4-5"
    }
  },
  "webSearch": {
    "apiKey": "BSA-xxx"
  }
}
```


**3. Chat**

```bash
mofaclaw agent -m "What is 2+2?"
```

That's it! You have a working AI assistant in 2 minutes.

## 🖥️ Local Models (vLLM)

Run mofaclaw with your own local models using vLLM or any OpenAI-compatible server.

**1. Start your vLLM server**

```bash
vllm serve meta-llama/Llama-3.1-8B-Instruct --port 8000
```

**2. Configure** (`~/.mofaclaw/config.json`)

```json
{
  "providers": {
    "vllm": {
      "apiKey": "dummy",
      "apiBase": "http://localhost:8000/v1"
    }
  },
  "agents": {
    "defaults": {
      "model": "meta-llama/Llama-3.1-8B-Instruct"
    }
  }
}
```

**3. Chat**

```bash
mofaclaw agent -m "Hello from my local LLM!"
```

> [!TIP]
> The `apiKey` can be any non-empty string for local servers that don't require authentication.

## 💬 Chat Apps

Talk to your mofaclaw through DingTalk or Feishu — anytime, anywhere.

| Channel      | Status   |
|--------------|----------|
| **DingTalk** | tested   |
| **Feishu**   | not test |

### Python Dependencies

DingTalk and Feishu channels require **Python 3.11+** with the following packages:

**DingTalk:**
```bash
pip install dingtalk-stream websockets certifi
```

**Feishu:**
```bash
pip install lark-oapi websockets cryptography
```

> **Note:** The gateway will automatically check and install these packages on first run. However, you may need to install them manually if you don't have write permissions to the Python environment, or prefer to manage dependencies explicitly.

**To check your Python version:**
```bash
python3 --version
```

**If Python is not installed:**
- Download from: https://www.python.org/downloads/
- On macOS: `brew install python3`
- On Ubuntu/Debian: `sudo apt-get install python3 python3-pip`
- On Windows: Use the Python Installer from python.org

<details>
<summary><b>DingTalk Configuration</b></summary>

**1. Create a DingTalk App**

- Go to [DingTalk Open Platform](https://open.dingtalk.com/)
- Create an app and get `client_id` and `client_secret`
- For detailed setup, see: [钉钉开发文档](https://developer.aliyun.com/article/1710350)

**2. Configure** (`~/.mofaclaw/config.json`)

```json
{
  "channels": {
    "dingtalk": {
      "enabled": true,
      "client_id": "YOUR_CLIENT_ID",
      "client_secret": "YOUR_CLIENT_SECRET"
    }
  }
}
```

**3. Run**

```bash
mofaclaw gateway
```

</details>

## ⚙️ Configuration

Config file: `~/.mofaclaw/config.json`

### Providers

> [!NOTE]
> Groq provides free voice transcription via Whisper. If configured, Telegram voice messages will be automatically transcribed.

| Provider | Purpose | Get API Key |
|----------|---------|-------------|
| `openrouter` | LLM (recommended, access to all models) | [openrouter.ai](https://openrouter.ai) |
| `anthropic` | LLM (Claude direct) | [console.anthropic.com](https://console.anthropic.com) |
| `openai` | LLM (GPT direct) | [platform.openai.com](https://platform.openai.com) |
| `groq` | LLM + **Voice transcription** (Whisper) | [console.groq.com](https://console.groq.com) |
| `gemini` | LLM (Gemini direct) | [aistudio.google.com](https://aistudio.google.com) |


<details>
<summary><b>Full config example</b></summary>

```json
{
  "agents": {
    "defaults": {
      "workspace": "~/.mofaclaw/workspace",
      "model": "glm-4.7-flash",
      "max_tokens": 16000,
      "temperature": 0.7,
      "max_tool_iterations": 5
    }
  },
  "channels": {
    "dingtalk": {
      "enabled": true,
      "client_id": "YOUR_CLIENT_ID",
      "client_secret": "YOUR_CLIENT_SECRET"
    },
    "feishu": {
      "enabled": false,
      "app_id": "",
      "app_secret": "",
      "encrypt_key": "",
      "verification_token": ""
    }
  },
  "providers": {
    "anthropic": {
      "api_key": ""
    },
    "openai": {
      "api_key": ""
    },
    "openrouter": {
      "api_key": ""
    },
    "zhipu": {
      "api_base": "https://open.bigmodel.cn/api/paas/v4",
      "api_key": ""
    },
    "vllm": {
      "api_key": ""
    },
    "gemini": {
      "api_key": ""
    },
    "groq": {
      "api_key": ""
    }
  },
  "gateway": {
    "host": "0.0.0.0",
    "port": 18790
  },
  "tools": {
    "web": {
      "search": {
        "api_key": "",
        "max_results": 0
      }
    },
    "transcription": {
      "groq_api_key": ""
    }
  }
}
```

</details>

## CLI Reference

| Command | Description |
|---------|-------------|
| `mofaclaw onboard` | Initialize config & workspace |
| `mofaclaw agent -m "..."` | Chat with the agent |
| `mofaclaw agent` | Interactive chat mode |
| `mofaclaw gateway` | Start the gateway |
| `mofaclaw status` | Show status |

<details>
<summary><b>Scheduled Tasks (Cron)</b></summary>

```bash
# Add a job
mofaclaw cron add --name "daily" --message "Good morning!" --cron "0 9 * * *"
mofaclaw cron add --name "hourly" --message "Check status" --every 3600

# List jobs
mofaclaw cron list

# Remove a job
mofaclaw cron remove <job_id>
```

</details>

## 🐳 Docker

> [!NOTE]
> Docker support coming soon for the Rust version. For now, please install from source.

```bash
# Coming soon: docker build -t mofaclaw .
# Coming soon: docker run -v ~/.mofaclaw:/root/.mofaclaw mofaclaw gateway
```

## 📁 Project Structure

```
mofaclaw/
├── core/           # 🧠 Core library (agent, tools, providers)
│   ├── agent/      #    Agent loop, context, memory, skills
│   ├── tools/      #    Built-in tools (filesystem, shell, web, spawn)
│   ├── provider/   #    LLM provider clients
│   ├── channels/   #    Channel integrations (DingTalk, Feishu)
│   ├── bus/        #    Message routing
│   ├── cron/       #    Scheduled tasks
│   └── heartbeat/  #    Proactive wake-up service
├── cli/            # 🖥️ Command-line interface
├── channels/       # 📱 Channel implementations
└── skills/         # 🎯 Bundled skills (github, weather, tmux...)
```