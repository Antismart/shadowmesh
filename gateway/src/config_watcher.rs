//! Configuration Hot-Reload for ShadowMesh Gateway
//!
//! Provides automatic configuration reloading when config files change,
//! and SIGHUP signal handling for manual reload triggers.

#![allow(dead_code)]

use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::config::Config;

/// Configuration watcher that monitors config files for changes
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    reload_rx: mpsc::Receiver<()>,
}

impl ConfigWatcher {
    /// Create a new ConfigWatcher that monitors the given paths
    ///
    /// Returns a ConfigWatcher and a receiver that signals when reload is needed
    pub fn new(paths: &[&str]) -> Result<Self, ConfigWatchError> {
        let (reload_tx, reload_rx) = mpsc::channel(1);

        let reload_tx_clone = reload_tx.clone();
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    let _ = reload_tx_clone.try_send(());
                }
            }
        })
        .map_err(|e| ConfigWatchError::WatcherInit(e.to_string()))?;

        // Configure debouncing
        watcher
            .configure(NotifyConfig::default().with_poll_interval(Duration::from_secs(2)))
            .map_err(|e| ConfigWatchError::WatcherInit(e.to_string()))?;

        // Watch all config paths
        for path_str in paths {
            let path = Path::new(path_str);

            // Watch the file if it exists, or the directory if file doesn't exist
            if path.exists() {
                watcher
                    .watch(path, RecursiveMode::NonRecursive)
                    .map_err(|e| {
                        ConfigWatchError::WatchPath(path_str.to_string(), e.to_string())
                    })?;
                tracing::info!(path = %path_str, "Watching config file for changes");
            } else if let Some(parent) = path.parent() {
                if parent.exists() {
                    watcher
                        .watch(parent, RecursiveMode::NonRecursive)
                        .map_err(|e| {
                            ConfigWatchError::WatchPath(
                                parent.to_string_lossy().to_string(),
                                e.to_string(),
                            )
                        })?;
                    tracing::info!(path = %parent.display(), "Watching config directory for changes");
                }
            }
        }

        Ok(Self {
            _watcher: watcher,
            reload_rx,
        })
    }

    /// Wait for a reload signal
    pub async fn wait_for_reload(&mut self) -> Option<()> {
        self.reload_rx.recv().await
    }
}

/// Reloadable configuration manager
pub struct ReloadableConfig {
    config: Arc<tokio::sync::RwLock<Config>>,
}

impl ReloadableConfig {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(tokio::sync::RwLock::new(config)),
        }
    }

    /// Get the current configuration
    pub async fn get(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Reload configuration from disk
    pub async fn reload(&self) -> Result<(), ConfigWatchError> {
        match Config::load() {
            Ok(new_config) => {
                let mut config = self.config.write().await;
                *config = new_config;
                tracing::info!("Configuration reloaded successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to reload configuration");
                Err(ConfigWatchError::LoadFailed(e.to_string()))
            }
        }
    }

    /// Get a reference to the inner config for direct access
    pub fn inner(&self) -> Arc<tokio::sync::RwLock<Config>> {
        self.config.clone()
    }
}

/// Start the config reload task that listens for file changes and SIGHUP
#[cfg(unix)]
pub async fn start_config_reload_task(
    config: Arc<tokio::sync::RwLock<Config>>,
    paths: Vec<String>,
) {
    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();

    // Create file watcher
    let mut watcher = match ConfigWatcher::new(&path_refs) {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to create config watcher - file watching disabled");
            None
        }
    };

    // Set up SIGHUP handler for Unix
    let mut sighup =
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to create SIGHUP handler, signal-based reload disabled");
                None
            }
        };

    loop {
        tokio::select! {
            // File change trigger
            Some(()) = async {
                if let Some(ref mut w) = watcher {
                    w.wait_for_reload().await
                } else {
                    std::future::pending::<Option<()>>().await
                }
            } => {
                tracing::info!("Config file changed, reloading...");
                reload_config(&config).await;
            }

            // SIGHUP trigger
            _ = async {
                if let Some(ref mut s) = sighup {
                    s.recv().await
                } else {
                    std::future::pending::<Option<()>>().await
                }
            } => {
                tracing::info!("Received SIGHUP, reloading configuration...");
                reload_config(&config).await;
            }

            // Keep the task alive
            _ = tokio::time::sleep(Duration::from_secs(3600)) => {}
        }
    }
}

/// Start the config reload task that listens for file changes (non-Unix)
#[cfg(not(unix))]
pub async fn start_config_reload_task(
    config: Arc<tokio::sync::RwLock<Config>>,
    paths: Vec<String>,
) {
    let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();

    // Create file watcher
    let mut watcher = match ConfigWatcher::new(&path_refs) {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to create config watcher - file watching disabled");
            None
        }
    };

    loop {
        tokio::select! {
            // File change trigger
            Some(()) = async {
                if let Some(ref mut w) = watcher {
                    w.wait_for_reload().await
                } else {
                    std::future::pending::<Option<()>>().await
                }
            } => {
                tracing::info!("Config file changed, reloading...");
                reload_config(&config).await;
            }

            // Keep the task alive
            _ = tokio::time::sleep(Duration::from_secs(3600)) => {}
        }
    }
}

async fn reload_config(config: &tokio::sync::RwLock<Config>) {
    match Config::load() {
        Ok(new_config) => {
            let mut current = config.write().await;

            // Log what's changing
            if current.rate_limit.requests_per_second != new_config.rate_limit.requests_per_second {
                tracing::info!(
                    old = current.rate_limit.requests_per_second,
                    new = new_config.rate_limit.requests_per_second,
                    "Rate limit updated"
                );
            }
            if current.cache.ttl_seconds != new_config.cache.ttl_seconds {
                tracing::info!(
                    old = current.cache.ttl_seconds,
                    new = new_config.cache.ttl_seconds,
                    "Cache TTL updated"
                );
            }

            *current = new_config;
            tracing::info!("Configuration reloaded successfully");
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to reload configuration - keeping current config");
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigWatchError {
    #[error("Failed to initialize watcher: {0}")]
    WatcherInit(String),
    #[error("Failed to watch path {0}: {1}")]
    WatchPath(String, String),
    #[error("Failed to load configuration: {0}")]
    LoadFailed(String),
}

/// Admin endpoint to trigger config reload
pub mod handler {
    use axum::{http::StatusCode, response::IntoResponse, Json};
    use serde::Serialize;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use crate::config::Config;

    #[derive(Serialize)]
    struct ReloadResponse {
        status: &'static str,
        message: String,
    }

    #[derive(Serialize)]
    struct ErrorResponse {
        error: String,
        code: &'static str,
    }

    /// POST /api/admin/reload - Trigger configuration reload
    ///
    /// Requires admin authentication
    pub async fn reload_config_handler(config: Arc<RwLock<Config>>) -> impl IntoResponse {
        match Config::load() {
            Ok(new_config) => {
                let mut current = config.write().await;
                *current = new_config;

                (
                    StatusCode::OK,
                    Json(ReloadResponse {
                        status: "success",
                        message: "Configuration reloaded".to_string(),
                    }),
                )
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReloadResponse {
                    status: "error",
                    message: format!("Failed to reload: {}", e),
                }),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reloadable_config() {
        let config = ReloadableConfig::new(Config::default());
        let current = config.get().await;
        assert_eq!(current.server.port, 8080);
    }
}
