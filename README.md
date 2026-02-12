[中文](README_CN.md) | English

# aobot

Multi-channel AI gateway built on top of [pi-agent-rs](pi-agent-rs/) SDK.

Provides CLI chat, WebSocket JSON-RPC gateway, multi-agent management, an extensible channel plugin framework, and persistent storage.

## Features

- **CLI Chat** — Interactive terminal chat with configurable AI agents
- **WebSocket Gateway** — JSON-RPC 2.0 server over WebSocket with Bearer token auth
- **Multi-Agent Management** — Named agent configurations with different models, system prompts, and tools
- **Channel Plugin Framework** — Extensible integration point for external platforms (Telegram, Discord, etc.)
- **Config Hot-Reload** — Live configuration updates via `~/.aobot/config.json5` file watching
- **Persistent Storage** — SQLite-based session metadata persistence; sessions survive gateway restarts

## Workspace Structure

```
crates/
  aobot-types/      Shared types (AgentConfig, InboundMessage, OutboundMessage, etc.)
  aobot-config/     Configuration system (JSON5, .env, hot-reload)
  aobot-storage/    SQLite persistence for session metadata and channel bindings
  aobot-gateway/    WebSocket Gateway + JSON-RPC server + ChannelManager
  aobot-cli/        CLI binary (chat, gateway, send, health subcommands)
pi-agent-rs/        Git submodule: AI SDK (pi-agent-core, pi-agent-ai, pi-coding-agent)
```

## Storage Layout

```
~/.aobot/
  config.json5      Configuration file
  aobot.db          SQLite database (session metadata, channel bindings)
```

Message content is managed by pi-agent's JSONL persistence in `~/.pi/agent/sessions/`.

## JSON-RPC Methods

| Method | Description |
|--------|-------------|
| `health` | System health check |
| `chat.send` | Send message, get full response |
| `chat.stream` | Send message, get streaming response |
| `chat.history` | Get session chat history |
| `sessions.list` | List active sessions |
| `sessions.delete` | Delete a session |
| `agents.list` | List configured agents |
| `agents.add` | Add/update agent configuration |
| `agents.delete` | Delete agent configuration |
| `channels.list` | List registered channels |
| `channels.status` | Query channel status |
| `config.get` | Get current configuration |
| `config.set` | Update configuration |

## Quick Start

```bash
# Build
cargo build --workspace

# Run CLI chat
cargo run -- chat --model <model_id>

# Run gateway
cargo run -- gateway --port 3000
```

## Development

```bash
# Test
cargo test --workspace

# Lint
cargo clippy -p aobot-types -p aobot-config -p aobot-storage -p aobot-gateway -p aobot-cli

# Format
cargo fmt
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `MINIMAX_API_KEY` | API key for MiniMax provider (loaded from `.env`) |
| `RUST_LOG` | Tracing log level filter (default: `info`) |
