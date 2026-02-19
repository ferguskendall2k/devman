# DevMan 🔧

**Lightweight AI agent framework. Single Rust binary, ~5MB, no runtime dependencies.**

[![CI](https://github.com/ferguskendall2k/devman/actions/workflows/ci.yml/badge.svg)](https://github.com/ferguskendall2k/devman/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

DevMan wraps the Claude API into a self-contained agent that runs as a Telegram bot, CLI tool, or background service. It supports multiple scoped bots, per-task storage, sub-agent orchestration, and self-healing — all from one binary.

## Install

```bash
# From source
cargo install --path .

# Or use the install script (downloads latest release)
curl -fsSL https://raw.githubusercontent.com/ferguskendall2k/devman/main/install.sh | bash
```

## Quick Start

```bash
# First-run setup
devman init

# Interactive chat
devman chat

# Run a single task
devman run -m "refactor the auth module"

# Start as a service (Telegram bots + dashboard + cron)
devman serve
```

## Features

### 🤖 Multi-Bot Telegram

Run multiple Telegram bots from one process — a **manager bot** (full access) and any number of **scoped bots** (task-specific, shareable).

```toml
# ~/.config/devman/config.toml

[telegram]
allowed_users = [12345678]

# Scoped bot — only sees its assigned tasks
[[telegram.bots]]
name = "marketing"
bot_token = "111:AAA..."
allowed_users = [12345678]
tasks = ["marketing-research"]
default_model = "standard"
memory_access = "scoped"
max_tokens = 4096
max_turns = 20
```

Bots are managed via chat — tell the manager *"assign this bot to task X"* and it uses the `assign_bot` tool to write the config, create the task, and restart itself.

### 📁 Per-Task Scoped Storage

Each task gets isolated file storage under `.devman/memory/tasks/<slug>/storage/`. Sub-agents and scoped bots can only access their own task's files. The manager can see everything.

**Storage tools:** `storage_write` (text + base64 binary), `storage_read`, `storage_list`, `storage_delete`

### 🧠 Task-Based Memory

```
.devman/memory/
  INDEX.md              # Task registry
  tasks/
    marketing.md        # Task context, decisions, status
    marketing/storage/  # Task file storage
    harnessview.md
    ...
```

**Memory tools:** `memory_search`, `memory_read`, `memory_write`, `memory_load_task`, `memory_create_task`, `memory_update_index`

### 📎 File Handling

Send photos, documents, voice messages, audio, video, or stickers to any DevMan bot via Telegram. Files are downloaded automatically and passed to the agent with metadata (dimensions, duration, mime type).

### 🔧 Built-in Tools

| Category | Tools |
|----------|-------|
| **Files** | `read_file`, `write_file`, `edit_file`, `apply_patch` |
| **Shell** | `shell` (arbitrary commands) |
| **Git** | `git_status`, `git_diff`, `git_commit`, `git_push`, `git_log`, `git_branch` |
| **GitHub** | `github_pr_create`, `github_pr_list`, `github_issues_list`, `github_issue_create`, `github_actions_status` |
| **Web** | `web_search` (Brave API), `web_fetch`, `deep_research` |
| **Memory** | `memory_search`, `memory_read`, `memory_write`, `memory_load_task`, `memory_create_task`, `memory_update_index` |
| **Storage** | `storage_write`, `storage_read`, `storage_list`, `storage_delete` |
| **Bot mgmt** | `assign_bot`, `list_bots`, `remove_bot` |
| **Agents** | `spawn_agent`, `list_agents`, `kill_agent` |
| **Other** | `tts` (ElevenLabs), `self_improve` |

### 🏗️ Sub-Agent Orchestration

The manager triages messages and spawns sub-agents on the right model tier:

- **Quick** (Haiku) — status checks, simple lookups
- **Standard** (Sonnet) — code changes, writing, debugging
- **Complex** (Opus) — architecture, complex refactors, novel problems

Sub-agents get their own conversation state, scoped storage, and checkpoint/recovery.

### 🛡️ Self-Healing

DevMan is designed to recover from failures without intervention:

| Failure | Recovery |
|---------|----------|
| **Process crash / OOM** | Systemd auto-restart in 3 seconds |
| **OAuth token expired** | Re-reads `~/.claude/.credentials.json`, retries on 401 |
| **Telegram rate limit** | Detects 429, waits `retry_after` seconds, retries |
| **Network outage** | Exponential backoff (1s → 2s → 4s → ... → 60s max) |
| **Context too long** | 3-layer auto-compaction (pre-turn, in-loop, error recovery) |
| **Orphaned tool blocks** | Compaction drops all tool blocks, rebuilds as text summary |
| **Crash during file write** | Atomic writes (temp + rename) prevent corruption |
| **Disk space low** | Warning on startup if <500MB free |
| **Memory ceiling** | Systemd MemoryMax=2GB — kills DevMan, not your other apps |

### 🌐 Web Dashboard

Built into the binary — no separate app. Start with `devman serve` and visit `http://localhost:18790`.

- Live conversation view
- Cost tracking per model and task
- WebSocket real-time updates

### ⏰ Cron Scheduler

Schedule recurring agent tasks:

```bash
devman cron add --name "daily-standup" --schedule "0 9 * * *" --message "Check git log and summarize yesterday's work"
devman cron list
```

## Configuration

### Config: `~/.config/devman/config.toml`

```toml
[models]
manager = "claude-haiku-4-5-20250512"     # triage (cheap, fast)
quick = "claude-haiku-4-5-20250512"       # simple lookups
standard = "claude-sonnet-4-20250514"     # default worker
complex = "claude-opus-4-20250414"        # architecture decisions

[tools]
shell_confirm = false
web_enabled = true

[agents]
max_concurrent = 5
max_turns = 50
max_tokens = 16384
recovery = "report"
checkpoint_interval = 1

[telegram]
allowed_users = [12345678]

[[telegram.bots]]
name = "dev"
bot_token = "111:AAA..."
allowed_users = [12345678]
tasks = ["my-project"]
default_model = "standard"
memory_access = "scoped"
system_prompt = "You are a dev assistant. Be concise."
max_tokens = 4096
max_turns = 20

[dashboard]
enabled = true
port = 18790
bind = "127.0.0.1"

[vault]
enabled = true

[logging]
level = "info"
```

### Credentials: `~/.config/devman/credentials.toml`

```toml
[telegram]
bot_token = "000:MGR..."       # manager bot token
allowed_users = [12345678]

[brave]
api_key = "BSA..."

[elevenlabs]
api_key = "..."
voice_id = "pFZP5JQG7iQjIQuC4Bku"

[github]
token = "ghp_..."
```

### Authentication

DevMan supports three auth methods (in priority order):

1. **Claude Code CLI OAuth** — reads `~/.claude/.credentials.json` automatically. Uses your Claude Pro/Max subscription. Auto-refreshes on expiry.
2. **Environment variable** — `ANTHROPIC_API_KEY`
3. **credentials.toml** — manual API key

## Running as a Service

Install the systemd service for auto-restart and memory protection:

```bash
sudo cp devman.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now devman
```

Or use the install script which offers to set this up:

```bash
curl -fsSL https://raw.githubusercontent.com/ferguskendall2k/devman/main/install.sh | bash
```

**Service features:**
- Auto-restart on crash (3 second delay)
- Memory ceiling: 2GB hard limit, 1.5GB soft limit
- Runs as your user (not root)
- Logs via journald: `journalctl -u devman -f`

## Architecture

```
                        ┌─────────────────────────┐
                        │      CLI (clap)          │
                        │  chat · run · serve      │
                        └────────────┬────────────┘
                                     │
              ┌──────────────────────┼──────────────────────┐
              │                      │                      │
     ┌────────▼────────┐  ┌─────────▼─────────┐  ┌────────▼────────┐
     │    Manager       │  │   Telegram Bots    │  │    Dashboard    │
     │  (triage/route)  │  │  (multi-bot poll)  │  │  (Axum + WS)   │
     └────────┬────────┘  └─────────┬─────────┘  └─────────────────┘
              │                     │
     ┌────────▼─────────────────────▼────────┐
     │            Agent Loop                  │
     │  (stream → tools → result → repeat)   │
     └────────┬──────────────────────────────┘
              │
    ┌─────────┼──────────┬───────────────┐
    │         │          │               │
┌───▼───┐ ┌──▼───┐ ┌────▼────┐ ┌───────▼───────┐
│ Tools │ │Memory│ │ Storage │ │ Orchestrator  │
│ 25+   │ │      │ │ (scoped)│ │ (sub-agents)  │
└───────┘ └──────┘ └─────────┘ └───────────────┘
```

## DevMan vs OpenClaw

| | DevMan | OpenClaw |
|---|--------|----------|
| **Binary** | ~5 MB Rust | Node.js platform |
| **Setup** | Download + `devman init` | Gateway + skills + config |
| **Bots** | Multi-bot Telegram | Telegram, Discord, WhatsApp, Signal, etc. |
| **Focus** | Developer agent, task-scoped | Multi-channel AI assistant |
| **Tools** | Built-in + scripts | Skill packages |
| **Storage** | Per-task scoped | Workspace-level |
| **Ideal for** | Solo devs, task bots, CI | Teams, multi-device, always-on |

## Project Structure

```
src/
  agent.rs         # Core agent loop (stream → tools → repeat)
  client.rs        # Anthropic API client (SSE, OAuth, retry)
  config.rs        # TOML configuration
  context.rs       # Conversation history + compaction
  cost.rs          # Token cost tracking
  cron.rs          # Cron scheduler
  manager.rs       # Manager agent (triage + orchestration)
  memory.rs        # Task memory + scoped storage
  orchestrator.rs  # Sub-agent pool
  auth.rs          # Multi-source credential resolution
  cli/
    chat.rs        # Interactive REPL
    run.rs         # Single-shot task
    serve.rs       # Daemon (Telegram + cron + dashboard)
    init.rs        # First-run setup
  tools/
    mod.rs         # Tool router (25+ tools)
    storage.rs     # Per-task file storage
    bot_management.rs  # assign/list/remove bots
    ...
  telegram/
    api.rs         # Telegram Bot API (polling, files, rate limits)
    types.rs       # Message types (photo, doc, voice, video, sticker)
  dashboard/
    mod.rs         # Axum HTTP + WebSocket server
    api.rs         # REST endpoints
    ws.rs          # Real-time updates
```

## License

[MIT](LICENSE) © 2026 Fergus Kendall
