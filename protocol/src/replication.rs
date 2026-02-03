//! Content replication for ShadowMesh
//!
//! Manages content distribution and redundancy across the network.

use crate::fragments::ContentManifest;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Replication factor (how many copies of content to maintain)
pub const DEFAULT_REPLICATION_FACTOR: usize = 3;

/// Minimum replication factor
pub const MIN_REPLICATION_FACTOR: usize = 1;

/// Maximum replication factor
pub const MAX_REPLICATION_FACTOR: usize = 10;

/// Content replication status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationStatus {
    /// Content ID
    pub cid: String,
    /// Current number of replicas
    pub replica_count: usize,
    /// Target number of replicas
    pub target_replicas: usize,
    /// Peers holding replicas
    pub replica_holders: Vec<String>,
    /// Whether replication is healthy
    pub healthy: bool,
    /// Last replication check
    pub last_checked: u64,
}

/// Fragment location tracking
#[derive(Debug, Clone)]
pub struct FragmentLocation {
    pub fragment_hash: String,
    pub holders: HashSet<PeerId>,
    pub last_verified: Instant,
}

/// Replication manager
pub struct ReplicationManager {
    /// Target replication factor
    replication_factor: usize,
    /// Content -> Fragment locations
    content_fragments: HashMap<String, Vec<FragmentLocation>>,
    /// Content -> Replication status
    replication_status: HashMap<String, ReplicationStatus>,
    /// Pending replication tasks
    pending_tasks: Vec<ReplicationTask>,
    /// Local pinned content
    pinned_content: HashSet<String>,
}

/// Task for replicating content
#[derive(Debug, Clone)]
pub struct ReplicationTask {
    pub cid: String,
    pub fragment_hash: Option<String>,
    pub target_peer: PeerId,
    pub priority: ReplicationPriority,
    pub created_at: Instant,
    pub attempts: u32,
}

/// Priority levels for replication
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReplicationPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl ReplicationManager {
    /// Create a new replication manager
    pub fn new() -> Self {
        Self::with_factor(DEFAULT_REPLICATION_FACTOR)
    }

    /// Create with custom replication factor
    pub fn with_factor(factor: usize) -> Self {
        let factor = factor.clamp(MIN_REPLICATION_FACTOR, MAX_REPLICATION_FACTOR);
        Self {
            replication_factor: factor,
            content_fragments: HashMap::new(),
            replication_status: HashMap::new(),
            pending_tasks: Vec::new(),
            pinned_content: HashSet::new(),
        }
    }

    /// Set replication factor
    pub fn set_replication_factor(&mut self, factor: usize) {
        self.replication_factor = factor.clamp(MIN_REPLICATION_FACTOR, MAX_REPLICATION_FACTOR);
    }

    /// Get current replication factor
    pub fn replication_factor(&self) -> usize {
        self.replication_factor
    }

    /// Register content and its fragments
    pub fn register_content(&mut self, cid: String, manifest: &ContentManifest) {
        let fragments: Vec<FragmentLocation> = manifest
            .fragments
            .iter()
            .map(|hash| FragmentLocation {
                fragment_hash: hash.clone(),
                holders: HashSet::new(),
                last_verified: Instant::now(),
            })
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let status = ReplicationStatus {
            cid: cid.clone(),
            replica_count: 0,
            target_replicas: self.replication_factor,
            replica_holders: Vec::new(),
            healthy: false,
            last_checked: now,
        };

        self.content_fragments.insert(cid.clone(), fragments);
        self.replication_status.insert(cid, status);
    }

    /// Add a holder for a fragment
    pub fn add_fragment_holder(&mut self, cid: &str, fragment_hash: &str, peer: PeerId) {
        if let Some(fragments) = self.content_fragments.get_mut(cid) {
            for frag in fragments.iter_mut() {
                if frag.fragment_hash == fragment_hash {
                    frag.holders.insert(peer);
                    frag.last_verified = Instant::now();
                    break;
                }
            }
            self.update_replication_status(cid);
        }
    }

    /// Remove a holder (peer went offline or lost data)
    pub fn remove_fragment_holder(&mut self, cid: &str, fragment_hash: &str, peer: &PeerId) {
        if let Some(fragments) = self.content_fragments.get_mut(cid) {
            for frag in fragments.iter_mut() {
                if frag.fragment_hash == fragment_hash {
                    frag.holders.remove(peer);
                    break;
                }
            }
            self.update_replication_status(cid);
        }
    }

    /// Update replication status for content
    fn update_replication_status(&mut self, cid: &str) {
        if let Some(fragments) = self.content_fragments.get(cid) {
            // Find minimum replica count across all fragments
            let min_replicas = fragments.iter().map(|f| f.holders.len()).min().unwrap_or(0);

            // Collect all unique holders
            let mut all_holders: HashSet<PeerId> = HashSet::new();
            for frag in fragments {
                all_holders.extend(frag.holders.iter().cloned());
            }

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            if let Some(status) = self.replication_status.get_mut(cid) {
                status.replica_count = min_replicas;
                status.replica_holders = all_holders.iter().map(|p| p.to_string()).collect();
                status.healthy = min_replicas >= self.replication_factor;
                status.last_checked = now;
            }
        }
    }

    /// Get replication status for content
    pub fn get_status(&self, cid: &str) -> Option<&ReplicationStatus> {
        self.replication_status.get(cid)
    }

    /// Get all content that needs more replicas
    pub fn get_under_replicated(&self) -> Vec<&ReplicationStatus> {
        self.replication_status
            .values()
            .filter(|s| !s.healthy)
            .collect()
    }

    /// Check if content needs replication
    pub fn needs_replication(&self, cid: &str) -> bool {
        self.replication_status
            .get(cid)
            .map(|s| !s.healthy)
            .unwrap_or(false)
    }

    /// Get fragments that need replication for a CID
    pub fn get_fragments_needing_replication(&self, cid: &str) -> Vec<&FragmentLocation> {
        self.content_fragments
            .get(cid)
            .map(|fragments| {
                fragments
                    .iter()
                    .filter(|f| f.holders.len() < self.replication_factor)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Create replication tasks for under-replicated content
    pub fn create_replication_tasks(&mut self, available_peers: &[PeerId]) -> Vec<ReplicationTask> {
        let mut tasks = Vec::new();

        for (cid, fragments) in &self.content_fragments {
            for frag in fragments {
                let needed = self.replication_factor.saturating_sub(frag.holders.len());
                if needed == 0 {
                    continue;
                }

                // Find peers that don't have this fragment
                let candidates: Vec<_> = available_peers
                    .iter()
                    .filter(|p| !frag.holders.contains(p))
                    .take(needed)
                    .collect();

                for peer in candidates {
                    let priority = if frag.holders.is_empty() {
                        ReplicationPriority::Critical
                    } else if frag.holders.len() == 1 {
                        ReplicationPriority::High
                    } else {
                        ReplicationPriority::Normal
                    };

                    tasks.push(ReplicationTask {
                        cid: cid.clone(),
                        fragment_hash: Some(frag.fragment_hash.clone()),
                        target_peer: *peer,
                        priority,
                        created_at: Instant::now(),
                        attempts: 0,
                    });
                }
            }
        }

        // Sort by priority (highest first)
        tasks.sort_by(|a, b| b.priority.cmp(&a.priority));
        tasks
    }

    /// Pin content locally (prevent garbage collection)
    pub fn pin(&mut self, cid: &str) {
        self.pinned_content.insert(cid.to_string());
    }

    /// Unpin content
    pub fn unpin(&mut self, cid: &str) {
        self.pinned_content.remove(cid);
    }

    /// Check if content is pinned
    pub fn is_pinned(&self, cid: &str) -> bool {
        self.pinned_content.contains(cid)
    }

    /// Get all pinned content
    pub fn get_pinned(&self) -> Vec<&String> {
        self.pinned_content.iter().collect()
    }

    /// Get replication health summary
    pub fn get_health_summary(&self) -> ReplicationHealth {
        let total = self.replication_status.len();
        let healthy = self
            .replication_status
            .values()
            .filter(|s| s.healthy)
            .count();
        let critical = self
            .replication_status
            .values()
            .filter(|s| s.replica_count == 0)
            .count();
        let under_replicated = total - healthy;

        ReplicationHealth {
            total_content: total,
            healthy_content: healthy,
            under_replicated_content: under_replicated,
            critical_content: critical,
            replication_factor: self.replication_factor,
            pinned_content: self.pinned_content.len(),
        }
    }
}

impl Default for ReplicationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of replication health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationHealth {
    pub total_content: usize,
    pub healthy_content: usize,
    pub under_replicated_content: usize,
    pub critical_content: usize,
    pub replication_factor: usize,
    pub pinned_content: usize,
}

impl ReplicationHealth {
    pub fn health_percentage(&self) -> f64 {
        if self.total_content == 0 {
            return 100.0;
        }
        (self.healthy_content as f64 / self.total_content as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;

    fn create_test_peer() -> PeerId {
        let keypair = identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    fn create_test_manifest() -> ContentManifest {
        ContentManifest {
            content_hash: "hash123".to_string(),
            fragments: vec![
                "frag1".to_string(),
                "frag2".to_string(),
                "frag3".to_string(),
            ],
            metadata: crate::fragments::ContentMetadata {
                name: "test.bin".to_string(),
                size: 1024,
                mime_type: "application/octet-stream".to_string(),
            },
        }
    }

    #[test]
    fn test_register_content() {
        let mut manager = ReplicationManager::new();
        let manifest = create_test_manifest();

        manager.register_content("cid123".to_string(), &manifest);

        let status = manager.get_status("cid123").unwrap();
        assert_eq!(status.replica_count, 0);
        assert!(!status.healthy);
    }

    #[test]
    fn test_add_holders() {
        let mut manager = ReplicationManager::with_factor(2);
        let manifest = create_test_manifest();

        manager.register_content("cid123".to_string(), &manifest);

        let peer1 = create_test_peer();
        let peer2 = create_test_peer();

        // Add holders for all fragments
        for frag in &manifest.fragments {
            manager.add_fragment_holder("cid123", frag, peer1);
            manager.add_fragment_holder("cid123", frag, peer2);
        }

        let status = manager.get_status("cid123").unwrap();
        assert_eq!(status.replica_count, 2);
        assert!(status.healthy);
    }

    #[test]
    fn test_pinning() {
        let mut manager = ReplicationManager::new();

        manager.pin("cid123");
        assert!(manager.is_pinned("cid123"));

        manager.unpin("cid123");
        assert!(!manager.is_pinned("cid123"));
    }

    #[test]
    fn test_health_summary() {
        let mut manager = ReplicationManager::with_factor(2);
        let manifest = create_test_manifest();

        manager.register_content("cid1".to_string(), &manifest);
        manager.register_content("cid2".to_string(), &manifest);

        let peer = create_test_peer();
        for frag in &manifest.fragments {
            manager.add_fragment_holder("cid1", frag, peer);
            manager.add_fragment_holder("cid1", frag, create_test_peer());
        }

        let health = manager.get_health_summary();
        assert_eq!(health.total_content, 2);
        assert_eq!(health.healthy_content, 1);
        assert_eq!(health.under_replicated_content, 1);
    }
}
