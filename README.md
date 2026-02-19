# DevMan 🔧

Lightweight agentic framework for Claude. Single Rust binary, no runtime dependencies.

## Quick Start

```bash
# Build
cargo build --release  # → 3.7MB binary

# Setup
devman init

# Chat
devman chat

# Run a single task
devman run -m "find all TODO comments in this project"
```

## Features

- **Agent loop** — prompt → tool → result → repeat, with streaming
- **Built-in tools** — shell, read/write/edit files, web search, web fetch
- **Claude Code OAuth** — uses your claude.ai subscription (no separate API costs)
- **Context management** — automatic compaction when approaching limits
- **Conversation persistence** — picks up where you left off
- **3.7MB binary** — no Node.js, no Python, no runtime

## Architecture

```
devman
├── src/
│   ├── main.rs          # CLI (clap)
│   ├── agent.rs         # Core agent loop
│   ├── client.rs        # Anthropic API + SSE streaming
│   ├── context.rs       # Conversation manager + compaction
│   ├── config.rs        # TOML config
│   ├── auth.rs          # Credential resolution (env → Claude Code → file)
│   ├── tools/           # Built-in tools
│   │   ├── shell.rs     # Shell execution
│   │   ├── read.rs      # File reading
│   │   ├── write.rs     # File writing
│   │   ├── edit.rs      # Search-and-replace
│   │   ├── web_search.rs # Brave Search
│   │   └── web_fetch.rs # URL → text
│   └── cli/             # CLI commands
│       ├── chat.rs      # Interactive REPL
│       ├── run.rs       # Single-task mode
│       └── init.rs      # Guided setup
└── Cargo.toml
```

## Roadmap

- [ ] Manager + sub-agent orchestration (Haiku triage → Sonnet/Opus workers)
- [ ] Task-based memory system
- [ ] Telegram integration
- [ ] Cron scheduler
- [ ] Git/GitHub tools
- [ ] Voice (ElevenLabs TTS/STT)
- [ ] Web dashboard
- [ ] Deep research engine
- [ ] Self-improvement engine
