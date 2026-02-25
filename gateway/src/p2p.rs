//! Gateway P2P Event Loop
//!
//! Runs a ShadowNode to participate in the P2P mesh. Handles:
//! - Inbound content requests (serves cached content to peers)
//! - Outbound commands from HTTP handlers (DHT lookups, content fetching)
//! - mDNS / Kademlia peer discovery

use futures::StreamExt;
use libp2p::{
    kad,
    request_response::{self, OutboundRequestId, ResponseChannel},
    swarm::SwarmEvent,
    PeerId,
};
use protocol::{
    content_protocol::{ContentRequest, ContentResponse},
    DHTManager, NamingManager, ShadowNode, NAMING_GOSSIP_TOPIC,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

use crate::cache::ContentCache;
use crate::p2p_commands::{FetchError, ManifestResult, P2pCommand};

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct ConnectedPeer {
    pub peer_id: String,
    pub address: String,
}

/// Shared P2P state accessible from HTTP handlers
pub struct P2pState {
    /// Currently connected peers
    pub peers: RwLock<HashMap<PeerId, ConnectedPeer>>,
    /// Addresses the node is listening on
    pub listen_addrs: RwLock<Vec<String>>,
    /// Sender half of the command channel for HTTP → event loop communication
    pub command_tx: mpsc::Sender<P2pCommand>,
}

impl P2pState {
    pub fn new(command_tx: mpsc::Sender<P2pCommand>) -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
            listen_addrs: RwLock::new(Vec::new()),
            command_tx,
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
}

/// Run the P2P swarm event loop.
///
/// Takes ownership of the `ShadowNode` and processes swarm events until
/// a shutdown signal is received.
pub async fn run_event_loop(
    mut node: ShadowNode,
    p2p_state: Arc<P2pState>,
    cache: Arc<ContentCache>,
    naming: Arc<std::sync::RwLock<NamingManager>>,
    mut command_rx: mpsc::Receiver<P2pCommand>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    tracing::info!("Gateway P2P event loop started");

    let mut pending_requests: HashMap<OutboundRequestId, PendingReply> = HashMap::new();
    let mut pending_dht_queries: HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>> =
        HashMap::new();
    let mut pending_name_queries: HashMap<
        String,
        oneshot::Sender<Result<Option<Vec<u8>>, FetchError>>,
    > = HashMap::new();

    loop {
        tokio::select! {
            event = node.swarm_mut().select_next_some() => {
                handle_swarm_event(
                    event,
                    &mut node,
                    &p2p_state,
                    &cache,
                    &naming,
                    &mut pending_requests,
                    &mut pending_dht_queries,
                    &mut pending_name_queries,
                ).await;
            }
            Some(cmd) = command_rx.recv() => {
                handle_command(
                    cmd,
                    &mut node,
                    &mut pending_requests,
                    &mut pending_dht_queries,
                    &mut pending_name_queries,
                );
            }
            _ = shutdown_rx.recv() => {
                tracing::info!("Gateway P2P event loop shutting down");
                break;
            }
        }
    }
}

/// Process a single swarm event.
async fn handle_swarm_event(
    event: SwarmEvent<protocol::node::ShadowBehaviourEvent>,
    node: &mut ShadowNode,
    p2p_state: &P2pState,
    cache: &ContentCache,
    naming: &std::sync::RwLock<NamingManager>,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
    pending_name_queries: &mut HashMap<
        String,
        oneshot::Sender<Result<Option<Vec<u8>>, FetchError>>,
    >,
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
        }

        SwarmEvent::ConnectionClosed {
            peer_id,
            num_established,
            ..
        } => {
            if num_established == 0 {
                tracing::info!(%peer_id, "Peer disconnected");
                p2p_state.peers.write().await.remove(&peer_id);
            }
        }

        SwarmEvent::NewListenAddr { address, .. } => {
            tracing::info!(%address, "Listening on new address");
            p2p_state
                .listen_addrs
                .write()
                .await
                .push(address.to_string());
        }

        SwarmEvent::Behaviour(behaviour_event) => {
            handle_behaviour_event(
                behaviour_event,
                node,
                cache,
                naming,
                pending_requests,
                pending_dht_queries,
                pending_name_queries,
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
    event: protocol::node::ShadowBehaviourEvent,
    node: &mut ShadowNode,
    cache: &ContentCache,
    naming: &std::sync::RwLock<NamingManager>,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
    pending_name_queries: &mut HashMap<
        String,
        oneshot::Sender<Result<Option<Vec<u8>>, FetchError>>,
    >,
) {
    use protocol::node::ShadowBehaviourEvent;

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
            handle_kademlia_get_record(result, pending_dht_queries, pending_name_queries);
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
            let naming_topic_hash =
                libp2p::gossipsub::IdentTopic::new(NAMING_GOSSIP_TOPIC).hash();
            if message.topic == naming_topic_hash {
                match NamingManager::deserialize_record(&message.data) {
                    Ok(record) => {
                        if protocol::naming::validate_record(&record).is_ok() {
                            if protocol::verify_record(&record).unwrap_or(false) {
                                if !record.is_revoked() {
                                    let mut nm =
                                        naming.write().unwrap_or_else(|p| p.into_inner());
                                    match nm.cache_record(record.clone()) {
                                        Ok(()) => {
                                            tracing::info!(
                                                name = %record.name,
                                                "Cached name update from GossipSub"
                                            );
                                        }
                                        Err(e) => {
                                            tracing::debug!(
                                                name = %record.name,
                                                %e,
                                                "Failed to cache GossipSub name update"
                                            );
                                        }
                                    }
                                }
                            } else {
                                tracing::debug!("GossipSub name record failed signature verification");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(%e, "Invalid GossipSub naming message");
                    }
                }
            } else {
                tracing::debug!(?message.topic, "GossipSub message on unknown topic");
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
                handle_content_request(peer, request, channel, node, cache);
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

// ── Inbound request handling (serving cached content) ────────────

/// Handle an incoming content request from a peer.
///
/// The gateway stores whole files by CID in its cache, not individual fragments.
/// If the requested hash matches a cached CID, serve it; otherwise NotFound.
fn handle_content_request(
    peer: PeerId,
    request: ContentRequest,
    channel: ResponseChannel<ContentResponse>,
    node: &mut ShadowNode,
    cache: &ContentCache,
) {
    let response = match request {
        ContentRequest::GetFragment { fragment_hash } => {
            match cache.get(&fragment_hash) {
                Some((data, _content_type)) => {
                    tracing::debug!(%peer, hash = %fragment_hash, bytes = data.len(),
                        "Serving cached content as fragment");
                    ContentResponse::Fragment {
                        fragment_hash,
                        data,
                    }
                }
                None => ContentResponse::NotFound {
                    key: fragment_hash,
                },
            }
        }

        ContentRequest::GetManifest { content_hash } => {
            // Gateway stores whole files — return a single-fragment manifest
            match cache.get(&content_hash) {
                Some((data, content_type)) => ContentResponse::Manifest {
                    content_hash: content_hash.clone(),
                    fragment_hashes: vec![content_hash],
                    total_size: data.len() as u64,
                    mime_type: content_type,
                },
                None => ContentResponse::NotFound { key: content_hash },
            }
        }

        ContentRequest::Ping => ContentResponse::Pong,
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

/// Route a received response back to the waiting HTTP handler.
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
        (
            PendingReply::Manifest { reply },
            ContentResponse::Manifest {
                content_hash,
                fragment_hashes,
                total_size,
                mime_type,
            },
        ) => {
            let _ = reply.send(Ok(ManifestResult {
                content_hash,
                fragment_hashes,
                total_size,
                mime_type,
            }));
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

/// Resolve a pending reply with an error.
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
                FetchError::Timeout => FetchError::Timeout,
            }));
        }
    }
}

// ── DHT query result handling ────────────────────────────────────

fn handle_kademlia_get_record(
    result: Result<kad::GetRecordOk, kad::GetRecordError>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
    pending_name_queries: &mut HashMap<
        String,
        oneshot::Sender<Result<Option<Vec<u8>>, FetchError>>,
    >,
) {
    match result {
        Ok(kad::GetRecordOk::FoundRecord(peer_record)) => {
            let key_str = String::from_utf8_lossy(peer_record.record.key.as_ref());

            if key_str.starts_with("/shadowmesh/names/") {
                // Name record — match by hash
                let hash_suffix = key_str
                    .strip_prefix("/shadowmesh/names/")
                    .unwrap_or("");
                let matching_name = pending_name_queries
                    .keys()
                    .find(|name| protocol::naming::name_to_hash(name) == hash_suffix)
                    .cloned();
                if let Some(name) = matching_name {
                    if let Some(reply) = pending_name_queries.remove(&name) {
                        tracing::debug!(%name, "DHT: found name record");
                        let _ = reply.send(Ok(Some(peer_record.record.value)));
                    }
                }
            } else if let Ok(content_record) =
                DHTManager::deserialize_record(&peer_record.record.value)
            {
                // Content record (existing logic)
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
        Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. }) => {}
        Err(e) => {
            tracing::debug!(?e, "DHT GetRecord failed");
            let key_bytes = match &e {
                kad::GetRecordError::NotFound { key, .. } => Some(key.as_ref().to_vec()),
                kad::GetRecordError::QuorumFailed { key, .. } => Some(key.as_ref().to_vec()),
                kad::GetRecordError::Timeout { key, .. } => Some(key.as_ref().to_vec()),
            };
            if let Some(key_bytes) = key_bytes {
                let key_str = String::from_utf8_lossy(&key_bytes);
                if key_str.starts_with("/shadowmesh/names/") {
                    let hash_suffix = key_str
                        .strip_prefix("/shadowmesh/names/")
                        .unwrap_or("");
                    let matching_name = pending_name_queries
                        .keys()
                        .find(|name| protocol::naming::name_to_hash(name) == hash_suffix)
                        .cloned();
                    if let Some(name) = matching_name {
                        if let Some(reply) = pending_name_queries.remove(&name) {
                            let _ = reply.send(Ok(None));
                        }
                    }
                } else if let Some(cid) = key_str.strip_prefix("/shadowmesh/content/") {
                    if let Some(reply) = pending_dht_queries.remove(cid) {
                        let _ = reply.send(Err(FetchError::NotFound));
                    }
                }
            }
        }
    }
}

// ── Command handling (HTTP → event loop) ─────────────────────────

/// Handle a command from an HTTP handler.
fn handle_command(
    cmd: P2pCommand,
    node: &mut ShadowNode,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    pending_dht_queries: &mut HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>>,
    pending_name_queries: &mut HashMap<
        String,
        oneshot::Sender<Result<Option<Vec<u8>>, FetchError>>,
    >,
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

        P2pCommand::PublishName {
            record_bytes,
            name,
        } => {
            // 1. Store in DHT
            let key = protocol::naming::name_to_key(&name);
            let kad_record = kad::Record {
                key,
                value: record_bytes.clone(),
                publisher: None,
                expires: None,
            };
            match node
                .swarm_mut()
                .behaviour_mut()
                .kademlia
                .put_record(kad_record, kad::Quorum::One)
            {
                Ok(_) => tracing::info!(%name, "Published name record to DHT"),
                Err(e) => tracing::warn!(%name, ?e, "Failed to publish name to DHT"),
            }

            // 2. Broadcast via GossipSub
            let topic = libp2p::gossipsub::IdentTopic::new(NAMING_GOSSIP_TOPIC);
            match node
                .swarm_mut()
                .behaviour_mut()
                .gossipsub
                .publish(topic, record_bytes)
            {
                Ok(_) => tracing::debug!(%name, "Broadcast name update via GossipSub"),
                Err(e) => {
                    tracing::debug!(%name, ?e, "GossipSub name publish failed (may have no peers)")
                }
            }
        }

        P2pCommand::ResolveName { name, reply } => {
            let key = protocol::naming::name_to_key(&name);
            node.swarm_mut()
                .behaviour_mut()
                .kademlia
                .get_record(key);
            pending_name_queries.insert(name, reply);
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
    use protocol::{ContentDHTMetadata, ContentRecord, ProviderInfo};

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
        ttl_seconds: 86400,
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
