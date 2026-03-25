//! Gateway P2P Event Loop
//!
//! Runs a ShadowNode to participate in the P2P mesh. Handles:
//! - Inbound content requests (serves cached content to peers)
//! - Outbound commands from HTTP handlers (DHT lookups, content fetching)
//! - mDNS / Kademlia peer discovery

use futures::StreamExt;
use libp2p::{
    autonat, identify, kad, relay, rendezvous, upnp,
    request_response::{self, OutboundRequestId, ResponseChannel},
    swarm::SwarmEvent,
    PeerId,
};
use protocol::{
    adaptive_routing::{AdaptiveRouter, CensorshipStatus, FailureType},
    content_protocol::{ContentRequest, ContentResponse},
    zk_relay::{
        CellType, CircuitId, CreatedHandshake, RelayAction, RelayCell, ZkRelayClient,
        ZkRelayConfig, ZkRelayNode,
    },
    DHTManager, NamingManager, ShadowNode, NAMING_GOSSIP_TOPIC,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

use crate::cache::ContentCache;
use crate::config::P2pConfig;
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
    /// Whether ZK relay is enabled
    pub zk_relay_enabled: bool,
    /// Number of hops for ZK relay circuits
    pub zk_relay_hops: usize,
}

impl P2pState {
    pub fn new(command_tx: mpsc::Sender<P2pCommand>) -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
            listen_addrs: RwLock::new(Vec::new()),
            command_tx,
            zk_relay_enabled: false,
            zk_relay_hops: 3,
        }
    }

    /// Create with ZK relay configuration
    pub fn with_p2p_config(command_tx: mpsc::Sender<P2pCommand>, p2p_config: &P2pConfig) -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
            listen_addrs: RwLock::new(Vec::new()),
            command_tx,
            zk_relay_enabled: p2p_config.enable_zk_relay,
            zk_relay_hops: p2p_config.zk_relay_hops.max(2),
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
    RelayResponse {
        target_peer: PeerId,
        circuit_id: CircuitId,
    },
}

impl PendingReply {
    /// Returns true if the receiver has been dropped (caller no longer waiting).
    fn is_closed(&self) -> bool {
        match self {
            PendingReply::Fragment { reply, .. } => reply.is_closed(),
            PendingReply::Manifest { reply, .. } => reply.is_closed(),
            PendingReply::RelayResponse { .. } => false, // relay responses are always valid
        }
    }

    /// Returns the peer this request was sent to.
    fn target_peer(&self) -> PeerId {
        match self {
            PendingReply::Fragment { target_peer, .. } => *target_peer,
            PendingReply::Manifest { target_peer, .. } => *target_peer,
            PendingReply::RelayResponse { target_peer, .. } => *target_peer,
        }
    }
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
    rendezvous_peers: HashSet<PeerId>,
) {
    tracing::info!("Gateway P2P event loop started");

    // Censorship-aware adaptive router — tracks path health and blocked paths
    let adaptive_router = AdaptiveRouter::new();
    let local_peer_id = *node.peer_id();

    // ZK relay state — client builds circuits for private fetches,
    // relay node forwards cells for peers building circuits through us.
    let zk_relay_enabled = p2p_state.zk_relay_enabled;
    let zk_relay_hops = p2p_state.zk_relay_hops;
    let client_secret: [u8; 32] = rand::random();
    let relay_secret: [u8; 32] = rand::random();
    let zk_config = ZkRelayConfig {
        default_hops: zk_relay_hops,
        ..ZkRelayConfig::default()
    };
    let mut zk_relay_client = ZkRelayClient::with_config(client_secret, zk_config.clone());
    let zk_relay_node = ZkRelayNode::with_config(relay_secret, zk_config);

    if zk_relay_enabled {
        tracing::info!(hops = zk_relay_hops, "ZK relay enabled on gateway");
    }

    let mut pending_requests: HashMap<OutboundRequestId, PendingReply> = HashMap::new();
    let mut pending_dht_queries: HashMap<String, oneshot::Sender<Result<Vec<PeerId>, FetchError>>> =
        HashMap::new();
    let mut pending_name_queries: HashMap<
        String,
        oneshot::Sender<Result<Option<Vec<u8>>, FetchError>>,
    > = HashMap::new();

    // Pending circuit builds (circuit_id -> build state)
    let mut pending_circuit_builds: HashMap<CircuitId, PendingCircuitBuild> = HashMap::new();

    // Pending relay-based content fetches (circuit_id -> fetch state)
    let mut pending_relay_fetches: HashMap<CircuitId, PendingRelayFetch> = HashMap::new();

    let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_secs(60));
    cleanup_interval.tick().await; // consume the immediate first tick

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
                    &rendezvous_peers,
                    &adaptive_router,
                    local_peer_id,
                    &zk_relay_node,
                    &mut zk_relay_client,
                    zk_relay_enabled,
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
                    &mut pending_name_queries,
                    &adaptive_router,
                    local_peer_id,
                    &mut zk_relay_client,
                    &zk_relay_node,
                    zk_relay_enabled,
                    &mut pending_circuit_builds,
                    &mut pending_relay_fetches,
                );
            }
            _ = cleanup_interval.tick() => {
                let before = pending_requests.len() + pending_dht_queries.len() + pending_name_queries.len();
                pending_requests.retain(|_, p| !p.is_closed());
                pending_dht_queries.retain(|_, tx| !tx.is_closed());
                pending_name_queries.retain(|_, tx| !tx.is_closed());
                let after = pending_requests.len() + pending_dht_queries.len() + pending_name_queries.len();
                if before > after {
                    tracing::debug!(removed = before - after, "Cleaned up stale pending queries");
                }

                // Clean up expired ZK relay circuits
                if zk_relay_enabled {
                    let client_expired = zk_relay_client.cleanup_expired();
                    let node_expired = zk_relay_node.cleanup_expired();
                    if client_expired > 0 || node_expired > 0 {
                        tracing::debug!(
                            client_expired,
                            node_expired,
                            "Cleaned up expired ZK relay circuits"
                        );
                    }
                }

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
    rendezvous_peers: &HashSet<PeerId>,
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    zk_relay_node: &ZkRelayNode,
    zk_relay_client: &mut ZkRelayClient,
    zk_relay_enabled: bool,
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

            // If this peer is a configured rendezvous point, register and discover
            if rendezvous_peers.contains(&peer_id) {
                tracing::info!(%peer_id, "Connected to rendezvous point — registering and discovering");
                let namespace = rendezvous::Namespace::from_static(protocol::RENDEZVOUS_NAMESPACE);
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
                adaptive_router,
                local_peer_id,
                zk_relay_node,
                zk_relay_client,
                zk_relay_enabled,
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
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    zk_relay_node: &ZkRelayNode,
    zk_relay_client: &mut ZkRelayClient,
    zk_relay_enabled: bool,
    pending_circuit_builds: &mut HashMap<CircuitId, PendingCircuitBuild>,
    pending_relay_fetches: &mut HashMap<CircuitId, PendingRelayFetch>,
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
                handle_content_request(
                    peer, request, channel, node, cache,
                    zk_relay_node, zk_relay_enabled,
                );
            }
            request_response::Message::Response {
                request_id,
                response,
            } => {
                handle_content_response(
                    request_id,
                    response,
                    pending_requests,
                    adaptive_router,
                    local_peer_id,
                    zk_relay_client,
                    pending_circuit_builds,
                    pending_relay_fetches,
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

// ── Inbound request handling (serving cached content) ────────────

/// Handle an incoming content request from a peer.
///
/// The gateway stores whole files by CID in its cache, not individual fragments.
/// If the requested hash matches a cached CID, serve it; otherwise NotFound.
///
/// If ZK relay is enabled and the request looks like a relay cell (prefixed with
/// `__zk_relay_`), we process it through the relay node and forward as needed.
fn handle_content_request(
    peer: PeerId,
    request: ContentRequest,
    channel: ResponseChannel<ContentResponse>,
    node: &mut ShadowNode,
    cache: &ContentCache,
    zk_relay_node: &ZkRelayNode,
    zk_relay_enabled: bool,
) {
    let response = match request {
        ContentRequest::RelayCell { cell_data } => {
            handle_relay_cell_request(
                peer, &cell_data, node, cache,
                zk_relay_node, zk_relay_enabled,
            )
        }

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

        ContentRequest::ListContent { limit } => {
            let all_entries = cache.entries();
            let iter = all_entries.iter();
            let items: Vec<protocol::content_protocol::ContentSummary> = if limit > 0 {
                iter.take(limit as usize)
            } else {
                iter.take(all_entries.len())
            }
            .map(|(cid, size, mime)| protocol::content_protocol::ContentSummary {
                cid: cid.clone(),
                total_size: *size as u64,
                fragment_count: 1, // gateway stores whole files as single fragments
                mime_type: mime.clone(),
            })
            .collect();
            tracing::debug!(%peer, count = items.len(), "Serving content list");
            ContentResponse::ContentList { items }
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

// ── ZK Relay cell handling ────────────────────────────────────────

/// Process an incoming ZK relay cell from a peer.
///
/// Deserializes the cell, processes it through the `ZkRelayNode`, and returns
/// the appropriate `ContentResponse`. If the action is `Forward`, the cell is
/// sent to the next hop via the swarm; the response to the sender is always
/// a `RelayCell` (carrying either a CREATED response or an empty ack).
fn handle_relay_cell_request(
    peer: PeerId,
    cell_data: &[u8],
    node: &mut ShadowNode,
    cache: &ContentCache,
    zk_relay_node: &ZkRelayNode,
    zk_relay_enabled: bool,
) -> ContentResponse {
    if !zk_relay_enabled {
        tracing::debug!(%peer, "Received relay cell but ZK relay is disabled, ignoring");
        return ContentResponse::Error {
            message: "ZK relay is not enabled on this gateway".to_string(),
        };
    }

    let cell = match protocol::zk_relay::RelayCell::deserialize(cell_data) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(%peer, %e, "Failed to deserialize relay cell");
            return ContentResponse::Error {
                message: format!("Invalid relay cell: {}", e),
            };
        }
    };

    let circuit_id_hex = hex::encode(&cell.circuit_id[..8]);
    tracing::debug!(
        %peer,
        circuit = %circuit_id_hex,
        cell_type = ?cell.cell_type,
        "Processing incoming relay cell"
    );

    match zk_relay_node.process_cell(cell, peer) {
        Ok(RelayAction::Respond(response_cell)) => {
            tracing::debug!(
                %peer,
                circuit = %circuit_id_hex,
                "Relay: responding to sender"
            );
            ContentResponse::RelayCell {
                cell_data: response_cell.serialize(),
            }
        }
        Ok(RelayAction::Forward { cell: fwd_cell, to_peer }) => {
            tracing::debug!(
                %peer,
                %to_peer,
                circuit = %circuit_id_hex,
                "Relay: forwarding cell to next hop"
            );
            // Forward the cell to the next peer via content request
            let forward_request = ContentRequest::RelayCell {
                cell_data: fwd_cell.serialize(),
            };
            let _req_id = node
                .swarm_mut()
                .behaviour_mut()
                .content_req_resp
                .send_request(&to_peer, forward_request);

            // Acknowledge receipt to sender with an empty relay cell response
            ContentResponse::RelayCell {
                cell_data: Vec::new(),
            }
        }
        Ok(RelayAction::Exit { data, circuit_id }) => {
            tracing::info!(
                %peer,
                circuit = %hex::encode(&circuit_id[..8]),
                bytes = data.len(),
                "Relay: exit node processing — retrieving content"
            );
            // The exit data is a serialized BlindRequest; extract the content
            // hash and try to serve from cache.
            match bincode::deserialize::<protocol::zk_relay::BlindRequest>(&data) {
                Ok(blind_req) => {
                    match cache.get(&blind_req.content_hash) {
                        Some((content_data, _mime)) => {
                            // Wrap content in a BlindResponse and send back through circuit
                            let blind_resp = protocol::zk_relay::BlindResponse {
                                request_id: blind_req.request_id,
                                encrypted_content: content_data,
                                content_hash: blind_req.content_hash,
                                success: true,
                                error: None,
                            };
                            let resp_bytes = bincode::serialize(&blind_resp).unwrap_or_default();
                            ContentResponse::RelayCell { cell_data: resp_bytes }
                        }
                        None => {
                            let blind_resp = protocol::zk_relay::BlindResponse {
                                request_id: blind_req.request_id,
                                encrypted_content: Vec::new(),
                                content_hash: blind_req.content_hash.clone(),
                                success: false,
                                error: Some("Content not found".to_string()),
                            };
                            let resp_bytes = bincode::serialize(&blind_resp).unwrap_or_default();
                            ContentResponse::RelayCell { cell_data: resp_bytes }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(%peer, %e, "Failed to deserialize BlindRequest at exit node");
                    ContentResponse::Error {
                        message: format!("Invalid exit payload: {}", e),
                    }
                }
            }
        }
        Ok(RelayAction::Drop) => {
            tracing::trace!(%peer, circuit = %circuit_id_hex, "Relay: dropped cell (padding)");
            ContentResponse::RelayCell {
                cell_data: Vec::new(),
            }
        }
        Err(e) => {
            tracing::warn!(%peer, circuit = %circuit_id_hex, %e, "Relay: cell processing failed");
            ContentResponse::Error {
                message: format!("Relay error: {}", e),
            }
        }
    }
}

// ── Outbound response handling ───────────────────────────────────

/// Route a received response back to the waiting HTTP handler.
fn handle_content_response(
    request_id: OutboundRequestId,
    response: ContentResponse,
    pending_requests: &mut HashMap<OutboundRequestId, PendingReply>,
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    zk_relay_client: &mut ZkRelayClient,
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
                zk_relay_client,
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
        (
            PendingReply::Manifest { reply, .. },
            ContentResponse::Manifest {
                content_hash,
                fragment_hashes,
                total_size,
                mime_type,
            },
        ) => {
            record_censorship_success(adaptive_router, local_peer_id, target_peer, total_size);
            let _ = reply.send(Ok(ManifestResult {
                content_hash,
                fragment_hashes,
                total_size,
                mime_type,
            }));
        }

        // NotFound — not a censorship signal
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
    zk_relay_client: &mut ZkRelayClient,
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
                        match zk_relay_client.process_created_response(&circuit_id, &handshake, hop_index) {
                            Ok(Some(extend_cell)) => {
                                // Need to extend to next hop — send EXTEND through entry node
                                build.next_hop_index += 1;
                                tracing::debug!(
                                    circuit = %hex::encode(&circuit_id[..8]),
                                    hop = hop_index,
                                    "Circuit build: hop {} complete, extending",
                                    hop_index
                                );

                                if let Some(entry_peer) = zk_relay_client.get_entry_node(&circuit_id) {
                                    let request = ContentRequest::RelayCell {
                                        cell_data: extend_cell.serialize(),
                                    };
                                    let _req_id = node
                                        .swarm_mut()
                                        .behaviour_mut()
                                        .content_req_resp
                                        .send_request(&entry_peer, request);
                                    tracing::debug!(
                                        circuit = %hex::encode(&circuit_id[..8]),
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
                match zk_relay_client.unwrap_response(&circuit_id, &cell) {
                    Ok(plaintext) => {
                        // Deserialize the BlindResponse
                        match bincode::deserialize::<protocol::zk_relay::BlindResponse>(
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
            let error_kind = match &e {
                kad::GetRecordError::NotFound { .. } => "not_found",
                kad::GetRecordError::QuorumFailed { .. } => "quorum_failed",
                kad::GetRecordError::Timeout { .. } => "timeout",
            };
            tracing::debug!(kind = error_kind, ?e, "DHT GetRecord failed");
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
    adaptive_router: &AdaptiveRouter,
    local_peer_id: PeerId,
    zk_relay_client: &mut ZkRelayClient,
    _zk_relay_node: &ZkRelayNode,
    zk_relay_enabled: bool,
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

        P2pCommand::BuildCircuit { peers, reply } => {
            if !zk_relay_enabled {
                tracing::warn!("BuildCircuit command received but ZK relay is disabled");
                let _ = reply.send(Err(FetchError::RelayError(
                    "ZK relay is not enabled".to_string(),
                )));
                return;
            }
            if peers.len() < 2 {
                let _ = reply.send(Err(FetchError::RelayError(
                    "Need at least 2 relay hops".to_string(),
                )));
                return;
            }

            match zk_relay_client.build_circuit(&peers) {
                Ok(circuit_id) => {
                    tracing::info!(
                        circuit = %hex::encode(&circuit_id[..8]),
                        hops = peers.len(),
                        "Building ZK relay circuit"
                    );

                    // Get the CREATE cell for the first hop
                    match zk_relay_client.get_create_cell(&circuit_id) {
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
            if !zk_relay_enabled {
                tracing::warn!("SendRelayCell command received but ZK relay is disabled");
                return;
            }
            let circuit_id = cell.circuit_id;
            let request = ContentRequest::RelayCell {
                cell_data: cell.serialize(),
            };
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
            if !zk_relay_enabled {
                tracing::warn!("FetchViaCircuit command received but ZK relay is disabled");
                let _ = reply.send(Err(FetchError::RelayError(
                    "ZK relay is not enabled".to_string(),
                )));
                return;
            }

            // Build a BlindRequest, wrap it in onion layers, send via circuit
            let blind_req = protocol::zk_relay::BlindRequest::new(content_hash.clone());
            let payload = match bincode::serialize(&blind_req) {
                Ok(p) => p,
                Err(e) => {
                    let _ = reply.send(Err(FetchError::RelayError(
                        format!("Failed to serialize request: {}", e),
                    )));
                    return;
                }
            };

            match zk_relay_client.wrap_request(&circuit_id, &payload) {
                Ok(relay_cell) => {
                    if let Some(entry_peer) = zk_relay_client.get_entry_node(&circuit_id) {
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

// ── Censorship detection helpers ──────────────────────────────────

/// Record a path failure in the adaptive router and log if censorship is detected.
///
/// Called from `OutboundFailure` events, `OutgoingConnectionError`, and
/// content-level failures (hash mismatch / tampering). The `AdaptiveRouter`
/// internally tracks per-path health and marks paths as `Suspected` or
/// `Confirmed` when enough censorship-indicative signals accumulate.
fn record_censorship_failure(
    router: &AdaptiveRouter,
    local_peer: PeerId,
    remote_peer: PeerId,
    failure_type: FailureType,
) {
    use protocol::adaptive_routing::RelayInfo;

    // Compute a synthetic route ID for this direct peer pair
    let mut route_id = [0u8; 16];
    {
        let mut hasher = blake3::Hasher::new();
        hasher.update(local_peer.to_bytes().as_slice());
        hasher.update(remote_peer.to_bytes().as_slice());
        let hash = hasher.finalize();
        route_id.copy_from_slice(&hash.as_bytes()[..16]);
    }

    // Ensure both peers are registered as relays so the router can track them
    if router.get_relay(&local_peer).is_none() {
        router.register_relay(RelayInfo::new(local_peer));
    }
    if router.get_relay(&remote_peer).is_none() {
        router.register_relay(RelayInfo::new(remote_peer));
    }

    // Try to record the failure via the router's route tracking
    let failover = router.record_failure(&route_id, 0, failure_type);

    if failover.is_some() {
        tracing::warn!(
            from = %local_peer,
            to = %remote_peer,
            failure = ?failure_type,
            "Path failure triggered route failover"
        );
    }

    // Check if this specific link is now confirmed censored
    if let Some((health_score, status)) = router.get_path_health(local_peer, remote_peer) {
        match status {
            CensorshipStatus::Confirmed => {
                router.block_path(local_peer, remote_peer);
                tracing::error!(
                    from = %local_peer,
                    to = %remote_peer,
                    health = health_score,
                    "CENSORSHIP CONFIRMED: path blocked, will use alternative routes"
                );
            }
            CensorshipStatus::Suspected => {
                tracing::warn!(
                    from = %local_peer,
                    to = %remote_peer,
                    health = health_score,
                    failure = ?failure_type,
                    "Censorship suspected on path — monitoring"
                );
            }
            _ => {
                tracing::debug!(
                    from = %local_peer,
                    to = %remote_peer,
                    failure = ?failure_type,
                    "Path failure recorded for censorship analysis"
                );
            }
        }
    } else {
        tracing::debug!(
            from = %local_peer,
            to = %remote_peer,
            failure = ?failure_type,
            "Path failure recorded for censorship analysis"
        );
    }
}

/// Record a successful path interaction — clears consecutive failure counters.
fn record_censorship_success(
    router: &AdaptiveRouter,
    local_peer: PeerId,
    remote_peer: PeerId,
    bytes: u64,
) {
    let mut route_id = [0u8; 16];
    {
        let mut hasher = blake3::Hasher::new();
        hasher.update(local_peer.to_bytes().as_slice());
        hasher.update(remote_peer.to_bytes().as_slice());
        let hash = hasher.finalize();
        route_id.copy_from_slice(&hash.as_bytes()[..16]);
    }

    router.record_success(&route_id, 0, bytes);
}
