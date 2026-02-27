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
use crate::lock_utils::{read_lock, write_lock};
use blake3::Hasher;
use chacha20poly1305::aead::{AeadCore, OsRng};
use chacha20poly1305::ChaCha20Poly1305;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use x25519_dalek::{PublicKey, SharedSecret, StaticSecret};

/// X25519 public key size
pub const X25519_PUBLIC_KEY_SIZE: usize = 32;

/// X25519 public key type for serialization
pub type X25519PublicKey = [u8; X25519_PUBLIC_KEY_SIZE];

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

// ============================================================================
// ECDH Key Exchange (X25519)
// ============================================================================

/// Keypair for X25519 ECDH key exchange
/// Used by both clients and relay nodes for establishing shared secrets
pub struct X25519Keypair {
    /// Static secret key (long-term identity)
    secret: StaticSecret,
    /// Corresponding public key
    pub public: PublicKey,
}

impl X25519Keypair {
    /// Generate a new random keypair
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Create from existing secret bytes
    pub fn from_secret(secret_bytes: &[u8; 32]) -> Self {
        let secret = StaticSecret::from(*secret_bytes);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Get public key bytes for transmission
    pub fn public_key_bytes(&self) -> X25519PublicKey {
        *self.public.as_bytes()
    }

    /// Perform ECDH key exchange with a peer's public key
    /// Returns a shared secret that both parties can derive
    pub fn diffie_hellman(&self, peer_public: &X25519PublicKey) -> SharedSecret {
        let peer_public = PublicKey::from(*peer_public);
        self.secret.diffie_hellman(&peer_public)
    }

    /// Derive a symmetric key from shared secret using HKDF-like construction
    ///
    /// # Arguments
    /// * `peer_public` - The peer's X25519 public key
    /// * `context` - Additional context for key derivation (circuit_id, hop_index, etc.)
    ///
    /// # Returns
    /// A 32-byte symmetric key suitable for ChaCha20Poly1305
    pub fn derive_symmetric_key(
        &self,
        peer_public: &X25519PublicKey,
        context: &[u8],
    ) -> [u8; KEY_SIZE] {
        let shared_secret = self.diffie_hellman(peer_public);

        // Use BLAKE3 for key derivation (similar to HKDF)
        let mut hasher = Hasher::new_keyed(shared_secret.as_bytes());
        hasher.update(b"shadowmesh-circuit-key-v1");
        hasher.update(context);

        // Include both public keys in sorted order for key confirmation
        // Sorting ensures both parties derive the same key regardless of who calls this
        let self_public = self.public.as_bytes();
        if self_public < peer_public {
            hasher.update(self_public);
            hasher.update(peer_public);
        } else {
            hasher.update(peer_public);
            hasher.update(self_public);
        }

        let derived = hasher.finalize();
        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&derived.as_bytes()[..KEY_SIZE]);

        key
    }
}

impl Clone for X25519Keypair {
    fn clone(&self) -> Self {
        // Clone by extracting and recreating from secret bytes
        let secret_bytes: [u8; 32] = self.secret.to_bytes();
        Self::from_secret(&secret_bytes)
    }
}

/// Handshake message for circuit creation (CREATE cell payload)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateHandshake {
    /// Client's ephemeral public key for this hop
    pub client_public_key: X25519PublicKey,
    /// Circuit ID (for binding)
    pub circuit_id: CircuitId,
    /// Timestamp for replay protection
    pub timestamp: u64,
    /// Random padding to fixed size
    pub padding: [u8; 32],
}

impl CreateHandshake {
    /// Create a new handshake
    pub fn new(client_public_key: X25519PublicKey, circuit_id: CircuitId) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let padding: [u8; 32] = rand::random();

        Self {
            client_public_key,
            circuit_id,
            timestamp,
            padding,
        }
    }

    /// Serialize for transmission
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, RelayError> {
        bincode::deserialize(data).map_err(|_| RelayError::InvalidPayload)
    }
}

/// Response to CREATE handshake (CREATED cell payload)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedHandshake {
    /// Relay's ephemeral public key for this circuit
    pub relay_public_key: X25519PublicKey,
    /// Key confirmation hash (proves relay derived the same key)
    pub key_confirmation: [u8; 16],
    /// Timestamp
    pub timestamp: u64,
}

impl CreatedHandshake {
    /// Create a new response with key confirmation
    pub fn new(relay_public_key: X25519PublicKey, shared_key: &[u8; KEY_SIZE]) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Key confirmation: hash of the derived key
        let mut hasher = Hasher::new();
        hasher.update(b"shadowmesh-key-confirm-v1");
        hasher.update(shared_key);
        let hash = hasher.finalize();
        let mut key_confirmation = [0u8; 16];
        key_confirmation.copy_from_slice(&hash.as_bytes()[..16]);

        Self {
            relay_public_key,
            key_confirmation,
            timestamp,
        }
    }

    /// Verify key confirmation
    pub fn verify_key(&self, shared_key: &[u8; KEY_SIZE]) -> bool {
        let mut hasher = Hasher::new();
        hasher.update(b"shadowmesh-key-confirm-v1");
        hasher.update(shared_key);
        let hash = hasher.finalize();

        let mut expected = [0u8; 16];
        expected.copy_from_slice(&hash.as_bytes()[..16]);

        self.key_confirmation == expected
    }

    /// Serialize for transmission
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, RelayError> {
        bincode::deserialize(data).map_err(|_| RelayError::InvalidPayload)
    }
}

/// Ephemeral keypair for a single circuit hop
/// These are generated fresh for each circuit and discarded after
#[derive(Clone)]
pub struct EphemeralHopKeypair {
    /// The keypair
    keypair: X25519Keypair,
    /// When this was created
    _created_at: Instant,
}

impl EphemeralHopKeypair {
    /// Generate a new ephemeral keypair
    pub fn generate() -> Self {
        Self {
            keypair: X25519Keypair::generate(),
            _created_at: Instant::now(),
        }
    }

    /// Get the public key bytes
    pub fn public_key_bytes(&self) -> X25519PublicKey {
        self.keypair.public_key_bytes()
    }

    /// Derive symmetric key with peer
    pub fn derive_symmetric_key(
        &self,
        peer_public: &X25519PublicKey,
        context: &[u8],
    ) -> [u8; KEY_SIZE] {
        self.keypair.derive_symmetric_key(peer_public, context)
    }
}

// ============================================================================
// Relay Cell Types
// ============================================================================

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
#[derive(Clone)]
pub struct CircuitHop {
    /// Peer ID of this hop
    pub peer_id: PeerId,
    /// Client's ephemeral keypair for this hop
    pub ephemeral_keypair: EphemeralHopKeypair,
    /// Relay's public key (received in CREATED response)
    pub relay_public_key: Option<X25519PublicKey>,
    /// Derived symmetric key for this hop (after handshake)
    pub symmetric_key: Option<[u8; KEY_SIZE]>,
    /// Hop identifier
    pub hop_id: HopId,
}

impl std::fmt::Debug for CircuitHop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitHop")
            .field("peer_id", &self.peer_id)
            .field("hop_id", &self.hop_id)
            .field("has_symmetric_key", &self.symmetric_key.is_some())
            .finish()
    }
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

    /// Add a hop to the circuit (generates ephemeral keypair)
    pub fn add_hop(&mut self, peer_id: PeerId) {
        let hop_id = Self::generate_hop_id(&self.id, self.hops.len());
        let ephemeral_keypair = EphemeralHopKeypair::generate();

        self.hops.push(CircuitHop {
            peer_id,
            ephemeral_keypair,
            relay_public_key: None,
            symmetric_key: None,
            hop_id,
        });
    }

    /// Add a hop with a pre-generated keypair (for testing)
    pub fn add_hop_with_keypair(&mut self, peer_id: PeerId, keypair: EphemeralHopKeypair) {
        let hop_id = Self::generate_hop_id(&self.id, self.hops.len());

        self.hops.push(CircuitHop {
            peer_id,
            ephemeral_keypair: keypair,
            relay_public_key: None,
            symmetric_key: None,
            hop_id,
        });
    }

    /// Complete handshake for a hop (called after receiving CREATED)
    pub fn complete_hop_handshake(
        &mut self,
        hop_index: usize,
        relay_public_key: X25519PublicKey,
        context: &[u8],
    ) -> Result<[u8; KEY_SIZE], RelayError> {
        let hop = self.hops.get_mut(hop_index).ok_or(RelayError::InvalidHop)?;

        // Derive symmetric key using ECDH
        let symmetric_key = hop
            .ephemeral_keypair
            .derive_symmetric_key(&relay_public_key, context);

        hop.relay_public_key = Some(relay_public_key);
        hop.symmetric_key = Some(symmetric_key);

        Ok(symmetric_key)
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
    /// Returns None if any hop hasn't completed handshake
    pub fn keys(&self) -> Option<Vec<[u8; KEY_SIZE]>> {
        self.hops.iter().map(|h| h.symmetric_key).collect()
    }

    /// Check if all hops have completed handshake
    pub fn is_handshake_complete(&self) -> bool {
        self.hops.iter().all(|h| h.symmetric_key.is_some())
    }

    /// Get CREATE handshake for a specific hop
    pub fn get_create_handshake(&self, hop_index: usize) -> Option<CreateHandshake> {
        self.hops
            .get(hop_index)
            .map(|hop| CreateHandshake::new(hop.ephemeral_keypair.public_key_bytes(), self.id))
    }
}

/// Relay node's view of a circuit (minimal information)
#[derive(Clone)]
pub struct RelayCircuitState {
    /// Circuit ID
    pub circuit_id: CircuitId,
    /// Relay's ephemeral keypair for this circuit
    pub ephemeral_keypair: EphemeralHopKeypair,
    /// Client's public key (from CREATE handshake)
    pub client_public_key: X25519PublicKey,
    /// Derived symmetric key for this hop
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

impl std::fmt::Debug for RelayCircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayCircuitState")
            .field("circuit_id", &hex::encode(&self.circuit_id[..8]))
            .field("prev_hop", &self.prev_hop)
            .field("next_hop", &self.next_hop)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl RelayCircuitState {
    /// Create a new relay circuit state from ECDH handshake
    pub fn from_handshake(
        circuit_id: CircuitId,
        client_public_key: X25519PublicKey,
        from_peer: PeerId,
        lifetime: Duration,
    ) -> (Self, CreatedHandshake) {
        // Generate ephemeral keypair for this circuit
        let ephemeral_keypair = EphemeralHopKeypair::generate();

        // Derive symmetric key using ECDH
        // Context includes circuit_id + hop_index (0 for first hop)
        let mut context = Vec::new();
        context.extend_from_slice(&circuit_id);
        context.extend_from_slice(&0usize.to_le_bytes());
        let hop_key = ephemeral_keypair.derive_symmetric_key(&client_public_key, &context);

        let now = Instant::now();

        let state = Self {
            circuit_id,
            ephemeral_keypair: ephemeral_keypair.clone(),
            client_public_key,
            hop_key,
            prev_hop: Some(from_peer),
            next_hop: None,
            created_at: now,
            expires_at: now + lifetime,
        };

        // Create response with key confirmation
        let response = CreatedHandshake::new(ephemeral_keypair.public_key_bytes(), &hop_key);

        (state, response)
    }

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
    _secret: [u8; KEY_SIZE],
    /// Active circuits
    circuits: HashMap<CircuitId, Circuit>,
    /// Configuration
    config: ZkRelayConfig,
}

impl ZkRelayClient {
    /// Create a new ZK relay client
    pub fn new(secret: [u8; KEY_SIZE]) -> Self {
        Self {
            _secret: secret,
            circuits: HashMap::new(),
            config: ZkRelayConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(secret: [u8; KEY_SIZE], config: ZkRelayConfig) -> Self {
        Self {
            _secret: secret,
            circuits: HashMap::new(),
            config,
        }
    }

    /// Build a new circuit through the specified peers
    ///
    /// This creates a circuit with ephemeral ECDH keypairs for each hop.
    /// The circuit is NOT ready until handshakes are completed via `process_created_response`.
    pub fn build_circuit(&mut self, peers: &[PeerId]) -> Result<CircuitId, RelayError> {
        if peers.len() < 2 {
            return Err(RelayError::CircuitBuildFailed(
                "Need at least 2 hops".to_string(),
            ));
        }

        let mut circuit = Circuit::new(self.config.circuit_lifetime);
        let circuit_id = circuit.id;

        // Generate ephemeral keypairs for each hop
        // Keys will be derived after ECDH handshake with each relay
        for peer in peers.iter() {
            circuit.add_hop(*peer);
        }

        // Circuit is NOT ready until handshakes complete
        circuit.ready = false;
        self.circuits.insert(circuit_id, circuit);

        Ok(circuit_id)
    }

    /// Get the CREATE cell for initiating circuit building
    ///
    /// This returns the handshake data to send to the first hop.
    pub fn get_create_cell(&self, circuit_id: &CircuitId) -> Result<RelayCell, RelayError> {
        let circuit = self
            .circuits
            .get(circuit_id)
            .ok_or(RelayError::CircuitNotFound)?;

        let handshake = circuit
            .get_create_handshake(0)
            .ok_or(RelayError::InvalidHop)?;

        Ok(RelayCell::new(
            *circuit_id,
            CellType::Create,
            handshake.serialize(),
        ))
    }

    /// Process a CREATED response and derive the symmetric key for a hop
    ///
    /// Returns the next EXTEND cell if there are more hops, or None if complete.
    pub fn process_created_response(
        &mut self,
        circuit_id: &CircuitId,
        response: &CreatedHandshake,
        hop_index: usize,
    ) -> Result<Option<RelayCell>, RelayError> {
        let circuit = self
            .circuits
            .get_mut(circuit_id)
            .ok_or(RelayError::CircuitNotFound)?;

        // Derive context for key derivation
        let mut context = Vec::new();
        context.extend_from_slice(circuit_id);
        context.extend_from_slice(&hop_index.to_le_bytes());

        // Complete ECDH handshake for this hop
        let symmetric_key =
            circuit.complete_hop_handshake(hop_index, response.relay_public_key, &context)?;

        // Verify key confirmation
        if !response.verify_key(&symmetric_key) {
            return Err(RelayError::DecryptionFailed(
                "Key confirmation failed".to_string(),
            ));
        }

        // Check if there are more hops to extend to
        let next_hop_index = hop_index + 1;
        if next_hop_index < circuit.hops.len() {
            // Create EXTEND cell for next hop
            let next_handshake = circuit
                .get_create_handshake(next_hop_index)
                .ok_or(RelayError::InvalidHop)?;

            let extend_request = ExtendRequest {
                next_peer: circuit.hops[next_hop_index].peer_id,
                handshake: next_handshake.serialize(),
            };

            let extend_payload =
                bincode::serialize(&extend_request).map_err(|_| RelayError::InvalidPayload)?;

            // Encrypt EXTEND for all completed hops (onion layers)
            let mut payload = extend_payload;
            for i in (0..=hop_index).rev() {
                if let Some(key) = circuit.hops[i].symmetric_key {
                    let manager = CryptoManager::new(&key);
                    payload = manager
                        .encrypt_raw(&payload)
                        .map_err(|e| RelayError::EncryptionFailed(e.to_string()))?;
                }
            }

            Ok(Some(RelayCell::new(*circuit_id, CellType::Extend, payload)))
        } else {
            // All hops complete - circuit is ready
            circuit.ready = true;
            Ok(None)
        }
    }

    /// Build circuit synchronously for testing (uses mock keys)
    ///
    /// This method simulates the full ECDH handshake by generating mock relay keys.
    /// In production, use `build_circuit` + `process_created_response` for async handshakes.
    ///
    /// # Warning
    /// This method is intended for testing only. The keys generated are not from real
    /// relay nodes and won't work with actual relay infrastructure.
    pub fn build_circuit_sync(&mut self, peers: &[PeerId]) -> Result<CircuitId, RelayError> {
        if peers.len() < 2 {
            return Err(RelayError::CircuitBuildFailed(
                "Need at least 2 hops".to_string(),
            ));
        }

        let mut circuit = Circuit::new(self.config.circuit_lifetime);
        let circuit_id = circuit.id;

        for (i, peer) in peers.iter().enumerate() {
            circuit.add_hop(*peer);

            // Simulate completed handshake with deterministic key
            let mut context = Vec::new();
            context.extend_from_slice(&circuit_id);
            context.extend_from_slice(&i.to_le_bytes());

            let hop = match circuit.hops.last_mut() {
                Some(h) => h,
                None => return Err(RelayError::CircuitBuildFailed("hop missing after add".into())),
            };
            let mock_relay_public: X25519PublicKey = rand::random();
            hop.relay_public_key = Some(mock_relay_public);
            hop.symmetric_key = Some(
                hop.ephemeral_keypair
                    .derive_symmetric_key(&mock_relay_public, &context),
            );
        }

        circuit.ready = true;
        self.circuits.insert(circuit_id, circuit);

        Ok(circuit_id)
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

            // Get the symmetric key (must be set after handshake)
            let key = hop.symmetric_key.ok_or_else(|| {
                RelayError::CircuitBuildFailed("Hop key not established".to_string())
            })?;

            // Encrypt for this hop
            let manager = CryptoManager::new(&key);
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
            let key = hop.symmetric_key.ok_or_else(|| {
                RelayError::DecryptionFailed("Hop key not established".to_string())
            })?;

            let manager = CryptoManager::new(&key);
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
    _node_secret: [u8; KEY_SIZE],
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
            _node_secret: node_secret,
            circuit_states: Arc::new(RwLock::new(HashMap::new())),
            config: ZkRelayConfig::default(),
            stats: Arc::new(RwLock::new(RelayStats::default())),
        }
    }

    /// Create with custom config
    pub fn with_config(node_secret: [u8; KEY_SIZE], config: ZkRelayConfig) -> Self {
        Self {
            _node_secret: node_secret,
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

    /// Handle circuit creation with ECDH key exchange
    fn handle_create(&self, cell: RelayCell, from_peer: PeerId) -> Result<RelayAction, RelayError> {
        let mut states = write_lock(&self.circuit_states);

        if states.len() >= self.config.max_circuits {
            return Err(RelayError::MaxCircuitsExceeded);
        }

        // Parse the CREATE handshake from client
        let handshake = CreateHandshake::deserialize(&cell.payload)?;

        // Perform ECDH key exchange
        let (state, created_response) = RelayCircuitState::from_handshake(
            cell.circuit_id,
            handshake.client_public_key,
            from_peer,
            self.config.circuit_lifetime,
        );

        states.insert(cell.circuit_id, state);

        // Update stats
        if let Ok(mut stats) = self.stats.write() {
            stats.circuits_created += 1;
            stats.active_circuits = states.len();
        }

        // Respond with Created cell containing our public key
        let response = RelayCell::new(
            cell.circuit_id,
            CellType::Created,
            created_response.serialize(),
        );
        Ok(RelayAction::Respond(response))
    }

    /// Handle circuit extension
    fn handle_extend(&self, cell: RelayCell, from_peer: PeerId) -> Result<RelayAction, RelayError> {
        let mut states = write_lock(&self.circuit_states);

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
        let create_cell =
            RelayCell::new(cell.circuit_id, CellType::Create, extend_request.handshake);

        Ok(RelayAction::Forward {
            cell: create_cell,
            to_peer: extend_request.next_peer,
        })
    }

    /// Handle relay cell (the main forwarding path)
    fn handle_relay(&self, cell: RelayCell, from_peer: PeerId) -> Result<RelayAction, RelayError> {
        let states = read_lock(&self.circuit_states);

        let state = states
            .get(&cell.circuit_id)
            .ok_or(RelayError::CircuitNotFound)?;

        if state.is_expired() {
            drop(states);
            write_lock(&self.circuit_states).remove(&cell.circuit_id);
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

                    let response_cell = RelayCell::new(cell.circuit_id, CellType::Relay, encrypted);

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
        let mut states = write_lock(&self.circuit_states);

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

    /// Get relay statistics
    pub fn stats(&self) -> RelayStats {
        read_lock(&self.stats).clone()
    }

    /// Get active circuit count
    pub fn active_circuits(&self) -> usize {
        read_lock(&self.circuit_states).len()
    }

    /// Clean up expired circuits
    pub fn cleanup_expired(&self) -> usize {
        let mut states = write_lock(&self.circuit_states);
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
    Exit {
        data: Vec<u8>,
        circuit_id: CircuitId,
    },
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
    _min_size: usize,
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
            _min_size: 512,
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
            _min_size: target_size,
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

        // Use sync version for testing (creates mock keys)
        let circuit_id = client.build_circuit_sync(&peers).unwrap();
        assert_eq!(client.active_circuits(), 1);
        assert!(client.get_entry_node(&circuit_id).is_some());
    }

    #[test]
    fn test_circuit_too_few_hops() {
        let secret = [42u8; KEY_SIZE];
        let mut client = ZkRelayClient::new(secret);
        let peers = create_test_peers(1);

        let result = client.build_circuit_sync(&peers);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrap_unwrap_request() {
        let secret = [42u8; KEY_SIZE];
        let mut client = ZkRelayClient::new(secret);
        let peers = create_test_peers(3);

        let circuit_id = client.build_circuit_sync(&peers).unwrap();
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

        let circuit = Circuit::new(config.circuit_lifetime);
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

        client.build_circuit_sync(&peers).unwrap();
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

        let circuit_id = client.build_circuit_sync(&peers).unwrap();
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

        let circuit_id = client.build_circuit_sync(&peers).unwrap();
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

        let circuit_id = client.build_circuit_sync(&peers).unwrap();
        let padding_cell = client.create_padding_cell(&circuit_id);

        assert!(padding_cell.is_none());
    }

    #[test]
    fn test_ecdh_key_exchange() {
        // Test that two parties can derive the same symmetric key
        let alice = X25519Keypair::generate();
        let bob = X25519Keypair::generate();

        let context = b"test-circuit-context";

        let alice_key = alice.derive_symmetric_key(&bob.public_key_bytes(), context);
        let bob_key = bob.derive_symmetric_key(&alice.public_key_bytes(), context);

        // Keys should be the same (ECDH property)
        assert_eq!(alice_key, bob_key);
    }

    #[test]
    fn test_create_handshake_serialization() {
        let keypair = X25519Keypair::generate();
        let circuit_id: CircuitId = rand::random();

        let handshake = CreateHandshake::new(keypair.public_key_bytes(), circuit_id);
        let serialized = handshake.serialize();
        let deserialized = CreateHandshake::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.circuit_id, circuit_id);
        assert_eq!(deserialized.client_public_key, keypair.public_key_bytes());
    }

    #[test]
    fn test_created_handshake_key_verification() {
        let shared_key: [u8; KEY_SIZE] = rand::random();
        let relay_keypair = X25519Keypair::generate();

        let response = CreatedHandshake::new(relay_keypair.public_key_bytes(), &shared_key);

        // Correct key should verify
        assert!(response.verify_key(&shared_key));

        // Wrong key should not verify
        let wrong_key: [u8; KEY_SIZE] = rand::random();
        assert!(!response.verify_key(&wrong_key));
    }

    #[test]
    fn test_full_ecdh_circuit_flow() {
        // Simulate full circuit creation with ECDH
        let secret = [42u8; KEY_SIZE];
        let mut client = ZkRelayClient::new(secret);
        let peers = create_test_peers(3);

        // Step 1: Client initiates circuit
        let circuit_id = client.build_circuit(&peers).unwrap();

        // Circuit should exist but not be ready (no handshakes complete)
        assert_eq!(client.active_circuits(), 1);

        // Step 2: Get CREATE cell for first hop
        let create_cell = client.get_create_cell(&circuit_id).unwrap();
        assert_eq!(create_cell.cell_type, CellType::Create);

        // Step 3: Simulate relay processing CREATE
        let node_secret = [99u8; KEY_SIZE];
        let node = ZkRelayNode::new(node_secret);

        let action = node.process_cell(create_cell, peers[0]).unwrap();

        // Node should respond with CREATED containing its public key
        match action {
            RelayAction::Respond(cell) => {
                assert_eq!(cell.cell_type, CellType::Created);

                // Step 4: Client processes CREATED response
                let response = CreatedHandshake::deserialize(&cell.payload).unwrap();
                let next_cell = client
                    .process_created_response(&circuit_id, &response, 0)
                    .unwrap();

                // Should return EXTEND cell for next hop
                assert!(next_cell.is_some());
                assert_eq!(next_cell.unwrap().cell_type, CellType::Extend);
            }
            _ => panic!("Expected Respond action"),
        }
    }
}
