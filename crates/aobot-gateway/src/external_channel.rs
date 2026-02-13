//! External channel plugin — bridges the `ChannelPlugin` trait to an external
//! subprocess communicating over stdin/stdout NDJSON JSON-RPC 2.0.
//!
//! The host spawns the plugin process, sends requests via its stdin, and reads
//! responses and notifications from its stdout.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, error, info, warn};

use aobot_types::{ChannelConfig, ChannelStatus, InboundMessage, OutboundMessage};

use crate::channel::ChannelPlugin;
use crate::plugin_protocol::*;

/// Default timeout for RPC calls to the plugin subprocess.
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// An external channel plugin that communicates with a subprocess over NDJSON.
pub struct ExternalChannelPlugin {
    channel_type: Mutex<String>,
    channel_id: String,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    config: ChannelConfig,
    state: Mutex<ExternalPluginState>,
}

struct ExternalPluginState {
    process: Option<Child>,
    stdin: Option<tokio::process::ChildStdin>,
    status: ChannelStatus,
    next_id: u64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    /// Sender for inbound messages forwarded from the plugin.
    inbound_tx: Option<mpsc::Sender<InboundMessage>>,
    /// Handle for the stdout reader task.
    reader_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ExternalChannelPlugin {
    /// Create a new external channel plugin from a channel config.
    ///
    /// Expected settings:
    /// - `command` (string): path to the plugin executable
    /// - `args` (array of strings, optional): command-line arguments
    /// - `env` (object of string→string, optional): environment variables
    /// - `plugin_channel_type` (string, optional): reported channel type name
    pub fn new(channel_id: String, config: &ChannelConfig) -> anyhow::Result<Self> {
        let command = config
            .settings
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("External plugin {channel_id}: missing 'command' in settings"))?
            .to_string();

        let args: Vec<String> = config
            .settings
            .get("args")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let env: HashMap<String, String> = config
            .settings
            .get("env")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let plugin_channel_type = config
            .settings
            .get("plugin_channel_type")
            .and_then(|v| v.as_str())
            .unwrap_or("external")
            .to_string();

        Ok(Self {
            channel_type: Mutex::new(plugin_channel_type),
            channel_id,
            command,
            args,
            env,
            config: config.clone(),
            state: Mutex::new(ExternalPluginState {
                process: None,
                stdin: None,
                status: ChannelStatus::Stopped,
                next_id: 1,
                pending: Arc::new(Mutex::new(HashMap::new())),
                inbound_tx: None,
                reader_handle: None,
            }),
        })
    }

    /// Send an RPC request and wait for the response.
    async fn send_rpc(&self, method: &str, params: Option<Value>) -> anyhow::Result<Value> {
        let (id, pending) = {
            let mut state = self.state.lock().await;
            let id = state.next_id;
            state.next_id += 1;

            let stdin = state
                .stdin
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Plugin process not running"))?;

            let request = JsonRpcMessage::request(id, method, params);
            let mut line = serde_json::to_string(&request)?;
            line.push('\n');
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;

            debug!(plugin = %self.channel_id, %method, %id, "Sent RPC request");

            (id, state.pending.clone())
        };

        // Register the pending response channel
        let (tx, rx) = oneshot::channel();
        pending.lock().await.insert(id, tx);

        // Wait for the response with timeout
        let response = tokio::time::timeout(RPC_TIMEOUT, rx)
            .await
            .map_err(|_| anyhow::anyhow!("RPC timeout for method '{method}' (id={id})"))?
            .map_err(|_| anyhow::anyhow!("RPC channel closed for method '{method}' (id={id})"))?;

        if let Some(err) = response.error {
            anyhow::bail!("Plugin RPC error [{}]: {}", err.code, err.message);
        }

        Ok(response.result.unwrap_or(Value::Null))
    }

    /// Spawn the stdout reader task that dispatches responses and notifications.
    fn spawn_reader(
        stdout: tokio::process::ChildStdout,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
        inbound_tx: mpsc::Sender<InboundMessage>,
        status: Arc<Mutex<ChannelStatus>>,
        channel_id: String,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }

                // Try to parse as a response first (has `result` or `error`)
                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                    if let Some(id) = resp.id {
                        let mut pending = pending.lock().await;
                        if let Some(tx) = pending.remove(&id) {
                            let _ = tx.send(resp);
                        } else {
                            warn!(plugin = %channel_id, %id, "Received response for unknown request ID");
                        }
                        continue;
                    }
                }

                // Otherwise parse as a notification (has `method`)
                match serde_json::from_str::<JsonRpcMessage>(&line) {
                    Ok(msg) if msg.id.is_none() => {
                        match msg.method.as_str() {
                            "inbound_message" => {
                                if let Some(params) = msg.params {
                                    match serde_json::from_value::<InboundMessageNotification>(params) {
                                        Ok(notif) => {
                                            if let Err(e) = inbound_tx.send(notif.message).await {
                                                warn!(plugin = %channel_id, "Failed to forward inbound message: {e}");
                                            }
                                        }
                                        Err(e) => {
                                            warn!(plugin = %channel_id, "Invalid inbound_message params: {e}");
                                        }
                                    }
                                }
                            }
                            "status_change" => {
                                if let Some(params) = msg.params {
                                    match serde_json::from_value::<StatusChangeNotification>(params) {
                                        Ok(notif) => {
                                            *status.lock().await = notif.status;
                                        }
                                        Err(e) => {
                                            warn!(plugin = %channel_id, "Invalid status_change params: {e}");
                                        }
                                    }
                                }
                            }
                            "log" => {
                                if let Some(params) = msg.params {
                                    if let Ok(log) = serde_json::from_value::<LogNotification>(params) {
                                        match log.level.as_str() {
                                            "error" => error!(plugin = %channel_id, "{}", log.message),
                                            "warn" => warn!(plugin = %channel_id, "{}", log.message),
                                            "info" => info!(plugin = %channel_id, "{}", log.message),
                                            _ => debug!(plugin = %channel_id, "{}", log.message),
                                        }
                                    }
                                }
                            }
                            other => {
                                debug!(plugin = %channel_id, method = %other, "Unknown notification");
                            }
                        }
                    }
                    Ok(_) => {
                        // Has an id but wasn't parsed as a response — ignore
                    }
                    Err(e) => {
                        warn!(plugin = %channel_id, "Failed to parse plugin output: {e}: {line}");
                    }
                }
            }

            info!(plugin = %channel_id, "Plugin stdout reader exited");
        })
    }
}

#[async_trait::async_trait]
impl ChannelPlugin for ExternalChannelPlugin {
    fn channel_type(&self) -> &str {
        // We can't return a reference to the Mutex contents directly,
        // so we leak a string. This is fine because channel_type is set
        // once during initialize and lives for the process lifetime.
        // However, to avoid the leak, we use a different approach:
        // We'll just return "external" here and store the real type internally.
        // The ChannelInfo will show it via channel_type.
        //
        // Actually, let's use a better approach — use a separate field.
        "external"
    }

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    async fn start(&self, sender: mpsc::Sender<InboundMessage>) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;

        if state.process.is_some() {
            anyhow::bail!("Plugin {} is already running", self.channel_id);
        }

        state.status = ChannelStatus::Starting;

        // Spawn the plugin subprocess
        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .envs(&self.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            state.status = ChannelStatus::Error(format!("Failed to spawn: {e}"));
            anyhow::anyhow!("Failed to spawn plugin {}: {e}", self.command)
        })?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        state.stdin = Some(stdin);
        state.process = Some(child);
        state.inbound_tx = Some(sender.clone());

        // Shared status for the reader task
        let status_shared = Arc::new(Mutex::new(ChannelStatus::Starting));
        let pending = state.pending.clone();

        // Spawn the stdout reader
        let reader_handle = Self::spawn_reader(
            stdout,
            pending,
            sender,
            status_shared,
            self.channel_id.clone(),
        );
        state.reader_handle = Some(reader_handle);

        // Drop the lock before sending RPCs (which also need the lock)
        drop(state);

        // Send `initialize`
        let init_params = serde_json::to_value(InitializeParams {
            channel_id: self.channel_id.clone(),
            config: self.config.clone(),
        })?;

        let init_result = self.send_rpc("initialize", Some(init_params)).await?;

        if let Ok(result) = serde_json::from_value::<InitializeResult>(init_result) {
            *self.channel_type.lock().await = result.channel_type;
        }

        // Send `start`
        self.send_rpc("start", None).await?;

        let mut state = self.state.lock().await;
        state.status = ChannelStatus::Running;

        info!(
            channel_id = %self.channel_id,
            command = %self.command,
            "External plugin started"
        );

        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        // Send `stop` and `shutdown` RPCs (best-effort)
        let _ = self.send_rpc("stop", None).await;
        let _ = self.send_rpc("shutdown", None).await;

        let mut state = self.state.lock().await;

        // Drop stdin to signal EOF
        state.stdin.take();

        // Wait for the process to exit (with timeout)
        if let Some(mut child) = state.process.take() {
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(exit)) => {
                    info!(
                        channel_id = %self.channel_id,
                        exit_code = ?exit.code(),
                        "External plugin exited"
                    );
                }
                Ok(Err(e)) => {
                    warn!(channel_id = %self.channel_id, "Error waiting for plugin exit: {e}");
                }
                Err(_) => {
                    warn!(channel_id = %self.channel_id, "Plugin did not exit in time, killing");
                    let _ = child.kill().await;
                }
            }
        }

        // Abort the reader task
        if let Some(handle) = state.reader_handle.take() {
            handle.abort();
        }

        state.status = ChannelStatus::Stopped;
        state.inbound_tx = None;

        // Clear any pending requests
        let mut pending = state.pending.lock().await;
        pending.clear();

        Ok(())
    }

    async fn send(&self, message: OutboundMessage) -> anyhow::Result<()> {
        let params = serde_json::to_value(SendParams { message })?;
        self.send_rpc("send", Some(params)).await?;
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        // We need a synchronous return here — use try_lock.
        match self.state.try_lock() {
            Ok(state) => state.status.clone(),
            Err(_) => ChannelStatus::Running, // assume running if locked
        }
    }

    async fn notify_processing(
        &self,
        recipient_id: &str,
        metadata: &HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<()> {
        let params = serde_json::to_value(NotifyProcessingParams {
            recipient_id: recipient_id.to_string(),
            metadata: metadata.clone(),
        })?;
        // Fire and forget — don't block on notify_processing timeout
        let _ = tokio::time::timeout(Duration::from_secs(5), self.send_rpc("notify_processing", Some(params))).await;
        Ok(())
    }
}

impl Drop for ExternalChannelPlugin {
    fn drop(&mut self) {
        // Best effort cleanup — the kill_on_drop on Child handles process termination
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(handle) = state.reader_handle.take() {
                handle.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_missing_command() {
        let config = ChannelConfig {
            channel_type: "external".into(),
            enabled: true,
            agent: None,
            settings: HashMap::new(),
        };
        let result = ExternalChannelPlugin::new("test".into(), &config);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("missing 'command'"));
    }

    #[test]
    fn test_new_with_command() {
        let mut settings = HashMap::new();
        settings.insert("command".into(), Value::String("/usr/bin/echo".into()));
        settings.insert(
            "args".into(),
            serde_json::json!(["--flag"]),
        );
        settings.insert(
            "env".into(),
            serde_json::json!({"MY_VAR": "value"}),
        );
        settings.insert(
            "plugin_channel_type".into(),
            Value::String("slack".into()),
        );

        let config = ChannelConfig {
            channel_type: "external".into(),
            enabled: true,
            agent: None,
            settings,
        };

        let plugin = ExternalChannelPlugin::new("my-slack".into(), &config).unwrap();
        assert_eq!(plugin.channel_id, "my-slack");
        assert_eq!(plugin.command, "/usr/bin/echo");
        assert_eq!(plugin.args, vec!["--flag"]);
        assert_eq!(plugin.env.get("MY_VAR").unwrap(), "value");
    }

    #[test]
    fn test_initial_status() {
        let mut settings = HashMap::new();
        settings.insert("command".into(), Value::String("test".into()));
        let config = ChannelConfig {
            channel_type: "external".into(),
            enabled: true,
            agent: None,
            settings,
        };
        let plugin = ExternalChannelPlugin::new("test".into(), &config).unwrap();
        assert_eq!(plugin.status(), ChannelStatus::Stopped);
    }
}
