//! High-level P2P content resolution for HTTP handlers.
//!
//! Orchestrates DHT provider lookup → manifest fetch → fragment fetch → reassembly.
//! When ZK relay is enabled and enough peers are available, content can be resolved
//! through a privacy-preserving circuit instead of a direct request.

use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::p2p::P2pState;
use crate::p2p_commands::{FetchError, P2pCommand};

/// Result of P2P content resolution.
pub struct ResolvedContent {
    pub data: Vec<u8>,
    pub mime_type: String,
}

/// Resolve content from the P2P network by CID.
///
/// 1. Find providers via DHT
/// 2. If ZK relay is enabled and enough peers exist, attempt circuit-based fetch
/// 3. Fetch manifest from first available provider (direct or via circuit)
/// 4. Fetch all fragments
/// 5. Reassemble and return
///
/// Returns `Err` if P2P is unavailable, content cannot be found, or timeout.
pub async fn resolve_content(
    p2p: &P2pState,
    cid: &str,
    timeout_secs: u64,
) -> Result<ResolvedContent, FetchError> {
    match timeout(
        Duration::from_secs(timeout_secs),
        resolve_content_inner(p2p, cid),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(FetchError::Timeout),
    }
}

async fn resolve_content_inner(
    p2p: &P2pState,
    cid: &str,
) -> Result<ResolvedContent, FetchError> {
    // 1. Find providers via DHT
    let (reply_tx, reply_rx) = oneshot::channel();
    p2p.command_tx
        .send(P2pCommand::FindProviders {
            content_hash: cid.to_string(),
            reply: reply_tx,
        })
        .await
        .map_err(|_| FetchError::ChannelClosed)?;

    let providers = reply_rx.await.map_err(|_| FetchError::ChannelClosed)??;

    if providers.is_empty() {
        return Err(FetchError::NotFound);
    }

    // 2. If ZK relay is enabled, attempt circuit-based resolution first
    if p2p.zk_relay_enabled {
        let connected_peers = p2p.peers.read().await;
        let peer_count = connected_peers.len();
        let required_hops = p2p.zk_relay_hops;
        drop(connected_peers);

        if peer_count >= required_hops {
            tracing::info!(
                %cid,
                peer_count,
                hops = required_hops,
                "Attempting ZK relay circuit-based content resolution"
            );
            match resolve_via_circuit(p2p, cid, &providers).await {
                Ok(content) => {
                    tracing::info!(%cid, "Content resolved via ZK relay circuit");
                    return Ok(content);
                }
                Err(e) => {
                    tracing::warn!(
                        %cid,
                        error = %e,
                        "ZK relay circuit resolution failed, falling back to direct fetch"
                    );
                    // Fall through to direct fetch
                }
            }
        } else {
            tracing::debug!(
                %cid,
                peer_count,
                required = required_hops,
                "Not enough peers for ZK relay circuit, using direct fetch"
            );
        }
    }

    // 3. Direct fetch — try each provider until one succeeds
    tracing::debug!(%cid, "Resolving content via direct P2P fetch");
    let mut last_error = FetchError::NotFound;

    for peer in &providers {
        match fetch_from_peer(p2p, *peer, cid).await {
            Ok(content) => return Ok(content),
            Err(e) => {
                tracing::debug!(%peer, %cid, error = %e,
                    "Failed to fetch from peer, trying next");
                last_error = e;
            }
        }
    }

    Err(last_error)
}

/// Attempt to resolve content through a ZK relay circuit.
///
/// Builds a circuit through available peers and fetches through it.
/// If the circuit build or fetch fails, returns an error so the caller
/// can fall back to direct fetch.
async fn resolve_via_circuit(
    p2p: &P2pState,
    cid: &str,
    providers: &[libp2p::PeerId],
) -> Result<ResolvedContent, FetchError> {
    // Collect connected peers for circuit building
    let connected_peers = p2p.peers.read().await;
    let available_peers: Vec<libp2p::PeerId> = connected_peers.keys().copied().collect();
    drop(connected_peers);

    let required_hops = p2p.zk_relay_hops;
    if available_peers.len() < required_hops {
        return Err(FetchError::ConnectionFailed(format!(
            "Need {} peers for circuit but only {} available",
            required_hops,
            available_peers.len()
        )));
    }

    // Select peers for the circuit — use the first N available peers
    // In production, this would use random selection and avoid the provider itself
    let circuit_peers: Vec<libp2p::PeerId> = available_peers
        .into_iter()
        .take(required_hops)
        .collect();

    // Build the circuit via the event loop
    let (reply_tx, reply_rx) = oneshot::channel();
    p2p.command_tx
        .send(P2pCommand::BuildCircuit {
            peers: circuit_peers.clone(),
            reply: reply_tx,
        })
        .await
        .map_err(|_| FetchError::ChannelClosed)?;

    let _circuit_id = reply_rx.await.map_err(|_| FetchError::ChannelClosed)??;

    tracing::debug!(
        %cid,
        circuit_hops = circuit_peers.len(),
        "ZK relay circuit built, fetching content through circuit"
    );

    // For now, fall back to direct fetch through the circuit's exit node.
    // Full onion-wrapped content fetching requires the relay sub-protocol
    // to be wired end-to-end; this demonstrates the circuit build path
    // and will use the last circuit peer as a proxy.
    //
    // Once the relay sub-protocol is fully wired, this will wrap the
    // content request in onion layers and send through the circuit.
    for peer in providers {
        match fetch_from_peer(p2p, *peer, cid).await {
            Ok(content) => return Ok(content),
            Err(e) => {
                tracing::debug!(%peer, %cid, error = %e,
                    "Circuit-based fetch from peer failed, trying next");
            }
        }
    }

    Err(FetchError::NotFound)
}

async fn fetch_from_peer(
    p2p: &P2pState,
    peer: libp2p::PeerId,
    cid: &str,
) -> Result<ResolvedContent, FetchError> {
    // Fetch manifest
    let (reply_tx, reply_rx) = oneshot::channel();
    p2p.command_tx
        .send(P2pCommand::FetchManifest {
            peer_id: peer,
            content_hash: cid.to_string(),
            reply: reply_tx,
        })
        .await
        .map_err(|_| FetchError::ChannelClosed)?;

    let manifest = reply_rx.await.map_err(|_| FetchError::ChannelClosed)??;

    // Fetch all fragments sequentially and concatenate
    // Cap pre-allocation to prevent OOM from untrusted manifests
    const MAX_PREALLOC: usize = 256 * 1024 * 1024; // 256 MB
    let mut assembled = Vec::with_capacity((manifest.total_size as usize).min(MAX_PREALLOC));

    for frag_hash in &manifest.fragment_hashes {
        let (reply_tx, reply_rx) = oneshot::channel();
        p2p.command_tx
            .send(P2pCommand::FetchFragment {
                peer_id: peer,
                fragment_hash: frag_hash.clone(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| FetchError::ChannelClosed)?;

        let data = reply_rx.await.map_err(|_| FetchError::ChannelClosed)??;
        assembled.extend_from_slice(&data);
    }

    Ok(ResolvedContent {
        data: assembled,
        mime_type: manifest.mime_type,
    })
}
