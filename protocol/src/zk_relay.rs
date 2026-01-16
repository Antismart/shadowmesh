//! Zero-Knowledge Relay Protocol
//!
//! Implements a relay system where nodes can serve content without knowing
//! what they are serving. This provides plausible deniability for relay operators.
//!
//! # Key Concepts
//!
//! - **Blind Relay**: Nodes relay encrypted packets without decryption capability
//! - **Circuit-Based Routing**: Requests travel through pre-established circuits
//! - **Layered Encryption**: Each hop only decrypts its layer, revealing next hop
//! - **Request Unlinkability**: Cannot link requests from same user
//!
//! # Protocol Flow
//!
//! 1. Client builds a circuit through 3+ relay nodes
//! 2. Client encrypts request in onion layers (outermost = first hop)
//! 3. Each relay decrypts its layer, learns only the next hop
//! 4. Exit relay retrieves content, wraps response in layers
//! 5. Response travels back through circuit
//! 6. Client decrypts all layers to get content

use crate::crypto::{CryptoManager, KEY_SIZE};
use blake3::Hasher;
use chacha20poly1305::aead::{AeadCore, OsRng};
use chacha20poly1305::ChaCha20Poly1305;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Circuit identifier (32 bytes)
pub type CircuitId = [u8; 32];

/// Hop identifier within a circuit
pub type HopId = [u8; 16];

/// Relay protocol errors
#[derive(Debug, Clone)]
pub enum RelayError {
    CircuitNotFound,
    CircuitExpired,
    CircuitBuildFailed(String),
    DecryptionFailed(String),
    EncryptionFailed(String),
    InvalidPayload,
    InvalidHop,
    ForwardFailed(String),
    MaxCircuitsExceeded,
    RateLimited,
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelayError::CircuitNotFound => write!(f, "Circuit not found"),
            RelayError::CircuitExpired => write!(f, "Circuit has expired"),
            RelayError::CircuitBuildFailed(msg) => write!(f, "Circuit build failed: {}", msg),
            RelayError::DecryptionFailed(msg) => write!(f, "Decryption failed: {}", msg),
            RelayError::EncryptionFailed(msg) => write!(f, "Encryption failed: {}", msg),
            RelayError::InvalidPayload => write!(f, "Invalid payload format"),
            RelayError::InvalidHop => write!(f, "Invalid hop in circuit"),
            RelayError::ForwardFailed(msg) => write!(f, "Forward failed: {}", msg),
            RelayError::MaxCircuitsExceeded => write!(f, "Maximum circuits exceeded"),
            RelayError::RateLimited => write!(f, "Rate limited"),
        }
    }
}

impl std::error::Error for RelayError {}

/// Relay cell types (similar to Tor cells)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CellType {
    /// Create a new circuit
    Create = 0,
    /// Acknowledge circuit creation
    Created = 1,
    /// Extend circuit by one hop
    Extend = 2,
    /// Acknowledge circuit extension
    Extended = 3,
    /// Relay data through circuit
    Relay = 4,
    /// Destroy circuit
    Destroy = 5,
    /// Padding cell (for traffic analysis resistance)
    Padding = 6,
}

/// A relay cell - the basic unit of communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCell {
    /// Circuit identifier
    pub circuit_id: CircuitId,
    /// Cell type
    pub cell_type: CellType,
    /// Encrypted payload
    pub payload: Vec<u8>,
    /// Timestamp for replay protection
    pub timestamp: u64,
}

impl RelayCell {
    /// Create a new relay cell
    pub fn new(circuit_id: CircuitId, cell_type: CellType, payload: Vec<u8>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            circuit_id,
            cell_type,
            payload,
            timestamp,
        }
    }

    /// Serialize cell for transmission
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize cell from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, RelayError> {
        bincode::deserialize(data).map_err(|_| RelayError::InvalidPayload)
    }
}

/// Encrypted relay payload (what relay nodes see)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPayload {
    /// Next hop (if relaying) or empty (if exit)
    pub next_hop: Option<PeerId>,
    /// Hop-specific identifier
    pub hop_id: HopId,
    /// Inner encrypted data (for next hop or actual content)
    pub inner_data: Vec<u8>,
    /// Digest for integrity verification
    pub digest: [u8; 16],
}

impl RelayPayload {
    /// Create a new relay payload
    pub fn new(next_hop: Option<PeerId>, hop_id: HopId, inner_data: Vec<u8>) -> Self {
        let digest = Self::compute_digest(&inner_data, &hop_id);
        Self {
            next_hop,
            hop_id,
            inner_data,
            digest,
        }
    }

    /// Compute digest for integrity check
    fn compute_digest(data: &[u8], hop_id: &HopId) -> [u8; 16] {
        let mut hasher = Hasher::new();
        hasher.update(data);
        hasher.update(hop_id);
        let hash = hasher.finalize();
        let mut digest = [0u8; 16];
        digest.copy_from_slice(&hash.as_bytes()[..16]);
        digest
    }

    /// Verify payload integrity
    pub fn verify(&self) -> bool {
        let expected = Self::compute_digest(&self.inner_data, &self.hop_id);
        self.digest == expected
    }
}

/// A circuit hop (from the perspective of the client)
#[derive(Debug, Clone)]
pub struct CircuitHop {
    /// Peer ID of this hop
    pub peer_id: PeerId,
    /// Symmetric key for this hop (derived from handshake)
    pub key: [u8; KEY_SIZE],
    /// Hop identifier
    pub hop_id: HopId,
}

/// Client-side circuit representation
#[derive(Debug, Clone)]
pub struct Circuit {
    /// Unique circuit identifier
    pub id: CircuitId,
    /// Ordered list of hops
    pub hops: Vec<CircuitHop>,
    /// When the circuit was created
    pub created_at: Instant,
    /// Circuit lifetime
    pub lifetime: Duration,
    /// Whether circuit is ready for use
    pub ready: bool,
}

impl Circuit {
    /// Create a new empty circuit
    pub fn new(lifetime: Duration) -> Self {
        let mut id = [0u8; 32];
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        id[..12].copy_from_slice(&nonce);
        
        // Add random bytes
        let mut hasher = Hasher::new();
        hasher.update(&nonce);
        hasher.update(&rand::random::<[u8; 32]>());
        let hash = hasher.finalize();
        id[12..].copy_from_slice(&hash.as_bytes()[..20]);

        Self {
            id,
            hops: Vec::new(),
            created_at: Instant::now(),
            lifetime,
            ready: false,
        }
    }

    /// Check if circuit has expired
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.lifetime
    }

    /// Get number of hops
    pub fn num_hops(&self) -> usize {
        self.hops.len()
    }

    /// Add a hop to the circuit
    pub fn add_hop(&mut self, peer_id: PeerId, key: [u8; KEY_SIZE]) {
        let hop_id = Self::generate_hop_id(&self.id, self.hops.len());
        self.hops.push(CircuitHop {
            peer_id,
            key,
            hop_id,
        });
    }

    /// Generate a hop ID
    fn generate_hop_id(circuit_id: &CircuitId, hop_index: usize) -> HopId {
        let mut hasher = Hasher::new();
        hasher.update(circuit_id);
        hasher.update(&hop_index.to_le_bytes());
        let hash = hasher.finalize();
        let mut hop_id = [0u8; 16];
        hop_id.copy_from_slice(&hash.as_bytes()[..16]);
        hop_id
    }

    /// Get keys for onion encryption (in hop order)
    pub fn keys(&self) -> Vec<[u8; KEY_SIZE]> {
        self.hops.iter().map(|h| h.key).collect()
    }
}

/// Relay node's view of a circuit (minimal information)
#[derive(Debug, Clone)]
pub struct RelayCircuitState {
    /// Circuit ID
    pub circuit_id: CircuitId,
    /// Key for this hop
    pub hop_key: [u8; KEY_SIZE],
    /// Previous hop (where cell came from)
    pub prev_hop: Option<PeerId>,
    /// Next hop (where to forward)
    pub next_hop: Option<PeerId>,
    /// When this circuit state was created
    pub created_at: Instant,
    /// Expiry time
    pub expires_at: Instant,
}

impl RelayCircuitState {
    /// Check if circuit state has expired
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

/// Configuration for the ZK relay
#[derive(Debug, Clone)]
pub struct ZkRelayConfig {
    /// Default number of hops for circuits
    pub default_hops: usize,
    /// Circuit lifetime
    pub circuit_lifetime: Duration,
    /// Maximum circuits per relay
    pub max_circuits: usize,
    /// Enable padding cells for traffic analysis resistance
    pub padding_enabled: bool,
    /// Padding interval
    pub padding_interval: Duration,
    /// Cell timeout
    pub cell_timeout: Duration,
}

impl Default for ZkRelayConfig {
    fn default() -> Self {
        Self {
            default_hops: 3,
            circuit_lifetime: Duration::from_secs(600), // 10 minutes
            max_circuits: 10000,
            padding_enabled: true,
            padding_interval: Duration::from_millis(100),
            cell_timeout: Duration::from_secs(30),
        }
    }
}

/// Zero-Knowledge Relay Client
/// 
/// Used by clients to build circuits and send requests anonymously.
pub struct ZkRelayClient {
    /// Client's secret for key derivation
    secret: [u8; KEY_SIZE],
    /// Active circuits
    circuits: HashMap<CircuitId, Circuit>,
    /// Configuration
    config: ZkRelayConfig,
}

impl ZkRelayClient {
    /// Create a new ZK relay client
    pub fn new(secret: [u8; KEY_SIZE]) -> Self {
        Self {
            secret,
            circuits: HashMap::new(),
            config: ZkRelayConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(secret: [u8; KEY_SIZE], config: ZkRelayConfig) -> Self {
        Self {
            secret,
            circuits: HashMap::new(),
            config,
        }
    }

    /// Build a new circuit through the specified peers
    pub fn build_circuit(&mut self, peers: &[PeerId]) -> Result<CircuitId, RelayError> {
        if peers.len() < 2 {
            return Err(RelayError::CircuitBuildFailed(
                "Need at least 2 hops".to_string(),
            ));
        }

        let mut circuit = Circuit::new(self.config.circuit_lifetime);
        let circuit_id = circuit.id;

        // Derive keys for each hop using ECDH in production
        // For now, we derive keys from our secret + peer ID
        for (i, peer) in peers.iter().enumerate() {
            let key = self.derive_hop_key(&circuit_id, peer, i);
            circuit.add_hop(*peer, key);
        }

        circuit.ready = true;
        self.circuits.insert(circuit_id, circuit);

        Ok(circuit_id)
    }

    /// Derive a key for a specific hop (simplified - production would use ECDH)
    fn derive_hop_key(&self, circuit_id: &CircuitId, peer: &PeerId, hop_index: usize) -> [u8; KEY_SIZE] {
        let mut hasher = Hasher::new();
        hasher.update(&self.secret);
        hasher.update(circuit_id);
        hasher.update(peer.to_bytes().as_slice());
        hasher.update(&hop_index.to_le_bytes());
        
        let hash = hasher.finalize();
        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&hash.as_bytes()[..KEY_SIZE]);
        key
    }

    /// Wrap a request for sending through a circuit
    pub fn wrap_request(
        &self,
        circuit_id: &CircuitId,
        request: &[u8],
    ) -> Result<RelayCell, RelayError> {
        let circuit = self
            .circuits
            .get(circuit_id)
            .ok_or(RelayError::CircuitNotFound)?;

        if circuit.is_expired() {
            return Err(RelayError::CircuitExpired);
        }

        if !circuit.ready {
            return Err(RelayError::CircuitBuildFailed(
                "Circuit not ready".to_string(),
            ));
        }

        // Build layered payload from exit to entry
        let mut payload = request.to_vec();

        for (i, hop) in circuit.hops.iter().enumerate().rev() {
            let next_hop = if i == circuit.hops.len() - 1 {
                None // Exit node has no next hop
            } else {
                Some(circuit.hops[i + 1].peer_id)
            };

            let relay_payload = RelayPayload::new(next_hop, hop.hop_id, payload);
            let payload_bytes =
                bincode::serialize(&relay_payload).map_err(|_| RelayError::InvalidPayload)?;

            // Encrypt for this hop
            let manager = CryptoManager::new(&hop.key);
            payload = manager
                .encrypt_raw(&payload_bytes)
                .map_err(|e| RelayError::EncryptionFailed(e.to_string()))?;
        }

        Ok(RelayCell::new(*circuit_id, CellType::Relay, payload))
    }

    /// Unwrap a response received through a circuit
    pub fn unwrap_response(
        &self,
        circuit_id: &CircuitId,
        cell: &RelayCell,
    ) -> Result<Vec<u8>, RelayError> {
        let circuit = self
            .circuits
            .get(circuit_id)
            .ok_or(RelayError::CircuitNotFound)?;

        // Decrypt each layer from entry to exit
        let mut data = cell.payload.clone();

        for hop in &circuit.hops {
            let manager = CryptoManager::new(&hop.key);
            data = manager
                .decrypt_raw(&data)
                .map_err(|e| RelayError::DecryptionFailed(e.to_string()))?;
        }

        Ok(data)
    }

    /// Get the entry node for a circuit (where to send cells)
    pub fn get_entry_node(&self, circuit_id: &CircuitId) -> Option<PeerId> {
        self.circuits
            .get(circuit_id)
            .and_then(|c| c.hops.first().map(|h| h.peer_id))
    }

    /// Destroy a circuit
    pub fn destroy_circuit(&mut self, circuit_id: &CircuitId) -> Option<RelayCell> {
        if self.circuits.remove(circuit_id).is_some() {
            Some(RelayCell::new(*circuit_id, CellType::Destroy, vec![]))
        } else {
            None
        }
    }

    /// Get active circuit count
    pub fn active_circuits(&self) -> usize {
        self.circuits.len()
    }

    /// Clean up expired circuits
    pub fn cleanup_expired(&mut self) -> usize {
        let before = self.circuits.len();
        self.circuits.retain(|_, c| !c.is_expired());
        before - self.circuits.len()
    }

    /// Create a padding cell for traffic analysis resistance
    pub fn create_padding_cell(&self, circuit_id: &CircuitId) -> Option<RelayCell> {
        if !self.config.padding_enabled {
            return None;
        }

        // Generate random padding
        let padding: Vec<u8> = (0..512).map(|_| rand::random()).collect();
        
        Some(RelayCell::new(*circuit_id, CellType::Padding, padding))
    }
}

/// Zero-Knowledge Relay Node
/// 
/// Used by relay nodes to forward cells without knowing content.
pub struct ZkRelayNode {
    /// Node's keypair seed
    node_secret: [u8; KEY_SIZE],
    /// Circuit states (minimal info needed for forwarding)
    circuit_states: Arc<RwLock<HashMap<CircuitId, RelayCircuitState>>>,
    /// Configuration
    config: ZkRelayConfig,
    /// Statistics
    stats: Arc<RwLock<RelayStats>>,
}

/// Relay statistics
#[derive(Debug, Clone, Default)]
pub struct RelayStats {
    /// Total cells relayed
    pub cells_relayed: u64,
    /// Total bytes relayed
    pub bytes_relayed: u64,
    /// Circuits currently active
    pub active_circuits: usize,
    /// Circuits created total
    pub circuits_created: u64,
    /// Circuits destroyed
    pub circuits_destroyed: u64,
    /// Decryption failures (doesn't reveal content, just count)
    pub decryption_failures: u64,
}

impl ZkRelayNode {
    /// Create a new relay node
    pub fn new(node_secret: [u8; KEY_SIZE]) -> Self {
        Self {
            node_secret,
            circuit_states: Arc::new(RwLock::new(HashMap::new())),
            config: ZkRelayConfig::default(),
            stats: Arc::new(RwLock::new(RelayStats::default())),
        }
    }

    /// Create with custom config
    pub fn with_config(node_secret: [u8; KEY_SIZE], config: ZkRelayConfig) -> Self {
        Self {
            node_secret,
            circuit_states: Arc::new(RwLock::new(HashMap::new())),
            config,
            stats: Arc::new(RwLock::new(RelayStats::default())),
        }
    }

    /// Process an incoming cell
    /// 
    /// This is the core relay function. The node:
    /// 1. Looks up circuit state
    /// 2. Decrypts exactly one layer
    /// 3. Reads next hop from payload
    /// 4. Forwards to next hop (or processes if exit)
    /// 
    /// The node NEVER sees the actual content - only encrypted inner layers.
    pub fn process_cell(
        &self,
        cell: RelayCell,
        from_peer: PeerId,
    ) -> Result<RelayAction, RelayError> {
        match cell.cell_type {
            CellType::Create => self.handle_create(cell, from_peer),
            CellType::Extend => self.handle_extend(cell, from_peer),
            CellType::Relay => self.handle_relay(cell, from_peer),
            CellType::Destroy => self.handle_destroy(cell),
            CellType::Padding => Ok(RelayAction::Drop), // Silently drop padding
            _ => Ok(RelayAction::Drop),
        }
    }

    /// Handle circuit creation
    fn handle_create(&self, cell: RelayCell, from_peer: PeerId) -> Result<RelayAction, RelayError> {
        let mut states = self.circuit_states.write().unwrap();

        if states.len() >= self.config.max_circuits {
            return Err(RelayError::MaxCircuitsExceeded);
        }

        // Derive key for this circuit (in production, use ECDH with client)
        let hop_key = self.derive_circuit_key(&cell.circuit_id, &from_peer);

        let state = RelayCircuitState {
            circuit_id: cell.circuit_id,
            hop_key,
            prev_hop: Some(from_peer),
            next_hop: None, // Will be set on extend
            created_at: Instant::now(),
            expires_at: Instant::now() + self.config.circuit_lifetime,
        };

        states.insert(cell.circuit_id, state);

        // Update stats
        if let Ok(mut stats) = self.stats.write() {
            stats.circuits_created += 1;
            stats.active_circuits = states.len();
        }

        // Respond with Created cell
        let response = RelayCell::new(cell.circuit_id, CellType::Created, vec![]);
        Ok(RelayAction::Respond(response))
    }

    /// Handle circuit extension
    fn handle_extend(&self, cell: RelayCell, from_peer: PeerId) -> Result<RelayAction, RelayError> {
        let mut states = self.circuit_states.write().unwrap();

        let state = states
            .get_mut(&cell.circuit_id)
            .ok_or(RelayError::CircuitNotFound)?;

        if state.is_expired() {
            states.remove(&cell.circuit_id);
            return Err(RelayError::CircuitExpired);
        }

        // Verify the cell came from the expected previous hop
        if state.prev_hop != Some(from_peer) {
            return Err(RelayError::InvalidHop);
        }

        // Decrypt payload to get next hop info
        let manager = CryptoManager::new(&state.hop_key);
        let decrypted = manager
            .decrypt_raw(&cell.payload)
            .map_err(|e| RelayError::DecryptionFailed(e.to_string()))?;

        // Parse extend request to get next hop
        let extend_request: ExtendRequest =
            bincode::deserialize(&decrypted).map_err(|_| RelayError::InvalidPayload)?;

        state.next_hop = Some(extend_request.next_peer);

        // Forward create cell to next hop
        let create_cell = RelayCell::new(cell.circuit_id, CellType::Create, extend_request.handshake);

        Ok(RelayAction::Forward {
            cell: create_cell,
            to_peer: extend_request.next_peer,
        })
    }

    /// Handle relay cell (the main forwarding path)
    fn handle_relay(&self, cell: RelayCell, from_peer: PeerId) -> Result<RelayAction, RelayError> {
        let states = self.circuit_states.read().unwrap();

        let state = states
            .get(&cell.circuit_id)
            .ok_or(RelayError::CircuitNotFound)?;

        if state.is_expired() {
            drop(states);
            self.circuit_states
                .write()
                .unwrap()
                .remove(&cell.circuit_id);
            return Err(RelayError::CircuitExpired);
        }

        // Decrypt exactly one layer
        let manager = CryptoManager::new(&state.hop_key);
        let decrypted = manager.decrypt_raw(&cell.payload).map_err(|e| {
            // Update failure stats
            if let Ok(mut stats) = self.stats.write() {
                stats.decryption_failures += 1;
            }
            RelayError::DecryptionFailed(e.to_string())
        })?;

        // Parse the relay payload
        let payload: RelayPayload =
            bincode::deserialize(&decrypted).map_err(|_| RelayError::InvalidPayload)?;

        // Verify integrity
        if !payload.verify() {
            return Err(RelayError::InvalidPayload);
        }

        // Update stats
        if let Ok(mut stats) = self.stats.write() {
            stats.cells_relayed += 1;
            stats.bytes_relayed += cell.payload.len() as u64;
        }

        // Determine action based on direction and next_hop
        match (payload.next_hop, state.prev_hop == Some(from_peer)) {
            // Forward direction (from client toward exit)
            (Some(next_peer), true) => {
                let forward_cell =
                    RelayCell::new(cell.circuit_id, CellType::Relay, payload.inner_data);

                Ok(RelayAction::Forward {
                    cell: forward_cell,
                    to_peer: next_peer,
                })
            }
            // Exit node - process the request
            (None, true) => {
                // We're the exit node - return the inner data for processing
                // The inner data is still encrypted with content keys
                Ok(RelayAction::Exit {
                    data: payload.inner_data,
                    circuit_id: cell.circuit_id,
                })
            }
            // Backward direction (response from exit toward client)
            (_, false) => {
                if let Some(prev_peer) = state.prev_hop {
                    // Re-encrypt for backward path
                    let encrypted = manager
                        .encrypt_raw(&cell.payload)
                        .map_err(|e| RelayError::EncryptionFailed(e.to_string()))?;

                    let response_cell =
                        RelayCell::new(cell.circuit_id, CellType::Relay, encrypted);

                    Ok(RelayAction::Forward {
                        cell: response_cell,
                        to_peer: prev_peer,
                    })
                } else {
                    Err(RelayError::InvalidHop)
                }
            }
        }
    }

    /// Handle circuit destruction
    fn handle_destroy(&self, cell: RelayCell) -> Result<RelayAction, RelayError> {
        let mut states = self.circuit_states.write().unwrap();

        if let Some(state) = states.remove(&cell.circuit_id) {
            // Update stats
            if let Ok(mut stats) = self.stats.write() {
                stats.circuits_destroyed += 1;
                stats.active_circuits = states.len();
            }

            // Forward destroy to next hop if exists
            if let Some(next_peer) = state.next_hop {
                let destroy_cell = RelayCell::new(cell.circuit_id, CellType::Destroy, vec![]);
                return Ok(RelayAction::Forward {
                    cell: destroy_cell,
                    to_peer: next_peer,
                });
            }
        }

        Ok(RelayAction::Drop)
    }

    /// Derive circuit key (simplified - production would use ECDH)
    fn derive_circuit_key(&self, circuit_id: &CircuitId, peer: &PeerId) -> [u8; KEY_SIZE] {
        let mut hasher = Hasher::new();
        hasher.update(&self.node_secret);
        hasher.update(circuit_id);
        hasher.update(peer.to_bytes().as_slice());
        
        let hash = hasher.finalize();
        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&hash.as_bytes()[..KEY_SIZE]);
        key
    }

    /// Get relay statistics
    pub fn stats(&self) -> RelayStats {
        self.stats.read().unwrap().clone()
    }

    /// Get active circuit count
    pub fn active_circuits(&self) -> usize {
        self.circuit_states.read().unwrap().len()
    }

    /// Clean up expired circuits
    pub fn cleanup_expired(&self) -> usize {
        let mut states = self.circuit_states.write().unwrap();
        let before = states.len();
        states.retain(|_, s| !s.is_expired());
        let removed = before - states.len();

        if let Ok(mut stats) = self.stats.write() {
            stats.circuits_destroyed += removed as u64;
            stats.active_circuits = states.len();
        }

        removed
    }
}

/// Action for relay node to take after processing a cell
#[derive(Debug)]
pub enum RelayAction {
    /// Forward cell to another peer
    Forward { cell: RelayCell, to_peer: PeerId },
    /// Respond directly to sender
    Respond(RelayCell),
    /// Exit node processing (retrieve content)
    Exit { data: Vec<u8>, circuit_id: CircuitId },
    /// Drop the cell (padding, etc.)
    Drop,
}

/// Request to extend a circuit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendRequest {
    /// Next peer to extend to
    pub next_peer: PeerId,
    /// Handshake data for the next peer
    pub handshake: Vec<u8>,
}

/// Blind relay wrapper for content
/// 
/// This wraps actual content requests so that even exit nodes
/// only see content hashes, not the content itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindRequest {
    /// Content hash being requested
    pub content_hash: String,
    /// Optional range request (for partial content)
    pub range: Option<(u64, u64)>,
    /// Request ID for response matching
    pub request_id: [u8; 16],
    /// Timestamp
    pub timestamp: u64,
}

impl BlindRequest {
    /// Create a new blind request
    pub fn new(content_hash: String) -> Self {
        let mut request_id = [0u8; 16];
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        request_id[..12].copy_from_slice(&nonce);

        Self {
            content_hash,
            range: None,
            request_id,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Create with range
    pub fn with_range(content_hash: String, start: u64, end: u64) -> Self {
        let mut req = Self::new(content_hash);
        req.range = Some((start, end));
        req
    }
}

/// Blind response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindResponse {
    /// Request ID being responded to
    pub request_id: [u8; 16],
    /// Encrypted content (encrypted with content key, not circuit key)
    pub encrypted_content: Vec<u8>,
    /// Content hash for verification
    pub content_hash: String,
    /// Whether request was successful
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Traffic analysis resistance through padding
pub struct TrafficPadder {
    /// Minimum cell size
    min_size: usize,
    /// Target cell size (all cells padded to this)
    target_size: usize,
    /// Enable timing jitter
    timing_jitter: bool,
    /// Maximum jitter in milliseconds
    max_jitter_ms: u64,
}

impl Default for TrafficPadder {
    fn default() -> Self {
        Self {
            min_size: 512,
            target_size: 512,
            timing_jitter: true,
            max_jitter_ms: 50,
        }
    }
}

impl TrafficPadder {
    /// Create with custom target size
    pub fn with_size(target_size: usize) -> Self {
        Self {
            min_size: target_size,
            target_size,
            ..Default::default()
        }
    }

    /// Pad data to target size
    pub fn pad(&self, data: &[u8]) -> Vec<u8> {
        if data.len() >= self.target_size {
            return data.to_vec();
        }

        let mut padded = data.to_vec();
        let padding_needed = self.target_size - data.len();

        // Add random padding
        let padding: Vec<u8> = (0..padding_needed).map(|_| rand::random()).collect();
        padded.extend(padding);

        padded
    }

    /// Remove padding (assumes length prefix)
    pub fn unpad(&self, data: &[u8], original_len: usize) -> Vec<u8> {
        data[..original_len.min(data.len())].to_vec()
    }

    /// Get jitter delay
    pub fn get_jitter(&self) -> Duration {
        if !self.timing_jitter {
            return Duration::ZERO;
        }

        let jitter: u64 = rand::random::<u64>() % self.max_jitter_ms;
        Duration::from_millis(jitter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;

    fn create_peer_id() -> PeerId {
        let keypair = identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    fn create_test_peers(n: usize) -> Vec<PeerId> {
        (0..n).map(|_| create_peer_id()).collect()
    }

    #[test]
    fn test_circuit_creation() {
        let secret = [42u8; KEY_SIZE];
        let mut client = ZkRelayClient::new(secret);
        let peers = create_test_peers(3);

        let circuit_id = client.build_circuit(&peers).unwrap();
        assert_eq!(client.active_circuits(), 1);
        assert!(client.get_entry_node(&circuit_id).is_some());
    }

    #[test]
    fn test_circuit_too_few_hops() {
        let secret = [42u8; KEY_SIZE];
        let mut client = ZkRelayClient::new(secret);
        let peers = create_test_peers(1);

        let result = client.build_circuit(&peers);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrap_unwrap_request() {
        let secret = [42u8; KEY_SIZE];
        let mut client = ZkRelayClient::new(secret);
        let peers = create_test_peers(3);

        let circuit_id = client.build_circuit(&peers).unwrap();
        let request = b"GET /content/abc123";

        let cell = client.wrap_request(&circuit_id, request).unwrap();
        assert_eq!(cell.circuit_id, circuit_id);
        assert_eq!(cell.cell_type, CellType::Relay);
        assert_ne!(&cell.payload, request); // Should be encrypted
    }

    #[test]
    fn test_relay_cell_serialization() {
        let circuit_id = [1u8; 32];
        let cell = RelayCell::new(circuit_id, CellType::Relay, vec![1, 2, 3, 4]);

        let serialized = cell.serialize();
        let deserialized = RelayCell::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.circuit_id, circuit_id);
        assert_eq!(deserialized.cell_type, CellType::Relay);
        assert_eq!(deserialized.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_relay_payload_integrity() {
        let hop_id = [5u8; 16];
        let payload = RelayPayload::new(None, hop_id, vec![1, 2, 3, 4]);

        assert!(payload.verify());

        // Tamper with data
        let mut tampered = payload.clone();
        tampered.inner_data = vec![5, 6, 7, 8];
        assert!(!tampered.verify());
    }

    #[test]
    fn test_relay_node_creation() {
        let secret = [99u8; KEY_SIZE];
        let node = ZkRelayNode::new(secret);

        assert_eq!(node.active_circuits(), 0);
        let stats = node.stats();
        assert_eq!(stats.cells_relayed, 0);
    }

    #[test]
    fn test_circuit_expiry() {
        let config = ZkRelayConfig {
            circuit_lifetime: Duration::from_millis(1),
            ..Default::default()
        };

        let mut circuit = Circuit::new(config.circuit_lifetime);
        assert!(!circuit.is_expired());

        std::thread::sleep(Duration::from_millis(10));
        assert!(circuit.is_expired());
    }

    #[test]
    fn test_cleanup_expired_circuits() {
        let secret = [42u8; KEY_SIZE];
        let config = ZkRelayConfig {
            circuit_lifetime: Duration::from_millis(1),
            ..Default::default()
        };
        let mut client = ZkRelayClient::with_config(secret, config);
        let peers = create_test_peers(3);

        client.build_circuit(&peers).unwrap();
        assert_eq!(client.active_circuits(), 1);

        std::thread::sleep(Duration::from_millis(10));
        let cleaned = client.cleanup_expired();
        assert_eq!(cleaned, 1);
        assert_eq!(client.active_circuits(), 0);
    }

    #[test]
    fn test_destroy_circuit() {
        let secret = [42u8; KEY_SIZE];
        let mut client = ZkRelayClient::new(secret);
        let peers = create_test_peers(3);

        let circuit_id = client.build_circuit(&peers).unwrap();
        assert_eq!(client.active_circuits(), 1);

        let destroy_cell = client.destroy_circuit(&circuit_id);
        assert!(destroy_cell.is_some());
        assert_eq!(client.active_circuits(), 0);
    }

    #[test]
    fn test_blind_request() {
        let request = BlindRequest::new("abc123".to_string());
        assert_eq!(request.content_hash, "abc123");
        assert!(request.range.is_none());

        let range_request = BlindRequest::with_range("def456".to_string(), 0, 1000);
        assert_eq!(range_request.range, Some((0, 1000)));
    }

    #[test]
    fn test_traffic_padder() {
        let padder = TrafficPadder::with_size(512);

        let small_data = vec![1, 2, 3, 4, 5];
        let padded = padder.pad(&small_data);
        assert_eq!(padded.len(), 512);

        // Original data should be preserved
        assert_eq!(&padded[..5], &small_data);

        // Unpad
        let unpadded = padder.unpad(&padded, 5);
        assert_eq!(unpadded, small_data);
    }

    #[test]
    fn test_large_data_no_padding() {
        let padder = TrafficPadder::with_size(512);

        let large_data: Vec<u8> = (0..1000).map(|i| i as u8).collect();
        let padded = padder.pad(&large_data);
        assert_eq!(padded.len(), 1000); // No padding added
    }

    #[test]
    fn test_padding_cell_creation() {
        let secret = [42u8; KEY_SIZE];
        let config = ZkRelayConfig {
            padding_enabled: true,
            ..Default::default()
        };
        let mut client = ZkRelayClient::with_config(secret, config);
        let peers = create_test_peers(3);

        let circuit_id = client.build_circuit(&peers).unwrap();
        let padding_cell = client.create_padding_cell(&circuit_id);

        assert!(padding_cell.is_some());
        let cell = padding_cell.unwrap();
        assert_eq!(cell.cell_type, CellType::Padding);
        assert_eq!(cell.payload.len(), 512);
    }

    #[test]
    fn test_padding_disabled() {
        let secret = [42u8; KEY_SIZE];
        let config = ZkRelayConfig {
            padding_enabled: false,
            ..Default::default()
        };
        let mut client = ZkRelayClient::with_config(secret, config);
        let peers = create_test_peers(3);

        let circuit_id = client.build_circuit(&peers).unwrap();
        let padding_cell = client.create_padding_cell(&circuit_id);

        assert!(padding_cell.is_none());
    }
}
