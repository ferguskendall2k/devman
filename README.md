# DevMan 🔧

**Lightweight agentic framework for Claude. Single Rust binary, no runtime.**

[![CI](https://github.com/fergusk96/DevMan/actions/workflows/ci.yml/badge.svg)](https://github.com/fergusk96/DevMan/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Binary Size](https://img.shields.io/badge/binary-~5MB-green)](https://github.com/fergusk96/DevMan/releases)

---

DevMan is a minimal, self-contained agent framework that wraps the Claude API into a single compiled binary. No Python, no Node, no Docker — just download and run.

## Quick Start

```bash
# Install from source
cargo install --path .

# First-run setup (creates ~/.devman/config.toml)
devman init

# Interactive chat
devman chat

# Run a single task
devman run -m "refactor the auth module"

# Start as a daemon with Telegram bot + dashboard
devman serve
```

## Features

- **Interactive REPL** — streaming chat with full conversation context
- **Single-shot tasks** — `devman run -m "..."` for scripting and CI
- **Tool system** — built-in tools + custom script-based tools (any language)
- **Telegram bot** — long-running agent accessible from your phone
- **Web dashboard** — real-time cost tracking, conversation history, cron management
- **Cost tracking** — per-model token accounting with daily/monthly summaries
- **Cron scheduler** — recurring agent tasks with cron expressions
- **Memory** — persistent context across sessions
- **Voice** — TTS integration for audio responses
- **Self-improvement** — `devman improve` for agent-driven code suggestions

## Architecture

```
┌──────────────────────────────────────────────┐
│                   CLI (clap)                 │
│  chat · run · init · serve · cost · cron     │
└──────────┬───────────────────────┬───────────┘
           │                       │
     ┌─────▼─────┐         ┌──────▼──────┐
     │   Agent    │         │  Telegram   │
     │ Orchestr.  │         │    Bot      │
     └─────┬─────┘         └──────┬──────┘
           │                       │
     ┌─────▼───────────────────────▼─────┐
     │         Claude API Client         │
     │     (streaming, tool calls)       │
     └─────┬─────────────────────┬───────┘
           │                     │
     ┌─────▼─────┐       ┌──────▼──────┐
     │   Tools    │       │  Dashboard  │
     │ built-in + │       │  (axum +    │
     │  scripts   │       │   WebSocket)│
     └───────────┘       └─────────────┘
```

## CLI Reference

| Command | Description | Example |
|---------|-------------|---------|
| `init` | Guided first-run setup | `devman init` |
| `chat` | Interactive REPL | `devman chat` |
| `run` | Execute a single task | `devman run -m "fix the tests"` |
| `serve` | Start daemon (Telegram + dashboard) | `devman serve` |
| `auth` | Show API key status | `devman auth` |
| `cost` | Token usage summary | `devman cost` |
| `cron` | Manage scheduled tasks | `devman cron list` |

## Configuration

DevMan stores its config at `~/.devman/config.toml`:

```toml
[api]
key = "sk-ant-..."
model = "claude-sonnet-4-20250514"

[agent]
max_turns = 25
system_prompt = "You are a helpful coding assistant."

[telegram]
bot_token = "123456:ABC..."
allowed_users = [12345678]

[dashboard]
port = 3000
bind = "127.0.0.1"

[cost]
daily_limit = 5.00
```

## Custom Tools

Create executable scripts in `~/.devman/tools/`:

```bash
#!/bin/bash
# ~/.devman/tools/deploy.sh
# TOOL_DESC: Deploy the current branch to staging
# TOOL_PARAM: environment: Target environment (staging|production)

echo "Deploying to $environment..."
git push origin HEAD:deploy-$environment
```

Any executable file with `TOOL_DESC` becomes available to the agent automatically.

## Dashboard

Start with `devman serve` and visit `http://localhost:3000`.

The dashboard provides:
- **Live conversation view** — watch the agent think in real-time
- **Cost graphs** — daily and monthly token spend
- **Cron management** — create, edit, and monitor scheduled tasks
- **Tool history** — full audit log of every tool invocation

## DevMan vs OpenClaw

| | DevMan | OpenClaw |
|---|--------|----------|
| **Runtime** | Single binary | Node.js + plugins |
| **Setup** | `cargo install` + one config | Gateway daemon + skills |
| **Focus** | Developer-first CLI agent | Multi-channel assistant platform |
| **Tools** | Script-based, zero config | Skill packages with schemas |
| **Channels** | Telegram + CLI | Discord, Telegram, WhatsApp, etc. |
| **Size** | ~5 MB | Full platform |
| **Ideal for** | Solo devs, CI, quick tasks | Teams, multi-device, always-on |

## License

[MIT](LICENSE) © 2026 Fergus Kendall
