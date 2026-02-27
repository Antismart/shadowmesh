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

/// Default bootstrap nodes shipped with the binary.
///
/// These are well-known entry points for joining the ShadowMesh network.
/// Operators can override via config file or `SHADOWMESH_BOOTSTRAP_NODES` env var.
///
/// Replace with real multiaddrs once public bootstrap infrastructure is deployed:
/// ```text
/// "/ip4/<IP>/tcp/4001/p2p/<PEER_ID>"
/// ```
pub const DEFAULT_BOOTSTRAP_NODES: &[&str] = &[
    // Placeholder — uncomment and replace once infra is live
    // "/ip4/34.82.79.240/tcp/4001/p2p/12D3KooW...",
    // "/ip4/35.198.7.12/tcp/4001/p2p/12D3KooW...",
];

/// Rendezvous namespace for ShadowMesh peer discovery.
pub const RENDEZVOUS_NAMESPACE: &str = "shadowmesh";

/// Merge bootstrap nodes from three sources (highest priority first):
///
/// 1. `SHADOWMESH_BOOTSTRAP_NODES` environment variable
/// 2. Config-file entries (`config_peers` parameter)
/// 3. [`DEFAULT_BOOTSTRAP_NODES`] compiled into the binary
///
/// Duplicates are removed. All entries are validated (warnings logged for invalid ones).
pub fn get_bootstrap_nodes_merged(config_peers: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    // Priority 1: Environment variable
    if let Ok(env_nodes) = std::env::var("SHADOWMESH_BOOTSTRAP_NODES") {
        for node in env_nodes.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            if seen.insert(node.clone()) {
                result.push(node);
            }
        }
    }

    // Priority 2: Config file entries
    for node in config_peers {
        let trimmed = node.trim().to_string();
        if !trimmed.is_empty() && seen.insert(trimmed.clone()) {
            result.push(trimmed);
        }
    }

    // Priority 3: Compiled-in defaults
    for &node in DEFAULT_BOOTSTRAP_NODES {
        let s = node.to_string();
        if !s.is_empty() && seen.insert(s.clone()) {
            result.push(s);
        }
    }

    for node in &result {
        if !is_valid_bootstrap_multiaddr(node) {
            tracing::warn!(node = %node, "Bootstrap node is not a valid IP-based multiaddr");
        }
    }

    result
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

/// Resolve bootstrap peers from DNS TXT records.
///
/// Each domain should have TXT records containing multiaddrs, one per record.
/// Example DNS setup:
/// ```text
/// _bootstrap.shadowmesh.example.com. TXT "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooW..."
/// ```
///
/// Falls back gracefully if DNS is unavailable — returns an empty vec.
pub async fn resolve_dns_seeds(domains: &[String]) -> Vec<String> {
    if domains.is_empty() {
        return Vec::new();
    }

    let resolver = match hickory_resolver::TokioResolver::builder_tokio() {
        Ok(builder) => builder.build(),
        Err(e) => {
            tracing::warn!(%e, "Failed to create DNS resolver for seed discovery");
            return Vec::new();
        }
    };

    let mut results = Vec::new();

    for domain in domains {
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            resolver.txt_lookup(domain.as_str()),
        )
        .await
        {
            Ok(Ok(response)) => {
                for record in response.iter() {
                    for txt_data in record.txt_data() {
                        let addr = String::from_utf8_lossy(txt_data).trim().to_string();
                        if !addr.is_empty() {
                            if is_valid_bootstrap_multiaddr(&addr) {
                                tracing::info!(
                                    domain = %domain,
                                    addr = %addr,
                                    "DNS seed: resolved bootstrap peer"
                                );
                                results.push(addr);
                            } else {
                                tracing::debug!(
                                    domain = %domain,
                                    addr = %addr,
                                    "DNS seed: invalid multiaddr in TXT record"
                                );
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                tracing::debug!(domain = %domain, %e, "DNS seed lookup failed");
            }
            Err(_) => {
                tracing::debug!(domain = %domain, "DNS seed lookup timed out");
            }
        }
    }

    results
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

    #[test]
    fn test_get_bootstrap_nodes_merged_deduplication() {
        let config = vec![
            "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWA".to_string(),
            "/ip4/5.6.7.8/tcp/4001/p2p/12D3KooWB".to_string(),
            "/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWA".to_string(), // duplicate
        ];
        let result = get_bootstrap_nodes_merged(&config);

        // Verify no duplicates in the result (env var may add extra entries)
        let unique: std::collections::HashSet<&String> = result.iter().collect();
        assert_eq!(unique.len(), result.len(), "Result should have no duplicates");

        // Both distinct config entries should be present
        assert!(result.contains(&"/ip4/1.2.3.4/tcp/4001/p2p/12D3KooWA".to_string()));
        assert!(result.contains(&"/ip4/5.6.7.8/tcp/4001/p2p/12D3KooWB".to_string()));
    }

    #[test]
    fn test_get_bootstrap_nodes_merged_env_priority() {
        let env_addr = "/ip4/10.0.0.1/tcp/4001/p2p/12D3KooWEnv";
        let config_addr = "/ip4/10.0.0.2/tcp/4001/p2p/12D3KooWCfg";

        std::env::set_var("SHADOWMESH_BOOTSTRAP_NODES", env_addr);
        let result = get_bootstrap_nodes_merged(&[config_addr.to_string()]);

        // Env should come first
        assert_eq!(result[0], env_addr);
        assert_eq!(result[1], config_addr);

        std::env::remove_var("SHADOWMESH_BOOTSTRAP_NODES");
    }

    #[test]
    fn test_rendezvous_namespace() {
        assert_eq!(RENDEZVOUS_NAMESPACE, "shadowmesh");
    }
}
