//! DNS-Free Bootstrap Configuration for ShadowMesh
//!
//! Provides hardcoded IP-based bootstrap nodes, STUN servers, and the bootstrap
//! sequence for joining the ShadowMesh network without any DNS dependency.
//!
//! # Bootstrap Sequence
//!
//! 1. Dial `OFFICIAL_BOOTSTRAP_NODES` (IP-based multiaddrs)
//! 2. Kademlia `FIND_NODE` on own PeerId to populate routing table
//! 3. Resolve `_bootstrap.shadow` via DHT for community bootstrap peers
//! 4. Resolve `_gateway.shadow`, `_signaling.shadow`, `_stun.shadow` for services
//! 5. Subscribe to GossipSub topics for real-time updates

/// Default bootstrap nodes (PLACEHOLDERS — override via `SHADOWMESH_BOOTSTRAP_NODES` env var).
///
/// These use raw IP addresses — no DNS resolution required.
/// At least one must be reachable to join the network.
///
/// Format: `/ip4/{ip}/tcp/{port}/p2p/{peer_id}`
const DEFAULT_BOOTSTRAP_NODES: &[&str] = &[
    // US East (New York)
    "/ip4/45.33.32.156/tcp/4001/p2p/12D3KooWBootstrapUSEast1placeholder",
    // EU West (Amsterdam)
    "/ip4/178.62.8.237/tcp/4001/p2p/12D3KooWBootstrapEUWest1placeholder",
    // Asia Pacific (Singapore)
    "/ip4/103.43.75.104/tcp/4001/p2p/12D3KooWBootstrapAPAC1placeholder",
];

/// Backward-compatible alias for default bootstrap nodes.
pub const OFFICIAL_BOOTSTRAP_NODES: &[&str] = DEFAULT_BOOTSTRAP_NODES;

/// Returns the effective bootstrap nodes.
///
/// Reads from `SHADOWMESH_BOOTSTRAP_NODES` (comma-separated multiaddrs).
/// Falls back to `DEFAULT_BOOTSTRAP_NODES` if the env var is not set.
/// Warns if placeholder peer IDs are still in use.
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
                    eprintln!(
                        "WARNING: Bootstrap node '{}' is not a valid IP-based multiaddr",
                        node
                    );
                }
            }
            return nodes;
        }
    }

    let defaults: Vec<String> = DEFAULT_BOOTSTRAP_NODES.iter().map(|s| s.to_string()).collect();
    let has_placeholders = defaults.iter().any(|n| n.contains("placeholder"));
    if has_placeholders {
        eprintln!(
            "WARNING: Default bootstrap nodes contain placeholder peer IDs and will be skipped. \
             Set SHADOWMESH_BOOTSTRAP_NODES with real peer IDs for production."
        );
        // Filter out placeholders — they can never connect
        return defaults
            .into_iter()
            .filter(|n| !n.contains("placeholder"))
            .collect();
    }
    defaults
}

/// ShadowMesh-operated STUN servers (IP-only, no DNS dependency).
///
/// These replace the Google STUN defaults for WebRTC NAT traversal.
pub const SHADOWMESH_STUN_SERVERS: &[&str] = &[
    "stun:45.33.32.156:3478",
    "stun:178.62.8.237:3478",
];

/// Fallback STUN servers (third-party, DNS-based).
/// Used only when ShadowMesh STUN servers are unreachable.
pub const FALLBACK_STUN_SERVERS: &[&str] = &[
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
];

/// GossipSub topic for bootstrap peer announcements.
pub const BOOTSTRAP_GOSSIP_TOPIC: &str = "shadowmesh/bootstrap";

/// Minimum number of bootstrap connections before the node is considered connected.
pub const MIN_BOOTSTRAP_CONNECTIONS: usize = 1;

/// Maximum time to wait for bootstrap connections before falling back.
pub const BOOTSTRAP_TIMEOUT_SECS: u64 = 30;

/// Returns all STUN servers: ShadowMesh-operated first, then fallbacks.
pub fn all_stun_servers() -> Vec<&'static str> {
    let mut servers = Vec::with_capacity(
        SHADOWMESH_STUN_SERVERS.len() + FALLBACK_STUN_SERVERS.len(),
    );
    servers.extend_from_slice(SHADOWMESH_STUN_SERVERS);
    servers.extend_from_slice(FALLBACK_STUN_SERVERS);
    servers
}

/// Parse a multiaddr string into its components for validation.
/// Returns `true` if the multiaddr looks like a valid IP-based peer address.
pub fn is_valid_bootstrap_multiaddr(addr: &str) -> bool {
    // Must start with /ip4/ or /ip6/ (no DNS)
    let is_ip = addr.starts_with("/ip4/") || addr.starts_with("/ip6/");
    // Must contain /p2p/ for peer ID
    let has_peer_id = addr.contains("/p2p/");
    // Must contain a transport (/tcp/ or /udp/)
    let has_transport = addr.contains("/tcp/") || addr.contains("/udp/");

    is_ip && has_peer_id && has_transport
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_nodes_are_ip_based() {
        for addr in OFFICIAL_BOOTSTRAP_NODES {
            assert!(
                addr.starts_with("/ip4/") || addr.starts_with("/ip6/"),
                "Bootstrap node must use IP address, not DNS: {}",
                addr
            );
            assert!(
                addr.contains("/p2p/"),
                "Bootstrap node must include peer ID: {}",
                addr
            );
        }
    }

    #[test]
    fn test_stun_servers_not_empty() {
        assert!(!SHADOWMESH_STUN_SERVERS.is_empty());
        assert!(!all_stun_servers().is_empty());
    }

    #[test]
    fn test_all_stun_prioritizes_shadowmesh() {
        let servers = all_stun_servers();
        // ShadowMesh servers should come first
        for (i, sm_server) in SHADOWMESH_STUN_SERVERS.iter().enumerate() {
            assert_eq!(servers[i], *sm_server);
        }
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
    fn test_get_bootstrap_nodes_filters_placeholders() {
        // When no env var is set, placeholders should be filtered out
        std::env::remove_var("SHADOWMESH_BOOTSTRAP_NODES");
        let nodes = get_bootstrap_nodes();
        for node in &nodes {
            assert!(
                !node.contains("placeholder"),
                "Placeholder node should be filtered: {}",
                node
            );
        }
    }
}
