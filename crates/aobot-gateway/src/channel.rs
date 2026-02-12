//! Channel plugin framework for external platform integrations.
//!
//! This module provides the extensibility layer for connecting external messaging
//! platforms (Telegram, Discord, Web API, etc.) to the aobot gateway.
//!
//! # Architecture
//!
//! ```text
//! External Platform
//!     ↓ (platform-specific protocol)
//! ChannelPlugin::start() spawns listener
//!     ↓ (InboundMessage via mpsc)
//! ChannelManager → GatewaySessionManager.send_message()
//!     ↓ (response text)
//! ChannelPlugin::send(OutboundMessage)
//!     ↓ (platform-specific protocol)
//! External Platform
//! ```
//!
//! # Implementing a Channel
//!
//! ```rust,ignore
//! use aobot_gateway::channel::ChannelPlugin;
//!
//! struct TelegramChannel { /* ... */ }
//!
//! #[async_trait::async_trait]
//! impl ChannelPlugin for TelegramChannel {
//!     fn channel_type(&self) -> &str { "telegram" }
//!     fn channel_id(&self) -> &str { &self.id }
//!     async fn start(&self, sender: mpsc::Sender<InboundMessage>) -> anyhow::Result<()> { /* ... */ }
//!     async fn stop(&self) -> anyhow::Result<()> { /* ... */ }
//!     async fn send(&self, message: OutboundMessage) -> anyhow::Result<()> { /* ... */ }
//!     fn status(&self) -> ChannelStatus { /* ... */ }
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

use aobot_types::{ChannelInfo, ChannelStatus, InboundMessage, OutboundMessage};

use crate::session_manager::GatewaySessionManager;

/// Trait for channel plugins that bridge external platforms to the gateway.
///
/// Implementors should handle platform-specific protocol details and convert
/// messages to/from the gateway's `InboundMessage`/`OutboundMessage` types.
///
/// Use `&self` for all methods — implementations should use interior mutability
/// (e.g. `Mutex`, `RwLock`) for any mutable state.
#[async_trait::async_trait]
pub trait ChannelPlugin: Send + Sync {
    /// Returns the channel type identifier (e.g. "telegram", "discord").
    fn channel_type(&self) -> &str;

    /// Returns the unique instance identifier for this channel.
    fn channel_id(&self) -> &str;

    /// Start the channel, connecting to the external platform.
    ///
    /// The `sender` should be used to push incoming messages to the gateway.
    /// Implementations typically spawn a background task for the listener.
    async fn start(&self, sender: mpsc::Sender<InboundMessage>) -> anyhow::Result<()>;

    /// Stop the channel, disconnecting from the external platform.
    async fn stop(&self) -> anyhow::Result<()>;

    /// Send a message to the external platform.
    async fn send(&self, message: OutboundMessage) -> anyhow::Result<()>;

    /// Returns the current status of this channel.
    fn status(&self) -> ChannelStatus;
}

/// Manages multiple channel plugins, routing messages between channels and agents.
pub struct ChannelManager {
    channels: RwLock<HashMap<String, Arc<dyn ChannelPlugin>>>,
    inbound_tx: mpsc::Sender<InboundMessage>,
    inbound_rx: tokio::sync::Mutex<mpsc::Receiver<InboundMessage>>,
}

impl ChannelManager {
    /// Create a new channel manager with the given buffer capacity.
    pub fn new(buffer_size: usize) -> Self {
        let (tx, rx) = mpsc::channel(buffer_size);
        Self {
            channels: RwLock::new(HashMap::new()),
            inbound_tx: tx,
            inbound_rx: tokio::sync::Mutex::new(rx),
        }
    }

    /// Register a channel plugin. Replaces any existing channel with the same ID.
    pub async fn register(&self, channel: Arc<dyn ChannelPlugin>) {
        let id = channel.channel_id().to_string();
        info!(
            channel_type = channel.channel_type(),
            channel_id = %id,
            "Registering channel plugin"
        );
        self.channels.write().await.insert(id, channel);
    }

    /// Unregister a channel plugin by ID. Stops it if running.
    pub async fn unregister(&self, channel_id: &str) -> bool {
        if let Some(channel) = self.channels.write().await.remove(channel_id) {
            if channel.status() == ChannelStatus::Running {
                if let Err(e) = channel.stop().await {
                    warn!(channel_id, "Failed to stop channel during unregister: {e}");
                }
            }
            true
        } else {
            false
        }
    }

    /// Start a specific channel by ID.
    pub async fn start_channel(&self, channel_id: &str) -> anyhow::Result<()> {
        let channels = self.channels.read().await;
        let channel = channels
            .get(channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found: {channel_id}"))?;

        channel.start(self.inbound_tx.clone()).await
    }

    /// Stop a specific channel by ID.
    pub async fn stop_channel(&self, channel_id: &str) -> anyhow::Result<()> {
        let channels = self.channels.read().await;
        let channel = channels
            .get(channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found: {channel_id}"))?;

        channel.stop().await
    }

    /// Start all registered channels.
    pub async fn start_all(&self) {
        let channels = self.channels.read().await;
        for (id, channel) in channels.iter() {
            if let Err(e) = channel.start(self.inbound_tx.clone()).await {
                warn!(channel_id = %id, "Failed to start channel: {e}");
            }
        }
    }

    /// Stop all registered channels.
    pub async fn stop_all(&self) {
        let channels = self.channels.read().await;
        for (id, channel) in channels.iter() {
            if let Err(e) = channel.stop().await {
                warn!(channel_id = %id, "Failed to stop channel: {e}");
            }
        }
    }

    /// Send a message through the appropriate channel.
    pub async fn send_message(&self, message: OutboundMessage) -> anyhow::Result<()> {
        let channels = self.channels.read().await;
        let channel = channels
            .get(&message.channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found: {}", message.channel_id))?;

        channel.send(message).await
    }

    /// List all registered channels with their status.
    pub async fn list_channels(&self) -> Vec<ChannelInfo> {
        let channels = self.channels.read().await;
        channels
            .values()
            .map(|ch| ChannelInfo {
                channel_type: ch.channel_type().to_string(),
                channel_id: ch.channel_id().to_string(),
                status: ch.status(),
            })
            .collect()
    }

    /// Get the status of a specific channel.
    pub async fn channel_status(&self, channel_id: &str) -> Option<ChannelStatus> {
        let channels = self.channels.read().await;
        channels.get(channel_id).map(|ch| ch.status())
    }

    /// Run the inbound message processing loop.
    ///
    /// This consumes messages from all channels and routes them through
    /// the session manager. Responses are sent back through the originating channel.
    ///
    /// Should be spawned as a background task.
    pub async fn run_message_loop(self: &Arc<Self>, manager: Arc<GatewaySessionManager>) {
        let mut rx = self.inbound_rx.lock().await;

        info!("Channel message loop started");

        while let Some(inbound) = rx.recv().await {
            let manager = manager.clone();
            let channel_mgr = self.clone();

            tokio::spawn(async move {
                // Derive session key from channel + sender if not provided
                let session_key = inbound
                    .session_key
                    .clone()
                    .unwrap_or_else(|| {
                        format!("{}:{}:{}", inbound.channel_type, inbound.channel_id, inbound.sender_id)
                    });

                let agent = inbound.agent.as_deref();

                info!(
                    channel_type = %inbound.channel_type,
                    channel_id = %inbound.channel_id,
                    sender = %inbound.sender_id,
                    session = %session_key,
                    "Processing inbound message"
                );

                match manager.send_message(&session_key, &inbound.text, agent).await {
                    Ok(response_text) => {
                        let outbound = OutboundMessage {
                            channel_type: inbound.channel_type,
                            channel_id: inbound.channel_id,
                            recipient_id: inbound.sender_id,
                            text: response_text,
                            session_key: Some(session_key),
                            metadata: inbound.metadata,
                        };

                        if let Err(e) = channel_mgr.send_message(outbound).await {
                            warn!("Failed to send response to channel: {e}");
                        }
                    }
                    Err(e) => {
                        warn!(session = %session_key, "Agent error: {e}");
                    }
                }
            });
        }

        info!("Channel message loop stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// A mock channel plugin for testing.
    struct MockChannel {
        id: String,
        state: AtomicU8, // 0=stopped, 1=starting, 2=running
        sent_messages: tokio::sync::Mutex<Vec<OutboundMessage>>,
    }

    impl MockChannel {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                state: AtomicU8::new(0),
                sent_messages: tokio::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl ChannelPlugin for MockChannel {
        fn channel_type(&self) -> &str {
            "mock"
        }

        fn channel_id(&self) -> &str {
            &self.id
        }

        async fn start(&self, _sender: mpsc::Sender<InboundMessage>) -> anyhow::Result<()> {
            self.state.store(2, Ordering::SeqCst);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.state.store(0, Ordering::SeqCst);
            Ok(())
        }

        async fn send(&self, message: OutboundMessage) -> anyhow::Result<()> {
            self.sent_messages.lock().await.push(message);
            Ok(())
        }

        fn status(&self) -> ChannelStatus {
            match self.state.load(Ordering::SeqCst) {
                0 => ChannelStatus::Stopped,
                1 => ChannelStatus::Starting,
                2 => ChannelStatus::Running,
                _ => ChannelStatus::Error("unknown".into()),
            }
        }
    }

    #[tokio::test]
    async fn test_register_and_list() {
        let mgr = ChannelManager::new(16);
        assert!(mgr.list_channels().await.is_empty());

        let ch = Arc::new(MockChannel::new("test-1"));
        mgr.register(ch).await;

        let list = mgr.list_channels().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].channel_type, "mock");
        assert_eq!(list[0].channel_id, "test-1");
        assert_eq!(list[0].status, ChannelStatus::Stopped);
    }

    #[tokio::test]
    async fn test_start_stop_channel() {
        let mgr = ChannelManager::new(16);
        let ch = Arc::new(MockChannel::new("test-1"));
        mgr.register(ch).await;

        mgr.start_channel("test-1").await.unwrap();
        assert_eq!(
            mgr.channel_status("test-1").await,
            Some(ChannelStatus::Running)
        );

        mgr.stop_channel("test-1").await.unwrap();
        assert_eq!(
            mgr.channel_status("test-1").await,
            Some(ChannelStatus::Stopped)
        );
    }

    #[tokio::test]
    async fn test_unregister() {
        let mgr = ChannelManager::new(16);
        let ch = Arc::new(MockChannel::new("test-1"));
        mgr.register(ch).await;

        assert!(mgr.unregister("test-1").await);
        assert!(!mgr.unregister("test-1").await); // already removed
        assert!(mgr.list_channels().await.is_empty());
    }

    #[tokio::test]
    async fn test_send_message() {
        let mgr = ChannelManager::new(16);
        let ch = Arc::new(MockChannel::new("test-1"));
        mgr.register(ch.clone()).await;

        let msg = OutboundMessage {
            channel_type: "mock".into(),
            channel_id: "test-1".into(),
            recipient_id: "user-1".into(),
            text: "Hello!".into(),
            session_key: None,
            metadata: HashMap::new(),
        };

        mgr.send_message(msg).await.unwrap();

        let sent = ch.sent_messages.lock().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].text, "Hello!");
    }

    #[tokio::test]
    async fn test_send_message_channel_not_found() {
        let mgr = ChannelManager::new(16);
        let msg = OutboundMessage {
            channel_type: "mock".into(),
            channel_id: "nonexistent".into(),
            recipient_id: "user-1".into(),
            text: "Hello!".into(),
            session_key: None,
            metadata: HashMap::new(),
        };

        assert!(mgr.send_message(msg).await.is_err());
    }

    #[tokio::test]
    async fn test_start_nonexistent_channel() {
        let mgr = ChannelManager::new(16);
        assert!(mgr.start_channel("nonexistent").await.is_err());
    }
}
