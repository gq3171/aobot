//! aobot-tools: Tool policy system, tool groups, and gateway tools.
//!
//! Provides:
//! - Tool profile and policy resolution (minimal/coding/messaging/full)
//! - Tool group definitions (fs, runtime, web, memory, sessions, messaging, etc.)
//! - Gateway tools that can operate on sessions, channels, and config
//! - Tool context for gateway tool access to shared state

pub mod context;
pub mod gateway_tool;
pub mod groups;
pub mod policy;
pub mod tools;
