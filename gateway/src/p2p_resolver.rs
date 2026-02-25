//! High-level P2P content resolution for HTTP handlers.
//!
//! Orchestrates DHT provider lookup → manifest fetch → fragment fetch → reassembly.

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
/// 2. Fetch manifest from first available provider
/// 3. Fetch all fragments
/// 4. Reassemble and return
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

    // 2. Try each provider until one succeeds
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
    let mut assembled = Vec::with_capacity(manifest.total_size as usize);

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
