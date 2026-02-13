//! Multi-session agent management for the Gateway.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use std::future::Future;
use std::pin::Pin;

use pi_agent_ai::register::create_default_registry;
use pi_agent_ai::stream::stream_simple;
use pi_agent_core::agent_types::{AgentEvent, AgentMessage, StreamFnBox};
use pi_agent_core::event_stream::create_assistant_message_event_stream;
use pi_agent_core::types::*;
use pi_coding_agent::agent_session::events::AgentSessionEvent;
use pi_coding_agent::agent_session::sdk::{CreateSessionOptions, create_agent_session};
use pi_coding_agent::agent_session::session::{AgentSession, PromptOptions, SummaryFn};
use pi_coding_agent::compaction::branch_summary;
use pi_coding_agent::compaction::compaction as compaction_utils;
use pi_coding_agent::error::CodingAgentError;
use pi_coding_agent::extensions::runner::ExtensionRunner;
use pi_coding_agent::extensions::types::ExtensionContext;
use pi_coding_agent::extensions::wrapper::{create_extension_tools, wrap_tools_with_extensions};
use pi_coding_agent::retry::RetryConfig as PiRetryConfig;
use pi_coding_agent::tools::{create_all_tools, create_coding_tools};

use aobot_config::AoBotConfig;
use aobot_storage::{AoBotStorage, SessionMetadata};
use aobot_types::{AgentConfig, AgentToolsConfig};

use aobot_tools::context::GatewayToolContext;

/// Information about a managed session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub session_key: String,
    pub agent_name: String,
    pub model_id: String,
    pub message_count: usize,
    pub created_at: i64,
}

/// Streaming events sent during chat.stream.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },
    #[serde(rename = "tool_start")]
    ToolStart { tool_name: String },
    #[serde(rename = "tool_end")]
    ToolEnd { tool_name: String, is_error: bool },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "done")]
    Done { full_response: String },
}

/// Manages multiple AgentSession instances.
pub struct GatewaySessionManager {
    sessions: RwLock<HashMap<String, Arc<Mutex<ManagedSession>>>>,
    config: RwLock<AoBotConfig>,
    working_dir: PathBuf,
    registry: Arc<pi_agent_ai::registry::ApiRegistry>,
    storage: Option<Arc<AoBotStorage>>,
    /// Sender for gateway operations — shared with all gateway tools.
    ops_tx: Option<tokio::sync::mpsc::UnboundedSender<aobot_tools::context::GatewayOp>>,
}

struct ManagedSession {
    session: AgentSession,
    agent_name: String,
    model_id: String,
    created_at: i64,
    /// Whether the pi-agent session ID has been captured and saved to SQLite.
    pi_session_id_saved: bool,
}

impl GatewaySessionManager {
    pub fn new(config: AoBotConfig, working_dir: PathBuf) -> Self {
        let registry = Arc::new(create_default_registry());
        Self {
            sessions: RwLock::new(HashMap::new()),
            config: RwLock::new(config),
            working_dir,
            registry,
            storage: None,
            ops_tx: None,
        }
    }

    /// Create a new manager with persistent storage.
    pub fn with_storage(
        config: AoBotConfig,
        working_dir: PathBuf,
        storage: Arc<AoBotStorage>,
    ) -> Self {
        let registry = Arc::new(create_default_registry());
        Self {
            sessions: RwLock::new(HashMap::new()),
            config: RwLock::new(config),
            working_dir,
            registry,
            storage: Some(storage),
            ops_tx: None,
        }
    }

    /// Set the gateway operations sender for gateway tools.
    pub fn set_ops_tx(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<aobot_tools::context::GatewayOp>,
    ) {
        self.ops_tx = Some(tx);
    }

    /// Create a new agent session with the given key.
    pub async fn create_session(
        &self,
        session_key: &str,
        agent_name: Option<&str>,
    ) -> Result<(), String> {
        let config = self.config.read().await;
        let agent_name = agent_name.unwrap_or(&config.default_agent);

        let agent_config = config
            .agents
            .get(agent_name)
            .cloned()
            .unwrap_or_else(|| AgentConfig {
                name: agent_name.to_string(),
                model: "anthropic/claude-sonnet-4".to_string(),
                system_prompt: Some("You are a helpful assistant.".to_string()),
                tools: AgentToolsConfig {
                    allow: vec![
                        "bash".to_string(),
                        "read".to_string(),
                        "write".to_string(),
                        "edit".to_string(),
                    ],
                    ..Default::default()
                },
                subagents: None,
                sandbox: None,
            });

        let mut session = create_agent_session(CreateSessionOptions {
            working_dir: self.working_dir.clone(),
            model_id: Some(agent_config.model.clone()),
            ..Default::default()
        })
        .map_err(|e| format!("Failed to create agent session: {e}"))?;

        // Set up stream function
        let registry = self.registry.clone();
        let stream_fn: StreamFnBox = Arc::new(move |model, context, options| {
            let cancel = CancellationToken::new();
            match stream_simple(model, context, options, &registry, cancel) {
                Ok(stream) => stream,
                Err(err) => {
                    let stream = create_assistant_message_event_stream();
                    let mut msg = AssistantMessage::empty(model);
                    msg.stop_reason = StopReason::Error;
                    msg.error_message = Some(err);
                    stream.push(AssistantMessageEvent::Error {
                        reason: StopReason::Error,
                        error: msg,
                    });
                    stream
                }
            }
        });
        session.set_stream_fn(stream_fn);

        // Set up MCP extensions if configured
        let mcp_configs = config.mcp.clone();
        let has_mcp = !mcp_configs.is_empty();

        let extension_runner = if has_mcp {
            let ext_context = ExtensionContext {
                working_dir: self.working_dir.clone(),
                session_id: None,
                model_id: Some(agent_config.model.clone()),
                config: serde_json::Value::Null,
            };
            let mut runner = ExtensionRunner::new(ext_context);

            for (key, mcp_config) in &mcp_configs {
                let aobot_mcp_config = aobot_mcp::config::McpServerConfig {
                    name: mcp_config.name.clone(),
                    transport: match &mcp_config.transport {
                        aobot_config::McpTransport::Stdio { command, args, env } => {
                            aobot_mcp::config::McpTransport::Stdio {
                                command: command.clone(),
                                args: args.clone(),
                                env: env.clone(),
                            }
                        }
                        aobot_config::McpTransport::Sse { url } => {
                            aobot_mcp::config::McpTransport::Sse { url: url.clone() }
                        }
                    },
                };
                let ext = aobot_mcp::McpExtension::new(aobot_mcp_config);
                if let Err(e) = runner.add_extension(Box::new(ext)).await {
                    tracing::warn!(mcp = %key, "Failed to load MCP extension: {e}");
                }
            }

            Some(Arc::new(runner))
        } else {
            None
        };

        // Set up tools based on agent config
        let mut tools =
            build_tools_for_agent(&self.working_dir, &agent_config.tools, &config.tools);

        // Add gateway tools if ops channel is available
        if let Some(ops_tx) = &self.ops_tx {
            let gateway_ctx = Arc::new(GatewayToolContext {
                current_session_key: session_key.to_string(),
                current_agent_id: agent_name.to_string(),
                config: Arc::new(tokio::sync::RwLock::new(config.clone())),
                ops_tx: ops_tx.clone(),
            });
            let gateway_tools = aobot_tools::tools::create_gateway_tools(gateway_ctx);
            let gateway_tool_names: Vec<String> = gateway_tools.keys().cloned().collect();
            // Add gateway tools that pass the policy filter
            let policy = aobot_tools::policy::ToolPolicy {
                profile: match &agent_config.tools.profile {
                    aobot_types::ToolProfile::Minimal => aobot_tools::policy::ToolProfile::Minimal,
                    aobot_types::ToolProfile::Coding => aobot_tools::policy::ToolProfile::Coding,
                    aobot_types::ToolProfile::Messaging => {
                        aobot_tools::policy::ToolProfile::Messaging
                    }
                    aobot_types::ToolProfile::Full => aobot_tools::policy::ToolProfile::Full,
                },
                allow: agent_config.tools.allow.clone(),
                also_allow: agent_config.tools.also_allow.clone(),
                deny: {
                    let mut deny = agent_config.tools.deny.clone();
                    deny.extend(config.tools.global_deny.clone());
                    deny
                },
                by_provider: Default::default(),
            };
            for (name, tool) in gateway_tools {
                if aobot_tools::policy::is_tool_allowed(&name, &policy, &gateway_tool_names) {
                    tools.push(tool);
                }
            }
        }

        // If we have MCP extensions, wrap tools and add extension tools
        if let Some(ref runner) = extension_runner {
            tools = wrap_tools_with_extensions(tools, runner.clone());
            let ext_tools = create_extension_tools(runner.clone());
            tools.extend(ext_tools);
        }

        session.set_tools(tools);

        // Set extension runner on session if available
        if let Some(runner) = extension_runner {
            session.set_extension_runner(runner);
        }

        // Set system prompt
        let prompt = agent_config
            .system_prompt
            .unwrap_or_else(|| "You are a helpful assistant.".to_string());
        session.set_system_prompt(prompt);

        // Set up summary function for compaction (uses the same LLM)
        let summary_registry = self.registry.clone();
        let summary_model_id = agent_config.model.clone();
        let summary_fn: SummaryFn = Arc::new(
            move |messages: Vec<AgentMessage>, previous_summary: Option<String>| {
                let registry = summary_registry.clone();
                let model_id = summary_model_id.clone();
                Box::pin(async move {
                    let summary_context = branch_summary::serialize_conversation(&messages);
                    let summary_prompt = branch_summary::generate_summary_prompt(
                        &summary_context,
                        previous_summary.as_deref(),
                    );

                    // Use the model registry to resolve the model
                    let model_registry = pi_coding_agent::model::registry::ModelRegistry::new();
                    let model = model_registry.find(&model_id).cloned().ok_or_else(|| {
                        CodingAgentError::Model(format!(
                            "Failed to resolve model for summary: {model_id}"
                        ))
                    })?;

                    let context = Context {
                        system_prompt: Some(
                            branch_summary::SUMMARIZATION_SYSTEM_PROMPT.to_string(),
                        ),
                        messages: vec![Message::User(UserMessage {
                            content: UserContent::Text(summary_prompt),
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        })],
                        tools: None,
                    };

                    let options = SimpleStreamOptions::default();
                    let cancel = CancellationToken::new();
                    let stream = stream_simple(&model, &context, &options, &registry, cancel)
                        .map_err(|e| {
                            CodingAgentError::Agent(format!("Summary stream error: {e}"))
                        })?;

                    // Collect the full response text
                    let mut text = String::new();
                    let mut pinned = Box::pin(futures::stream::unfold(
                        stream.clone(),
                        |mut s| async move {
                            let event = s.next().await;
                            event.map(|e| (e, s))
                        },
                    ));
                    use futures::StreamExt;
                    while let Some(event) = pinned.next().await {
                        if let AssistantMessageEvent::TextDelta { delta, .. } = event {
                            text.push_str(&delta);
                        }
                    }

                    if text.is_empty() {
                        // Fallback to basic summary
                        let max_len = 500;
                        let end = summary_context
                            .char_indices()
                            .take_while(|&(i, _)| i <= max_len)
                            .last()
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(0);
                        Ok(format!("Conversation summary: {}", &summary_context[..end]))
                    } else {
                        Ok(text)
                    }
                })
                    as Pin<Box<dyn Future<Output = Result<String, CodingAgentError>> + Send>>
            },
        );
        session.set_summary_fn(summary_fn);

        // Set up retry configuration from aobot config
        let retry_config = &config.retry;
        session.set_retry_config(PiRetryConfig {
            enabled: retry_config.enabled,
            max_retries: retry_config.max_retries,
            base_delay_ms: retry_config.base_delay_ms,
            max_delay_ms: retry_config.max_delay_ms,
        });

        let now = chrono::Utc::now().timestamp_millis();
        let managed = ManagedSession {
            session,
            agent_name: agent_name.to_string(),
            model_id: agent_config.model.clone(),
            created_at: now,
            pi_session_id_saved: false,
        };

        self.sessions
            .write()
            .await
            .insert(session_key.to_string(), Arc::new(Mutex::new(managed)));

        // Persist session metadata to storage
        if let Some(storage) = &self.storage {
            let meta = SessionMetadata {
                session_key: session_key.to_string(),
                agent_name: agent_name.to_string(),
                model_id: agent_config.model,
                created_at: now,
                last_active_at: now,
                message_count: 0,
                is_active: true,
                pi_session_id: None,
            };
            if let Err(e) = storage.save_session(&meta).await {
                tracing::warn!("Failed to persist session metadata: {e}");
            }
        }

        Ok(())
    }

    /// Ensure a session exists, creating one if needed. Returns the Arc<Mutex<ManagedSession>>.
    async fn ensure_session(
        &self,
        session_key: &str,
        agent_name: Option<&str>,
    ) -> Result<Arc<Mutex<ManagedSession>>, String> {
        // Check if session exists
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(session_key) {
                return Ok(session.clone());
            }
        }

        // Create session
        self.create_session(session_key, agent_name).await?;

        let sessions = self.sessions.read().await;
        sessions
            .get(session_key)
            .cloned()
            .ok_or_else(|| "Session not found after creation".to_string())
    }

    /// Build UserContent from text and optional attachments.
    fn build_user_content(
        message: &str,
        attachments: &[aobot_types::Attachment],
    ) -> pi_agent_core::types::UserContent {
        if attachments.is_empty() {
            return pi_agent_core::types::UserContent::Text(message.to_string());
        }

        let mut blocks = Vec::new();

        // Add text block if non-empty
        if !message.is_empty() {
            blocks.push(pi_agent_core::types::ContentBlock::Text(
                pi_agent_core::types::TextContent {
                    text: message.to_string(),
                    text_signature: None,
                },
            ));
        }

        // Convert attachments to content blocks
        for att in attachments {
            match att {
                aobot_types::Attachment::Image { base64, mime_type } => {
                    blocks.push(pi_agent_core::types::ContentBlock::Image(
                        pi_agent_core::types::ImageContent {
                            data: base64.clone(),
                            mime_type: mime_type.clone(),
                        },
                    ));
                }
                aobot_types::Attachment::Document {
                    base64,
                    mime_type,
                    file_name,
                } => {
                    // Documents that are images get sent as images
                    if mime_type.starts_with("image/") {
                        blocks.push(pi_agent_core::types::ContentBlock::Image(
                            pi_agent_core::types::ImageContent {
                                data: base64.clone(),
                                mime_type: mime_type.clone(),
                            },
                        ));
                    } else {
                        // Non-image documents: describe as text
                        let desc =
                            format!("[Document: {}]", file_name.as_deref().unwrap_or("unknown"));
                        blocks.push(pi_agent_core::types::ContentBlock::Text(
                            pi_agent_core::types::TextContent {
                                text: desc,
                                text_signature: None,
                            },
                        ));
                    }
                }
                aobot_types::Attachment::Audio { .. } => {
                    // Audio not directly supported in most LLM APIs; note it
                    blocks.push(pi_agent_core::types::ContentBlock::Text(
                        pi_agent_core::types::TextContent {
                            text: "[Audio message attached]".to_string(),
                            text_signature: None,
                        },
                    ));
                }
            }
        }

        pi_agent_core::types::UserContent::Blocks(blocks)
    }

    /// Send a prompt to a session. Creates the session if it doesn't exist.
    /// Returns collected text response.
    pub async fn send_message(
        &self,
        session_key: &str,
        message: &str,
        agent_name: Option<&str>,
    ) -> Result<String, String> {
        self.send_message_with_attachments(session_key, message, agent_name, &[])
            .await
    }

    /// Send a prompt with attachments to a session.
    /// Returns collected text response.
    pub async fn send_message_with_attachments(
        &self,
        session_key: &str,
        message: &str,
        agent_name: Option<&str>,
        attachments: &[aobot_types::Attachment],
    ) -> Result<String, String> {
        let session_arc = self.ensure_session(session_key, agent_name).await?;
        let mut managed = session_arc.lock().await;

        // Collect text response via event listener
        let response_text = Arc::new(std::sync::Mutex::new(String::new()));
        let text_collector = response_text.clone();

        managed.session.subscribe(Box::new(move |event| {
            if let AgentSessionEvent::Agent(AgentEvent::MessageUpdate {
                assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
                ..
            }) = &event
            {
                let mut text = text_collector.lock().unwrap();
                text.push_str(delta);
            }
        }));

        // Auto-compact before prompting if needed
        self.maybe_compact(session_key, &mut managed).await;

        let content = Self::build_user_content(message, attachments);
        let prompt_result = managed
            .session
            .prompt_with_content(content.clone(), PromptOptions::default())
            .await;

        // On context overflow, try emergency compaction and retry once
        if let Err(ref e) = prompt_result {
            let err_str = e.to_string();
            if err_str.contains("too long")
                || err_str.contains("context")
                || err_str.contains("token")
            {
                tracing::warn!(
                    session_key,
                    "Context overflow detected, attempting emergency compaction"
                );
                if managed.session.compact(None).await.is_ok() {
                    managed
                        .session
                        .prompt_with_content(content, PromptOptions::default())
                        .await
                        .map_err(|e| format!("Prompt error after compaction: {e}"))?;
                } else {
                    prompt_result.map_err(|e| format!("Prompt error: {e}"))?;
                }
            } else {
                prompt_result.map_err(|e| format!("Prompt error: {e}"))?;
            }
        }

        // Capture pi-agent session ID on first prompt
        if !managed.pi_session_id_saved {
            if let Some(pi_sid) = managed.session.session_id().map(|s| s.to_string()) {
                if let Some(storage) = &self.storage {
                    if let Err(e) = storage.save_pi_session_id(session_key, &pi_sid).await {
                        tracing::warn!("Failed to save pi_session_id: {e}");
                    } else {
                        managed.pi_session_id_saved = true;
                        tracing::debug!(session_key, pi_session_id = %pi_sid, "Captured pi_session_id");
                    }
                }
            }
        }

        // Update activity in storage
        if let Some(storage) = &self.storage {
            if let Err(e) = storage.update_session_activity(session_key).await {
                tracing::warn!("Failed to update session activity: {e}");
            }
        }

        let result = response_text.lock().unwrap().clone();
        Ok(result)
    }

    /// Send a prompt with streaming events through an mpsc channel.
    /// Returns the full response text after completion.
    pub async fn send_message_streaming(
        &self,
        session_key: &str,
        message: &str,
        agent_name: Option<&str>,
        event_tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<String, String> {
        self.send_message_streaming_with_attachments(
            session_key,
            message,
            agent_name,
            &[],
            event_tx,
        )
        .await
    }

    /// Send a prompt with attachments and streaming events.
    /// Returns the full response text after completion.
    pub async fn send_message_streaming_with_attachments(
        &self,
        session_key: &str,
        message: &str,
        agent_name: Option<&str>,
        attachments: &[aobot_types::Attachment],
        event_tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<String, String> {
        let session_arc = self.ensure_session(session_key, agent_name).await?;
        let mut managed = session_arc.lock().await;

        // Collect text response and stream events
        let response_text = Arc::new(std::sync::Mutex::new(String::new()));
        let text_collector = response_text.clone();

        // Clone for sending Done after prompt completes
        let done_tx = event_tx.clone();

        // Active flag: deactivated after prompt so old subscribers become no-ops
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_flag = active.clone();

        managed.session.subscribe(Box::new(move |event| {
            if !active_flag.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            match &event {
                AgentSessionEvent::Agent(AgentEvent::MessageUpdate {
                    assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
                    ..
                }) => {
                    let mut text = text_collector.lock().unwrap();
                    text.push_str(delta);
                    let _ = event_tx.send(StreamEvent::TextDelta {
                        delta: delta.clone(),
                    });
                }
                AgentSessionEvent::Agent(AgentEvent::ToolExecutionStart { tool_name, .. }) => {
                    let _ = event_tx.send(StreamEvent::ToolStart {
                        tool_name: tool_name.clone(),
                    });
                }
                AgentSessionEvent::Agent(AgentEvent::ToolExecutionEnd {
                    tool_name,
                    is_error,
                    ..
                }) => {
                    let _ = event_tx.send(StreamEvent::ToolEnd {
                        tool_name: tool_name.clone(),
                        is_error: *is_error,
                    });
                }
                AgentSessionEvent::Error { message } => {
                    let _ = event_tx.send(StreamEvent::Error {
                        message: message.clone(),
                    });
                }
                _ => {}
            }
        }));

        // Auto-compact before prompting if needed
        self.maybe_compact(session_key, &mut managed).await;

        let content = Self::build_user_content(message, attachments);
        let prompt_result = managed
            .session
            .prompt_with_content(content.clone(), PromptOptions::default())
            .await;

        // On context overflow, try emergency compaction and retry once
        if let Err(ref e) = prompt_result {
            let err_str = e.to_string();
            if err_str.contains("too long")
                || err_str.contains("context")
                || err_str.contains("token")
            {
                tracing::warn!(
                    session_key,
                    "Context overflow detected, attempting emergency compaction"
                );
                if managed.session.compact(None).await.is_ok() {
                    managed
                        .session
                        .prompt_with_content(content, PromptOptions::default())
                        .await
                        .map_err(|e| format!("Prompt error after compaction: {e}"))?;
                } else {
                    prompt_result.map_err(|e| format!("Prompt error: {e}"))?;
                }
            } else {
                prompt_result.map_err(|e| format!("Prompt error: {e}"))?;
            }
        }

        // Deactivate the subscriber so it becomes a no-op on future prompts
        active.store(false, std::sync::atomic::Ordering::Relaxed);

        // Capture pi-agent session ID on first prompt
        if !managed.pi_session_id_saved {
            if let Some(pi_sid) = managed.session.session_id().map(|s| s.to_string()) {
                if let Some(storage) = &self.storage {
                    if let Err(e) = storage.save_pi_session_id(session_key, &pi_sid).await {
                        tracing::warn!("Failed to save pi_session_id: {e}");
                    } else {
                        managed.pi_session_id_saved = true;
                        tracing::debug!(session_key, pi_session_id = %pi_sid, "Captured pi_session_id");
                    }
                }
            }
        }

        // Update activity in storage
        if let Some(storage) = &self.storage {
            if let Err(e) = storage.update_session_activity(session_key).await {
                tracing::warn!("Failed to update session activity: {e}");
            }
        }

        let result = response_text.lock().unwrap().clone();

        // Signal streaming completion so send_streaming() can do its final edit
        let _ = done_tx.send(StreamEvent::Done {
            full_response: result.clone(),
        });

        Ok(result)
    }

    /// Get chat history for a session.
    pub async fn get_history(&self, session_key: &str) -> Result<Vec<serde_json::Value>, String> {
        let sessions = self.sessions.read().await;
        let session_arc = sessions.get(session_key).ok_or("Session not found")?;
        let managed = session_arc.lock().await;

        let messages: Vec<serde_json::Value> = managed
            .session
            .messages()
            .iter()
            .filter_map(|msg| {
                msg.as_message()
                    .map(|m| serde_json::to_value(m).unwrap_or(serde_json::Value::Null))
            })
            .collect();

        Ok(messages)
    }

    /// List all active sessions.
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        let mut result = Vec::new();
        for (key, session_arc) in sessions.iter() {
            let managed = session_arc.lock().await;
            result.push(SessionInfo {
                session_key: key.clone(),
                agent_name: managed.agent_name.clone(),
                model_id: managed.model_id.clone(),
                message_count: managed.session.messages().len(),
                created_at: managed.created_at,
            });
        }
        result
    }

    /// Delete a session.
    pub async fn delete_session(&self, session_key: &str) -> bool {
        let removed = self.sessions.write().await.remove(session_key).is_some();
        if removed {
            if let Some(storage) = &self.storage {
                if let Err(e) = storage.delete_session(session_key).await {
                    tracing::warn!("Failed to delete session from storage: {e}");
                }
            }
        }
        removed
    }

    /// Get current config.
    pub async fn get_config(&self) -> AoBotConfig {
        self.config.read().await.clone()
    }

    /// Update config.
    pub async fn set_config(&self, config: AoBotConfig) {
        *self.config.write().await = config;
    }

    /// Apply config update (from hot-reload). Updates config and logs change.
    pub async fn apply_config(&self, config: AoBotConfig) {
        tracing::info!(
            "Applying config update: {} agents configured",
            config.agents.len()
        );
        self.set_config(config).await;
    }

    /// List all configured agent names and their configs.
    pub async fn list_agents(&self) -> HashMap<String, AgentConfig> {
        self.config.read().await.agents.clone()
    }

    /// Add or update an agent configuration.
    pub async fn add_agent(&self, name: String, agent_config: AgentConfig) {
        self.config.write().await.agents.insert(name, agent_config);
    }

    /// Delete an agent configuration. Returns true if the agent existed.
    pub async fn delete_agent(&self, name: &str) -> bool {
        self.config.write().await.agents.remove(name).is_some()
    }

    /// Build CompactionSettings from aobot config.
    fn build_compaction_settings(
        config: &aobot_config::CompactionConfig,
    ) -> compaction_utils::CompactionSettings {
        compaction_utils::CompactionSettings {
            enabled: config.enabled,
            reserve_tokens: config.reserve_tokens,
            keep_recent_tokens: config.keep_recent_tokens,
        }
    }

    /// Check if auto-compaction should run and execute it if needed.
    async fn maybe_compact(&self, session_key: &str, managed: &mut ManagedSession) {
        let config = self.config.read().await;
        let settings = Self::build_compaction_settings(&config.compaction);
        drop(config); // release read lock before await

        if !settings.enabled {
            return;
        }

        let model = match managed.session.model() {
            Some(m) => m.clone(),
            None => return,
        };

        let messages = managed.session.messages();
        if compaction_utils::should_compact(messages, model.context_window, &settings) {
            tracing::info!(
                session_key,
                messages = messages.len(),
                "Auto-compaction triggered"
            );
            match managed.session.compact(Some(&settings)).await {
                Ok(result) => {
                    tracing::info!(
                        session_key,
                        messages_before = result.messages_before,
                        messages_after = result.messages_after,
                        tokens_before = result.tokens_before,
                        tokens_after = result.tokens_after,
                        "Auto-compaction complete"
                    );
                }
                Err(e) => {
                    tracing::warn!(session_key, "Auto-compaction failed: {e}");
                }
            }
        }
    }

    /// Check if a session exists.
    pub async fn has_session(&self, session_key: &str) -> bool {
        self.sessions.read().await.contains_key(session_key)
    }

    /// Restore active sessions from persistent storage.
    ///
    /// Reads session metadata from SQLite and re-creates in-memory sessions.
    /// When a `pi_session_id` is present, the JSONL history is loaded so the
    /// agent remembers previous conversations across restarts.
    pub async fn restore_sessions(&self) -> Result<usize, String> {
        let storage = match &self.storage {
            Some(s) => s,
            None => return Ok(0),
        };

        let saved = storage
            .list_sessions()
            .await
            .map_err(|e| format!("Failed to load sessions from storage: {e}"))?;

        let count = saved.len();
        tracing::info!("Restoring {count} sessions from storage");

        for meta in saved {
            if let Err(e) = self
                .create_session(&meta.session_key, Some(&meta.agent_name))
                .await
            {
                tracing::warn!(
                    session_key = %meta.session_key,
                    "Failed to restore session: {e}"
                );
                continue;
            }

            // Restore JSONL history if pi_session_id is available
            if let Some(pi_sid) = &meta.pi_session_id {
                let sessions = self.sessions.read().await;
                if let Some(session_arc) = sessions.get(&meta.session_key) {
                    let mut managed = session_arc.lock().await;
                    match managed.session.restore_session(pi_sid) {
                        Ok(()) => {
                            managed.pi_session_id_saved = true;
                            let msg_count = managed.session.messages().len();
                            tracing::info!(
                                session_key = %meta.session_key,
                                pi_session_id = %pi_sid,
                                messages = msg_count,
                                "Restored session history from JSONL"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                session_key = %meta.session_key,
                                pi_session_id = %pi_sid,
                                "Failed to restore session history: {e}"
                            );
                        }
                    }
                }
            }
        }

        tracing::info!("Session restoration complete");
        Ok(count)
    }
}

/// Build a tool set for an agent based on its tool configuration.
///
/// Uses the tool policy system to resolve the effective tool set:
/// 1. If legacy config (just tool names), use them directly
/// 2. Otherwise, resolve profile + allow/also_allow/deny
/// 3. Apply global deny list
fn build_tools_for_agent(
    working_dir: &std::path::Path,
    tools_config: &AgentToolsConfig,
    global_tools: &aobot_config::GlobalToolsConfig,
) -> Vec<Arc<dyn pi_agent_core::agent_types::AgentTool>> {
    let all = create_all_tools(working_dir);
    let all_names: Vec<String> = all.keys().cloned().collect();

    // Resolve effective tool names using policy
    let policy = aobot_tools::policy::ToolPolicy {
        profile: match &tools_config.profile {
            aobot_types::ToolProfile::Minimal => aobot_tools::policy::ToolProfile::Minimal,
            aobot_types::ToolProfile::Coding => aobot_tools::policy::ToolProfile::Coding,
            aobot_types::ToolProfile::Messaging => aobot_tools::policy::ToolProfile::Messaging,
            aobot_types::ToolProfile::Full => aobot_tools::policy::ToolProfile::Full,
        },
        allow: tools_config.allow.clone(),
        also_allow: tools_config.also_allow.clone(),
        deny: {
            let mut deny = tools_config.deny.clone();
            deny.extend(global_tools.global_deny.clone());
            deny
        },
        by_provider: tools_config
            .by_provider
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    aobot_tools::policy::ToolPolicyOverride {
                        allow: v.allow.clone(),
                        deny: v.deny.clone(),
                    },
                )
            })
            .collect(),
    };

    let effective_names = aobot_tools::policy::resolve_effective_tools(&policy, &all_names);

    if effective_names.is_empty() {
        // Fallback: if policy resolves to nothing, use legacy behavior
        if tools_config.allow.is_empty() {
            return create_coding_tools(working_dir);
        }
        tracing::warn!("Tool policy resolved to empty set, using default coding tools");
        return create_coding_tools(working_dir);
    }

    let mut tools: Vec<Arc<dyn pi_agent_core::agent_types::AgentTool>> = Vec::new();
    for name in &effective_names {
        if let Some(tool) = all.get(name.as_str()) {
            tools.push(tool.clone());
        }
        // Gateway tools (sessions_list, message, etc.) are not in the base tool registry
        // They will be added separately when gateway tool context is available
    }

    if tools.is_empty() {
        tracing::warn!("No valid tools found after policy resolution, using default coding tools");
        return create_coding_tools(working_dir);
    }

    tools
}
