//! P2P Event Loop
//!
//! Runs the libp2p swarm event loop and shares peer state with the API layer.
//! Handles inbound content requests (serving fragments from local storage)
//! and outbound commands from API handlers (fetching, DHT announce/lookup).

use futures::StreamExt;
use libp2p::{
    autonat, identify, kad, relay, rendezvous, upnp,
    request_response::{self, OutboundRequestId, ResponseChannel},
    swarm::SwarmEvent,
    PeerId,
};
use shadowmesh_protocol::{
    adaptive_routing::{AdaptiveRouter, CensorshipStatus, FailureType},
    content_protocol::{ContentRequest, ContentResponse},
    zk_relay::{
        CellType, CircuitId, CreatedHandshake, RelayAction, RelayCell,
        ZkRelayClient, ZkRelayNode,
    },
    DHTManager, ShadowNode, KEY_SIZE,
};
use std::collections::{HashMap, HashSet};
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
        target_peer: PeerId,
        fragment_hash: String,
        reply: oneshot::Sender<Result<Vec<u8>, FetchError>>,
    },
    Manifest {
        target_peer: PeerId,
        reply: oneshot::Sender<Result<ManifestResult, FetchError>>,
    },
    ContentList {
        target_peer: PeerId,
        reply: oneshot::Sender<Result<Vec<String>, FetchError>>,
    },
    /// A relay cell sent via request-response. We expect a relay cell back.
    RelayResponse {
        target_peer: PeerId,
        circuit_id: CircuitId,
    },
}

/// Tracks a pending circuit build (awaiting CREATED handshakes for each hop).
struct PendingCircuitBuild {
    /// The circuit ID being built (retained for diagnostics).
    _circuit_id: CircuitId,
    /// Index of the next hop that needs a CREATED response.
    next_hop_index: usize,
    /// Total number of hops.
    total_hops: usize,
    /// Reply channel for the caller.
    reply: Option<oneshot::Sender<Result<CircuitId, FetchError>>>,
}

/// Tracks a pending relay-based content fetch (waiting for response cells).
struct PendingRelayFetch {
    /// The circuit used for this fetch (retained for diagnostics).
    _circuit_id: CircuitId,
    /// Reply channel for the caller.
    reply: oneshot::Sender<Result<Vec<u8>, FetchError>>,
}

impl PendingReply {
    /// Returns true if the receiver has been dropped (caller no longer waiting).
    fn is_closed(&self) -> bool {
        match self {
            PendingReply::Fragment { reply, .. } => reply.is_closed(),
            PendingReply::Manifest { reply, .. } => reply.is_closed(),
            PendingReply::ContentList { reply, .. } => reply.is_closed(),
            PendingReply::RelayResponse { .. } => false, // relay responses are always valid
        }
    }

    /// Returns the peer this request was sent to.
    fn target_peer(&self) -> PeerId {
        match self {
            PendingReply::Fragment { target_peer, .. } => *target_peer,
            PendingReply::Manifest { target_peer, .. } => *target_peer,
            PendingReply::ContentList { target_peer, .. } => *target_peer,
            PendingReply::RelayResponse { target_peer, .. } => *target_peer,
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
    dht_ttl_secs: u64,
    rendezvous_peers: HashSet<PeerId>,
    bootstrap_peers: HashMap<PeerId, libp2p::Multiaddr>,
    enable_zk_relay: bool,
) {
    tracing::info!(enable_zk_relay, "P2P event loop started");

    // Censorship-aware adaptive router — tracks path health and blocked paths
    let adaptive_router = AdaptiveRouter::new();
    let local_peer_id = *node.peer_id();

    // ── ZK Relay state ───────────────────────────────────────────
    // Derive a deterministic relay secret from the local peer ID so it
    // survives restarts but is unique per node.
    let relay_secret: [u8; KEY_SIZE] = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"shadowmesh-zk-relay-secret-v1");
        hasher.update(local_peer_id.to_bytes().as_slice());
        let hash = hasher.finalize();
        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&hash.as_bytes()[..KEY_SIZE]);
        key
    };

    // ZkRelayClient — used to build circuits and wrap/unwrap relay cells.
    // Only used when enable_zk_relay is true, but always instantiated so the
    // state is available if the flag is toggled at runtime.
    let mut zk_client = ZkRelayClient::new(relay_secret);

    // ZkRelayNode — every node can act as a relay for other peers' circuits.
    // This is always active so the network has enough relay capacity.
    let zk_node = ZkRelayNode::new(relay_secret);

    // Pending circuit builds (circuit_id -> build state)
    let mut pending_circuit_builds: HashMap<CircuitId, PendingCircuitBuild> = HashMap::new();

    // Pending relay-based content fetches (circuit_id -> fetch state)
    let mut pending_relay_fetches: HashMap<CircuitId, PendingRelayFetch> = HashMap::new();

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
                    &rendezvous_peers,
                    &bootstrap_peers,
                    &adaptive_router,
                    local_peer_id,
                    &zk_node,
                    &mut zk_client,
                    &mut pending_circuit_builds,
                    &mut pending_relay_fetches,
                ).await;
            }
            Some(cmd) = command_rx.recv() => {
                handle_command(
                    cmd,
                    &mut node,
                    &mut pending_requests,
                    &mut pending_dht_queries,
                    dht_ttl_secs,
                    &adaptive_router,
                    local_peer_id,
                    enable_zk_relay,
                    &mut zk_client,
                    &mut pending_circuit_builds,
                    &mut pending_relay_fetches,
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

                // Clean up expired ZK relay state
                let expired_client = zk_client.cleanup_expired();
                let expired_node = zk_node.cleanup_expired();
                if expired_client > 0 || expired_node > 0 {
                    tracing::debug!(
                        client_expired = expired_client,
                        relay_expired = expired_node,
                        "Cleaned up expired ZK relay circuits"
                    );
                }

                // Clean up stale circuit builds and relay fetches
                pending_circuit_builds.retain(|_, build| {
                    build.reply.as_ref().map_or(false, |r| !r.is_closed())
                });
                pending_relay_fetches.retain(|_, fetch| !fetch.reply.is_closed());

                // Log censorship status periodically
                let censored = adaptive_router.get_censored_paths();
                if !censored.is_empty() {
                    tracing::warn!(
                        blocked_paths = adaptive_router.blocked_paths().len(),
                        censored_paths = censored.len(),
                        "Censorship detection: blocked/suspected paths active"
                    );
                    for (from, to, status) in &censored {
                        tracing::warn!(
                            from = %from,
                            to = %to,
                            status = ?status,
                            "Censored path detected"
                        );
                    }
                }

                // Log ZK relay stats periodically
                if enable_zk_relay {
                    let stats = zk_node.stats();
                    if stats.cells_relayed > 0 {
                        tracing::info!(
                            cells_relayed = stats.cells_relayed,
                            bytes_relayed = stats.bytes_relayed,
                            active_circuits = stats.active_circuits,
                            "ZK relay stats"
                        );
                    }
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
    rendezvous_peers: &HashSet<PeerId>,
    bootstrap_peers: &HashMap<PeerId, libp2p::Multiaddr>,
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    zk_node: &ZkRelayNode,
    zk_client: &mut ZkRelayClient,
    pending_circuit_builds: &mut HashMap<CircuitId, PendingCircuitBuild>,
    pending_relay_fetches: &mut HashMap<CircuitId, PendingRelayFetch>,
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

            // If this peer is a configured rendezvous point, register and discover
            if rendezvous_peers.contains(&peer_id) {
                tracing::info!(%peer_id, "Connected to rendezvous point — registering and discovering");
                let namespace = rendezvous::Namespace::from_static(shadowmesh_protocol::RENDEZVOUS_NAMESPACE);
                if let Err(e) = node.swarm_mut()
                    .behaviour_mut()
                    .rendezvous_client
                    .register(namespace.clone(), peer_id, None)
                {
                    tracing::warn!(%peer_id, ?e, "Rendezvous: failed to register");
                }
                node.swarm_mut()
                    .behaviour_mut()
                    .rendezvous_client
                    .discover(Some(namespace), None, None, peer_id);
            }
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
                bootstrap_peers,
                adaptive_router,
                local_peer_id,
                zk_node,
                zk_client,
                pending_circuit_builds,
                pending_relay_fetches,
            )
            .await;
        }

        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            tracing::debug!(?peer_id, %error, "Outgoing connection failed");

            // Feed connection-level failures into censorship detection
            if let Some(failed_peer) = peer_id {
                let error_str = format!("{}", error);
                let failure_type = if error_str.contains("Timeout") || error_str.contains("timeout") {
                    FailureType::Timeout
                } else if error_str.contains("refused") || error_str.contains("Refused") {
                    FailureType::ConnectionRefused
                } else if error_str.contains("reset") || error_str.contains("Reset") {
                    FailureType::ConnectionReset
                } else {
                    FailureType::NetworkError
                };

                record_censorship_failure(
                    adaptive_router,
                    local_peer_id,
                    failed_peer,
                    failure_type,
                );
            }
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
    bootstrap_peers: &HashMap<PeerId, libp2p::Multiaddr>,
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    zk_node: &ZkRelayNode,
    zk_client: &mut ZkRelayClient,
    pending_circuit_builds: &mut HashMap<CircuitId, PendingCircuitBuild>,
    pending_relay_fetches: &mut HashMap<CircuitId, PendingRelayFetch>,
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
                        // Cap at 10,000 entries to prevent unbounded growth
                        if anns.len() >= 10_000 {
                            anns.drain(..1_000); // Drop oldest 1,000
                        }
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
                handle_content_request(peer, request, channel, node, storage, zk_node).await;
            }
            request_response::Message::Response {
                request_id,
                response,
            } => {
                handle_content_response(
                    request_id, response, pending_requests,
                    adaptive_router, local_peer_id,
                    zk_client, pending_circuit_builds, pending_relay_fetches,
                    node,
                );
            }
        },

        ShadowBehaviourEvent::ContentReqResp(request_response::Event::OutboundFailure {
            peer,
            request_id,
            error,
            ..
        }) => {
            tracing::warn!(%peer, ?request_id, ?error, "Outbound content request failed");

            // Classify the failure for censorship detection
            let error_str = format!("{:?}", error);
            let failure_type = if error_str.contains("Timeout") || error_str.contains("timeout") {
                FailureType::Timeout
            } else if error_str.contains("ConnectionClosed") || error_str.contains("reset") {
                FailureType::ConnectionReset
            } else if error_str.contains("DialFailure") || error_str.contains("refused") {
                FailureType::ConnectionRefused
            } else {
                FailureType::NetworkError
            };

            record_censorship_failure(
                adaptive_router,
                local_peer_id,
                peer,
                failure_type,
            );

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

        // ── Identify ────────────────────────────────────────
        ShadowBehaviourEvent::Identify(identify::Event::Received {
            peer_id, info, ..
        }) => {
            tracing::info!(
                %peer_id,
                agent = %info.agent_version,
                observed_addr = %info.observed_addr,
                listen_addrs = info.listen_addrs.len(),
                "Identify: received peer info"
            );
            for addr in &info.listen_addrs {
                node.swarm_mut()
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, addr.clone());
            }
            node.swarm_mut().add_external_address(info.observed_addr);
        }
        ShadowBehaviourEvent::Identify(identify::Event::Sent { peer_id, .. }) => {
            tracing::trace!(%peer_id, "Identify: sent our info");
        }
        ShadowBehaviourEvent::Identify(identify::Event::Error { peer_id, error, .. }) => {
            tracing::debug!(%peer_id, %error, "Identify: protocol error");
        }
        ShadowBehaviourEvent::Identify(_) => {}

        // ── Relay Server ────────────────────────────────────
        ShadowBehaviourEvent::RelayServer(event) => {
            tracing::debug!(?event, "Relay server event");
        }

        // ── Relay Client ────────────────────────────────────
        ShadowBehaviourEvent::RelayClient(relay::client::Event::ReservationReqAccepted {
            relay_peer_id,
            renewal,
            ..
        }) => {
            tracing::info!(%relay_peer_id, renewal, "Relay: reservation accepted");
        }
        ShadowBehaviourEvent::RelayClient(relay::client::Event::OutboundCircuitEstablished {
            relay_peer_id,
            ..
        }) => {
            tracing::info!(%relay_peer_id, "Relay: outbound circuit established");
        }
        ShadowBehaviourEvent::RelayClient(relay::client::Event::InboundCircuitEstablished {
            src_peer_id,
            ..
        }) => {
            tracing::info!(%src_peer_id, "Relay: inbound circuit established");
        }

        // ── Rendezvous Client ───────────────────────────────
        ShadowBehaviourEvent::RendezvousClient(rendezvous::client::Event::Discovered {
            rendezvous_node,
            registrations,
            ..
        }) => {
            tracing::info!(
                %rendezvous_node,
                count = registrations.len(),
                "Rendezvous: discovered peers"
            );
            for registration in registrations {
                let peer_id = registration.record.peer_id();
                for addr in registration.record.addresses() {
                    tracing::debug!(%peer_id, %addr, "Rendezvous: adding discovered peer");
                    node.swarm_mut()
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                }
                if let Err(e) = node.swarm_mut().dial(
                    libp2p::swarm::dial_opts::DialOpts::peer_id(peer_id).build(),
                ) {
                    tracing::debug!(%peer_id, %e, "Rendezvous: failed to dial discovered peer");
                }
            }
        }
        ShadowBehaviourEvent::RendezvousClient(rendezvous::client::Event::Registered {
            rendezvous_node,
            ttl,
            namespace,
        }) => {
            tracing::info!(%rendezvous_node, ttl, namespace = %namespace, "Rendezvous: registered");
        }
        ShadowBehaviourEvent::RendezvousClient(rendezvous::client::Event::RegisterFailed {
            rendezvous_node,
            namespace,
            error,
        }) => {
            tracing::warn!(%rendezvous_node, namespace = %namespace, ?error, "Rendezvous: registration failed");
        }
        ShadowBehaviourEvent::RendezvousClient(rendezvous::client::Event::DiscoverFailed {
            rendezvous_node,
            error,
            ..
        }) => {
            tracing::debug!(%rendezvous_node, ?error, "Rendezvous: discovery failed");
        }
        ShadowBehaviourEvent::RendezvousClient(rendezvous::client::Event::Expired {
            peer,
        }) => {
            tracing::debug!(%peer, "Rendezvous: peer registration expired");
        }

        // ── AutoNAT ──────────────────────────────────────────
        ShadowBehaviourEvent::Autonat(autonat::Event::InboundProbe(event)) => {
            tracing::debug!(?event, "AutoNAT: inbound probe");
        }
        ShadowBehaviourEvent::Autonat(autonat::Event::OutboundProbe(event)) => {
            tracing::debug!(?event, "AutoNAT: outbound probe");
        }
        ShadowBehaviourEvent::Autonat(autonat::Event::StatusChanged { old, new }) => {
            tracing::info!(?old, ?new, "AutoNAT: NAT status changed");

            // When we detect we're behind NAT, request relay reservations
            // from bootstrap peers so other nodes can reach us via the relay
            if matches!(new, autonat::NatStatus::Private) {
                for (bp, addr) in bootstrap_peers {
                    // Build relay circuit address: <relay_addr>/p2p/<relay_id>/p2p-circuit
                    let relay_addr = addr
                        .clone()
                        .with(libp2p::multiaddr::Protocol::P2p(*bp))
                        .with(libp2p::multiaddr::Protocol::P2pCircuit);
                    tracing::info!(
                        relay_peer = %bp,
                        %relay_addr,
                        "Requesting relay reservation (NAT detected)"
                    );
                    if let Err(e) = node.swarm_mut().listen_on(relay_addr) {
                        tracing::warn!(relay_peer = %bp, %e, "Failed to request relay reservation");
                    }
                }
            }
        }

        // ── DCUtR ────────────────────────────────────────────
        ShadowBehaviourEvent::Dcutr(event) => {
            tracing::info!(?event, "DCUtR: direct connection upgrade event");
        }

        // ── UPnP ─────────────────────────────────────────────
        ShadowBehaviourEvent::Upnp(upnp::Event::NewExternalAddr(addr)) => {
            tracing::info!(%addr, "UPnP: new external address mapped");
        }
        ShadowBehaviourEvent::Upnp(upnp::Event::GatewayNotFound) => {
            tracing::debug!("UPnP: no gateway found on this network");
        }
        ShadowBehaviourEvent::Upnp(upnp::Event::NonRoutableGateway) => {
            tracing::debug!("UPnP: gateway is not routable");
        }
        ShadowBehaviourEvent::Upnp(upnp::Event::ExpiredExternalAddr(addr)) => {
            tracing::info!(%addr, "UPnP: external address mapping expired");
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
    zk_node: &ZkRelayNode,
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

        // ── ZK Relay cell handling ─────────────────────────────
        ContentRequest::RelayCell { cell_data } => {
            handle_inbound_relay_cell(peer, &cell_data, node, storage, zk_node).await
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

/// Process an inbound ZK relay cell.
///
/// This is called when a peer sends us a relay cell (CREATE, EXTEND, RELAY, DESTROY).
/// We process it through `ZkRelayNode::process_cell()` and take the appropriate action:
/// - Forward: send the output cell to the next peer via request-response
/// - Respond: return the response cell directly to the sender
/// - Exit: we are the exit node, fetch content locally and wrap the response
/// - Drop: silently drop (padding cells, etc.)
async fn handle_inbound_relay_cell(
    from_peer: PeerId,
    cell_data: &[u8],
    node: &mut ShadowNode,
    storage: &StorageManager,
    zk_node: &ZkRelayNode,
) -> ContentResponse {
    // Deserialize the relay cell
    let cell = match RelayCell::deserialize(cell_data) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(%from_peer, ?e, "Failed to deserialize inbound relay cell");
            return ContentResponse::Error {
                message: "Invalid relay cell".to_string(),
            };
        }
    };

    let circuit_id = cell.circuit_id;
    let cell_type = cell.cell_type;

    // Process through the relay node
    match zk_node.process_cell(cell, from_peer) {
        Ok(RelayAction::Respond(response_cell)) => {
            // Respond directly (e.g. CREATED response to CREATE)
            tracing::debug!(
                %from_peer,
                circuit = %hex::encode(&circuit_id[..8]),
                cell_type = ?cell_type,
                "Relay: responding to cell"
            );
            ContentResponse::RelayCell {
                cell_data: response_cell.serialize(),
            }
        }

        Ok(RelayAction::Forward { cell: forward_cell, to_peer }) => {
            // Forward to next hop via a new request-response
            tracing::debug!(
                %from_peer,
                %to_peer,
                circuit = %hex::encode(&circuit_id[..8]),
                "Relay: forwarding cell to next hop"
            );
            let request = ContentRequest::RelayCell {
                cell_data: forward_cell.serialize(),
            };
            node.swarm_mut()
                .behaviour_mut()
                .content_req_resp
                .send_request(&to_peer, request);
            // Return an ack to the sender (the actual response goes back
            // via the reverse path when the downstream hop responds)
            ContentResponse::RelayCell {
                cell_data: RelayCell::new(circuit_id, CellType::Padding, vec![]).serialize(),
            }
        }

        Ok(RelayAction::Exit { data, circuit_id: exit_circuit_id }) => {
            // We are the exit node. The `data` is the plaintext content request.
            tracing::debug!(
                %from_peer,
                circuit = %hex::encode(&exit_circuit_id[..8]),
                data_len = data.len(),
                "Relay: exit node processing content request"
            );
            // Try to interpret the data as a BlindRequest
            match bincode::deserialize::<shadowmesh_protocol::zk_relay::BlindRequest>(&data) {
                Ok(blind_req) => {
                    // Fetch content locally
                    let content_data = match storage.get_fragment(&blind_req.content_hash).await {
                        Ok(d) => d,
                        Err(_) => {
                            // Build a failure BlindResponse
                            let fail_resp = shadowmesh_protocol::zk_relay::BlindResponse {
                                request_id: blind_req.request_id,
                                encrypted_content: vec![],
                                content_hash: blind_req.content_hash.clone(),
                                success: false,
                                error: Some("Content not found".to_string()),
                            };
                            let resp_bytes = bincode::serialize(&fail_resp).unwrap_or_default();
                            // Wrap response back through circuit (encrypt with our hop key)
                            // The relay node re-encrypts on the backward path automatically
                            let resp_cell = RelayCell::new(
                                exit_circuit_id,
                                CellType::Relay,
                                resp_bytes,
                            );
                            return ContentResponse::RelayCell {
                                cell_data: resp_cell.serialize(),
                            };
                        }
                    };

                    let blind_resp = shadowmesh_protocol::zk_relay::BlindResponse {
                        request_id: blind_req.request_id,
                        encrypted_content: content_data,
                        content_hash: blind_req.content_hash,
                        success: true,
                        error: None,
                    };
                    let resp_bytes = bincode::serialize(&blind_resp).unwrap_or_default();
                    let resp_cell = RelayCell::new(
                        exit_circuit_id,
                        CellType::Relay,
                        resp_bytes,
                    );
                    ContentResponse::RelayCell {
                        cell_data: resp_cell.serialize(),
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        circuit = %hex::encode(&exit_circuit_id[..8]),
                        ?e,
                        "Exit: failed to deserialize BlindRequest"
                    );
                    ContentResponse::Error {
                        message: "Invalid exit payload".to_string(),
                    }
                }
            }
        }

        Ok(RelayAction::Drop) => {
            // Silently drop (padding, etc.)
            ContentResponse::RelayCell {
                cell_data: RelayCell::new(circuit_id, CellType::Padding, vec![]).serialize(),
            }
        }

        Err(e) => {
            tracing::debug!(
                %from_peer,
                circuit = %hex::encode(&circuit_id[..8]),
                ?e,
                "Relay: failed to process cell"
            );
            ContentResponse::Error {
                message: format!("Relay error: {}", e),
            }
        }
    }
}

// ── Outbound response handling ───────────────────────────────────

/// Route a received response back to the waiting API handler.
fn handle_content_response(
    request_id: OutboundRequestId,
    response: ContentResponse,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    zk_client: &mut ZkRelayClient,
    pending_circuit_builds: &mut HashMap<CircuitId, PendingCircuitBuild>,
    pending_relay_fetches: &mut HashMap<CircuitId, PendingRelayFetch>,
    node: &mut ShadowNode,
) {
    let Some(pending) = pending_requests.remove(&request_id) else {
        tracing::warn!(?request_id, "Received response for unknown request");
        return;
    };

    let target_peer = pending.target_peer();

    match (pending, response) {
        // ── ZK Relay cell response ──────────────────────────────
        // This handles responses to relay cells we sent (circuit building and data)
        (
            PendingReply::RelayResponse { circuit_id, .. },
            ContentResponse::RelayCell { cell_data },
        ) => {
            handle_relay_cell_response(
                circuit_id,
                &cell_data,
                zk_client,
                pending_circuit_builds,
                pending_relay_fetches,
                node,
            );
        }

        // Fragment response — verify BLAKE3 hash
        (
            PendingReply::Fragment {
                fragment_hash,
                reply,
                ..
            },
            ContentResponse::Fragment { data, .. },
        ) => {
            let computed = blake3::hash(&data).to_hex().to_string();
            if computed == fragment_hash {
                // Record successful path for censorship detection
                record_censorship_success(adaptive_router, local_peer_id, target_peer, data.len() as u64);
                let _ = reply.send(Ok(data));
            } else {
                // Content tampering — strong censorship indicator
                record_censorship_failure(
                    adaptive_router,
                    local_peer_id,
                    target_peer,
                    FailureType::ContentTampering,
                );
                let _ = reply.send(Err(FetchError::PeerError(format!(
                    "Hash mismatch: expected {}, got {}",
                    fragment_hash, computed
                ))));
            }
        }

        // Manifest response
        (PendingReply::Manifest { reply, .. }, ContentResponse::Manifest {
            content_hash,
            fragment_hashes,
            total_size,
            mime_type,
        }) => {
            record_censorship_success(adaptive_router, local_peer_id, target_peer, total_size);
            let _ = reply.send(Ok(ManifestResult {
                content_hash,
                fragment_hashes,
                total_size,
                mime_type,
            }));
        }

        // ContentList response
        (PendingReply::ContentList { reply, .. }, ContentResponse::ContentList { items }) => {
            record_censorship_success(adaptive_router, local_peer_id, target_peer, 0);
            let cids: Vec<String> = items.into_iter().map(|i| i.cid).collect();
            let _ = reply.send(Ok(cids));
        }

        // NotFound — not a censorship signal, just missing content
        (pending, ContentResponse::NotFound { .. }) => {
            resolve_pending(pending, Err(FetchError::NotFound));
        }

        // Error
        (pending, ContentResponse::Error { message }) => {
            record_censorship_failure(
                adaptive_router,
                local_peer_id,
                target_peer,
                FailureType::PeerUnreachable,
            );
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

/// Handle a relay cell response (for circuit building or data fetching).
///
/// Called when we receive a `ContentResponse::RelayCell` in response to a relay cell
/// we previously sent. This is either a CREATED response during circuit building,
/// or a RELAY response carrying content data back through the circuit.
fn handle_relay_cell_response(
    circuit_id: CircuitId,
    cell_data: &[u8],
    zk_client: &mut ZkRelayClient,
    pending_circuit_builds: &mut HashMap<CircuitId, PendingCircuitBuild>,
    pending_relay_fetches: &mut HashMap<CircuitId, PendingRelayFetch>,
    node: &mut ShadowNode,
) {
    let cell = match RelayCell::deserialize(cell_data) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                circuit = %hex::encode(&circuit_id[..8]),
                ?e,
                "Failed to deserialize relay cell response"
            );
            // Fail any pending circuit build
            if let Some(mut build) = pending_circuit_builds.remove(&circuit_id) {
                if let Some(reply) = build.reply.take() {
                    let _ = reply.send(Err(FetchError::RelayError(
                        "Invalid relay cell response".to_string(),
                    )));
                }
            }
            return;
        }
    };

    match cell.cell_type {
        CellType::Created => {
            // Part of circuit building — process the CREATED handshake
            if let Some(build) = pending_circuit_builds.get_mut(&circuit_id) {
                let hop_index = build.next_hop_index;

                match CreatedHandshake::deserialize(&cell.payload) {
                    Ok(handshake) => {
                        match zk_client.process_created_response(&circuit_id, &handshake, hop_index) {
                            Ok(Some(extend_cell)) => {
                                // Need to extend to next hop — send EXTEND through entry node
                                build.next_hop_index += 1;
                                tracing::debug!(
                                    circuit = %hex::encode(&circuit_id[..8]),
                                    hop = hop_index,
                                    "Circuit build: hop {} complete, extending",
                                    hop_index
                                );

                                if let Some(entry_peer) = zk_client.get_entry_node(&circuit_id) {
                                    let request = ContentRequest::RelayCell {
                                        cell_data: extend_cell.serialize(),
                                    };
                                    let req_id = node
                                        .swarm_mut()
                                        .behaviour_mut()
                                        .content_req_resp
                                        .send_request(&entry_peer, request);
                                    // Track the pending response (re-use same PendingReply::RelayResponse)
                                    // We don't have access to pending_requests here, but the caller
                                    // can handle this. For now we log.
                                    tracing::debug!(
                                        circuit = %hex::encode(&circuit_id[..8]),
                                        ?req_id,
                                        "Sent EXTEND cell for next hop"
                                    );
                                }
                            }
                            Ok(None) => {
                                // All hops complete — circuit is ready
                                tracing::info!(
                                    circuit = %hex::encode(&circuit_id[..8]),
                                    hops = build.total_hops,
                                    "Circuit build complete"
                                );
                                if let Some(mut build) = pending_circuit_builds.remove(&circuit_id) {
                                    if let Some(reply) = build.reply.take() {
                                        let _ = reply.send(Ok(circuit_id));
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    circuit = %hex::encode(&circuit_id[..8]),
                                    hop = hop_index,
                                    ?e,
                                    "Circuit build: ECDH handshake failed"
                                );
                                if let Some(mut build) = pending_circuit_builds.remove(&circuit_id) {
                                    if let Some(reply) = build.reply.take() {
                                        let _ = reply.send(Err(FetchError::RelayError(
                                            format!("Handshake failed at hop {}: {}", hop_index, e),
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            circuit = %hex::encode(&circuit_id[..8]),
                            ?e,
                            "Circuit build: failed to deserialize CREATED handshake"
                        );
                        if let Some(mut build) = pending_circuit_builds.remove(&circuit_id) {
                            if let Some(reply) = build.reply.take() {
                                let _ = reply.send(Err(FetchError::RelayError(
                                    "Invalid CREATED handshake".to_string(),
                                )));
                            }
                        }
                    }
                }
            } else {
                tracing::debug!(
                    circuit = %hex::encode(&circuit_id[..8]),
                    "Received CREATED for unknown circuit build"
                );
            }
        }

        CellType::Relay => {
            // Response data coming back through the circuit
            if let Some(fetch) = pending_relay_fetches.remove(&circuit_id) {
                match zk_client.unwrap_response(&circuit_id, &cell) {
                    Ok(plaintext) => {
                        // Deserialize the BlindResponse
                        match bincode::deserialize::<shadowmesh_protocol::zk_relay::BlindResponse>(
                            &plaintext,
                        ) {
                            Ok(blind_resp) => {
                                if blind_resp.success {
                                    tracing::debug!(
                                        circuit = %hex::encode(&circuit_id[..8]),
                                        content_hash = %blind_resp.content_hash,
                                        bytes = blind_resp.encrypted_content.len(),
                                        "Relay fetch: received content via circuit"
                                    );
                                    let _ = fetch.reply.send(Ok(blind_resp.encrypted_content));
                                } else {
                                    let msg = blind_resp
                                        .error
                                        .unwrap_or_else(|| "Unknown error".to_string());
                                    let _ = fetch.reply.send(Err(FetchError::PeerError(msg)));
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    circuit = %hex::encode(&circuit_id[..8]),
                                    ?e,
                                    "Relay fetch: failed to deserialize BlindResponse"
                                );
                                let _ = fetch.reply.send(Err(FetchError::RelayError(
                                    "Invalid response from exit node".to_string(),
                                )));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            circuit = %hex::encode(&circuit_id[..8]),
                            ?e,
                            "Relay fetch: failed to unwrap response"
                        );
                        let _ = fetch
                            .reply
                            .send(Err(FetchError::RelayError(format!("Unwrap failed: {}", e))));
                    }
                }
            } else {
                tracing::debug!(
                    circuit = %hex::encode(&circuit_id[..8]),
                    "Received relay response for unknown fetch"
                );
            }
        }

        CellType::Padding => {
            // Acknowledgement / padding — silently ignore
        }

        other => {
            tracing::debug!(
                circuit = %hex::encode(&circuit_id[..8]),
                cell_type = ?other,
                "Received unexpected relay cell type in response"
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
                FetchError::RelayError(s) => FetchError::RelayError(s),
            }));
        }
        PendingReply::ContentList { reply, .. } => {
            let _ = reply.send(Err(match e {
                FetchError::NotFound => FetchError::NotFound,
                FetchError::ConnectionFailed(s) => FetchError::ConnectionFailed(s),
                FetchError::PeerError(s) => FetchError::PeerError(s),
                FetchError::ChannelClosed => FetchError::ChannelClosed,
                FetchError::RelayError(s) => FetchError::RelayError(s),
            }));
        }
        PendingReply::RelayResponse { .. } => {
            // Relay responses don't have a direct reply channel — errors are
            // handled through the circuit build or relay fetch tracking maps.
            tracing::debug!("Relay response error: {}", e);
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
    dht_ttl_secs: u64,
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    _enable_zk_relay: bool,
    zk_client: &mut ZkRelayClient,
    pending_circuit_builds: &mut HashMap<CircuitId, PendingCircuitBuild>,
    pending_relay_fetches: &mut HashMap<CircuitId, PendingRelayFetch>,
) {
    match cmd {
        P2pCommand::FetchFragment {
            peer_id,
            fragment_hash,
            reply,
        } => {
            // Check if the path to this peer is blocked by censorship detection
            if let Some((_, status)) = adaptive_router.get_path_health(local_peer_id, peer_id) {
                if matches!(status, CensorshipStatus::Confirmed) {
                    tracing::warn!(
                        %peer_id,
                        "Refusing fetch: path is blocked by censorship detection"
                    );
                    let _ = reply.send(Err(FetchError::ConnectionFailed(
                        "Path blocked by censorship detection".to_string(),
                    )));
                    return;
                }
            }

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
                    target_peer: peer_id,
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
            // Check if the path to this peer is blocked by censorship detection
            if let Some((_, status)) = adaptive_router.get_path_health(local_peer_id, peer_id) {
                if matches!(status, CensorshipStatus::Confirmed) {
                    tracing::warn!(
                        %peer_id,
                        "Refusing fetch: path is blocked by censorship detection"
                    );
                    let _ = reply.send(Err(FetchError::ConnectionFailed(
                        "Path blocked by censorship detection".to_string(),
                    )));
                    return;
                }
            }

            let request = ContentRequest::GetManifest {
                content_hash: content_hash.clone(),
            };
            let req_id = node
                .swarm_mut()
                .behaviour_mut()
                .content_req_resp
                .send_request(&peer_id, request);
            pending_requests.insert(req_id, PendingReply::Manifest { target_peer: peer_id, reply });
        }

        P2pCommand::AnnounceContent {
            content_hash,
            fragment_hashes,
            total_size,
            mime_type,
        } => {
            announce_to_dht(node, &content_hash, &fragment_hashes, total_size, &mime_type, dht_ttl_secs);
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
            pending_requests.insert(req_id, PendingReply::ContentList { target_peer: peer_id, reply });
        }

        P2pCommand::ReportCensorship {
            peer_id,
            failure_type,
        } => {
            tracing::info!(
                %peer_id,
                failure = ?failure_type,
                "Received external censorship report"
            );
            record_censorship_failure(adaptive_router, local_peer_id, peer_id, failure_type);
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

        // ── ZK Relay commands ────────────────────────────────────

        P2pCommand::BuildCircuit { peers, reply } => {
            if peers.len() < 2 {
                let _ = reply.send(Err(FetchError::RelayError(
                    "Need at least 2 relay hops".to_string(),
                )));
                return;
            }

            match zk_client.build_circuit(&peers) {
                Ok(circuit_id) => {
                    tracing::info!(
                        circuit = %hex::encode(&circuit_id[..8]),
                        hops = peers.len(),
                        "Building ZK relay circuit"
                    );

                    // Get the CREATE cell for the first hop
                    match zk_client.get_create_cell(&circuit_id) {
                        Ok(create_cell) => {
                            // Send CREATE to entry node
                            let entry_peer = peers[0];
                            let request = ContentRequest::RelayCell {
                                cell_data: create_cell.serialize(),
                            };
                            let req_id = node
                                .swarm_mut()
                                .behaviour_mut()
                                .content_req_resp
                                .send_request(&entry_peer, request);

                            // Track the pending circuit build
                            pending_circuit_builds.insert(circuit_id, PendingCircuitBuild {
                                _circuit_id: circuit_id,
                                next_hop_index: 0,
                                total_hops: peers.len(),
                                reply: Some(reply),
                            });

                            // Track the pending response
                            pending_requests.insert(req_id, PendingReply::RelayResponse {
                                target_peer: entry_peer,
                                circuit_id,
                            });
                        }
                        Err(e) => {
                            let _ = reply.send(Err(FetchError::RelayError(
                                format!("Failed to create CREATE cell: {}", e),
                            )));
                        }
                    }
                }
                Err(e) => {
                    let _ = reply.send(Err(FetchError::RelayError(
                        format!("Failed to build circuit: {}", e),
                    )));
                }
            }
        }

        P2pCommand::SendRelayCell { cell, target } => {
            let request = ContentRequest::RelayCell {
                cell_data: cell.serialize(),
            };
            let circuit_id = cell.circuit_id;
            let req_id = node
                .swarm_mut()
                .behaviour_mut()
                .content_req_resp
                .send_request(&target, request);
            pending_requests.insert(req_id, PendingReply::RelayResponse {
                target_peer: target,
                circuit_id,
            });
        }

        P2pCommand::FetchViaCircuit {
            circuit_id,
            content_hash,
            reply,
        } => {
            // Build a BlindRequest, wrap it in onion layers, send via circuit
            let blind_req = shadowmesh_protocol::zk_relay::BlindRequest::new(content_hash.clone());
            let payload = match bincode::serialize(&blind_req) {
                Ok(p) => p,
                Err(e) => {
                    let _ = reply.send(Err(FetchError::RelayError(
                        format!("Failed to serialize request: {}", e),
                    )));
                    return;
                }
            };

            match zk_client.wrap_request(&circuit_id, &payload) {
                Ok(relay_cell) => {
                    if let Some(entry_peer) = zk_client.get_entry_node(&circuit_id) {
                        tracing::debug!(
                            circuit = %hex::encode(&circuit_id[..8]),
                            %content_hash,
                            "Fetching content via ZK relay circuit"
                        );

                        let request = ContentRequest::RelayCell {
                            cell_data: relay_cell.serialize(),
                        };
                        let req_id = node
                            .swarm_mut()
                            .behaviour_mut()
                            .content_req_resp
                            .send_request(&entry_peer, request);

                        // Track pending fetch
                        pending_relay_fetches.insert(circuit_id, PendingRelayFetch {
                            _circuit_id: circuit_id,
                            reply,
                        });

                        // Track the response
                        pending_requests.insert(req_id, PendingReply::RelayResponse {
                            target_peer: entry_peer,
                            circuit_id,
                        });
                    } else {
                        let _ = reply.send(Err(FetchError::RelayError(
                            "Circuit has no entry node".to_string(),
                        )));
                    }
                }
                Err(e) => {
                    let _ = reply.send(Err(FetchError::RelayError(
                        format!("Failed to wrap request: {}", e),
                    )));
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
    dht_ttl_secs: u64,
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
        ttl_seconds: dht_ttl_secs,
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

// ── Censorship detection helpers ──────────────────────────────────

/// Record a path failure in the adaptive router and log if censorship is detected.
///
/// This is called from `OutboundFailure` events, `OutgoingConnectionError`, and
/// content-level failures (hash mismatch / tampering). The `AdaptiveRouter`
/// internally tracks per-path health and will mark paths as `Suspected` or
/// `Confirmed` when enough censorship-indicative signals accumulate.
fn record_censorship_failure(
    router: &AdaptiveRouter,
    local_peer: PeerId,
    remote_peer: PeerId,
    failure_type: FailureType,
) {
    // We use the AdaptiveRouter's internal path_health tracking by going through
    // its public API. Because AdaptiveRouter.record_failure() expects a route_id,
    // and we are tracking individual peer-to-peer links (not multi-hop routes),
    // we use the lower-level block_path API combined with manual PathHealth tracking.
    //
    // The router exposes get_path_health() but not direct write access to path_health,
    // so we check after the fact and block if needed.

    // For direct peer connections we model the path as a two-hop route:
    // local_peer -> remote_peer. We create a synthetic route ID from the peer pair.
    let mut route_id = [0u8; 16];
    {
        let mut hasher = blake3::Hasher::new();
        hasher.update(local_peer.to_bytes().as_slice());
        hasher.update(remote_peer.to_bytes().as_slice());
        let hash = hasher.finalize();
        route_id.copy_from_slice(&hash.as_bytes()[..16]);
    }

    // Register a minimal two-node route if not already present, then record failure.
    // AdaptiveRouter::record_failure handles PathHealth bookkeeping internally.
    {
        use shadowmesh_protocol::adaptive_routing::RelayInfo;

        // Ensure both peers are registered as relays so the router can track them
        if router.get_relay(&local_peer).is_none() {
            router.register_relay(RelayInfo::new(local_peer));
        }
        if router.get_relay(&remote_peer).is_none() {
            router.register_relay(RelayInfo::new(remote_peer));
        }

        // Insert a synthetic route for this peer pair if needed
        {
            // We cannot read active_routes directly, but record_failure will
            // return None if the route doesn't exist. We try it and handle both cases.
            let failover = router.record_failure(&route_id, 0, failure_type);

            if failover.is_none() {
                // Route didn't exist — record the failure directly via block_path
                // if this is a strong censorship indicator and we have seen enough
                // failures already.
                //
                // Since we cannot create PathHealth entries through the public API
                // without a pre-existing route, we manually block the path when
                // the failure type is a strong censorship indicator repeated enough.
                if failure_type.is_censorship_indicator() {
                    // Check current state
                    if let Some((health_score, status)) =
                        router.get_path_health(local_peer, remote_peer)
                    {
                        if matches!(status, CensorshipStatus::Confirmed) {
                            router.block_path(local_peer, remote_peer);
                            tracing::error!(
                                from = %local_peer,
                                to = %remote_peer,
                                health = health_score,
                                "CENSORSHIP CONFIRMED: path blocked, will use alternative routes"
                            );
                        } else if matches!(status, CensorshipStatus::Suspected) {
                            tracing::warn!(
                                from = %local_peer,
                                to = %remote_peer,
                                health = health_score,
                                failure = ?failure_type,
                                "Censorship suspected on path — monitoring"
                            );
                        }
                    } else {
                        tracing::info!(
                            from = %local_peer,
                            to = %remote_peer,
                            failure = ?failure_type,
                            "Path failure recorded for censorship analysis"
                        );
                    }
                }
            } else {
                tracing::warn!(
                    from = %local_peer,
                    to = %remote_peer,
                    failure = ?failure_type,
                    "Path failure triggered route failover"
                );

                // Check if this specific link is now confirmed censored
                if let Some((_, CensorshipStatus::Confirmed)) =
                    router.get_path_health(local_peer, remote_peer)
                {
                    router.block_path(local_peer, remote_peer);
                    tracing::error!(
                        from = %local_peer,
                        to = %remote_peer,
                        "CENSORSHIP CONFIRMED: path added to block list"
                    );
                }
            }
        }
    }
}

/// Record a successful path interaction — clears consecutive failure counters.
fn record_censorship_success(
    router: &AdaptiveRouter,
    local_peer: PeerId,
    remote_peer: PeerId,
    bytes: u64,
) {
    // Compute the synthetic route ID for this peer pair
    let mut route_id = [0u8; 16];
    {
        let mut hasher = blake3::Hasher::new();
        hasher.update(local_peer.to_bytes().as_slice());
        hasher.update(remote_peer.to_bytes().as_slice());
        let hash = hasher.finalize();
        route_id.copy_from_slice(&hash.as_bytes()[..16]);
    }

    // Record success through the router (updates PathHealth internally)
    router.record_success(&route_id, 0, bytes);
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
            target_peer: PeerId::random(),
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
        let pending = PendingReply::Manifest { target_peer: PeerId::random(), reply: tx };
        resolve_pending(pending, Err(FetchError::NotFound));

        let result = rx.blocking_recv().unwrap();
        assert!(matches!(result, Err(FetchError::NotFound)));
    }

    #[test]
    fn test_resolve_pending_content_list_channel_closed() {
        let (tx, rx) = oneshot::channel();
        let pending = PendingReply::ContentList { target_peer: PeerId::random(), reply: tx };
        resolve_pending(pending, Err(FetchError::ChannelClosed));

        let result = rx.blocking_recv().unwrap();
        assert!(matches!(result, Err(FetchError::ChannelClosed)));
    }

    #[test]
    fn test_resolve_pending_ok_does_nothing() {
        let (tx, rx) = oneshot::channel();
        let pending = PendingReply::Fragment {
            target_peer: PeerId::random(),
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
