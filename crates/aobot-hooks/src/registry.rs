//! Hook registry — manages hook subscriptions and dispatches events.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::events::HookEvent;

/// Async hook handler function type.
pub type HookHandler =
    Arc<dyn Fn(HookEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Manages hook subscriptions and dispatches events.
pub struct HookRegistry {
    /// Map from event type name to handlers.
    handlers: RwLock<HashMap<String, Vec<HookHandler>>>,
}

impl HookRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a handler for a specific event type.
    pub async fn on(&self, event_type: &str, handler: HookHandler) {
        let mut handlers = self.handlers.write().await;
        handlers
            .entry(event_type.to_string())
            .or_default()
            .push(handler);
    }

    /// Register a handler for multiple event types.
    pub async fn on_many(&self, event_types: &[&str], handler: HookHandler) {
        for event_type in event_types {
            self.on(event_type, handler.clone()).await;
        }
    }

    /// Dispatch an event to all registered handlers.
    pub async fn emit(&self, event: HookEvent) {
        let event_type = event_type_name(&event);
        let handlers = self.handlers.read().await;

        if let Some(handler_list) = handlers.get(event_type) {
            for handler in handler_list {
                let event_clone = event.clone();
                let handler = handler.clone();
                tokio::spawn(async move {
                    handler(event_clone).await;
                });
            }
        }

        // Also dispatch to "*" (wildcard) handlers
        if let Some(wildcard_handlers) = handlers.get("*") {
            for handler in wildcard_handlers {
                let event_clone = event.clone();
                let handler = handler.clone();
                tokio::spawn(async move {
                    handler(event_clone).await;
                });
            }
        }
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the event type name for routing.
fn event_type_name(event: &HookEvent) -> &'static str {
    match event {
        HookEvent::GatewayStartup => "gateway_startup",
        HookEvent::GatewayShutdown => "gateway_shutdown",
        HookEvent::SessionStart { .. } => "session_start",
        HookEvent::SessionEnd { .. } => "session_end",
        HookEvent::CommandNew { .. } => "command_new",
        HookEvent::CommandHelp { .. } => "command_help",
        HookEvent::MessageReceived { .. } => "message_received",
        HookEvent::MessageSending { .. } => "message_sending",
        HookEvent::ToolCallBefore { .. } => "tool_call_before",
        HookEvent::ToolCallAfter { .. } => "tool_call_after",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_emit_event() {
        let registry = HookRegistry::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c = counter.clone();
        registry
            .on(
                "gateway_startup",
                Arc::new(move |_event| {
                    let c = c.clone();
                    Box::pin(async move {
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                }),
            )
            .await;

        registry.emit(HookEvent::GatewayStartup).await;
        // Give the spawned task time to run
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_wildcard_handler() {
        let registry = HookRegistry::new();
        let counter = Arc::new(AtomicU32::new(0));

        let c = counter.clone();
        registry
            .on(
                "*",
                Arc::new(move |_event| {
                    let c = c.clone();
                    Box::pin(async move {
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                }),
            )
            .await;

        registry.emit(HookEvent::GatewayStartup).await;
        registry.emit(HookEvent::GatewayShutdown).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
