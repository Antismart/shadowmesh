//! P2P Event Loop
//!
//! Runs the libp2p swarm event loop and shares peer state with the API layer.

use futures::StreamExt;
use libp2p::{swarm::SwarmEvent, PeerId};
use shadowmesh_protocol::ShadowNode;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::metrics::MetricsCollector;

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
}

impl P2pState {
    pub fn new() -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
            listen_addrs: RwLock::new(Vec::new()),
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
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    tracing::info!("P2P event loop started");

    loop {
        tokio::select! {
            event = node.swarm_mut().select_next_some() => {
                handle_swarm_event(event, &mut node, &p2p_state, &metrics).await;
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
            handle_behaviour_event(behaviour_event, node, p2p_state).await;
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

/// Process behaviour-specific events (mDNS discovery, Kademlia, GossipSub).
async fn handle_behaviour_event(
    event: shadowmesh_protocol::node::ShadowBehaviourEvent,
    node: &mut ShadowNode,
    _p2p_state: &P2pState,
) {
    use shadowmesh_protocol::node::ShadowBehaviourEvent;

    match event {
        ShadowBehaviourEvent::Mdns(libp2p::mdns::Event::Discovered(list)) => {
            for (peer_id, addr) in list {
                tracing::info!(%peer_id, %addr, "mDNS: discovered peer");

                // Add to Kademlia routing table
                node.swarm_mut()
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, addr.clone());

                // Dial the discovered peer
                if let Err(e) = node.dial(addr) {
                    tracing::debug!(%peer_id, %e, "Failed to dial discovered peer");
                }
            }
        }

        ShadowBehaviourEvent::Mdns(libp2p::mdns::Event::Expired(list)) => {
            tracing::debug!(count = list.len(), "mDNS: peers expired");
        }

        ShadowBehaviourEvent::Kademlia(event) => {
            tracing::debug!(?event, "Kademlia event");
        }

        ShadowBehaviourEvent::Gossipsub(event) => {
            tracing::debug!(?event, "GossipSub event");
        }
    }
}
