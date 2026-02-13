//! aobot-hooks: Event-driven hook system.
//!
//! Hooks respond to gateway lifecycle events (startup, session start/end,
//! messages, tool calls) and can execute custom logic.

pub mod events;
pub mod registry;
