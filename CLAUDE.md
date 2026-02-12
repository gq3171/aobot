# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

aobot is a Rust workspace (edition 2024) — a multi-channel AI gateway built on top of `pi-agent-rs` SDK. It provides CLI chat, WebSocket JSON-RPC gateway, multi-agent management, and an extensible channel plugin framework.

## Workspace Structure

- `crates/aobot-types` — Shared types (AgentConfig, InboundMessage, OutboundMessage, ChannelPlugin types)
- `crates/aobot-config` — Configuration system (JSON5, dotenvy .env, hot-reload support)
- `crates/aobot-gateway` — WebSocket Gateway + JSON-RPC server + ChannelManager
- `crates/aobot-cli` — CLI binary (chat, gateway, send, health subcommands)
- `pi-agent-rs/` — Git submodule: AI SDK (pi-agent-core, pi-agent-ai, pi-coding-agent)

## Key Architecture

- **StreamFnBox bridge**: `aobot-cli` and `aobot-gateway` both create `StreamFnBox` (from pi-agent-core) that wraps `stream_simple()` (from pi-agent-ai) with `create_default_registry()` to connect `AgentSession` to LLM providers.
- **JSON-RPC 2.0 over WebSocket**: Gateway uses axum + WebSocket upgrade. Methods: `health`, `chat.send`, `chat.stream`, `chat.history`, `sessions.*`, `agents.*`, `channels.*`, `config.*`.
- **ChannelPlugin trait**: Extension point in `aobot-gateway::channel` for external platforms (Telegram, Discord, etc.). `ChannelManager` routes `InboundMessage` → agent → `OutboundMessage`.
- **Config hot-reload**: `notify` crate watches `~/.aobot/config.json5`, auto-applies changes to `GatewaySessionManager`.
- **Custom models**: `~/.pi/agent/models.json` for custom LLM model definitions (e.g. MiniMax CN domain override).

## Build & Development Commands

- **Build:** `cargo build --workspace`
- **Test all:** `cargo test --workspace`
- **Test single crate:** `cargo test -p aobot-gateway`
- **Test single test:** `cargo test <test_name>`
- **Lint:** `cargo clippy -p aobot-config -p aobot-gateway -p aobot-cli -p aobot-types`
- **Format:** `cargo fmt`
- **Format check:** `cargo fmt -- --check`
- **Run CLI chat:** `cargo run -- chat --model <model_id>`
- **Run gateway:** `cargo run -- gateway --port 3000`

## Environment

- `MINIMAX_API_KEY` — API key for MiniMax provider (loaded from `.env`)
- `RUST_LOG` — tracing log level filter (default: `info`)
