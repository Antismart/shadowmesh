//! P2P Event Loop
//!
//! Runs the libp2p swarm event loop and shares peer state with the API layer.
//! Handles inbound content requests (serving fragments from local storage)
//! and outbound commands from API handlers (fetching, DHT announce/lookup).

use futures::StreamExt;
use libp2p::{
    kad,
    request_response::{self, OutboundRequestId, ResponseChannel},
    swarm::SwarmEvent,
    PeerId,
};
use shadowmesh_protocol::{
    content_protocol::{ContentRequest, ContentResponse},
    DHTManager, ShadowNode,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

use crate::metrics::MetricsCollector;
use crate::p2p_commands::{FetchError, ManifestResult, P2pCommand};
use crate::storage::StorageManager;

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct ConnectedPeer {
    pub peer_id: String,
    pub address: String,
}

/// Shared P2P state accessible from API handlers
pub struct P2pState {
    /// Currently connected peers
    pub peers: RwLock<HashMap<PeerId, ConnectedPeer>>,
    /// Addresses the node is listening on
    pub listen_addrs: RwLock<Vec<String>>,
    /// Sender half of the command channel for API → event loop communication
    pub command_tx: mpsc::Sender<P2pCommand>,
    /// Content announcements received via GossipSub (drained by replication loop)
    pub content_announcements: RwLock<Vec<shadowmesh_protocol::ContentAnnouncement>>,
}

impl P2pState {
    pub fn new(command_tx: mpsc::Sender<P2pCommand>) -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
            listen_addrs: RwLock::new(Vec::new()),
            command_tx,
            content_announcements: RwLock::new(Vec::new()),
        }
    }
}

/// Tracks a pending outbound request so the response can be routed back.
enum PendingReply {
    Fragment {
        fragment_hash: String,
        reply: oneshot::Sender<Result<Vec<u8>, FetchError>>,
    },
    Manifest {
        reply: oneshot::Sender<Result<ManifestResult, FetchError>>,
    },
    ContentList {
        reply: oneshot::Sender<Result<Vec<String>, FetchError>>,
    },
}

impl PendingReply {
    /// Returns true if the receiver has been dropped (caller no longer waiting).
    fn is_closed(&self) -> bool {
        match self {
            PendingReply::Fragment { reply, .. } => reply.is_closed(),
            PendingReply::Manifest { reply } => reply.is_closed(),
            PendingReply::ContentList { reply } => reply.is_closed(),
        }
    }
}

/// Run the P2P swarm event loop.
///
/// Takes ownership of the `ShadowNode` and processes swarm events until
/// a shutdown signal is received. Peer connections and listen addresses
/// are written to `p2p_state` so the API layer can read them.
pub async fn run_event_loop(
    mut node: ShadowNode,
    p2p_state: Arc<P2pState>,
    metrics: Arc<MetricsCollector>,
    storage: Arc<StorageManager>,
    mut command_rx: mpsc::Receiver<P2pCommand>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    tracing::info!("P2P event loop started");

    // Pending outbound content requests awaiting responses
    let mut pending_requests: HashMap<OutboundRequestId, PendingReply> = HashMap::new();

    // Pending DHT provider lookups awaiting Kademlia results
    let mut pending_dht_queries: HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>> =
        HashMap::new();

    let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_secs(60));
    cleanup_interval.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            event = node.swarm_mut().select_next_some() => {
                handle_swarm_event(
                    event,
                    &mut node,
                    &p2p_state,
                    &metrics,
                    &storage,
                    &mut pending_requests,
                    &mut pending_dht_queries,
                ).await;
            }
            Some(cmd) = command_rx.recv() => {
                handle_command(
                    cmd,
                    &mut node,
                    &mut pending_requests,
                    &mut pending_dht_queries,
                );
            }
            _ = cleanup_interval.tick() => {
                let before = pending_requests.len() + pending_dht_queries.len();
                pending_requests.retain(|_, p| !p.is_closed());
                pending_dht_queries.retain(|_, tx| !tx.is_closed());
                let after = pending_requests.len() + pending_dht_queries.len();
                if before > after {
                    tracing::debug!(removed = before - after, "Cleaned up stale pending queries");
                }
            }
            _ = shutdown_rx.recv() => {
                tracing::info!("P2P event loop shutting down");
                break;
            }
        }
    }
}

/// Process a single swarm event.
async fn handle_swarm_event(
    event: SwarmEvent<shadowmesh_protocol::node::ShadowBehaviourEvent>,
    node: &mut ShadowNode,
    p2p_state: &P2pState,
    metrics: &MetricsCollector,
    storage: &StorageManager,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
) {
    match event {
        SwarmEvent::ConnectionEstablished {
            peer_id, endpoint, ..
        } => {
            let addr = endpoint.get_remote_address().to_string();
            tracing::info!(%peer_id, %addr, "Peer connected");

            let mut peers = p2p_state.peers.write().await;
            peers.insert(
                peer_id,
                ConnectedPeer {
                    peer_id: peer_id.to_string(),
                    address: addr,
                },
            );
            let count = peers.len() as u32;
            drop(peers);

            metrics.set_connected_peers(count).await;
        }

        SwarmEvent::ConnectionClosed {
            peer_id,
            num_established,
            ..
        } => {
            // Only remove when all connections to this peer are gone
            if num_established == 0 {
                tracing::info!(%peer_id, "Peer disconnected");

                let mut peers = p2p_state.peers.write().await;
                peers.remove(&peer_id);
                let count = peers.len() as u32;
                drop(peers);

                metrics.set_connected_peers(count).await;
            }
        }

        SwarmEvent::NewListenAddr { address, .. } => {
            tracing::info!(%address, "Listening on new address");
            let mut addrs = p2p_state.listen_addrs.write().await;
            addrs.push(address.to_string());
        }

        SwarmEvent::Behaviour(behaviour_event) => {
            handle_behaviour_event(
                behaviour_event,
                node,
                p2p_state,
                storage,
                pending_requests,
                pending_dht_queries,
            )
            .await;
        }

        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            tracing::debug!(?peer_id, %error, "Outgoing connection failed");
        }

        SwarmEvent::IncomingConnectionError { error, .. } => {
            tracing::debug!(%error, "Incoming connection failed");
        }

        _ => {}
    }
}

/// Process behaviour-specific events.
async fn handle_behaviour_event(
    event: shadowmesh_protocol::node::ShadowBehaviourEvent,
    node: &mut ShadowNode,
    _p2p_state: &P2pState,
    storage: &StorageManager,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
) {
    use shadowmesh_protocol::node::ShadowBehaviourEvent;

    match event {
        // ── mDNS ─────────────────────────────────────────────
        ShadowBehaviourEvent::Mdns(libp2p::mdns::Event::Discovered(list)) => {
            for (peer_id, addr) in list {
                tracing::info!(%peer_id, %addr, "mDNS: discovered peer");

                node.swarm_mut()
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, addr.clone());

                if let Err(e) = node.dial(addr) {
                    tracing::debug!(%peer_id, %e, "Failed to dial discovered peer");
                }
            }
        }

        ShadowBehaviourEvent::Mdns(libp2p::mdns::Event::Expired(list)) => {
            tracing::debug!(count = list.len(), "mDNS: peers expired");
        }

        // ── Kademlia ─────────────────────────────────────────
        ShadowBehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed {
            result: kad::QueryResult::GetRecord(result),
            ..
        }) => {
            handle_kademlia_get_record(result, pending_dht_queries);
        }

        ShadowBehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed {
            result: kad::QueryResult::PutRecord(result),
            ..
        }) => match result {
            Ok(kad::PutRecordOk { key }) => {
                tracing::debug!(?key, "DHT record stored successfully");
            }
            Err(e) => {
                tracing::warn!(?e, "DHT PutRecord failed");
            }
        },

        ShadowBehaviourEvent::Kademlia(event) => {
            tracing::debug!(?event, "Kademlia event");
        }

        // ── GossipSub ────────────────────────────────────────
        ShadowBehaviourEvent::Gossipsub(libp2p::gossipsub::Event::Message {
            message, ..
        }) => {
            let content_topic_hash = libp2p::gossipsub::IdentTopic::new(
                shadowmesh_protocol::CONTENT_GOSSIP_TOPIC,
            )
            .hash();

            if message.topic == content_topic_hash {
                match serde_json::from_slice::<shadowmesh_protocol::ContentAnnouncement>(
                    &message.data,
                ) {
                    Ok(announcement) => {
                        tracing::info!(
                            cid = %announcement.cid,
                            peer = %announcement.peer_id,
                            size = announcement.total_size,
                            "Received content announcement via GossipSub"
                        );
                        let mut anns = _p2p_state.content_announcements.write().await;
                        anns.push(announcement);
                    }
                    Err(e) => {
                        tracing::debug!(%e, "Invalid GossipSub content announcement");
                    }
                }
            } else {
                tracing::debug!("GossipSub message on unhandled topic");
            }
        }

        ShadowBehaviourEvent::Gossipsub(event) => {
            tracing::debug!(?event, "GossipSub event");
        }

        // ── Content request-response ─────────────────────────
        ShadowBehaviourEvent::ContentReqResp(request_response::Event::Message {
            peer,
            message,
            ..
        }) => match message {
            request_response::Message::Request {
                request, channel, ..
            } => {
                handle_content_request(peer, request, channel, node, storage).await;
            }
            request_response::Message::Response {
                request_id,
                response,
            } => {
                handle_content_response(request_id, response, pending_requests);
            }
        },

        ShadowBehaviourEvent::ContentReqResp(request_response::Event::OutboundFailure {
            peer,
            request_id,
            error,
            ..
        }) => {
            tracing::warn!(%peer, ?request_id, ?error, "Outbound content request failed");
            if let Some(pending) = pending_requests.remove(&request_id) {
                let err = FetchError::ConnectionFailed(format!("{:?}", error));
                resolve_pending(pending, Err(err));
            }
        }

        ShadowBehaviourEvent::ContentReqResp(request_response::Event::InboundFailure {
            peer,
            error,
            ..
        }) => {
            tracing::debug!(%peer, ?error, "Inbound content request failed");
        }

        ShadowBehaviourEvent::ContentReqResp(request_response::Event::ResponseSent {
            peer,
            ..
        }) => {
            tracing::trace!(%peer, "Content response sent");
        }
    }
}

// ── Inbound request handling (serving) ───────────────────────────

/// Handle an incoming content request from a peer.
async fn handle_content_request(
    peer: PeerId,
    request: ContentRequest,
    channel: ResponseChannel<ContentResponse>,
    node: &mut ShadowNode,
    storage: &StorageManager,
) {
    let response = match request {
        ContentRequest::GetFragment { fragment_hash } => {
            match storage.get_fragment(&fragment_hash).await {
                Ok(data) => {
                    tracing::debug!(%peer, hash = %fragment_hash, bytes = data.len(),
                        "Serving fragment to peer");
                    ContentResponse::Fragment {
                        fragment_hash,
                        data,
                    }
                }
                Err(_) => {
                    tracing::debug!(%peer, hash = %fragment_hash, "Fragment not found locally");
                    ContentResponse::NotFound {
                        key: fragment_hash,
                    }
                }
            }
        }

        ContentRequest::GetManifest { content_hash } => {
            match storage.get_content(&content_hash).await {
                Some(content) => ContentResponse::Manifest {
                    content_hash: content.cid,
                    fragment_hashes: content.fragments,
                    total_size: content.total_size,
                    mime_type: content.mime_type,
                },
                None => ContentResponse::NotFound { key: content_hash },
            }
        }

        ContentRequest::Ping => ContentResponse::Pong,

        ContentRequest::ListContent { limit } => {
            let content_list = storage.list_content().await;
            let cap = if limit == 0 { usize::MAX } else { limit as usize };
            let items: Vec<shadowmesh_protocol::ContentSummary> = content_list
                .iter()
                .take(cap)
                .map(|c| shadowmesh_protocol::ContentSummary {
                    cid: c.cid.clone(),
                    total_size: c.total_size,
                    fragment_count: c.fragment_count,
                    mime_type: c.mime_type.clone(),
                })
                .collect();
            tracing::debug!(%peer, count = items.len(), "Serving content list to peer");
            ContentResponse::ContentList { items }
        }
    };

    if node
        .swarm_mut()
        .behaviour_mut()
        .content_req_resp
        .send_response(channel, response)
        .is_err()
    {
        tracing::warn!(%peer, "Failed to send content response (channel closed)");
    }
}

// ── Outbound response handling ───────────────────────────────────

/// Route a received response back to the waiting API handler.
fn handle_content_response(
    request_id: OutboundRequestId,
    response: ContentResponse,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
) {
    let Some(pending) = pending_requests.remove(&request_id) else {
        tracing::warn!(?request_id, "Received response for unknown request");
        return;
    };

    match (pending, response) {
        // Fragment response — verify BLAKE3 hash
        (
            PendingReply::Fragment {
                fragment_hash,
                reply,
            },
            ContentResponse::Fragment { data, .. },
        ) => {
            let computed = blake3::hash(&data).to_hex().to_string();
            if computed == fragment_hash {
                let _ = reply.send(Ok(data));
            } else {
                let _ = reply.send(Err(FetchError::PeerError(format!(
                    "Hash mismatch: expected {}, got {}",
                    fragment_hash, computed
                ))));
            }
        }

        // Manifest response
        (PendingReply::Manifest { reply }, ContentResponse::Manifest {
            content_hash,
            fragment_hashes,
            total_size,
            mime_type,
        }) => {
            let _ = reply.send(Ok(ManifestResult {
                content_hash,
                fragment_hashes,
                total_size,
                mime_type,
            }));
        }

        // ContentList response
        (PendingReply::ContentList { reply }, ContentResponse::ContentList { items }) => {
            let cids: Vec<String> = items.into_iter().map(|i| i.cid).collect();
            let _ = reply.send(Ok(cids));
        }

        // NotFound
        (pending, ContentResponse::NotFound { .. }) => {
            resolve_pending(pending, Err(FetchError::NotFound));
        }

        // Error
        (pending, ContentResponse::Error { message }) => {
            resolve_pending(pending, Err(FetchError::PeerError(message)));
        }

        // Unexpected response type
        (pending, resp) => {
            tracing::warn!(?resp, "Unexpected response type for pending request");
            resolve_pending(
                pending,
                Err(FetchError::PeerError("Unexpected response type".to_string())),
            );
        }
    }
}

/// Resolve a pending reply with a generic error.
fn resolve_pending(pending: PendingReply, err: Result<(), FetchError>) {
    let Err(e) = err else { return };
    match pending {
        PendingReply::Fragment { reply, .. } => {
            let _ = reply.send(Err(e));
        }
        PendingReply::Manifest { reply, .. } => {
            let _ = reply.send(Err(match e {
                FetchError::NotFound => FetchError::NotFound,
                FetchError::ConnectionFailed(s) => FetchError::ConnectionFailed(s),
                FetchError::PeerError(s) => FetchError::PeerError(s),
                FetchError::ChannelClosed => FetchError::ChannelClosed,
            }));
        }
        PendingReply::ContentList { reply, .. } => {
            let _ = reply.send(Err(match e {
                FetchError::NotFound => FetchError::NotFound,
                FetchError::ConnectionFailed(s) => FetchError::ConnectionFailed(s),
                FetchError::PeerError(s) => FetchError::PeerError(s),
                FetchError::ChannelClosed => FetchError::ChannelClosed,
            }));
        }
    }
}

// ── DHT query result handling ────────────────────────────────────

fn handle_kademlia_get_record(
    result: Result<kad::GetRecordOk, kad::GetRecordError>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
) {
    match result {
        Ok(kad::GetRecordOk::FoundRecord(peer_record)) => {
            if let Ok(content_record) =
                DHTManager::deserialize_record(&peer_record.record.value)
            {
                let cid = content_record.cid.clone();
                if let Some(reply) = pending_dht_queries.remove(&cid) {
                    let peers: Vec<PeerId> = content_record
                        .providers
                        .iter()
                        .filter_map(|p| p.peer_id.parse().ok())
                        .collect();
                    tracing::debug!(%cid, count = peers.len(), "DHT: found providers");
                    let _ = reply.send(Ok(peers));
                }
            }
        }
        Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. }) => {
            // No more records — nothing to do
        }
        Err(e) => {
            tracing::debug!(?e, "DHT GetRecord failed");
            // Try to extract key and resolve pending query
            let key_bytes = match &e {
                kad::GetRecordError::NotFound { key, .. } => Some(key.as_ref().to_vec()),
                kad::GetRecordError::QuorumFailed { key, .. } => Some(key.as_ref().to_vec()),
                kad::GetRecordError::Timeout { key, .. } => Some(key.as_ref().to_vec()),
            };
            if let Some(key_bytes) = key_bytes {
                let key_str = String::from_utf8_lossy(&key_bytes);
                // Extract CID from key format "/shadowmesh/content/{cid}"
                if let Some(cid) = key_str.strip_prefix("/shadowmesh/content/") {
                    if let Some(reply) = pending_dht_queries.remove(cid) {
                        let _ = reply.send(Err(FetchError::NotFound));
                    }
                }
            }
        }
    }
}

// ── Command handling (API → event loop) ──────────────────────────

/// Handle a command from an API handler.
fn handle_command(
    cmd: P2pCommand,
    node: &mut ShadowNode,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
) {
    match cmd {
        P2pCommand::FetchFragment {
            peer_id,
            fragment_hash,
            reply,
        } => {
            let request = ContentRequest::GetFragment {
                fragment_hash: fragment_hash.clone(),
            };
            let req_id = node
                .swarm_mut()
                .behaviour_mut()
                .content_req_resp
                .send_request(&peer_id, request);
            pending_requests.insert(
                req_id,
                PendingReply::Fragment {
                    fragment_hash,
                    reply,
                },
            );
        }

        P2pCommand::FetchManifest {
            peer_id,
            content_hash,
            reply,
        } => {
            let request = ContentRequest::GetManifest {
                content_hash: content_hash.clone(),
            };
            let req_id = node
                .swarm_mut()
                .behaviour_mut()
                .content_req_resp
                .send_request(&peer_id, request);
            pending_requests.insert(req_id, PendingReply::Manifest { reply });
        }

        P2pCommand::AnnounceContent {
            content_hash,
            fragment_hashes,
            total_size,
            mime_type,
        } => {
            announce_to_dht(node, &content_hash, &fragment_hashes, total_size, &mime_type);
        }

        P2pCommand::FindProviders {
            content_hash,
            reply,
        } => {
            let key = DHTManager::cid_to_key(&content_hash);
            node.swarm_mut()
                .behaviour_mut()
                .kademlia
                .get_record(key);
            pending_dht_queries.insert(content_hash, reply);
        }

        P2pCommand::ListPeerContent { peer_id, reply } => {
            let request = ContentRequest::ListContent { limit: 100 };
            let req_id = node
                .swarm_mut()
                .behaviour_mut()
                .content_req_resp
                .send_request(&peer_id, request);
            pending_requests.insert(req_id, PendingReply::ContentList { reply });
        }

        P2pCommand::BroadcastContentAnnouncement {
            cid,
            total_size,
            fragment_count,
            mime_type,
        } => {
            let peer_id = node.peer_id().to_string();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let announcement = shadowmesh_protocol::ContentAnnouncement {
                cid: cid.clone(),
                peer_id,
                total_size,
                fragment_count,
                mime_type,
                timestamp: now,
            };

            if let Ok(data) = serde_json::to_vec(&announcement) {
                let topic =
                    libp2p::gossipsub::IdentTopic::new(shadowmesh_protocol::CONTENT_GOSSIP_TOPIC);
                match node
                    .swarm_mut()
                    .behaviour_mut()
                    .gossipsub
                    .publish(topic, data)
                {
                    Ok(_) => {
                        tracing::debug!(%cid, "Broadcast content announcement via GossipSub")
                    }
                    Err(e) => {
                        tracing::debug!(%cid, ?e, "GossipSub content publish failed")
                    }
                }
            }
        }
    }
}

/// Announce content availability to the Kademlia DHT.
fn announce_to_dht(
    node: &mut ShadowNode,
    content_hash: &str,
    fragment_hashes: &[String],
    total_size: u64,
    mime_type: &str,
) {
    use shadowmesh_protocol::{ContentDHTMetadata, ContentRecord, ProviderInfo};

    let peer_id = *node.peer_id();
    let listen_addrs: Vec<String> = node.listen_addrs().iter().map(|a| a.to_string()).collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let record = ContentRecord {
        cid: content_hash.to_string(),
        providers: vec![ProviderInfo {
            peer_id: peer_id.to_string(),
            addresses: listen_addrs,
            reputation: 100,
            online: true,
            last_seen: now,
        }],
        metadata: ContentDHTMetadata {
            size: total_size,
            mime_type: mime_type.to_string(),
            fragment_count: fragment_hashes.len() as u32,
            encrypted: false,
        },
        created_at: now,
        ttl_seconds: 86400, // 24 hours
    };

    let key = DHTManager::cid_to_key(content_hash);
    let value = match DHTManager::serialize_record(&record) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(%content_hash, %e, "Failed to serialize DHT record");
            return;
        }
    };

    let kad_record = kad::Record {
        key,
        value,
        publisher: None,
        expires: None,
    };

    match node
        .swarm_mut()
        .behaviour_mut()
        .kademlia
        .put_record(kad_record, kad::Quorum::One)
    {
        Ok(_) => tracing::info!(%content_hash, "Announced content to DHT"),
        Err(e) => tracing::warn!(%content_hash, ?e, "Failed to announce content to DHT"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── P2pState tests ──────────────────────────────────────────

    #[tokio::test]
    async fn test_p2p_state_new_is_empty() {
        let (tx, _rx) = mpsc::channel(1);
        let state = P2pState::new(tx);

        assert!(state.peers.read().await.is_empty());
        assert!(state.listen_addrs.read().await.is_empty());
        assert!(state.content_announcements.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_p2p_state_peer_tracking() {
        let (tx, _rx) = mpsc::channel(1);
        let state = P2pState::new(tx);

        let peer_id = PeerId::random();
        state.peers.write().await.insert(
            peer_id,
            ConnectedPeer {
                peer_id: peer_id.to_string(),
                address: "/ip4/127.0.0.1/tcp/4001".to_string(),
            },
        );

        let peers = state.peers.read().await;
        assert_eq!(peers.len(), 1);
        assert!(peers.contains_key(&peer_id));
        assert_eq!(peers[&peer_id].address, "/ip4/127.0.0.1/tcp/4001");
    }

    #[tokio::test]
    async fn test_p2p_state_peer_disconnect() {
        let (tx, _rx) = mpsc::channel(1);
        let state = P2pState::new(tx);

        let peer1 = PeerId::random();
        let peer2 = PeerId::random();
        {
            let mut peers = state.peers.write().await;
            peers.insert(peer1, ConnectedPeer {
                peer_id: peer1.to_string(),
                address: "/ip4/10.0.0.1/tcp/4001".to_string(),
            });
            peers.insert(peer2, ConnectedPeer {
                peer_id: peer2.to_string(),
                address: "/ip4/10.0.0.2/tcp/4001".to_string(),
            });
        }

        state.peers.write().await.remove(&peer1);
        let peers = state.peers.read().await;
        assert_eq!(peers.len(), 1);
        assert!(!peers.contains_key(&peer1));
        assert!(peers.contains_key(&peer2));
    }

    #[tokio::test]
    async fn test_p2p_state_listen_addrs() {
        let (tx, _rx) = mpsc::channel(1);
        let state = P2pState::new(tx);

        state.listen_addrs.write().await.push("/ip4/0.0.0.0/tcp/4001".to_string());
        state.listen_addrs.write().await.push("/ip4/10.0.0.1/tcp/4001".to_string());

        let addrs = state.listen_addrs.read().await;
        assert_eq!(addrs.len(), 2);
    }

    #[tokio::test]
    async fn test_p2p_state_content_announcements() {
        let (tx, _rx) = mpsc::channel(1);
        let state = P2pState::new(tx);

        let announcement = shadowmesh_protocol::ContentAnnouncement {
            cid: "abc123".to_string(),
            peer_id: PeerId::random().to_string(),
            total_size: 1024,
            fragment_count: 1,
            mime_type: "text/plain".to_string(),
            timestamp: 12345,
        };
        state.content_announcements.write().await.push(announcement);

        let anns = state.content_announcements.read().await;
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].cid, "abc123");
        assert_eq!(anns[0].total_size, 1024);
    }

    #[tokio::test]
    async fn test_p2p_state_command_channel() {
        let (tx, mut rx) = mpsc::channel(4);
        let _state = P2pState::new(tx.clone());

        let (reply_tx, _reply_rx) = oneshot::channel();
        tx.send(P2pCommand::FindProviders {
            content_hash: "test_cid".to_string(),
            reply: reply_tx,
        })
        .await
        .unwrap();

        let cmd = rx.recv().await.unwrap();
        match cmd {
            P2pCommand::FindProviders { content_hash, .. } => {
                assert_eq!(content_hash, "test_cid");
            }
            _ => panic!("Expected FindProviders command"),
        }
    }

    // ── BLAKE3 hash verification tests ──────────────────────────

    #[test]
    fn test_blake3_fragment_verification_valid() {
        let data = b"hello world fragment data";
        let expected_hash = blake3::hash(data).to_hex().to_string();
        let computed = blake3::hash(data).to_hex().to_string();
        assert_eq!(computed, expected_hash);
    }

    #[test]
    fn test_blake3_fragment_verification_mismatch() {
        let data = b"hello world";
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let computed = blake3::hash(data).to_hex().to_string();
        assert_ne!(computed, wrong_hash);
    }

    #[test]
    fn test_blake3_empty_data() {
        let data = b"";
        let hash = blake3::hash(data).to_hex().to_string();
        assert_eq!(hash.len(), 64); // BLAKE3 always produces 256-bit output
    }

    #[test]
    fn test_blake3_large_fragment() {
        let data = vec![0xABu8; 256 * 1024]; // 256KB fragment
        let hash = blake3::hash(&data).to_hex().to_string();
        // Same data always produces same hash
        let hash2 = blake3::hash(&data).to_hex().to_string();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_blake3_different_data_different_hash() {
        let data1 = b"fragment A";
        let data2 = b"fragment B";
        let hash1 = blake3::hash(data1).to_hex().to_string();
        let hash2 = blake3::hash(data2).to_hex().to_string();
        assert_ne!(hash1, hash2);
    }

    // ── resolve_pending tests ───────────────────────────────────

    #[test]
    fn test_resolve_pending_fragment_error() {
        let (tx, rx) = oneshot::channel();
        let pending = PendingReply::Fragment {
            fragment_hash: "abc".to_string(),
            reply: tx,
        };
        resolve_pending(pending, Err(FetchError::ConnectionFailed("timeout".to_string())));

        let result = rx.blocking_recv().unwrap();
        assert!(matches!(result, Err(FetchError::ConnectionFailed(_))));
    }

    #[test]
    fn test_resolve_pending_manifest_not_found() {
        let (tx, rx) = oneshot::channel();
        let pending = PendingReply::Manifest { reply: tx };
        resolve_pending(pending, Err(FetchError::NotFound));

        let result = rx.blocking_recv().unwrap();
        assert!(matches!(result, Err(FetchError::NotFound)));
    }

    #[test]
    fn test_resolve_pending_content_list_channel_closed() {
        let (tx, rx) = oneshot::channel();
        let pending = PendingReply::ContentList { reply: tx };
        resolve_pending(pending, Err(FetchError::ChannelClosed));

        let result = rx.blocking_recv().unwrap();
        assert!(matches!(result, Err(FetchError::ChannelClosed)));
    }

    #[test]
    fn test_resolve_pending_ok_does_nothing() {
        let (tx, rx) = oneshot::channel();
        let pending = PendingReply::Fragment {
            fragment_hash: "abc".to_string(),
            reply: tx,
        };
        // Ok(()) should not send anything
        resolve_pending(pending, Ok(()));

        // Channel should still be open (nothing sent), receiver should get RecvError
        assert!(rx.blocking_recv().is_err());
    }

    // ── FetchError tests ────────────────────────────────────────

    #[test]
    fn test_fetch_error_display() {
        assert_eq!(FetchError::NotFound.to_string(), "Content not found on peer");
        assert_eq!(
            FetchError::ConnectionFailed("timeout".to_string()).to_string(),
            "Connection failed: timeout"
        );
        assert_eq!(
            FetchError::PeerError("disk full".to_string()).to_string(),
            "Peer error: disk full"
        );
        assert_eq!(FetchError::ChannelClosed.to_string(), "P2P event loop is shut down");
    }

    // ── ContentAnnouncement serialization tests ─────────────────

    #[test]
    fn test_content_announcement_round_trip() {
        let announcement = shadowmesh_protocol::ContentAnnouncement {
            cid: "abc123".to_string(),
            peer_id: PeerId::random().to_string(),
            total_size: 2048,
            fragment_count: 4,
            mime_type: "image/png".to_string(),
            timestamp: 1000000,
        };

        let json = serde_json::to_vec(&announcement).unwrap();
        let decoded: shadowmesh_protocol::ContentAnnouncement =
            serde_json::from_slice(&json).unwrap();

        assert_eq!(decoded.cid, announcement.cid);
        assert_eq!(decoded.peer_id, announcement.peer_id);
        assert_eq!(decoded.total_size, announcement.total_size);
        assert_eq!(decoded.fragment_count, announcement.fragment_count);
        assert_eq!(decoded.mime_type, announcement.mime_type);
    }

    // ── ConnectedPeer tests ─────────────────────────────────────

    #[test]
    fn test_connected_peer_clone() {
        let peer = ConnectedPeer {
            peer_id: "12D3KooWTest".to_string(),
            address: "/ip4/1.2.3.4/tcp/4001".to_string(),
        };
        let cloned = peer.clone();
        assert_eq!(cloned.peer_id, peer.peer_id);
        assert_eq!(cloned.address, peer.address);
    }
}
