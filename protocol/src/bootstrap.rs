//! Bootstrap Configuration for ShadowMesh
//!
//! In ShadowMesh, there are no centralized "official" bootstrap nodes.
//! Any node can serve as a bootstrap peer for others. Nodes discover
//! peers through:
//!
//! 1. **Config-based bootstrap** — dial known peers from config file
//! 2. **mDNS** — automatic LAN peer discovery
//! 3. **DHT** — Kademlia routing table propagation after initial connection
//! 4. **GossipSub** — real-time peer and content announcements
//!
//! # Bootstrap Sequence
//!
//! 1. Dial bootstrap peers from config (`bootstrap_nodes` / `bootstrap_peers`)
//! 2. Kademlia `FIND_NODE` on own PeerId to populate routing table
//! 3. mDNS discovers LAN peers automatically
//! 4. GossipSub subscribes to topics for real-time updates

/// Returns bootstrap nodes from the `SHADOWMESH_BOOTSTRAP_NODES` environment variable.
///
/// This is an alternative to config-file-based bootstrap. The env var takes a
/// comma-separated list of multiaddrs (e.g., `/ip4/1.2.3.4/tcp/4001/p2p/12D3KooW...`).
///
/// Returns an empty list if the env var is not set, which is normal —
/// bootstrap peers are typically configured in config files instead.
pub fn get_bootstrap_nodes() -> Vec<String> {
    if let Ok(env_nodes) = std::env::var("SHADOWMESH_BOOTSTRAP_NODES") {
        let nodes: Vec<String> = env_nodes
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !nodes.is_empty() {
            for node in &nodes {
                if !is_valid_bootstrap_multiaddr(node) {
                    tracing::warn!(
                        node = %node,
                        "Bootstrap node is not a valid IP-based multiaddr"
                    );
                }
            }
            return nodes;
        }
    }

    // No env var set — bootstrap peers come from config files
    Vec::new()
}

/// STUN servers for WebRTC NAT traversal.
///
/// Uses well-known public STUN servers. Operators can deploy their own
/// and configure them via the browser SDK's `stun_servers` option.
pub const PUBLIC_STUN_SERVERS: &[&str] = &[
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
    "stun:stun2.l.google.com:19302",
];

/// Backward-compatible alias.
pub const FALLBACK_STUN_SERVERS: &[&str] = PUBLIC_STUN_SERVERS;

/// GossipSub topic for bootstrap peer announcements.
pub const BOOTSTRAP_GOSSIP_TOPIC: &str = "shadowmesh/bootstrap";

/// Minimum number of bootstrap connections before the node is considered connected.
pub const MIN_BOOTSTRAP_CONNECTIONS: usize = 1;

/// Maximum time to wait for bootstrap connections before falling back.
pub const BOOTSTRAP_TIMEOUT_SECS: u64 = 30;

/// Returns all available STUN servers.
pub fn all_stun_servers() -> Vec<&'static str> {
    PUBLIC_STUN_SERVERS.to_vec()
}

/// Validate that a multiaddr string looks like a valid IP-based peer address.
/// Returns `true` if it has an IP protocol, a transport, and a peer ID.
pub fn is_valid_bootstrap_multiaddr(addr: &str) -> bool {
    let is_ip = addr.starts_with("/ip4/") || addr.starts_with("/ip6/");
    let has_peer_id = addr.contains("/p2p/");
    let has_transport = addr.contains("/tcp/") || addr.contains("/udp/");
    is_ip && has_peer_id && has_transport
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stun_servers_not_empty() {
        assert!(!PUBLIC_STUN_SERVERS.is_empty());
        assert!(!all_stun_servers().is_empty());
    }

    #[test]
    fn test_valid_bootstrap_multiaddr() {
        assert!(is_valid_bootstrap_multiaddr(
            "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWTest"
        ));
        assert!(is_valid_bootstrap_multiaddr(
            "/ip6/::1/tcp/4001/p2p/12D3KooWTest"
        ));
        assert!(!is_valid_bootstrap_multiaddr(
            "/dns4/example.com/tcp/4001/p2p/12D3KooWTest"
        ));
        assert!(!is_valid_bootstrap_multiaddr("/ip4/1.2.3.4/tcp/4001")); // no peer ID
    }

    #[test]
    fn test_get_bootstrap_nodes_env_handling() {
        // Test env-based bootstrap (single test to avoid parallel env var races)
        let addr = "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWTest";
        std::env::set_var("SHADOWMESH_BOOTSTRAP_NODES", addr);
        let nodes = get_bootstrap_nodes();
        assert_eq!(nodes, vec![addr.to_string()]);

        // Clear and verify empty default
        std::env::remove_var("SHADOWMESH_BOOTSTRAP_NODES");
        let nodes = get_bootstrap_nodes();
        assert!(nodes.is_empty(), "Should be empty when no env var set");
    }
}
