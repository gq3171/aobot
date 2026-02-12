中文 | [English](README.md)

# aobot

基于 [pi-agent-rs](pi-agent-rs/) SDK 构建的多通道 AI 网关。

提供 CLI 聊天、WebSocket JSON-RPC 网关、多 Agent 管理、可扩展的通道插件框架以及持久化存储。

## 功能特性

- **CLI 聊天** — 支持可配置 AI Agent 的交互式终端聊天
- **WebSocket 网关** — 基于 WebSocket 的 JSON-RPC 2.0 服务器，支持 Bearer Token 认证
- **多 Agent 管理** — 命名式 Agent 配置，支持不同模型、系统提示词和工具
- **通道插件框架** — 可扩展的外部平台集成接口（Telegram、Discord 等）
- **Telegram 通道** — 内置 Telegram 机器人集成，支持长轮询、消息分割和内联键盘
- **自动上下文压缩** — 基于 token 的压缩策略，结构化序列化与增量 LLM 摘要（对齐 pi-mono）
- **自动重试** — 对瞬态 API 错误（限流、5xx、网络错误）进行指数退避重试；上下文溢出通过压缩处理
- **配置热重载** — 通过监听 `~/.aobot/config.toml` 文件变化实时更新配置
- **持久化存储** — 基于 SQLite 的会话元数据持久化，会话可跨网关重启恢复

## 工作区结构

```
crates/
  aobot-types/      共享类型（AgentConfig、InboundMessage、OutboundMessage 等）
  aobot-config/     配置系统（TOML、.env、热重载）
  aobot-storage/    SQLite 持久化（会话元数据、通道绑定）
  aobot-gateway/    WebSocket 网关 + JSON-RPC 服务器 + ChannelManager
  aobot-cli/        CLI 二进制（chat、gateway、send、health 子命令）
  aobot-channel-telegram/  Telegram 通道插件（长轮询、消息分割、内联键盘）
pi-agent-rs/        Git 子模块：AI SDK（pi-agent-core、pi-agent-ai、pi-coding-agent）
```

## 存储布局

```
~/.aobot/
  config.toml       配置文件
  aobot.db          SQLite 数据库（会话元数据、通道绑定）
```

消息内容由 pi-agent 的 JSONL 持久化管理，存储在 `~/.pi/agent/sessions/`。

## JSON-RPC 方法

| 方法 | 说明 |
|------|------|
| `health` | 系统健康检查 |
| `chat.send` | 发送消息，获取完整响应 |
| `chat.stream` | 发送消息，获取流式响应 |
| `chat.history` | 获取会话聊天历史 |
| `sessions.list` | 列出活跃会话 |
| `sessions.delete` | 删除会话 |
| `agents.list` | 列出已配置的 Agent |
| `agents.add` | 添加/更新 Agent 配置 |
| `agents.delete` | 删除 Agent 配置 |
| `channels.list` | 列出已注册的通道 |
| `channels.status` | 查询通道状态 |
| `config.get` | 获取当前配置 |
| `config.set` | 更新配置 |

## 快速开始

```bash
# 构建
cargo build --workspace

# 运行 CLI 聊天
cargo run -- chat --model <model_id>

# 运行网关
cargo run -- gateway --port 3000
```

## 开发

```bash
# 测试
cargo test --workspace

# 代码检查
cargo clippy -p aobot-types -p aobot-config -p aobot-storage -p aobot-gateway -p aobot-cli

# 格式化
cargo fmt
```

## 环境变量

| 变量 | 说明 |
|------|------|
| `MINIMAX_API_KEY` | MiniMax 提供商的 API 密钥（从 `.env` 加载） |
| `RUST_LOG` | tracing 日志级别过滤（默认：`info`） |
