use libp2p::{
    gossipsub::{Behaviour as Gossipsub, Config as GossipsubConfig, MessageAuthenticity},
    identity,
    kad::{store::MemoryStore, Behaviour as Kademlia},
    noise,
    swarm::NetworkBehaviour,
    tcp, yamux, PeerId, Swarm, Transport,
};
use std::error::Error;

pub struct ShadowNode {
    swarm: Swarm<ShadowBehaviour>,
    peer_id: PeerId,
}

#[derive(NetworkBehaviour)]
pub struct ShadowBehaviour {
    kademlia: Kademlia<MemoryStore>,
    gossipsub: Gossipsub,
}

impl ShadowNode {
    pub async fn new() -> Result<Self, Box<dyn Error>> {
        // Generate node identity
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        // Create transport
        let transport = tcp::tokio::Transport::new(tcp::Config::default())
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .boxed();

        // Set up Kademlia DHT
        let store = MemoryStore::new(local_peer_id);
        let kademlia = Kademlia::new(local_peer_id, store);

        // Set up GossipSub
        let gossipsub_config = GossipsubConfig::default();
        let gossipsub = Gossipsub::new(
            MessageAuthenticity::Signed(local_key.clone()),
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
            local_peer_id,
            libp2p::swarm::Config::with_tokio_executor(),
        );

        Ok(ShadowNode {
            swarm,
            peer_id: local_peer_id,
        })
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub async fn start(&mut self) -> Result<(), Box<dyn Error>> {
        // Listen on all interfaces
        self.swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

        // Bootstrap to network (connect to known peers)
        // TODO: Add bootstrap nodes

        Ok(())
    }
}
