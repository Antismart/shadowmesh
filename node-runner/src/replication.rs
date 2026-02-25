//! Background Content Replication
//!
//! Pull-based replication loop that discovers under-replicated content
//! via the DHT and fetches copies from existing holders.

use serde::{Deserialize, Serialize};
use shadowmesh_protocol::replication::{ReplicationHealth, ReplicationManager};
use shadowmesh_protocol::ContentManifest;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};

use crate::config::ReplicationConfig;
use crate::metrics::MetricsCollector;
use crate::p2p::P2pState;
use crate::p2p_commands::{FetchError, P2pCommand};
use crate::storage::{StorageManager, StoredContent};

/// Shared replication state accessible from the API layer.
pub struct ReplicationState {
    /// Protocol-level replication health tracker
    pub manager: RwLock<ReplicationManager>,
    /// Cumulative statistics
    pub stats: RwLock<ReplicationStats>,
}

impl ReplicationState {
    pub fn new(replication_factor: usize) -> Self {
        Self {
            manager: RwLock::new(ReplicationManager::with_factor(replication_factor)),
            stats: RwLock::new(ReplicationStats::default()),
        }
    }

    /// Build a combined health report (health + stats).
    pub async fn health_report(&self) -> ReplicationHealthReport {
        let health = self.manager.read().await.get_health_summary();
        let stats = self.stats.read().await.clone();
        ReplicationHealthReport { health, stats }
    }
}

/// Cumulative replication statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplicationStats {
    /// Total items successfully replicated since startup
    pub total_replicated: u64,
    /// Total bytes pulled from the network for replication
    pub total_bytes_replicated: u64,
    /// Total replication failures since startup
    pub total_failures: u64,
    /// Items replicated in the last scan cycle
    pub last_cycle_replicated: u64,
    /// Unix timestamp of the last completed scan
    pub last_scan_timestamp: Option<u64>,
    /// Whether a scan is currently running
    pub scan_in_progress: bool,
}

/// Combined health + stats for the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationHealthReport {
    pub health: ReplicationHealth,
    pub stats: ReplicationStats,
}

/// Errors during replication.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ReplicationError {
    NoProviders,
    FetchFailed(String),
    StorageError(String),
    P2pChannelClosed,
}

impl std::fmt::Display for ReplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProviders => write!(f, "No providers found"),
            Self::FetchFailed(e) => write!(f, "Fetch failed: {}", e),
            Self::StorageError(e) => write!(f, "Storage error: {}", e),
            Self::P2pChannelClosed => write!(f, "P2P channel closed"),
        }
    }
}

impl std::error::Error for ReplicationError {}

// ── Background loop ──────────────────────────────────────────────

/// Run the background replication loop until shutdown.
pub async fn run_replication_loop(
    config: ReplicationConfig,
    _peer_id: String,
    p2p: Arc<P2pState>,
    storage: Arc<StorageManager>,
    metrics: Arc<MetricsCollector>,
    replication_state: Arc<ReplicationState>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    tracing::info!(
        interval_secs = config.scan_interval_secs,
        max_concurrent = config.max_concurrent_replications,
        max_storage_pct = config.max_storage_usage_pct,
        "Replication loop started"
    );

    let mut interval =
        tokio::time::interval(std::time::Duration::from_secs(config.scan_interval_secs));
    // First tick fires immediately — skip it so the node has time to discover peers.
    interval.tick().await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                scan_and_replicate(
                    &config,
                    &p2p,
                    &storage,
                    &metrics,
                    &replication_state,
                ).await;
            }
            _ = shutdown_rx.recv() => {
                tracing::info!("Replication loop shutting down");
                break;
            }
        }
    }
}

// ── Scan cycle ───────────────────────────────────────────────────

/// Run one replication scan cycle (5 phases).
async fn scan_and_replicate(
    config: &ReplicationConfig,
    p2p: &P2pState,
    storage: &StorageManager,
    metrics: &MetricsCollector,
    replication_state: &ReplicationState,
) {
    // Mark scan as in-progress
    {
        let mut stats = replication_state.stats.write().await;
        stats.scan_in_progress = true;
        stats.last_cycle_replicated = 0;
    }

    // Phase 1: Check storage budget
    let storage_stats = storage.get_stats().await;
    if storage_stats.usage_percentage() >= config.max_storage_usage_pct {
        tracing::info!(
            usage_pct = storage_stats.usage_percentage(),
            limit_pct = config.max_storage_usage_pct,
            "Skipping replication cycle — storage budget exceeded"
        );
        finish_scan(replication_state).await;
        return;
    }

    // Phase 2: Register local content in the replication manager
    let local_content = storage.list_content().await;
    if local_content.is_empty() {
        tracing::debug!("No local content — skipping replication cycle");
        finish_scan(replication_state).await;
        return;
    }

    {
        let mut mgr = replication_state.manager.write().await;
        for content in &local_content {
            let manifest = ContentManifest {
                content_hash: content.cid.clone(),
                fragments: content.fragments.clone(),
                metadata: shadowmesh_protocol::ContentMetadata {
                    name: content.name.clone(),
                    size: content.total_size,
                    mime_type: content.mime_type.clone(),
                },
            };
            mgr.register_content(content.cid.clone(), &manifest);

            // Mark self as a holder for every fragment we have locally
            let self_peer: libp2p::PeerId = libp2p::PeerId::random();
            for frag in &content.fragments {
                mgr.add_fragment_holder(&content.cid, frag, self_peer);
            }
        }
    }

    // Phase 3: Discover remote holders for each content via DHT
    for content in &local_content {
        if let Ok(providers) = find_providers_for(p2p, &content.cid).await {
            let mut mgr = replication_state.manager.write().await;
            for provider in &providers {
                for frag in &content.fragments {
                    mgr.add_fragment_holder(&content.cid, frag, *provider);
                }
            }
        }
    }

    // Phase 4: Identify under-replicated content we don't already store
    let targets: Vec<String> = {
        let mgr = replication_state.manager.read().await;
        mgr.get_under_replicated()
            .iter()
            .map(|s| s.cid.clone())
            .filter(|cid| !local_content.iter().any(|c| c.cid == *cid))
            .take(config.max_items_per_cycle)
            .collect()
    };

    if targets.is_empty() {
        tracing::debug!("No under-replicated content to pull");
        finish_scan(replication_state).await;
        return;
    }

    tracing::info!(count = targets.len(), "Replicating under-replicated content");

    // Phase 5: Pull content with bounded concurrency
    let semaphore = Arc::new(tokio::sync::Semaphore::new(
        config.max_concurrent_replications,
    ));

    let mut handles = Vec::new();

    for cid in targets {
        let permit = semaphore.clone().acquire_owned().await;
        if permit.is_err() {
            break;
        }
        let permit = permit.unwrap();
        let p2p_tx = p2p.command_tx.clone();
        let cid_clone = cid.clone();

        handles.push(tokio::spawn(async move {
            let result = replicate_content(&p2p_tx, &cid_clone).await;
            drop(permit);
            (cid_clone, result)
        }));
    }

    // Collect results
    let mut cycle_replicated = 0u64;
    let mut cycle_bytes = 0u64;

    for handle in handles {
        if let Ok((cid, result)) = handle.await {
            match result {
                Ok((manifest_result, fragment_data)) => {
                    // Store fragments and content
                    let mut total_bytes = 0u64;
                    let mut store_ok = true;

                    for (idx, (frag_hash, data)) in fragment_data.iter().enumerate() {
                        if storage.has_fragment(frag_hash).await {
                            continue;
                        }
                        if let Err(e) = storage
                            .store_fragment(frag_hash, &cid, idx as u32, data)
                            .await
                        {
                            tracing::warn!(%cid, %frag_hash, %e, "Failed to store replicated fragment");
                            store_ok = false;
                            break;
                        }
                        total_bytes += data.len() as u64;
                    }

                    if store_ok {
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        let stored = StoredContent {
                            cid: cid.clone(),
                            name: cid.clone(),
                            total_size: manifest_result.total_size,
                            fragment_count: manifest_result.fragment_hashes.len() as u32,
                            fragments: manifest_result.fragment_hashes.clone(),
                            stored_at: now,
                            pinned: false,
                            mime_type: manifest_result.mime_type.clone(),
                        };

                        if let Err(e) = storage.store_content(stored).await {
                            tracing::warn!(%cid, %e, "Failed to store replicated content metadata");
                        } else {
                            // Pin to prevent GC
                            let _ = storage.pin(&cid).await;

                            // Announce to DHT
                            let _ = p2p.command_tx.send(P2pCommand::AnnounceContent {
                                content_hash: cid.clone(),
                                fragment_hashes: manifest_result.fragment_hashes,
                                total_size: manifest_result.total_size,
                                mime_type: manifest_result.mime_type,
                            }).await;

                            metrics.record_received(total_bytes);
                            cycle_replicated += 1;
                            cycle_bytes += total_bytes;

                            tracing::info!(%cid, bytes = total_bytes, "Content replicated successfully");
                        }
                    } else {
                        let mut stats = replication_state.stats.write().await;
                        stats.total_failures += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!(%cid, %e, "Failed to replicate content");
                    let mut stats = replication_state.stats.write().await;
                    stats.total_failures += 1;
                }
            }
        }
    }

    // Update stats
    {
        let mut stats = replication_state.stats.write().await;
        stats.total_replicated += cycle_replicated;
        stats.total_bytes_replicated += cycle_bytes;
        stats.last_cycle_replicated = cycle_replicated;
    }

    if cycle_replicated > 0 {
        tracing::info!(
            replicated = cycle_replicated,
            bytes = cycle_bytes,
            "Replication cycle complete"
        );
    }

    finish_scan(replication_state).await;
}

/// Mark scan as finished and record timestamp.
async fn finish_scan(replication_state: &ReplicationState) {
    let mut stats = replication_state.stats.write().await;
    stats.scan_in_progress = false;
    stats.last_scan_timestamp = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
}

// ── Content fetching ─────────────────────────────────────────────

/// Fetch a single content item from the P2P network.
///
/// Returns the manifest result and all fragment data on success.
async fn replicate_content(
    p2p_tx: &tokio::sync::mpsc::Sender<P2pCommand>,
    cid: &str,
) -> Result<(crate::p2p_commands::ManifestResult, Vec<(String, Vec<u8>)>), ReplicationError> {
    // 1. Find providers
    let providers = find_providers(p2p_tx, cid).await?;
    if providers.is_empty() {
        return Err(ReplicationError::NoProviders);
    }

    // 2. Fetch manifest — try each provider
    let mut manifest = None;
    for provider in &providers {
        match fetch_manifest(p2p_tx, *provider, cid).await {
            Ok(m) => {
                manifest = Some(m);
                break;
            }
            Err(e) => {
                tracing::debug!(%cid, %provider, %e, "Manifest fetch failed, trying next provider");
            }
        }
    }
    let manifest = manifest.ok_or_else(|| {
        ReplicationError::FetchFailed("All providers failed for manifest".to_string())
    })?;

    // 3. Fetch each fragment — try providers in order
    let mut fragments = Vec::new();
    for frag_hash in &manifest.fragment_hashes {
        let mut fetched = false;
        for provider in &providers {
            match fetch_fragment(p2p_tx, *provider, frag_hash).await {
                Ok(data) => {
                    fragments.push((frag_hash.clone(), data));
                    fetched = true;
                    break;
                }
                Err(e) => {
                    tracing::debug!(%frag_hash, %provider, %e, "Fragment fetch failed, trying next");
                }
            }
        }
        if !fetched {
            return Err(ReplicationError::FetchFailed(format!(
                "Failed to fetch fragment {} from any provider",
                frag_hash
            )));
        }
    }

    Ok((manifest, fragments))
}

// ── P2P helpers ──────────────────────────────────────────────────

/// Find providers for a CID via the P2P command channel.
async fn find_providers_for(
    p2p: &P2pState,
    cid: &str,
) -> Result<Vec<libp2p::PeerId>, ReplicationError> {
    find_providers(&p2p.command_tx, cid).await
}

async fn find_providers(
    tx: &tokio::sync::mpsc::Sender<P2pCommand>,
    cid: &str,
) -> Result<Vec<libp2p::PeerId>, ReplicationError> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(P2pCommand::FindProviders {
        content_hash: cid.to_string(),
        reply: reply_tx,
    })
    .await
    .map_err(|_| ReplicationError::P2pChannelClosed)?;

    match tokio::time::timeout(std::time::Duration::from_secs(30), reply_rx).await {
        Ok(Ok(Ok(peers))) => Ok(peers),
        Ok(Ok(Err(FetchError::NotFound))) => Ok(Vec::new()),
        Ok(Ok(Err(e))) => Err(ReplicationError::FetchFailed(e.to_string())),
        Ok(Err(_)) => Err(ReplicationError::P2pChannelClosed),
        Err(_) => Ok(Vec::new()), // timeout — treat as no providers
    }
}

async fn fetch_manifest(
    tx: &tokio::sync::mpsc::Sender<P2pCommand>,
    peer_id: libp2p::PeerId,
    cid: &str,
) -> Result<crate::p2p_commands::ManifestResult, ReplicationError> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(P2pCommand::FetchManifest {
        peer_id,
        content_hash: cid.to_string(),
        reply: reply_tx,
    })
    .await
    .map_err(|_| ReplicationError::P2pChannelClosed)?;

    match tokio::time::timeout(std::time::Duration::from_secs(30), reply_rx).await {
        Ok(Ok(Ok(manifest))) => Ok(manifest),
        Ok(Ok(Err(e))) => Err(ReplicationError::FetchFailed(e.to_string())),
        Ok(Err(_)) => Err(ReplicationError::P2pChannelClosed),
        Err(_) => Err(ReplicationError::FetchFailed("Manifest fetch timed out".to_string())),
    }
}

async fn fetch_fragment(
    tx: &tokio::sync::mpsc::Sender<P2pCommand>,
    peer_id: libp2p::PeerId,
    fragment_hash: &str,
) -> Result<Vec<u8>, ReplicationError> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(P2pCommand::FetchFragment {
        peer_id,
        fragment_hash: fragment_hash.to_string(),
        reply: reply_tx,
    })
    .await
    .map_err(|_| ReplicationError::P2pChannelClosed)?;

    match tokio::time::timeout(std::time::Duration::from_secs(60), reply_rx).await {
        Ok(Ok(Ok(data))) => Ok(data),
        Ok(Ok(Err(e))) => Err(ReplicationError::FetchFailed(e.to_string())),
        Ok(Err(_)) => Err(ReplicationError::P2pChannelClosed),
        Err(_) => Err(ReplicationError::FetchFailed("Fragment fetch timed out".to_string())),
    }
}
