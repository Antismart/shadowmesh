//! Multi-hop onion routing for ShadowMesh
//!
//! Provides proper onion-encrypted routing modeled after Tor's design:
//!
//! - Each relay has a long-term X25519 keypair
//! - The client generates a fresh ephemeral X25519 keypair **per hop**
//! - Each layer's symmetric key is derived from ECDH(client_ephemeral_i, relay_i_static)
//! - A relay at hop N can only decrypt its own layer; it cannot derive keys for any other hop
//! - Packets are padded to a fixed size to prevent size-based traffic correlation
//!
//! # Packet format (one onion layer)
//!
//! ```text
//! [client_ephemeral_pubkey: 32 bytes]
//! [nonce: 12 bytes]
//! [AEAD ciphertext + 16-byte tag]
//! ```
//!
//! The plaintext inside the AEAD is either the next onion layer (intermediate hops) or the
//! actual payload (final hop), both padded to a uniform size.

use crate::crypto::{CryptoManager, KeyDerivation, KEY_SIZE, NONCE_SIZE};
use blake3::Hasher;
use chacha20poly1305::aead::OsRng;
use libp2p::PeerId;
use rand::seq::SliceRandom;
use x25519_dalek::{PublicKey, StaticSecret};

/// Fixed packet size for all onion packets (prevents size correlation attacks).
/// Each layer adds 32 (ephemeral pubkey) + 12 (nonce) + 16 (AEAD tag) = 60 bytes of overhead.
/// The payload region is padded to this size minus the per-layer overhead.
pub const ONION_PACKET_SIZE: usize = 4096;

/// Per-layer overhead: 32-byte ephemeral pubkey + 12-byte nonce + 16-byte AEAD tag
pub const LAYER_OVERHEAD: usize = 32 + NONCE_SIZE + 16;

/// Maximum payload size for a 3-hop onion packet.
/// Each hop adds LAYER_OVERHEAD bytes, so the usable payload is:
/// ONION_PACKET_SIZE - (num_hops * LAYER_OVERHEAD)
pub const MAX_HOPS: usize = 10;

/// X25519 public key size
const X25519_PUBKEY_SIZE: usize = 32;

// ---------------------------------------------------------------------------
// Relay keypair -- wraps x25519-dalek for ergonomic use in routing
// ---------------------------------------------------------------------------

/// An X25519 keypair representing a relay node's long-term identity for onion routing.
///
/// In a real deployment each relay would persist this across restarts and publish
/// the public half in the DHT so that clients can build circuits.
#[derive(Clone)]
pub struct RelayKeypair {
    secret: StaticSecret,
    pub public: PublicKey,
}

impl RelayKeypair {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Reconstruct from raw secret bytes (e.g. loaded from disk).
    pub fn from_secret_bytes(bytes: &[u8; 32]) -> Self {
        let secret = StaticSecret::from(*bytes);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Return the 32-byte public key suitable for wire transmission.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.public.as_bytes()
    }

    /// Perform ECDH with a peer's ephemeral public key and derive a 32-byte symmetric key.
    ///
    /// The derivation uses BLAKE3 in keyed mode over:
    ///   key   = raw ECDH shared secret
    ///   input = domain separator ++ context ++ sorted(pubA, pubB)
    ///
    /// Both sides (client and relay) compute the same value.
    fn derive_symmetric_key(
        &self,
        peer_public_bytes: &[u8; 32],
        context: &[u8],
    ) -> [u8; KEY_SIZE] {
        let peer_public = PublicKey::from(*peer_public_bytes);
        let shared_secret = self.secret.diffie_hellman(&peer_public);

        derive_key_from_shared_secret(
            shared_secret.as_bytes(),
            self.public.as_bytes(),
            peer_public_bytes,
            context,
        )
    }
}

impl std::fmt::Debug for RelayKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayKeypair")
            .field("public", &hex::encode(self.public.as_bytes()))
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Shared key derivation helper (used by both client and relay sides)
// ---------------------------------------------------------------------------

/// Derive a 32-byte symmetric key from raw ECDH output.
///
/// Inputs:
///   - `shared_secret`: the 32-byte raw X25519 output
///   - `pub_a`, `pub_b`: both parties' public keys (sorted internally)
///   - `context`: additional binding data (e.g. hop index)
fn derive_key_from_shared_secret(
    shared_secret: &[u8; 32],
    pub_a: &[u8; 32],
    pub_b: &[u8; 32],
    context: &[u8],
) -> [u8; KEY_SIZE] {
    let mut hasher = Hasher::new_keyed(shared_secret);
    hasher.update(b"shadowmesh-onion-hop-key-v1");
    hasher.update(context);

    // Include both public keys in sorted order for key confirmation symmetry.
    if pub_a < pub_b {
        hasher.update(pub_a);
        hasher.update(pub_b);
    } else {
        hasher.update(pub_b);
        hasher.update(pub_a);
    }

    let derived = hasher.finalize();
    let mut key = [0u8; KEY_SIZE];
    key.copy_from_slice(&derived.as_bytes()[..KEY_SIZE]);
    key
}

// ---------------------------------------------------------------------------
// OnionPacket -- the wire format
// ---------------------------------------------------------------------------

/// An onion-encrypted packet ready for transmission.
///
/// The raw bytes contain nested layers of:
///   `[ephemeral_pubkey (32)] [nonce (12)] [AEAD ciphertext]`
///
/// Each relay peels exactly one layer using its own private key.
#[derive(Debug, Clone)]
pub struct OnionPacket {
    /// The serialised onion data (fixed size = ONION_PACKET_SIZE).
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Route
// ---------------------------------------------------------------------------

/// A route through the network with associated relay public keys.
#[derive(Debug, Clone)]
pub struct Route {
    pub hops: Vec<PeerId>,
    pub encrypted_data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// RoutingLayer -- the main API surface
// ---------------------------------------------------------------------------

/// High-level routing API.
///
/// After the redesign the `RoutingLayer` no longer holds a single shared secret
/// for key derivation.  Instead, each encryption/decryption operation uses
/// proper per-hop ECDH.
///
/// For **client-side** operations (building onion packets) the caller supplies
/// the relay public keys.
///
/// For **relay-side** operations (peeling a layer) the caller supplies its own
/// `RelayKeypair`.
///
/// The `secret` field is retained only for the `encrypt_fragment` /
/// `decrypt_fragment` convenience helpers which provide simple symmetric
/// encryption unrelated to onion routing.
pub struct RoutingLayer {
    secret: Vec<u8>,
}

impl RoutingLayer {
    /// Create a new routing layer with a secret (used only for fragment encryption).
    pub fn new(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }

    /// Create a new routing layer with a random secret.
    pub fn new_random() -> Self {
        use rand::Rng;
        let secret: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen()).collect();
        Self { secret }
    }

    /// Select a random subset of peers to form a route.
    pub fn build_route(&self, available_peers: &[PeerId], num_hops: usize) -> Vec<PeerId> {
        let mut rng = rand::thread_rng();
        let mut peers = available_peers.to_vec();
        peers.shuffle(&mut rng);
        peers.truncate(num_hops);
        peers
    }

    // ------------------------------------------------------------------
    // Onion packet construction (client side)
    // ------------------------------------------------------------------

    /// Build an onion packet for a given route.
    ///
    /// # Arguments
    /// * `data`             - The plaintext payload
    /// * `relay_pubkeys`    - Ordered relay X25519 public keys (hop 0 is first relay)
    ///
    /// # Returns
    /// The fully wrapped `OnionPacket` (padded to `ONION_PACKET_SIZE`).
    pub fn build_onion_packet(
        data: &[u8],
        relay_pubkeys: &[[u8; 32]],
    ) -> Result<OnionPacket, String> {
        let num_hops = relay_pubkeys.len();
        if num_hops == 0 {
            return Err("At least one hop is required".into());
        }
        if num_hops > MAX_HOPS {
            return Err(format!("Too many hops ({num_hops} > {MAX_HOPS})"));
        }

        // Compute the max payload capacity after all layers of overhead.
        // The padded region is: [4-byte length prefix] [payload] [zero padding]
        // So the usable payload is (total_pad_region - 4).
        let pad_region = ONION_PACKET_SIZE.saturating_sub(num_hops * LAYER_OVERHEAD);
        let max_payload = pad_region.saturating_sub(4); // 4 bytes for the LE length prefix
        if data.len() > max_payload {
            return Err(format!(
                "Payload too large ({} bytes, max {} for {} hops)",
                data.len(),
                max_payload,
                num_hops
            ));
        }

        // Pad the plaintext to pad_region so the innermost ciphertext is
        // always the same size regardless of actual payload length.
        // Format: [payload_len: 4 bytes LE] [payload] [zero padding]
        let mut padded = Vec::with_capacity(pad_region);
        let payload_len = data.len() as u32;
        padded.extend_from_slice(&payload_len.to_le_bytes());
        padded.extend_from_slice(data);
        padded.resize(pad_region, 0u8);

        // Wrap layers from innermost (last hop) to outermost (first hop).
        let mut current = padded;

        for i in (0..num_hops).rev() {
            // Generate a fresh ephemeral keypair for this hop.
            let eph_secret = StaticSecret::random_from_rng(OsRng);
            let eph_public = PublicKey::from(&eph_secret);

            // ECDH with the relay's static public key.
            let relay_pub = PublicKey::from(relay_pubkeys[i]);
            let shared = eph_secret.diffie_hellman(&relay_pub);

            // Derive symmetric key.
            let context = format!("onion-layer-{}", i);
            let sym_key = derive_key_from_shared_secret(
                shared.as_bytes(),
                eph_public.as_bytes(),
                &relay_pubkeys[i],
                context.as_bytes(),
            );

            // Encrypt with ChaCha20-Poly1305 (random nonce).
            let manager = CryptoManager::new(&sym_key);
            let encrypted = manager
                .encrypt_raw(&current)
                .map_err(|e| format!("Encryption at hop {i} failed: {e}"))?;

            // Prepend ephemeral public key: [eph_pub (32)] [nonce (12)] [ciphertext+tag]
            let mut layer = Vec::with_capacity(X25519_PUBKEY_SIZE + encrypted.len());
            layer.extend_from_slice(eph_public.as_bytes());
            layer.extend_from_slice(&encrypted);

            current = layer;
        }

        // The outermost layer should be exactly ONION_PACKET_SIZE.
        // Due to AEAD tag sizes it will be: max_payload + num_hops * LAYER_OVERHEAD
        // which equals ONION_PACKET_SIZE by construction.
        debug_assert_eq!(
            current.len(),
            ONION_PACKET_SIZE,
            "Onion packet size mismatch: got {} expected {}",
            current.len(),
            ONION_PACKET_SIZE
        );

        Ok(OnionPacket { data: current })
    }

    // ------------------------------------------------------------------
    // Onion packet peeling (relay side)
    // ------------------------------------------------------------------

    /// Peel one layer of onion encryption using the relay's private key.
    ///
    /// # Arguments
    /// * `packet`    - The incoming onion packet bytes
    /// * `relay_kp`  - This relay's X25519 keypair
    /// * `hop_index` - The position of this relay in the circuit (used as KDF context)
    ///
    /// # Returns
    /// The inner packet (next layer or, if this is the last hop, the padded payload).
    pub fn peel_layer(
        packet: &[u8],
        relay_kp: &RelayKeypair,
        hop_index: usize,
    ) -> Result<Vec<u8>, String> {
        if packet.len() < LAYER_OVERHEAD {
            return Err("Packet too small to contain an onion layer".into());
        }

        // Extract the client's ephemeral public key (first 32 bytes).
        let eph_pub_bytes: [u8; 32] = packet[..X25519_PUBKEY_SIZE]
            .try_into()
            .map_err(|_| "Invalid ephemeral pubkey")?;

        // The rest is [nonce (12)] [ciphertext+tag].
        let encrypted = &packet[X25519_PUBKEY_SIZE..];

        // ECDH + key derivation.
        let context = format!("onion-layer-{}", hop_index);
        let sym_key =
            relay_kp.derive_symmetric_key(&eph_pub_bytes, context.as_bytes());

        let manager = CryptoManager::new(&sym_key);
        let inner = manager
            .decrypt_raw(encrypted)
            .map_err(|e| format!("Peel failed at hop {hop_index}: {e}"))?;

        Ok(inner)
    }

    /// Extract the original payload from the innermost (fully peeled) plaintext.
    ///
    /// The plaintext is `[payload_len: 4 LE bytes] [payload] [zero padding]`.
    pub fn extract_payload(plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if plaintext.len() < 4 {
            return Err("Plaintext too short to contain length prefix".into());
        }
        let len = u32::from_le_bytes(
            plaintext[..4]
                .try_into()
                .map_err(|_| "Invalid length prefix")?,
        ) as usize;
        if 4 + len > plaintext.len() {
            return Err(format!(
                "Length prefix ({len}) exceeds available data ({})",
                plaintext.len() - 4
            ));
        }
        Ok(plaintext[4..4 + len].to_vec())
    }

    // ------------------------------------------------------------------
    // Legacy convenience wrappers (kept for API compatibility)
    // ------------------------------------------------------------------

    /// Encrypt data for a route described by relay public keys.
    ///
    /// This is a convenience wrapper around `build_onion_packet`.
    pub fn encrypt_for_route_ecdh(
        &self,
        data: &[u8],
        relay_pubkeys: &[[u8; 32]],
    ) -> Result<Vec<u8>, String> {
        let packet = Self::build_onion_packet(data, relay_pubkeys)?;
        Ok(packet.data)
    }

    /// Encrypt data for a route (backward-compatible signature).
    ///
    /// **NOTE**: This method exists only for API compatibility with code that
    /// passes `PeerId`s.  Because `PeerId`s do not carry X25519 public keys,
    /// this method falls back to the (insecure) deterministic key derivation.
    /// New code should use `encrypt_for_route_ecdh` or `build_onion_packet`.
    pub fn encrypt_for_route(&self, data: &[u8], route: &[PeerId]) -> Result<Vec<u8>, String> {
        // Legacy path: deterministic key derivation (kept for backward compat).
        let keys = KeyDerivation::derive_route_keys(&self.secret, route.len());
        let router = crate::crypto::OnionRouter::new(&keys);
        router
            .wrap(data)
            .map_err(|e| format!("Encryption failed: {e}"))
    }

    /// Decrypt one layer using legacy deterministic keys (backward-compatible).
    pub fn decrypt_layer(
        &self,
        data: &[u8],
        layer_index: usize,
        num_hops: usize,
    ) -> Result<Vec<u8>, String> {
        let keys = KeyDerivation::derive_route_keys(&self.secret, num_hops);
        let router = crate::crypto::OnionRouter::new(&keys);
        router
            .unwrap_layer(data, layer_index)
            .map_err(|e| format!("Decryption failed: {e}"))
    }

    /// Decrypt all layers using legacy deterministic keys (backward-compatible).
    pub fn decrypt_all(&self, data: &[u8], num_hops: usize) -> Result<Vec<u8>, String> {
        let keys = KeyDerivation::derive_route_keys(&self.secret, num_hops);
        let router = crate::crypto::OnionRouter::new(&keys);
        router
            .unwrap_all(data.to_vec())
            .map_err(|e| format!("Decryption failed: {e}"))
    }

    // ------------------------------------------------------------------
    // Fragment encryption (symmetric, unrelated to onion routing)
    // ------------------------------------------------------------------

    /// Encrypt a single fragment (simple symmetric encryption, not onion routing).
    pub fn encrypt_fragment(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let key = KeyDerivation::derive_key(&self.secret, "fragment");
        let manager = CryptoManager::new(&key);
        manager
            .encrypt_raw(data)
            .map_err(|e| format!("Fragment encryption failed: {e}"))
    }

    /// Decrypt a single fragment.
    pub fn decrypt_fragment(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let key = KeyDerivation::derive_key(&self.secret, "fragment");
        let manager = CryptoManager::new(&key);
        manager
            .decrypt_raw(data)
            .map_err(|e| format!("Fragment decryption failed: {e}"))
    }

    /// Get the secret (for key sharing / fragment encryption).
    pub fn secret(&self) -> &[u8] {
        &self.secret
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;

    fn create_test_peers(n: usize) -> Vec<PeerId> {
        (0..n)
            .map(|_| {
                let keypair = identity::Keypair::generate_ed25519();
                PeerId::from(keypair.public())
            })
            .collect()
    }

    // ---------------------------------------------------------------
    // Legacy API tests (backward compatibility)
    // ---------------------------------------------------------------

    #[test]
    fn test_build_route() {
        let layer = RoutingLayer::new_random();
        let peers = create_test_peers(10);

        let route = layer.build_route(&peers, 5);
        assert_eq!(route.len(), 5);
    }

    #[test]
    fn test_onion_encryption_legacy() {
        let layer = RoutingLayer::new(b"test-secret");
        let peers = create_test_peers(3);

        let plaintext = b"Secret message through the network";

        let encrypted = layer.encrypt_for_route(plaintext, &peers).unwrap();
        assert_ne!(encrypted, plaintext.to_vec());

        let decrypted = layer.decrypt_all(&encrypted, 3).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_fragment_encryption() {
        let layer = RoutingLayer::new(b"fragment-secret");
        let data = b"Fragment data to encrypt";

        let encrypted = layer.encrypt_fragment(data).unwrap();
        let decrypted = layer.decrypt_fragment(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_layer_by_layer_decryption_legacy() {
        let layer = RoutingLayer::new(b"layer-test-secret");
        let peers = create_test_peers(3);

        let plaintext = b"Multi-layer message";

        let encrypted = layer.encrypt_for_route(plaintext, &peers).unwrap();

        let after_layer0 = layer.decrypt_layer(&encrypted, 0, 3).unwrap();
        let after_layer1 = layer.decrypt_layer(&after_layer0, 1, 3).unwrap();
        let after_layer2 = layer.decrypt_layer(&after_layer1, 2, 3).unwrap();

        assert_eq!(after_layer2, plaintext);
    }

    // ---------------------------------------------------------------
    // New ECDH-based onion routing tests
    // ---------------------------------------------------------------

    #[test]
    fn test_ecdh_onion_3_hops() {
        // Simulate 3 relay nodes, each with their own keypair.
        let relay0 = RelayKeypair::generate();
        let relay1 = RelayKeypair::generate();
        let relay2 = RelayKeypair::generate();

        let relay_pubkeys = [
            relay0.public_key_bytes(),
            relay1.public_key_bytes(),
            relay2.public_key_bytes(),
        ];

        let plaintext = b"Top-secret payload through 3 hops";

        // Client builds the onion packet.
        let packet = RoutingLayer::build_onion_packet(plaintext, &relay_pubkeys).unwrap();
        assert_eq!(packet.data.len(), ONION_PACKET_SIZE);

        // Hop 0 peels its layer.
        let after_hop0 = RoutingLayer::peel_layer(&packet.data, &relay0, 0).unwrap();

        // Hop 1 peels its layer.
        let after_hop1 = RoutingLayer::peel_layer(&after_hop0, &relay1, 1).unwrap();

        // Hop 2 peels its layer.
        let after_hop2 = RoutingLayer::peel_layer(&after_hop1, &relay2, 2).unwrap();

        // Extract the original payload.
        let recovered = RoutingLayer::extract_payload(&after_hop2).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_ecdh_onion_1_hop() {
        let relay = RelayKeypair::generate();
        let plaintext = b"Single hop message";

        let packet =
            RoutingLayer::build_onion_packet(plaintext, &[relay.public_key_bytes()]).unwrap();
        assert_eq!(packet.data.len(), ONION_PACKET_SIZE);

        let inner = RoutingLayer::peel_layer(&packet.data, &relay, 0).unwrap();
        let recovered = RoutingLayer::extract_payload(&inner).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_packet_is_fixed_size() {
        let relay0 = RelayKeypair::generate();
        let relay1 = RelayKeypair::generate();
        let pubkeys = [relay0.public_key_bytes(), relay1.public_key_bytes()];

        // Small payload.
        let p1 =
            RoutingLayer::build_onion_packet(b"short", &pubkeys).unwrap();
        // Larger payload.
        let p2 = RoutingLayer::build_onion_packet(
            &vec![0xAB; 1024],
            &pubkeys,
        )
        .unwrap();

        assert_eq!(p1.data.len(), ONION_PACKET_SIZE);
        assert_eq!(p2.data.len(), ONION_PACKET_SIZE);
        // Both packets are the same size despite different payload lengths.
        assert_eq!(p1.data.len(), p2.data.len());
    }

    #[test]
    fn test_different_keys_per_hop() {
        // Verify that each hop uses an independent key by showing that a relay
        // at hop 0 cannot peel the layer meant for hop 1.
        let relay0 = RelayKeypair::generate();
        let relay1 = RelayKeypair::generate();

        let pubkeys = [relay0.public_key_bytes(), relay1.public_key_bytes()];
        let plaintext = b"Independence test";

        let packet = RoutingLayer::build_onion_packet(plaintext, &pubkeys).unwrap();

        // Relay 0 correctly peels its layer.
        let after_hop0 = RoutingLayer::peel_layer(&packet.data, &relay0, 0).unwrap();

        // Relay 0 tries to peel the *next* layer (meant for relay 1) -- must fail.
        let bad_attempt = RoutingLayer::peel_layer(&after_hop0, &relay0, 1);
        assert!(
            bad_attempt.is_err(),
            "Relay 0 must NOT be able to decrypt hop 1's layer"
        );
    }

    #[test]
    fn test_relay_cannot_derive_other_hops_keys() {
        // Even if relay 0 knows relay 1's *public* key it still cannot
        // compute the symmetric key because it does not know the client's
        // ephemeral private key for hop 1.
        let relay0 = RelayKeypair::generate();
        let relay1 = RelayKeypair::generate();
        let relay2 = RelayKeypair::generate();

        let pubkeys = [
            relay0.public_key_bytes(),
            relay1.public_key_bytes(),
            relay2.public_key_bytes(),
        ];

        let packet =
            RoutingLayer::build_onion_packet(b"secret", &pubkeys).unwrap();

        // Relay 0 peels its own layer.
        let inner = RoutingLayer::peel_layer(&packet.data, &relay0, 0).unwrap();

        // Relay 0 attempts to peel the next layer using relay 1's position
        // but with its own key -- ECDH will produce the wrong shared secret.
        assert!(RoutingLayer::peel_layer(&inner, &relay0, 1).is_err());

        // Relay 2 tries to peel hop 1's layer (wrong key).
        assert!(RoutingLayer::peel_layer(&inner, &relay2, 1).is_err());

        // Only relay 1 can peel.
        let inner2 = RoutingLayer::peel_layer(&inner, &relay1, 1).unwrap();
        let inner3 = RoutingLayer::peel_layer(&inner2, &relay2, 2).unwrap();
        let payload = RoutingLayer::extract_payload(&inner3).unwrap();
        assert_eq!(payload, b"secret");
    }

    #[test]
    fn test_wrong_hop_index_fails() {
        // Using the correct relay key but the wrong hop index should also fail,
        // because the hop index is mixed into the KDF context.
        let relay = RelayKeypair::generate();
        let packet =
            RoutingLayer::build_onion_packet(b"hop-idx-test", &[relay.public_key_bytes()])
                .unwrap();

        // Correct index.
        assert!(RoutingLayer::peel_layer(&packet.data, &relay, 0).is_ok());
        // Wrong index.
        assert!(RoutingLayer::peel_layer(&packet.data, &relay, 1).is_err());
        assert!(RoutingLayer::peel_layer(&packet.data, &relay, 99).is_err());
    }

    #[test]
    fn test_two_packets_same_route_differ() {
        // Because ephemeral keys are random, two packets to the same route
        // will have completely different ciphertexts.
        let relay = RelayKeypair::generate();
        let pubkeys = [relay.public_key_bytes()];

        let p1 = RoutingLayer::build_onion_packet(b"same payload", &pubkeys).unwrap();
        let p2 = RoutingLayer::build_onion_packet(b"same payload", &pubkeys).unwrap();

        assert_ne!(
            p1.data, p2.data,
            "Packets with the same payload must differ (ephemeral randomness)"
        );
    }

    #[test]
    fn test_payload_too_large() {
        let relay = RelayKeypair::generate();
        let pad_region = ONION_PACKET_SIZE - LAYER_OVERHEAD;
        let max_payload = pad_region - 4; // 4 bytes for LE length prefix

        // Exactly max_payload should succeed.
        let exact = vec![0u8; max_payload];
        assert!(
            RoutingLayer::build_onion_packet(&exact, &[relay.public_key_bytes()]).is_ok(),
            "Payload of exactly max_payload bytes should fit"
        );

        // One byte over should fail.
        let too_big = vec![0u8; max_payload + 1];
        assert!(
            RoutingLayer::build_onion_packet(&too_big, &[relay.public_key_bytes()]).is_err(),
            "Payload exceeding max_payload must be rejected"
        );
    }

    #[test]
    fn test_encrypt_for_route_ecdh_convenience() {
        let relay0 = RelayKeypair::generate();
        let relay1 = RelayKeypair::generate();

        let routing = RoutingLayer::new_random();
        let pubkeys = [relay0.public_key_bytes(), relay1.public_key_bytes()];

        let encrypted = routing
            .encrypt_for_route_ecdh(b"convenience test", &pubkeys)
            .unwrap();

        let inner0 = RoutingLayer::peel_layer(&encrypted, &relay0, 0).unwrap();
        let inner1 = RoutingLayer::peel_layer(&inner0, &relay1, 1).unwrap();
        let payload = RoutingLayer::extract_payload(&inner1).unwrap();
        assert_eq!(payload, b"convenience test");
    }

    #[test]
    fn test_tampered_packet_fails() {
        let relay = RelayKeypair::generate();
        let mut packet =
            RoutingLayer::build_onion_packet(b"integrity", &[relay.public_key_bytes()])
                .unwrap();

        // Flip a byte in the ciphertext region (past the ephemeral pubkey).
        packet.data[64] ^= 0xFF;

        let result = RoutingLayer::peel_layer(&packet.data, &relay, 0);
        assert!(result.is_err(), "Tampered packet must fail AEAD verification");
    }
}
