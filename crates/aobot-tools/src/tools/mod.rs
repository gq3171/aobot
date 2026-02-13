//! Gateway tool implementations.
//!
//! Each tool implements `AgentTool` and uses `GatewayToolContext` for
//! access to sessions, channels, and configuration.

pub mod agents_list;
pub mod cron;
pub mod exec;
pub mod gateway;
pub mod image;
pub mod memory_get;
pub mod memory_search;
pub mod message;
pub mod process;
pub mod session_status;
pub mod sessions_history;
pub mod sessions_list;
pub mod sessions_send;
pub mod sessions_spawn;
pub mod tts;

use std::collections::HashMap;
use std::sync::Arc;

use pi_agent_core::agent_types::AgentTool;

use crate::context::GatewayToolContext;

/// Create all gateway tools, keyed by tool name.
pub fn create_gateway_tools(ctx: Arc<GatewayToolContext>) -> HashMap<String, Arc<dyn AgentTool>> {
    let tools: Vec<Arc<dyn AgentTool>> = vec![
        Arc::new(sessions_list::SessionsListTool::new(ctx.clone())),
        Arc::new(sessions_history::SessionsHistoryTool::new(ctx.clone())),
        Arc::new(sessions_send::SessionsSendTool::new(ctx.clone())),
        Arc::new(sessions_spawn::SessionsSpawnTool::new(ctx.clone())),
        Arc::new(session_status::SessionStatusTool::new(ctx.clone())),
        Arc::new(agents_list::AgentsListTool::new(ctx.clone())),
        Arc::new(gateway::GatewayConfigTool::new(ctx.clone())),
        Arc::new(message::MessageTool::new(ctx.clone())),
        Arc::new(image::ImageTool::new(ctx.clone())),
        Arc::new(memory_search::MemorySearchTool::new(ctx.clone())),
        Arc::new(memory_get::MemoryGetTool::new(ctx.clone())),
        Arc::new(process::ProcessTool::new(ctx.clone())),
        Arc::new(exec::ExecTool::new(ctx.clone())),
        Arc::new(tts::TtsTool::new(ctx.clone())),
        Arc::new(cron::CronTool::new(ctx)),
    ];

    tools
        .into_iter()
        .map(|t| (t.name().to_string(), t))
        .collect()
}
