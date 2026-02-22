//! Decentralized Naming Layer for ShadowMesh
//!
//! Provides DHT-native name resolution that eliminates centralized DNS dependencies.
//! Names (e.g., `myapp.shadow`) are stored in Kademlia DHT under the namespace
//! `/shadowmesh/names/{blake3(name)}` and ownership is proven via Ed25519 signatures.
//!
//! # Record Types
//!
//! A single name can resolve to multiple targets:
//! - **Content**: Maps to a CID for content delivery
//! - **Gateway**: Maps to a gateway node's multiaddrs
//! - **Service**: Maps to infrastructure services (signaling, STUN, TURN, bootstrap)
//! - **Peer**: Maps to a peer identity
//! - **Alias**: Redirects to another `.shadow` name

use libp2p::identity::Keypair;
use libp2p::kad::RecordKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default TTL for name records (1 hour)
pub const DEFAULT_NAME_TTL: u64 = 3600;

/// Maximum TTL for name records (24 hours)
pub const MAX_NAME_TTL: u64 = 86400;

/// Minimum TTL for name records (60 seconds)
pub const MIN_NAME_TTL: u64 = 60;

/// Negative cache TTL (5 minutes)
pub const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(300);

/// Maximum cache entries
pub const MAX_CACHE_ENTRIES: usize = 10_000;

/// Service registry heartbeat timeout (1 hour)
pub const SERVICE_HEARTBEAT_TIMEOUT: u64 = 3600;

/// Maximum name length (excluding `.shadow` suffix)
pub const MAX_NAME_LENGTH: usize = 63;

/// GossipSub topic for name updates
pub const NAMING_GOSSIP_TOPIC: &str = "shadowmesh/names/updates";

// ---------------------------------------------------------------------------
// Well-Known Service Names
// ---------------------------------------------------------------------------

/// Well-known names for infrastructure service discovery.
/// These use the `_` prefix convention and are multi-writer registries.
pub struct WellKnownNames;

impl WellKnownNames {
    /// Gateway service discovery
    pub const GATEWAY: &'static str = "_gateway.shadow";
    /// WebRTC signaling servers
    pub const SIGNALING: &'static str = "_signaling.shadow";
    /// Bootstrap peer list
    pub const BOOTSTRAP: &'static str = "_bootstrap.shadow";
    /// TURN relay servers
    pub const TURN: &'static str = "_turn.shadow";
    /// STUN servers
    pub const STUN: &'static str = "_stun.shadow";

    /// Returns all well-known service names.
    pub fn all() -> Vec<&'static str> {
        vec![
            Self::GATEWAY,
            Self::SIGNALING,
            Self::BOOTSTRAP,
            Self::TURN,
            Self::STUN,
        ]
    }

    /// Check if a name is a well-known service name.
    pub fn is_well_known(name: &str) -> bool {
        Self::all().contains(&name)
    }
}

// ---------------------------------------------------------------------------
// Error Types
// ---------------------------------------------------------------------------

/// Errors from naming operations.
#[derive(Debug, Clone, PartialEq)]
pub enum NamingError {
    /// Name format is invalid
    InvalidName(String),
    /// Record signature verification failed
    InvalidSignature,
    /// Record validation failed
    ValidationFailed(String),
    /// Signing failed
    SigningFailed(String),
    /// Name is already registered by a different owner
    NameTaken,
    /// Caller does not own this name
    NotOwner,
    /// Record has been revoked
    Revoked,
    /// Serialization/deserialization error
    SerializationError(String),
    /// TTL out of bounds
    InvalidTtl(u64),
    /// Sequence number is not higher than existing
    StaleSequence,
}

impl std::fmt::Display for NamingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NamingError::InvalidName(msg) => write!(f, "Invalid name: {}", msg),
            NamingError::InvalidSignature => write!(f, "Invalid record signature"),
            NamingError::ValidationFailed(msg) => write!(f, "Validation failed: {}", msg),
            NamingError::SigningFailed(msg) => write!(f, "Signing failed: {}", msg),
            NamingError::NameTaken => write!(f, "Name is already registered"),
            NamingError::NotOwner => write!(f, "Not the owner of this name"),
            NamingError::Revoked => write!(f, "Name has been revoked"),
            NamingError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            NamingError::InvalidTtl(ttl) => {
                write!(f, "TTL {} out of range [{}, {}]", ttl, MIN_NAME_TTL, MAX_NAME_TTL)
            }
            NamingError::StaleSequence => write!(f, "Sequence number is not higher than existing"),
        }
    }
}

impl std::error::Error for NamingError {}

// ---------------------------------------------------------------------------
// Record Types
// ---------------------------------------------------------------------------

/// The type of infrastructure service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ServiceType {
    Signaling,
    Turn,
    Stun,
    Bootstrap,
    Relay,
    Custom(String),
}

/// A resolution target within a name record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NameRecordType {
    /// Points to content (CID)
    Content { cid: String },
    /// Points to a gateway node
    Gateway {
        peer_id: String,
        multiaddrs: Vec<String>,
        /// Relative weight for load balancing (1-100)
        weight: u8,
    },
    /// Points to an infrastructure service
    Service {
        service_type: ServiceType,
        peer_id: String,
        multiaddrs: Vec<String>,
    },
    /// Points to a peer identity
    Peer {
        peer_id: String,
        multiaddrs: Vec<String>,
    },
    /// Alias to another `.shadow` name
    Alias { target: String },
}

/// A signed, versioned name record stored in the DHT.
///
/// Ownership is proven by the Ed25519 signature. Updates must increment
/// `sequence` and be signed by the same key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameRecord {
    /// Human-readable name (e.g., `myapp.shadow`)
    pub name: String,
    /// BLAKE3 hash of the name — used as DHT key suffix
    pub name_hash: String,
    /// Owner's Ed25519 public key bytes, hex-encoded
    pub owner_pubkey: String,
    /// Resolution targets (a name can point to multiple things)
    pub records: Vec<NameRecordType>,
    /// Monotonically increasing — prevents replay attacks
    pub sequence: u64,
    /// Unix timestamp (seconds) when the name was first registered
    pub created_at: u64,
    /// Unix timestamp (seconds) of last update
    pub updated_at: u64,
    /// Cache TTL in seconds
    pub ttl_seconds: u64,
    /// Ed25519 signature over the canonical serialization (with `signature` set to `""`)
    pub signature: String,
}

// ---------------------------------------------------------------------------
// Service Registry (multi-writer model for well-known names)
// ---------------------------------------------------------------------------

/// An individually signed entry in a service registry.
/// Used for well-known names where multiple nodes register themselves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRegistryEntry {
    /// Type of service being offered
    pub service_type: ServiceType,
    /// Peer ID of the registering node
    pub peer_id: String,
    /// Multiaddresses where the service is reachable
    pub multiaddrs: Vec<String>,
    /// Reputation score from peer discovery (0-100)
    pub reputation: u8,
    /// When this entry was first registered (unix seconds)
    pub registered_at: u64,
    /// Last heartbeat timestamp (unix seconds)
    pub last_heartbeat: u64,
    /// Ed25519 signature by the registering peer
    pub signature: String,
}

/// A collection of service entries for a well-known name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRegistry {
    /// The well-known name (e.g., `_gateway.shadow`)
    pub name: String,
    /// Individual service entries, each signed by their registrant
    pub entries: Vec<ServiceRegistryEntry>,
    /// Last modification timestamp
    pub updated_at: u64,
}

impl ServiceRegistry {
    /// Create a new empty service registry.
    pub fn new(name: String) -> Self {
        Self {
            name,
            entries: Vec::new(),
            updated_at: now_unix(),
        }
    }

    /// Add or update an entry. If an entry with the same `peer_id` exists, replace it.
    pub fn upsert_entry(&mut self, entry: ServiceRegistryEntry) {
        self.entries.retain(|e| e.peer_id != entry.peer_id);
        self.entries.push(entry);
        self.updated_at = now_unix();
    }

    /// Remove stale entries that haven't sent a heartbeat within the timeout.
    pub fn prune_stale(&mut self) {
        let cutoff = now_unix().saturating_sub(SERVICE_HEARTBEAT_TIMEOUT);
        self.entries.retain(|e| e.last_heartbeat > cutoff);
        self.updated_at = now_unix();
    }

    /// Get entries sorted by reputation (highest first).
    pub fn best_entries(&self, limit: usize) -> Vec<&ServiceRegistryEntry> {
        let mut sorted: Vec<_> = self.entries.iter().collect();
        sorted.sort_by(|a, b| b.reputation.cmp(&a.reputation));
        sorted.truncate(limit);
        sorted
    }

    /// Get entries filtered by service type.
    pub fn entries_by_type(&self, service_type: &ServiceType) -> Vec<&ServiceRegistryEntry> {
        self.entries
            .iter()
            .filter(|e| &e.service_type == service_type)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Name Validation
// ---------------------------------------------------------------------------

/// Validate a `.shadow` name format.
///
/// Rules:
/// - Must end with `.shadow`
/// - Label before `.shadow` must be 1-63 characters
/// - Characters: `[a-z0-9-]`
/// - Must start and end with alphanumeric (not hyphen)
/// - Well-known names start with `_` (e.g., `_gateway.shadow`)
pub fn validate_name(name: &str) -> Result<(), NamingError> {
    let label = match name.strip_suffix(".shadow") {
        Some(l) => l,
        None => return Err(NamingError::InvalidName("must end with .shadow".into())),
    };

    if label.is_empty() || label.len() > MAX_NAME_LENGTH {
        return Err(NamingError::InvalidName(format!(
            "label must be 1-{} characters, got {}",
            MAX_NAME_LENGTH,
            label.len()
        )));
    }

    // Well-known names: allow leading underscore
    let check_label = if let Some(rest) = label.strip_prefix('_') {
        if rest.is_empty() {
            return Err(NamingError::InvalidName(
                "underscore prefix requires a label".into(),
            ));
        }
        rest
    } else {
        label
    };

    // Must start and end with alphanumeric
    if !check_label
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return Err(NamingError::InvalidName(
            "must start with alphanumeric character".into(),
        ));
    }
    if !check_label
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return Err(NamingError::InvalidName(
            "must end with alphanumeric character".into(),
        ));
    }

    // Only lowercase alphanumeric and hyphens
    for ch in check_label.chars() {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' {
            return Err(NamingError::InvalidName(format!(
                "invalid character '{}'; only [a-z0-9-] allowed",
                ch
            )));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Hashing & DHT Keys
// ---------------------------------------------------------------------------

/// Hash a name to create a deterministic DHT key suffix.
pub fn name_to_hash(name: &str) -> String {
    let hash = blake3::hash(name.as_bytes());
    hash.to_hex().to_string()
}

/// Create a Kademlia `RecordKey` for a name.
///
/// Format: `/shadowmesh/names/{blake3_hex}`
pub fn name_to_key(name: &str) -> RecordKey {
    RecordKey::new(&format!("/shadowmesh/names/{}", name_to_hash(name)).into_bytes())
}

// ---------------------------------------------------------------------------
// Signature Helpers
// ---------------------------------------------------------------------------

/// Produce the canonical bytes for signing a `NameRecord`.
/// The `signature` field is set to `""` before serialization.
fn canonical_bytes(record: &NameRecord) -> Result<Vec<u8>, NamingError> {
    let mut canonical = record.clone();
    canonical.signature = String::new();
    serde_json::to_vec(&canonical).map_err(|e| NamingError::SerializationError(e.to_string()))
}

/// Sign a `NameRecord` with the given keypair, populating its `signature` field.
fn sign_record(record: &mut NameRecord, keypair: &Keypair) -> Result<(), NamingError> {
    let bytes = canonical_bytes(record)?;
    let sig = keypair
        .sign(&bytes)
        .map_err(|e| NamingError::SigningFailed(e.to_string()))?;
    record.signature = hex::encode(sig);
    Ok(())
}

/// Verify the Ed25519 signature on a `NameRecord`.
pub fn verify_record(record: &NameRecord) -> Result<bool, NamingError> {
    let pubkey_bytes =
        hex::decode(&record.owner_pubkey).map_err(|e| NamingError::ValidationFailed(e.to_string()))?;

    let public_key = libp2p::identity::PublicKey::try_decode_protobuf(&pubkey_bytes)
        .map_err(|e| NamingError::ValidationFailed(format!("invalid public key: {}", e)))?;

    let bytes = canonical_bytes(record)?;
    let sig =
        hex::decode(&record.signature).map_err(|e| NamingError::ValidationFailed(e.to_string()))?;

    Ok(public_key.verify(&bytes, &sig))
}

/// Validate that a `NameRecord` is well-formed (does not verify signature).
pub fn validate_record(record: &NameRecord) -> Result<(), NamingError> {
    validate_name(&record.name)?;

    if !(MIN_NAME_TTL..=MAX_NAME_TTL).contains(&record.ttl_seconds) {
        return Err(NamingError::InvalidTtl(record.ttl_seconds));
    }

    if record.name_hash != name_to_hash(&record.name) {
        return Err(NamingError::ValidationFailed(
            "name_hash does not match name".into(),
        ));
    }

    if record.owner_pubkey.is_empty() {
        return Err(NamingError::ValidationFailed(
            "owner_pubkey is required".into(),
        ));
    }

    // Alias targets must also be valid .shadow names
    for rec in &record.records {
        if let NameRecordType::Alias { target } = rec {
            validate_name(target)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Conflict Resolution
// ---------------------------------------------------------------------------

/// Given two records for the same name, determine which one wins.
///
/// Rules:
/// 1. Higher `sequence` number wins
/// 2. If equal, earlier `created_at` wins
/// 3. If still tied, lexicographic comparison of `owner_pubkey`
pub fn resolve_conflict<'a>(a: &'a NameRecord, b: &'a NameRecord) -> &'a NameRecord {
    if a.sequence != b.sequence {
        return if a.sequence > b.sequence { a } else { b };
    }
    if a.created_at != b.created_at {
        return if a.created_at < b.created_at { a } else { b };
    }
    if a.owner_pubkey <= b.owner_pubkey {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------------
// NameRecord Helpers
// ---------------------------------------------------------------------------

impl NameRecord {
    /// Check if this record has been revoked (empty records with max sequence).
    pub fn is_revoked(&self) -> bool {
        self.records.is_empty() && self.sequence == u64::MAX
    }

    /// Get all content CIDs this name points to.
    pub fn content_cids(&self) -> Vec<&str> {
        self.records
            .iter()
            .filter_map(|r| match r {
                NameRecordType::Content { cid } => Some(cid.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Get all gateway entries.
    pub fn gateways(&self) -> Vec<(&str, &[String], u8)> {
        self.records
            .iter()
            .filter_map(|r| match r {
                NameRecordType::Gateway {
                    peer_id,
                    multiaddrs,
                    weight,
                } => Some((peer_id.as_str(), multiaddrs.as_slice(), *weight)),
                _ => None,
            })
            .collect()
    }

    /// Get service entries of a specific type.
    pub fn services(&self, filter: &ServiceType) -> Vec<(&str, &[String])> {
        self.records
            .iter()
            .filter_map(|r| match r {
                NameRecordType::Service {
                    service_type,
                    peer_id,
                    multiaddrs,
                } if service_type == filter => Some((peer_id.as_str(), multiaddrs.as_slice())),
                _ => None,
            })
            .collect()
    }

    /// Get peer identity entries.
    pub fn peers(&self) -> Vec<(&str, &[String])> {
        self.records
            .iter()
            .filter_map(|r| match r {
                NameRecordType::Peer {
                    peer_id,
                    multiaddrs,
                } => Some((peer_id.as_str(), multiaddrs.as_slice())),
                _ => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// NamingManager
// ---------------------------------------------------------------------------

/// Core manager for name records — handles creation, storage, and local cache.
pub struct NamingManager {
    /// Names owned by this node (name_hash -> record)
    owned_names: HashMap<String, NameRecord>,
    /// Locally cached records from DHT (name_hash -> record)
    local_cache: HashMap<String, NameRecord>,
    /// Service registries for well-known names
    service_registries: HashMap<String, ServiceRegistry>,
}

impl NamingManager {
    /// Create a new empty NamingManager.
    pub fn new() -> Self {
        Self {
            owned_names: HashMap::new(),
            local_cache: HashMap::new(),
            service_registries: HashMap::new(),
        }
    }

    // --- Record Creation ---

    /// Create and sign a new name record.
    pub fn create_name_record(
        &self,
        name: &str,
        records: Vec<NameRecordType>,
        keypair: &Keypair,
        ttl_seconds: u64,
    ) -> Result<NameRecord, NamingError> {
        validate_name(name)?;

        if !(MIN_NAME_TTL..=MAX_NAME_TTL).contains(&ttl_seconds) {
            return Err(NamingError::InvalidTtl(ttl_seconds));
        }

        let now = now_unix();
        let pubkey_bytes = keypair.public().encode_protobuf();

        let mut record = NameRecord {
            name: name.to_string(),
            name_hash: name_to_hash(name),
            owner_pubkey: hex::encode(pubkey_bytes),
            records,
            sequence: 1,
            created_at: now,
            updated_at: now,
            ttl_seconds,
            signature: String::new(),
        };

        sign_record(&mut record, keypair)?;
        Ok(record)
    }

    /// Update an existing name record — bumps sequence and re-signs.
    pub fn update_name_record(
        &self,
        existing: &NameRecord,
        new_records: Vec<NameRecordType>,
        keypair: &Keypair,
    ) -> Result<NameRecord, NamingError> {
        // Verify the caller owns this record
        let pubkey_bytes = keypair.public().encode_protobuf();
        let pubkey_hex = hex::encode(pubkey_bytes);
        if pubkey_hex != existing.owner_pubkey {
            return Err(NamingError::NotOwner);
        }

        let mut record = existing.clone();
        record.records = new_records;
        record.sequence = existing.sequence.saturating_add(1);
        record.updated_at = now_unix();
        record.signature = String::new();

        sign_record(&mut record, keypair)?;
        Ok(record)
    }

    /// Revoke a name — sets empty records with max sequence.
    pub fn revoke_name_record(
        &self,
        existing: &NameRecord,
        keypair: &Keypair,
    ) -> Result<NameRecord, NamingError> {
        let pubkey_bytes = keypair.public().encode_protobuf();
        let pubkey_hex = hex::encode(pubkey_bytes);
        if pubkey_hex != existing.owner_pubkey {
            return Err(NamingError::NotOwner);
        }

        let mut record = existing.clone();
        record.records = Vec::new();
        record.sequence = u64::MAX;
        record.updated_at = now_unix();
        record.signature = String::new();

        sign_record(&mut record, keypair)?;
        Ok(record)
    }

    // --- Registration Flow ---

    /// Register a new name, storing it as owned. Returns the signed record for DHT storage.
    ///
    /// The caller must first check the DHT to ensure the name is unclaimed.
    pub fn register_name(
        &mut self,
        name: &str,
        records: Vec<NameRecordType>,
        keypair: &Keypair,
        ttl_seconds: u64,
    ) -> Result<NameRecord, NamingError> {
        let hash = name_to_hash(name);

        // Check if we already own this name
        if let Some(existing) = self.owned_names.get(&hash) {
            let pubkey_hex = hex::encode(keypair.public().encode_protobuf());
            if existing.owner_pubkey != pubkey_hex {
                return Err(NamingError::NameTaken);
            }
            // Already owned by us — update instead
            return self.update_name_record(existing, records, keypair);
        }

        // Check if someone else owns it (from cache)
        if let Some(cached) = self.local_cache.get(&hash) {
            let pubkey_hex = hex::encode(keypair.public().encode_protobuf());
            if cached.owner_pubkey != pubkey_hex {
                return Err(NamingError::NameTaken);
            }
        }

        let record = self.create_name_record(name, records, keypair, ttl_seconds)?;
        self.owned_names.insert(hash, record.clone());
        Ok(record)
    }

    /// Update a name this node owns.
    pub fn update_owned_name(
        &mut self,
        name: &str,
        new_records: Vec<NameRecordType>,
        keypair: &Keypair,
    ) -> Result<NameRecord, NamingError> {
        let hash = name_to_hash(name);
        let existing = self
            .owned_names
            .get(&hash)
            .ok_or(NamingError::NotOwner)?
            .clone();

        let record = self.update_name_record(&existing, new_records, keypair)?;
        self.owned_names.insert(hash, record.clone());
        Ok(record)
    }

    // --- Local Cache ---

    /// Store a validated record in the local cache.
    pub fn cache_record(&mut self, record: NameRecord) -> Result<(), NamingError> {
        validate_record(&record)?;
        if !verify_record(&record)? {
            return Err(NamingError::InvalidSignature);
        }

        let hash = record.name_hash.clone();

        // Conflict resolution: only store if this record wins
        if let Some(existing) = self.local_cache.get(&hash) {
            let winner = resolve_conflict(existing, &record);
            if std::ptr::eq(winner, existing) {
                return Ok(()); // Existing record wins, ignore incoming
            }
        }

        self.local_cache.insert(hash, record);

        // Evict if over limit (simple: remove oldest updated_at)
        if self.local_cache.len() > MAX_CACHE_ENTRIES {
            if let Some(oldest_key) = self
                .local_cache
                .iter()
                .min_by_key(|(_, r)| r.updated_at)
                .map(|(k, _)| k.clone())
            {
                self.local_cache.remove(&oldest_key);
            }
        }

        Ok(())
    }

    /// Look up a name in the local cache.
    pub fn resolve_local(&self, name: &str) -> Option<&NameRecord> {
        let hash = name_to_hash(name);
        // Check owned names first, then cache
        self.owned_names
            .get(&hash)
            .or_else(|| self.local_cache.get(&hash))
    }

    /// Invalidate a cached record (e.g., on GossipSub update).
    pub fn invalidate_cache(&mut self, name: &str) {
        let hash = name_to_hash(name);
        self.local_cache.remove(&hash);
    }

    /// Get all names owned by this node.
    pub fn owned_names(&self) -> Vec<&NameRecord> {
        self.owned_names.values().collect()
    }

    // --- Service Registries ---

    /// Get or create a service registry for a well-known name.
    pub fn service_registry(&mut self, name: &str) -> &mut ServiceRegistry {
        self.service_registries
            .entry(name.to_string())
            .or_insert_with(|| ServiceRegistry::new(name.to_string()))
    }

    /// Get a service registry (read-only).
    pub fn get_service_registry(&self, name: &str) -> Option<&ServiceRegistry> {
        self.service_registries.get(name)
    }

    /// Create a signed service registry entry.
    pub fn create_service_entry(
        &self,
        service_type: ServiceType,
        peer_id: &str,
        multiaddrs: Vec<String>,
        reputation: u8,
        keypair: &Keypair,
    ) -> Result<ServiceRegistryEntry, NamingError> {
        let now = now_unix();

        let mut entry = ServiceRegistryEntry {
            service_type,
            peer_id: peer_id.to_string(),
            multiaddrs,
            reputation,
            registered_at: now,
            last_heartbeat: now,
            signature: String::new(),
        };

        // Sign the entry
        let bytes = serde_json::to_vec(&{
            let mut e = entry.clone();
            e.signature = String::new();
            e
        })
        .map_err(|e| NamingError::SerializationError(e.to_string()))?;

        let sig = keypair
            .sign(&bytes)
            .map_err(|e| NamingError::SigningFailed(e.to_string()))?;
        entry.signature = hex::encode(sig);

        Ok(entry)
    }

    /// Prune stale entries from all service registries.
    pub fn prune_all_registries(&mut self) {
        for registry in self.service_registries.values_mut() {
            registry.prune_stale();
        }
    }

    // --- Serialization ---

    /// Serialize a name record for DHT storage.
    pub fn serialize_record(record: &NameRecord) -> Result<Vec<u8>, NamingError> {
        serde_json::to_vec(record).map_err(|e| NamingError::SerializationError(e.to_string()))
    }

    /// Deserialize a name record from DHT data.
    pub fn deserialize_record(data: &[u8]) -> Result<NameRecord, NamingError> {
        serde_json::from_slice(data).map_err(|e| NamingError::SerializationError(e.to_string()))
    }

    /// Serialize a service registry for DHT storage.
    pub fn serialize_registry(registry: &ServiceRegistry) -> Result<Vec<u8>, NamingError> {
        serde_json::to_vec(registry).map_err(|e| NamingError::SerializationError(e.to_string()))
    }

    /// Deserialize a service registry from DHT data.
    pub fn deserialize_registry(data: &[u8]) -> Result<ServiceRegistry, NamingError> {
        serde_json::from_slice(data).map_err(|e| NamingError::SerializationError(e.to_string()))
    }
}

impl Default for NamingManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// NameResolver — Cache layer with TTL
// ---------------------------------------------------------------------------

/// Cache entry with expiration tracking.
struct CachedEntry {
    record: NameRecord,
    cached_at: Instant,
    ttl: Duration,
}

impl CachedEntry {
    fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}

/// Result of a name resolution attempt.
#[derive(Debug)]
pub enum ResolveResult {
    /// Name was found (from cache or DHT result)
    Found(NameRecord),
    /// Name was confirmed to not exist
    NotFound,
    /// Cache miss — caller should perform a DHT lookup
    CacheMiss,
}

/// Resolver with positive and negative caching.
pub struct NameResolver {
    /// Positive cache: name_hash -> cached record
    positive_cache: HashMap<String, CachedEntry>,
    /// Negative cache: name_hash -> when the miss was recorded
    negative_cache: HashMap<String, Instant>,
    /// Maximum positive cache entries
    max_entries: usize,
}

impl NameResolver {
    /// Create a new resolver with default cache size.
    pub fn new() -> Self {
        Self::with_capacity(MAX_CACHE_ENTRIES)
    }

    /// Create a resolver with a specific cache capacity.
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            positive_cache: HashMap::new(),
            negative_cache: HashMap::new(),
            max_entries,
        }
    }

    /// Resolve a name from the local cache.
    ///
    /// Returns `Found` on cache hit, `NotFound` on negative cache hit,
    /// or `CacheMiss` if the caller should query the DHT.
    pub fn resolve(&self, name: &str) -> ResolveResult {
        let hash = name_to_hash(name);

        // Check positive cache
        if let Some(entry) = self.positive_cache.get(&hash) {
            if !entry.is_expired() {
                return ResolveResult::Found(entry.record.clone());
            }
            // Expired — treat as cache miss
        }

        // Check negative cache
        if let Some(cached_at) = self.negative_cache.get(&hash) {
            if cached_at.elapsed() < NEGATIVE_CACHE_TTL {
                return ResolveResult::NotFound;
            }
            // Expired negative entry — treat as miss
        }

        ResolveResult::CacheMiss
    }

    /// Process a DHT lookup result — validates, caches, and returns the result.
    pub fn process_dht_result(
        &mut self,
        name: &str,
        data: Option<&[u8]>,
    ) -> ResolveResult {
        let hash = name_to_hash(name);

        let Some(bytes) = data else {
            // Name not found in DHT — add to negative cache
            self.negative_cache.insert(hash, Instant::now());
            return ResolveResult::NotFound;
        };

        // Deserialize
        let record = match NamingManager::deserialize_record(bytes) {
            Ok(r) => r,
            Err(_) => {
                self.negative_cache.insert(hash, Instant::now());
                return ResolveResult::NotFound;
            }
        };

        // Validate
        if validate_record(&record).is_err() {
            self.negative_cache.insert(hash, Instant::now());
            return ResolveResult::NotFound;
        }

        // Verify signature
        match verify_record(&record) {
            Ok(true) => {}
            _ => {
                self.negative_cache.insert(hash, Instant::now());
                return ResolveResult::NotFound;
            }
        }

        // Check for revocation
        if record.is_revoked() {
            self.negative_cache.insert(hash, Instant::now());
            return ResolveResult::NotFound;
        }

        // Conflict resolution with existing cache
        if let Some(existing) = self.positive_cache.get(&hash) {
            let winner = resolve_conflict(&existing.record, &record);
            if std::ptr::eq(winner, &existing.record) {
                return ResolveResult::Found(existing.record.clone());
            }
        }

        // Cache the record
        let ttl = Duration::from_secs(record.ttl_seconds.min(MAX_NAME_TTL));
        self.positive_cache.insert(
            hash.clone(),
            CachedEntry {
                record: record.clone(),
                cached_at: Instant::now(),
                ttl,
            },
        );
        // Remove from negative cache if present
        self.negative_cache.remove(&hash);

        // Evict if over capacity
        self.evict_if_needed();

        ResolveResult::Found(record)
    }

    /// Invalidate a cached entry (e.g., when a GossipSub update arrives).
    pub fn invalidate(&mut self, name: &str) {
        let hash = name_to_hash(name);
        self.positive_cache.remove(&hash);
        self.negative_cache.remove(&hash);
    }

    /// Clean up all expired entries.
    pub fn cleanup_expired(&mut self) {
        self.positive_cache.retain(|_, entry| !entry.is_expired());
        self.negative_cache
            .retain(|_, cached_at| cached_at.elapsed() < NEGATIVE_CACHE_TTL);
    }

    /// Evict oldest entries if over capacity.
    fn evict_if_needed(&mut self) {
        while self.positive_cache.len() > self.max_entries {
            // Find the entry with the oldest `cached_at`
            if let Some(oldest_key) = self
                .positive_cache
                .iter()
                .min_by_key(|(_, e)| e.cached_at)
                .map(|(k, _)| k.clone())
            {
                self.positive_cache.remove(&oldest_key);
            } else {
                break;
            }
        }
    }

    /// Get the number of cached entries.
    pub fn cache_size(&self) -> usize {
        self.positive_cache.len()
    }

    /// Get the number of negative cache entries.
    pub fn negative_cache_size(&self) -> usize {
        self.negative_cache.len()
    }
}

impl Default for NameResolver {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get the current unix timestamp in seconds.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;

    fn test_keypair() -> Keypair {
        identity::Keypair::generate_ed25519()
    }

    // -- Name Validation --

    #[test]
    fn test_valid_names() {
        assert!(validate_name("myapp.shadow").is_ok());
        assert!(validate_name("my-app.shadow").is_ok());
        assert!(validate_name("a.shadow").is_ok());
        assert!(validate_name("123.shadow").is_ok());
        assert!(validate_name("a1b2c3.shadow").is_ok());
        assert!(validate_name("_gateway.shadow").is_ok());
        assert!(validate_name("_signaling.shadow").is_ok());
    }

    #[test]
    fn test_invalid_names() {
        assert!(validate_name("myapp").is_err()); // missing .shadow
        assert!(validate_name(".shadow").is_err()); // empty label
        assert!(validate_name("-app.shadow").is_err()); // starts with hyphen
        assert!(validate_name("app-.shadow").is_err()); // ends with hyphen
        assert!(validate_name("MyApp.shadow").is_err()); // uppercase
        assert!(validate_name("my app.shadow").is_err()); // space
        assert!(validate_name("my_app.shadow").is_err()); // underscore in middle (not well-known)
        assert!(validate_name("_.shadow").is_err()); // just underscore
    }

    // -- Hashing --

    #[test]
    fn test_name_to_hash_deterministic() {
        let h1 = name_to_hash("myapp.shadow");
        let h2 = name_to_hash("myapp.shadow");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // BLAKE3 hex output

        // Different names produce different hashes
        let h3 = name_to_hash("other.shadow");
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_name_to_key() {
        let key = name_to_key("myapp.shadow");
        let expected_prefix = "/shadowmesh/names/";
        let key_str = String::from_utf8_lossy(key.as_ref());
        assert!(key_str.starts_with(expected_prefix));
    }

    // -- Record Creation & Signing --

    #[test]
    fn test_create_and_verify_record() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        let record = manager
            .create_name_record(
                "myapp.shadow",
                vec![NameRecordType::Content {
                    cid: "QmTest123".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        assert_eq!(record.name, "myapp.shadow");
        assert_eq!(record.sequence, 1);
        assert!(!record.signature.is_empty());
        assert!(verify_record(&record).unwrap());
        assert!(validate_record(&record).is_ok());
    }

    #[test]
    fn test_tampered_record_fails_verification() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        let mut record = manager
            .create_name_record(
                "myapp.shadow",
                vec![NameRecordType::Content {
                    cid: "QmTest123".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        // Tamper with the record
        record.records = vec![NameRecordType::Content {
            cid: "QmEvil".into(),
        }];

        assert!(!verify_record(&record).unwrap());
    }

    #[test]
    fn test_wrong_key_fails_verification() {
        let keypair1 = test_keypair();
        let keypair2 = test_keypair();
        let manager = NamingManager::new();

        let mut record = manager
            .create_name_record(
                "myapp.shadow",
                vec![NameRecordType::Content {
                    cid: "QmTest123".into(),
                }],
                &keypair1,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        // Replace the pubkey with a different key
        record.owner_pubkey = hex::encode(keypair2.public().encode_protobuf());

        // Signature was made with keypair1 but pubkey now says keypair2
        assert!(!verify_record(&record).unwrap());
    }

    // -- Record Update --

    #[test]
    fn test_update_record() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        let original = manager
            .create_name_record(
                "myapp.shadow",
                vec![NameRecordType::Content {
                    cid: "QmOld".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        let updated = manager
            .update_name_record(
                &original,
                vec![NameRecordType::Content {
                    cid: "QmNew".into(),
                }],
                &keypair,
            )
            .unwrap();

        assert_eq!(updated.sequence, 2);
        assert!(verify_record(&updated).unwrap());
        assert_eq!(updated.content_cids(), vec!["QmNew"]);
    }

    #[test]
    fn test_update_by_non_owner_fails() {
        let keypair1 = test_keypair();
        let keypair2 = test_keypair();
        let manager = NamingManager::new();

        let original = manager
            .create_name_record(
                "myapp.shadow",
                vec![NameRecordType::Content {
                    cid: "QmOld".into(),
                }],
                &keypair1,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        let result = manager.update_name_record(
            &original,
            vec![NameRecordType::Content {
                cid: "QmEvil".into(),
            }],
            &keypair2,
        );

        assert_eq!(result.unwrap_err(), NamingError::NotOwner);
    }

    // -- Revocation --

    #[test]
    fn test_revoke_record() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        let original = manager
            .create_name_record(
                "myapp.shadow",
                vec![NameRecordType::Content {
                    cid: "QmTest".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        let revoked = manager.revoke_name_record(&original, &keypair).unwrap();
        assert!(revoked.is_revoked());
        assert!(revoked.records.is_empty());
        assert_eq!(revoked.sequence, u64::MAX);
        assert!(verify_record(&revoked).unwrap());
    }

    // -- Conflict Resolution --

    #[test]
    fn test_conflict_higher_sequence_wins() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        let v1 = manager
            .create_name_record("x.shadow", vec![], &keypair, DEFAULT_NAME_TTL)
            .unwrap();
        let v2 = manager
            .update_name_record(&v1, vec![], &keypair)
            .unwrap();

        let winner = resolve_conflict(&v1, &v2);
        assert_eq!(winner.sequence, 2);
    }

    // -- NamingManager Registration --

    #[test]
    fn test_register_and_resolve() {
        let keypair = test_keypair();
        let mut manager = NamingManager::new();

        let record = manager
            .register_name(
                "mysite.shadow",
                vec![NameRecordType::Content {
                    cid: "QmSite".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        assert_eq!(record.name, "mysite.shadow");

        let resolved = manager.resolve_local("mysite.shadow").unwrap();
        assert_eq!(resolved.content_cids(), vec!["QmSite"]);
    }

    #[test]
    fn test_register_duplicate_by_different_owner() {
        let keypair1 = test_keypair();
        let keypair2 = test_keypair();
        let mut manager = NamingManager::new();

        manager
            .register_name("taken.shadow", vec![], &keypair1, DEFAULT_NAME_TTL)
            .unwrap();

        // Cache the record so the manager knows it exists
        let record = manager.resolve_local("taken.shadow").unwrap().clone();
        manager.cache_record(record).unwrap();

        let result = manager.register_name("taken.shadow", vec![], &keypair2, DEFAULT_NAME_TTL);
        assert_eq!(result.unwrap_err(), NamingError::NameTaken);
    }

    // -- NameResolver --

    #[test]
    fn test_resolver_cache_miss() {
        let resolver = NameResolver::new();
        assert!(matches!(
            resolver.resolve("unknown.shadow"),
            ResolveResult::CacheMiss
        ));
    }

    #[test]
    fn test_resolver_dht_result_caching() {
        let keypair = test_keypair();
        let manager = NamingManager::new();
        let mut resolver = NameResolver::new();

        let record = manager
            .create_name_record(
                "cached.shadow",
                vec![NameRecordType::Content {
                    cid: "QmCached".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        let data = NamingManager::serialize_record(&record).unwrap();
        let result = resolver.process_dht_result("cached.shadow", Some(&data));

        assert!(matches!(result, ResolveResult::Found(_)));
        assert!(matches!(
            resolver.resolve("cached.shadow"),
            ResolveResult::Found(_)
        ));
    }

    #[test]
    fn test_resolver_negative_cache() {
        let mut resolver = NameResolver::new();

        let result = resolver.process_dht_result("missing.shadow", None);
        assert!(matches!(result, ResolveResult::NotFound));

        // Should be in negative cache now
        assert!(matches!(
            resolver.resolve("missing.shadow"),
            ResolveResult::NotFound
        ));
    }

    #[test]
    fn test_resolver_rejects_invalid_signature() {
        let keypair = test_keypair();
        let manager = NamingManager::new();
        let mut resolver = NameResolver::new();

        let mut record = manager
            .create_name_record(
                "bad.shadow",
                vec![NameRecordType::Content {
                    cid: "QmBad".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        // Tamper with the record
        record.records = vec![NameRecordType::Content {
            cid: "QmTampered".into(),
        }];

        let data = serde_json::to_vec(&record).unwrap();
        let result = resolver.process_dht_result("bad.shadow", Some(&data));

        assert!(matches!(result, ResolveResult::NotFound));
    }

    // -- Service Registry --

    #[test]
    fn test_service_registry() {
        let mut registry = ServiceRegistry::new("_gateway.shadow".into());

        let entry1 = ServiceRegistryEntry {
            service_type: ServiceType::Bootstrap,
            peer_id: "peer1".into(),
            multiaddrs: vec!["/ip4/1.2.3.4/tcp/4001".into()],
            reputation: 80,
            registered_at: now_unix(),
            last_heartbeat: now_unix(),
            signature: String::new(),
        };

        let entry2 = ServiceRegistryEntry {
            service_type: ServiceType::Bootstrap,
            peer_id: "peer2".into(),
            multiaddrs: vec!["/ip4/5.6.7.8/tcp/4001".into()],
            reputation: 90,
            registered_at: now_unix(),
            last_heartbeat: now_unix(),
            signature: String::new(),
        };

        registry.upsert_entry(entry1);
        registry.upsert_entry(entry2);

        assert_eq!(registry.entries.len(), 2);

        let best = registry.best_entries(1);
        assert_eq!(best[0].peer_id, "peer2"); // Higher reputation
    }

    #[test]
    fn test_service_registry_prune() {
        let mut registry = ServiceRegistry::new("_gateway.shadow".into());

        let stale = ServiceRegistryEntry {
            service_type: ServiceType::Bootstrap,
            peer_id: "stale_peer".into(),
            multiaddrs: vec![],
            reputation: 50,
            registered_at: 0,
            last_heartbeat: 0, // Very old
            signature: String::new(),
        };

        let fresh = ServiceRegistryEntry {
            service_type: ServiceType::Bootstrap,
            peer_id: "fresh_peer".into(),
            multiaddrs: vec![],
            reputation: 50,
            registered_at: now_unix(),
            last_heartbeat: now_unix(),
            signature: String::new(),
        };

        registry.upsert_entry(stale);
        registry.upsert_entry(fresh);
        assert_eq!(registry.entries.len(), 2);

        registry.prune_stale();
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.entries[0].peer_id, "fresh_peer");
    }

    // -- Well-Known Names --

    #[test]
    fn test_well_known_names() {
        assert!(WellKnownNames::is_well_known("_gateway.shadow"));
        assert!(WellKnownNames::is_well_known("_signaling.shadow"));
        assert!(WellKnownNames::is_well_known("_bootstrap.shadow"));
        assert!(!WellKnownNames::is_well_known("myapp.shadow"));
    }

    // -- Record Accessors --

    #[test]
    fn test_record_accessors() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        let record = manager
            .create_name_record(
                "multi.shadow",
                vec![
                    NameRecordType::Content {
                        cid: "QmContent".into(),
                    },
                    NameRecordType::Gateway {
                        peer_id: "peer1".into(),
                        multiaddrs: vec!["/ip4/1.2.3.4/tcp/4001".into()],
                        weight: 100,
                    },
                    NameRecordType::Service {
                        service_type: ServiceType::Signaling,
                        peer_id: "peer2".into(),
                        multiaddrs: vec!["/ip4/5.6.7.8/tcp/4002".into()],
                    },
                    NameRecordType::Peer {
                        peer_id: "peer3".into(),
                        multiaddrs: vec!["/ip4/9.10.11.12/tcp/4001".into()],
                    },
                ],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        assert_eq!(record.content_cids(), vec!["QmContent"]);
        assert_eq!(record.gateways().len(), 1);
        assert_eq!(record.services(&ServiceType::Signaling).len(), 1);
        assert_eq!(record.peers().len(), 1);
    }

    // -- TTL Validation --

    #[test]
    fn test_ttl_bounds() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        // Too low
        let result = manager.create_name_record("low.shadow", vec![], &keypair, 10);
        assert!(matches!(result, Err(NamingError::InvalidTtl(10))));

        // Too high
        let result = manager.create_name_record("high.shadow", vec![], &keypair, 100_000);
        assert!(matches!(result, Err(NamingError::InvalidTtl(100_000))));

        // Just right
        let result = manager.create_name_record("ok.shadow", vec![], &keypair, 3600);
        assert!(result.is_ok());
    }

    // -- Serialization Round-trip --

    #[test]
    fn test_record_serialization_roundtrip() {
        let keypair = test_keypair();
        let manager = NamingManager::new();

        let original = manager
            .create_name_record(
                "serial.shadow",
                vec![NameRecordType::Content {
                    cid: "QmSerial".into(),
                }],
                &keypair,
                DEFAULT_NAME_TTL,
            )
            .unwrap();

        let bytes = NamingManager::serialize_record(&original).unwrap();
        let deserialized = NamingManager::deserialize_record(&bytes).unwrap();

        assert_eq!(deserialized.name, original.name);
        assert_eq!(deserialized.name_hash, original.name_hash);
        assert_eq!(deserialized.signature, original.signature);
        assert!(verify_record(&deserialized).unwrap());
    }
}
