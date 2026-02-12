# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

aobot is a Rust workspace (edition 2024) — a multi-channel AI gateway built on top of `pi-agent-rs` SDK. It provides CLI chat, WebSocket JSON-RPC gateway, multi-agent management, and an extensible channel plugin framework.

## Workspace Structure

- `crates/aobot-types` — Shared types (AgentConfig, InboundMessage, OutboundMessage, ChannelPlugin types)
- `crates/aobot-config` — Configuration system (TOML, dotenvy .env, hot-reload support)
- `crates/aobot-storage` — SQLite persistence for session metadata and channel bindings
- `crates/aobot-gateway` — WebSocket Gateway + JSON-RPC server + ChannelManager
- `crates/aobot-cli` — CLI binary (chat, gateway, send, health subcommands)
- `crates/aobot-channel-telegram` — Telegram channel plugin (long polling, message splitting, inline keyboards)
- `pi-agent-rs/` — Git submodule: AI SDK (pi-agent-core, pi-agent-ai, pi-coding-agent including retry module)

## Key Architecture

- **StreamFnBox bridge**: `aobot-cli` and `aobot-gateway` both create `StreamFnBox` (from pi-agent-core) that wraps `stream_simple()` (from pi-agent-ai) with `create_default_registry()` to connect `AgentSession` to LLM providers.
- **JSON-RPC 2.0 over WebSocket**: Gateway uses axum + WebSocket upgrade. Methods: `health`, `chat.send`, `chat.stream`, `chat.history`, `sessions.*`, `agents.*`, `channels.*`, `config.*`.
- **ChannelPlugin trait**: Extension point in `aobot-gateway::channel` for external platforms (Telegram, Discord, etc.). `ChannelManager` routes `InboundMessage` → agent → `OutboundMessage`.
- **Config hot-reload**: `notify` crate watches `~/.aobot/config.toml`, auto-applies changes to `GatewaySessionManager`.
- **Persistent Storage**: `aobot-storage` uses SQLite (`~/.aobot/aobot.db`) for session metadata and channel bindings. Message content is managed by pi-agent's JSONL persistence. `GatewaySessionManager` restores active sessions on startup.
- **Compaction & Incremental Summary**: pi-coding-agent's compaction system uses structured serialization (`[User]`, `[Assistant]`, `[Assistant thinking]`, `[Assistant tool calls]`, `[Tool result]`) and supports incremental summarization via `previous_summary`. Summary output is wrapped in `<summary>` tags. Prompt templates align with pi-mono.
- **Automatic Retry**: pi-coding-agent's `retry` module provides exponential backoff for transient API errors (rate limits, 5xx, network errors). Context overflow errors are excluded from retry and handled via compaction instead. Configured via `AoBotConfig.retry`.
- **Custom models**: `~/.pi/agent/models.json` for custom LLM model definitions (e.g. MiniMax CN domain override).

## Build & Development Commands

- **Build:** `cargo build --workspace`
- **Test all:** `cargo test --workspace`
- **Test single crate:** `cargo test -p aobot-gateway`
- **Test single test:** `cargo test <test_name>`
- **Lint:** `cargo clippy -p aobot-types -p aobot-config -p aobot-storage -p aobot-gateway -p aobot-cli`
- **Format:** `cargo fmt`
- **Format check:** `cargo fmt -- --check`
- **Run CLI chat:** `cargo run -- chat --model <model_id>`
- **Run gateway:** `cargo run -- gateway --port 3000`

## Environment

- `MINIMAX_API_KEY` — API key for MiniMax provider (loaded from `.env`)
- `RUST_LOG` — tracing log level filter (default: `info`)
