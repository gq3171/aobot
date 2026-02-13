中文 | [English](README.md)

# aobot

基于 [pi-agent-rs](pi-agent-rs/) SDK 构建的多通道 AI 网关。

提供 CLI 聊天、WebSocket JSON-RPC 网关、多 Agent 管理、可扩展的通道插件框架以及持久化存储。

## 功能特性

- **CLI 聊天** — 支持可配置 AI Agent 的交互式终端聊天
- **WebSocket 网关** — 基于 WebSocket 的 JSON-RPC 2.0 服务器，支持 Bearer Token 认证
- **多 Agent 管理** — 命名式 Agent 配置，支持不同模型、系统提示词和工具
- **通道插件框架** — 可扩展的外部平台集成接口（Telegram、Discord 等）
- **外部插件系统** — 通过子进程 + NDJSON JSON-RPC 协议在运行时加载通道插件；用任何语言编写插件，无需重新编译 aobot
- **Plugin SDK** — Rust crate（`aobot-plugin-sdk`），帮助快速构建外部通道插件
- **Telegram 通道** — 内置 Telegram 机器人集成，支持长轮询、消息分割和内联键盘
- **Discord 通道** — 内置 Discord 机器人集成
- **自动上下文压缩** — 基于 token 的压缩策略，结构化序列化与增量 LLM 摘要（对齐 pi-mono）
- **自动重试** — 对瞬态 API 错误（限流、5xx、网络错误）进行指数退避重试；上下文溢出通过压缩处理
- **配置热重载** — 通过监听 `~/.aobot/config.toml` 文件变化实时更新配置
- **持久化存储** — 基于 SQLite 的会话元数据持久化，会话可跨网关重启恢复

## 工作区结构

```
crates/
  aobot-types/               共享类型（AgentConfig、InboundMessage、OutboundMessage 等）
  aobot-config/              配置系统（TOML、.env、热重载）
  aobot-storage/             SQLite 持久化（会话元数据、通道绑定）
  aobot-gateway/             WebSocket 网关 + JSON-RPC 服务器 + ChannelManager + 外部插件宿主
  aobot-cli/                 CLI 二进制（chat、gateway、send、health 子命令）
  aobot-channel-telegram/    Telegram 通道插件（长轮询、消息分割、内联键盘）
  aobot-channel-discord/     Discord 通道插件
  aobot-plugin-sdk/          Plugin SDK，用于用 Rust 构建外部通道插件
pi-agent-rs/                 Git 子模块：AI SDK（pi-agent-core、pi-agent-ai、pi-coding-agent）
```

## 存储布局

```
~/.aobot/
  config.toml       配置文件
  aobot.db          SQLite 数据库（会话元数据、通道绑定）
```

消息内容由 pi-agent 的 JSONL 持久化管理，存储在 `~/.pi/agent/sessions/`。

## 外部插件系统

aobot 采用混合插件架构：

- **内置通道**（Telegram、Discord）通过可选 feature flag 编译进二进制
- **外部通道**作为独立进程运行，通过 stdin/stdout 的 NDJSON JSON-RPC 2.0 与 aobot 通信

这意味着你可以添加新的通道集成（飞书、Slack、WhatsApp 等）**而无需重新编译 aobot** — 只需提供一个插件可执行文件并在 `config.toml` 中配置即可。

### Feature Flags

```bash
# 默认构建（包含 Telegram + Discord）
cargo build -p aobot-cli

# 最小构建（不含内置通道，仅支持外部插件）
cargo build -p aobot-cli --no-default-features

# 仅 Telegram
cargo build -p aobot-cli --no-default-features --features channel-telegram
```

### 配置外部插件

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

### 编写插件（Rust）

添加 `aobot-plugin-sdk` 作为依赖：

```rust
use aobot_plugin_sdk::{PluginChannel, PluginContext, run_plugin};

struct MyChannel { /* ... */ }

#[async_trait::async_trait]
impl PluginChannel for MyChannel {
    fn channel_type(&self) -> &str { "my-channel" }
    async fn initialize(&mut self, channel_id: &str, config: &ChannelConfig) -> anyhow::Result<()> { Ok(()) }
    async fn start(&self, ctx: PluginContext) -> anyhow::Result<()> {
        // 监听消息，收到后调用 ctx.emit_inbound(message)
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

### 编写插件（任何语言）

插件是任何从 stdin 读取 NDJSON JSON-RPC 2.0 请求并向 stdout 写入响应/通知的可执行文件。完整协议规范参见 `crates/aobot-gateway/src/plugin_protocol.rs`。

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
cargo clippy -p aobot-types -p aobot-config -p aobot-storage -p aobot-gateway -p aobot-cli -p aobot-plugin-sdk

# 格式化
cargo fmt
```

## 环境变量

| 变量 | 说明 |
|------|------|
| `MINIMAX_API_KEY` | MiniMax 提供商的 API 密钥（从 `.env` 加载） |
| `RUST_LOG` | tracing 日志级别过滤（默认：`info`） |
