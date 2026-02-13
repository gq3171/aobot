//! Gateway tool context — shared state available to gateway tools.

use std::sync::Arc;
use tokio::sync::RwLock;

use aobot_config::AoBotConfig;

/// Shared context that gateway tools use to access the gateway system.
///
/// This is passed to gateway tools at construction time so they can
/// interact with sessions, channels, and configuration.
#[derive(Clone)]
pub struct GatewayToolContext {
    /// Current session key (the session this tool invocation belongs to).
    pub current_session_key: String,
    /// Current agent ID.
    pub current_agent_id: String,
    /// Live configuration (hot-reloadable).
    pub config: Arc<RwLock<AoBotConfig>>,
    /// Sender for dispatching gateway operations.
    /// Gateway tools send `GatewayOp` commands through this channel,
    /// which the gateway loop processes against the real SessionManager/ChannelManager.
    pub ops_tx: tokio::sync::mpsc::UnboundedSender<GatewayOp>,
}

/// Operations that gateway tools can request.
#[derive(Debug)]
pub enum GatewayOp {
    /// List all sessions.
    ListSessions {
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Get history for a session.
    GetHistory {
        session_key: String,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Send a message to a session.
    SendMessage {
        session_key: String,
        message: String,
        agent: Option<String>,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Spawn a sub-agent session.
    SpawnSession {
        task: String,
        agent_id: Option<String>,
        label: Option<String>,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Send an outbound message through a channel.
    ChannelSend {
        channel_id: String,
        recipient_id: String,
        text: String,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// List all agents.
    ListAgents {
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Get current config.
    GetConfig {
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Patch config.
    PatchConfig {
        patch: serde_json::Value,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Search memory.
    MemorySearch {
        query: String,
        max_results: usize,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Get memory content by path and line range.
    MemoryGet {
        path: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// List cron jobs.
    CronList {
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Add a cron job.
    CronAdd {
        schedule: String,
        task: String,
        agent_id: Option<String>,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Remove a cron job.
    CronRemove {
        job_id: String,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Update a cron job (enable/disable).
    CronUpdate {
        job_id: String,
        enabled: Option<bool>,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
    /// Run a cron job immediately.
    CronRun {
        job_id: String,
        reply: tokio::sync::oneshot::Sender<GatewayOpResult>,
    },
}

/// Results from gateway operations.
#[derive(Debug)]
pub enum GatewayOpResult {
    Json(serde_json::Value),
    Text(String),
    Error(String),
}
