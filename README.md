[中文](README_CN.md) | English

# aobot

Multi-channel AI gateway built on top of [pi-agent-rs](pi-agent-rs/) SDK.

Provides CLI chat, WebSocket JSON-RPC gateway, multi-agent management, an extensible channel plugin framework, and persistent storage.

## Features

- **CLI Chat** — Interactive terminal chat with configurable AI agents
- **WebSocket Gateway** — JSON-RPC 2.0 server over WebSocket with Bearer token auth
- **Multi-Agent Management** — Named agent configurations with different models, system prompts, and tools
- **Channel Plugin Framework** — Extensible integration point for external platforms (Telegram, Discord, etc.)
- **External Plugin System** — Runtime-loadable channel plugins via subprocess + NDJSON JSON-RPC protocol; write plugins in any language without recompiling aobot
- **Plugin SDK** — Rust crate (`aobot-plugin-sdk`) for quickly building external channel plugins
- **Telegram Channel** — Built-in Telegram bot integration with long polling, message splitting, and inline keyboards
- **Discord Channel** — Built-in Discord bot integration
- **Automatic Context Compaction** — Token-based compaction with structured serialization and incremental LLM summarization (aligned with pi-mono)
- **Automatic Retry** — Exponential backoff for transient API errors (rate limits, 5xx, network errors); context overflow handled via compaction
- **Config Hot-Reload** — Live configuration updates via `~/.aobot/config.toml` file watching
- **Persistent Storage** — SQLite-based session metadata persistence; sessions survive gateway restarts

## Workspace Structure

```
crates/
  aobot-types/               Shared types (AgentConfig, InboundMessage, OutboundMessage, etc.)
  aobot-config/              Configuration system (TOML, .env, hot-reload)
  aobot-storage/             SQLite persistence for session metadata and channel bindings
  aobot-gateway/             WebSocket Gateway + JSON-RPC server + ChannelManager + External Plugin host
  aobot-cli/                 CLI binary (chat, gateway, send, health subcommands)
  aobot-channel-telegram/    Telegram channel plugin (long polling, message splitting, inline keyboards)
  aobot-channel-discord/     Discord channel plugin
  aobot-plugin-sdk/          Plugin SDK for building external channel plugins in Rust
pi-agent-rs/                 Git submodule: AI SDK (pi-agent-core, pi-agent-ai, pi-coding-agent)
```

## Storage Layout

```
~/.aobot/
  config.toml       Configuration file
  aobot.db          SQLite database (session metadata, channel bindings)
```

Message content is managed by pi-agent's JSONL persistence in `~/.pi/agent/sessions/`.

## External Plugin System

aobot supports a hybrid plugin architecture:

- **Built-in channels** (Telegram, Discord) are compiled into the binary via optional feature flags
- **External channels** run as separate processes, communicating with aobot over stdin/stdout using NDJSON JSON-RPC 2.0

This means you can add new channel integrations (Feishu/Lark, Slack, WhatsApp, etc.) **without recompiling aobot** — just provide a plugin executable and configure it in `config.toml`.

### Feature Flags

```bash
# Default build (includes Telegram + Discord)
cargo build -p aobot-cli

# Minimal build (no built-in channels, external plugins only)
cargo build -p aobot-cli --no-default-features

# Only Telegram
cargo build -p aobot-cli --no-default-features --features channel-telegram
```

### Configuring an External Plugin

```toml
[channels.my-feishu]
channel_type = "external"
enabled = true
agent = "default"

[channels.my-feishu.settings]
command = "/path/to/aobot-plugin-feishu"
args = []
env = { FEISHU_APP_ID = "cli_xxx", FEISHU_APP_SECRET = "xxx" }
plugin_channel_type = "feishu"
```

### Writing a Plugin (Rust)

Add `aobot-plugin-sdk` as a dependency:

```rust
use aobot_plugin_sdk::{PluginChannel, PluginContext, run_plugin};

struct MyChannel { /* ... */ }

#[async_trait::async_trait]
impl PluginChannel for MyChannel {
    fn channel_type(&self) -> &str { "my-channel" }
    async fn initialize(&mut self, channel_id: &str, config: &ChannelConfig) -> anyhow::Result<()> { Ok(()) }
    async fn start(&self, ctx: PluginContext) -> anyhow::Result<()> {
        // Listen for messages, then call ctx.emit_inbound(message)
        Ok(())
    }
    async fn stop(&self) -> anyhow::Result<()> { Ok(()) }
    async fn send(&self, message: OutboundMessage) -> anyhow::Result<()> { Ok(()) }
    fn status(&self) -> ChannelStatus { ChannelStatus::Running }
}

#[tokio::main]
async fn main() {
    run_plugin(MyChannel { /* ... */ }).await.unwrap();
}
```

### Writing a Plugin (Any Language)

A plugin is any executable that reads NDJSON JSON-RPC 2.0 requests from stdin and writes responses/notifications to stdout. See `crates/aobot-gateway/src/plugin_protocol.rs` for the full protocol specification.

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
cargo clippy -p aobot-types -p aobot-config -p aobot-storage -p aobot-gateway -p aobot-cli -p aobot-plugin-sdk

# Format
cargo fmt
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `MINIMAX_API_KEY` | API key for MiniMax provider (loaded from `.env`) |
| `RUST_LOG` | Tracing log level filter (default: `info`) |
