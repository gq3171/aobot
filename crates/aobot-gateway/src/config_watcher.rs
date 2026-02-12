//! Configuration file watcher for hot-reload.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use tracing::{info, warn};

use crate::session_manager::GatewaySessionManager;

/// Start watching the config file for changes.
/// Returns a JoinHandle that can be used to abort the watcher.
pub fn start_config_watcher(
    manager: Arc<GatewaySessionManager>,
) -> Option<tokio::task::JoinHandle<()>> {
    let config_path = match aobot_config::config_file_path() {
        Ok(p) => p,
        Err(e) => {
            warn!("Cannot resolve config path for watching: {e}");
            return None;
        }
    };

    // Only watch if the config directory exists
    let watch_dir = match config_path.parent() {
        Some(dir) if dir.exists() => dir.to_path_buf(),
        Some(dir) => {
            info!(
                "Config directory {} does not exist yet, skipping watcher",
                dir.display()
            );
            return None;
        }
        None => return None,
    };

    let handle = tokio::task::spawn_blocking(move || {
        run_watcher(watch_dir, config_path, manager);
    });

    Some(handle)
}

fn run_watcher(
    watch_dir: PathBuf,
    config_path: PathBuf,
    manager: Arc<GatewaySessionManager>,
) {
    let (tx, rx) = std::sync::mpsc::channel();

    let mut debouncer = match new_debouncer(Duration::from_secs(1), tx) {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to create file watcher: {e}");
            return;
        }
    };

    if let Err(e) = debouncer
        .watcher()
        .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
    {
        warn!("Failed to watch config directory: {e}");
        return;
    }

    info!("Config watcher started: watching {}", watch_dir.display());

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                let config_changed = events.iter().any(|event| {
                    event.kind == DebouncedEventKind::Any && event.path == config_path
                });

                if config_changed {
                    info!("Config file changed, reloading...");
                    reload_config(&config_path, &manager);
                }
            }
            Ok(Err(e)) => {
                warn!("Config watcher error: {e:?}");
            }
            Err(_) => {
                info!("Config watcher channel closed, stopping");
                break;
            }
        }
    }
}

fn reload_config(config_path: &std::path::Path, manager: &Arc<GatewaySessionManager>) {
    match aobot_config::load_config_from(config_path) {
        Ok(config) => {
            let manager = manager.clone();
            // Use a blocking-safe approach: create a small runtime for the async call
            let rt = tokio::runtime::Handle::try_current();
            match rt {
                Ok(handle) => {
                    handle.spawn(async move {
                        manager.apply_config(config).await;
                        info!("Config reloaded successfully");
                    });
                }
                Err(_) => {
                    warn!("No tokio runtime available for config reload");
                }
            }
        }
        Err(e) => {
            warn!("Failed to reload config: {e}");
        }
    }
}
