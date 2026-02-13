//! Gateway tool trait — tools that need access to the gateway context.

use crate::context::GatewayToolContext;
use std::sync::Arc;

/// Trait for tools that need access to gateway state.
///
/// Gateway tools implement both `AgentTool` (from pi-agent-core) and this trait.
/// The context is set after construction, before the tool is used.
pub trait GatewayTool: pi_agent_core::agent_types::AgentTool {
    /// Set the gateway context for this tool.
    fn set_context(&self, ctx: Arc<GatewayToolContext>);
}
