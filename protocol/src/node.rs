//! ShadowMesh P2P Node Implementation
//!
//! Provides the core P2P networking node with support for multiple transports.

use crate::transport::TransportConfig;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::OrTransport, upgrade},
    gossipsub::{Behaviour as Gossipsub, Config as GossipsubConfig, MessageAuthenticity},
    identity::{self, Keypair},
    kad::{store::MemoryStore, Behaviour as Kademlia},
    noise,
    swarm::NetworkBehaviour,
    tcp, yamux, Multiaddr, PeerId, Swarm, Transport,
};
use libp2p_webrtc as webrtc;
use std::error::Error;

/// ShadowMesh P2P network node
pub struct ShadowNode {
    swarm: Swarm<ShadowBehaviour>,
    peer_id: PeerId,
    keypair: Keypair,
    config: TransportConfig,
    listen_addrs: Vec<Multiaddr>,
}

/// Network behaviour combining Kademlia DHT and GossipSub
#[derive(NetworkBehaviour)]
pub struct ShadowBehaviour {
    /// Kademlia DHT for content discovery
    pub kademlia: Kademlia<MemoryStore>,
    /// GossipSub for pub/sub messaging
    pub gossipsub: Gossipsub,
}

/// Error types for ShadowNode operations
#[derive(Debug)]
pub enum NodeError {
    /// Transport initialization failed
    Transport(String),
    /// Swarm creation failed
    Swarm(String),
    /// Listen address binding failed
    Listen(String),
    /// Configuration validation failed
    Config(Vec<String>),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Transport(e) => write!(f, "Transport error: {}", e),
            NodeError::Swarm(e) => write!(f, "Swarm error: {}", e),
            NodeError::Listen(e) => write!(f, "Listen error: {}", e),
            NodeError::Config(errors) => write!(f, "Config errors: {}", errors.join(", ")),
        }
    }
}

impl std::error::Error for NodeError {}

impl ShadowNode {
    /// Create a new ShadowNode with default TCP-only configuration
    pub async fn new() -> Result<Self, Box<dyn Error>> {
        Self::with_config(TransportConfig::default()).await
    }

    /// Create a new ShadowNode with custom transport configuration
    pub async fn with_config(config: TransportConfig) -> Result<Self, Box<dyn Error>> {
        // Validate configuration
        if let Err(errors) = config.validate() {
            return Err(Box::new(NodeError::Config(errors)));
        }

        // Generate node identity
        let keypair = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(keypair.public());

        // Build transport based on configuration
        let transport = Self::build_transport(&keypair, &config)?;

        // Set up Kademlia DHT
        let store = MemoryStore::new(peer_id);
        let kademlia = Kademlia::new(peer_id, store);

        // Set up GossipSub
        let gossipsub_config = GossipsubConfig::default();
        let gossipsub = Gossipsub::new(
            MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )?;

        // Create behaviour and swarm
        let behaviour = ShadowBehaviour {
            kademlia,
            gossipsub,
        };

        let swarm = Swarm::new(
            transport,
            behaviour,
            peer_id,
            libp2p::swarm::Config::with_tokio_executor(),
        );

        Ok(ShadowNode {
            swarm,
            peer_id,
            keypair,
            config,
            listen_addrs: Vec::new(),
        })
    }

    /// Build the transport stack based on configuration
    fn build_transport(
        keypair: &Keypair,
        config: &TransportConfig,
    ) -> Result<libp2p::core::transport::Boxed<(PeerId, StreamMuxerBox)>, Box<dyn Error>> {
        // Build TCP transport with Noise + Yamux
        let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::Config::new(keypair)?)
            .multiplex(yamux::Config::default());

        if config.enable_webrtc && config.enable_tcp {
            // Dual transport: TCP + WebRTC
            let webrtc_transport = Self::build_webrtc_transport(keypair)?;

            // Combine transports using OrTransport
            let combined = OrTransport::new(tcp_transport, webrtc_transport);
            Ok(combined.map(|out, _| match out {
                futures::future::Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
                futures::future::Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            }).boxed())
        } else if config.enable_webrtc {
            // WebRTC only
            let webrtc_transport = Self::build_webrtc_transport(keypair)?;
            Ok(webrtc_transport.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer))).boxed())
        } else {
            // TCP only (default)
            Ok(tcp_transport.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer))).boxed())
        }
    }

    /// Build WebRTC transport
    fn build_webrtc_transport(
        keypair: &Keypair,
    ) -> Result<webrtc::tokio::Transport, Box<dyn Error>> {
        // Generate a certificate for WebRTC DTLS
        let certificate = webrtc::tokio::Certificate::generate(&mut rand::thread_rng())?;

        let transport = webrtc::tokio::Transport::new(keypair.clone(), certificate);

        Ok(transport)
    }

    /// Get the local peer ID
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Get the keypair
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    /// Get the transport configuration
    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    /// Get currently listening addresses
    pub fn listen_addrs(&self) -> &[Multiaddr] {
        &self.listen_addrs
    }

    /// Start the node and begin listening
    pub async fn start(&mut self) -> Result<(), Box<dyn Error>> {
        // Listen on TCP if enabled
        if self.config.enable_tcp {
            let tcp_addr: Multiaddr = self.config.tcp_multiaddr().parse()?;
            self.swarm.listen_on(tcp_addr.clone())?;
            self.listen_addrs.push(tcp_addr);
            tracing::info!("Listening on TCP port {}", self.config.tcp_port);
        }

        // Listen on WebRTC if enabled
        if self.config.enable_webrtc {
            let webrtc_addr: Multiaddr = self.config.webrtc_multiaddr().parse()?;
            self.swarm.listen_on(webrtc_addr.clone())?;
            self.listen_addrs.push(webrtc_addr);
            tracing::info!("Listening on WebRTC port {}", self.config.webrtc_port);
        }

        Ok(())
    }

    /// Start with specific listen addresses
    pub async fn start_with_addrs(&mut self, addrs: Vec<Multiaddr>) -> Result<(), Box<dyn Error>> {
        for addr in addrs {
            self.swarm.listen_on(addr.clone())?;
            self.listen_addrs.push(addr);
        }
        Ok(())
    }

    /// Connect to a peer
    pub fn dial(&mut self, addr: Multiaddr) -> Result<(), Box<dyn Error>> {
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Get mutable access to the swarm for event handling
    pub fn swarm_mut(&mut self) -> &mut Swarm<ShadowBehaviour> {
        &mut self.swarm
    }

    /// Get read access to the swarm
    pub fn swarm(&self) -> &Swarm<ShadowBehaviour> {
        &self.swarm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_node_default() {
        let node = ShadowNode::new().await;
        assert!(node.is_ok());
        let node = node.unwrap();
        assert!(!node.peer_id().to_string().is_empty());
    }

    #[tokio::test]
    async fn test_create_node_tcp_only() {
        let config = TransportConfig::tcp_only(4001);
        let node = ShadowNode::with_config(config).await;
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn test_create_node_dual_transport() {
        let config = TransportConfig::dual(4001, 4002);
        let node = ShadowNode::with_config(config).await;
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_config() {
        let config = TransportConfig {
            enable_tcp: false,
            enable_webrtc: false,
            ..Default::default()
        };
        let node = ShadowNode::with_config(config).await;
        assert!(node.is_err());
    }
}
