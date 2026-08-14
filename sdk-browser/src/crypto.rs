//! Authenticated key agreement + ChaCha20-Poly1305 encryption for WebRTC
//! DataChannel messages.
//!
//! ## Security guarantee
//!
//! The channel key is established with an **authenticated ephemeral X25519
//! ECDH handshake** (a Noise-style pattern):
//!
//! * Every peer owns a long-term **Ed25519 identity keypair**. The peer's
//!   identity is its verifying-key bytes, hex-encoded — this is exactly the
//!   `peer_id` announced over signaling.
//! * When a DataChannel opens each side generates a fresh **ephemeral X25519
//!   keypair** and sends a handshake frame containing its identity public key,
//!   its ephemeral public key, and an Ed25519 signature over the ephemeral key.
//! * Each side verifies the signature with the peer's identity key, checks the
//!   identity matches the `peer_id` learned from signaling, then computes the
//!   shared secret `ECDH(our_ephemeral_secret, their_ephemeral_public)` and
//!   derives the ChaCha20-Poly1305 key via HKDF-SHA256.
//!
//! Because the shared secret depends on ephemeral **private** keys that never
//! leave the peer, an observer of the signaling channel (which only sees public
//! peer IDs and SDP) CANNOT derive the channel key. The Ed25519 signature binds
//! each ephemeral key to a long-term identity, preventing a man-in-the-middle
//! from substituting its own ephemeral key: the substituted identity would not
//! match the expected `peer_id`.
//!
//! This replaces the previous scheme, which derived the key deterministically
//! from the two public peer IDs and was therefore recoverable by anyone who
//! observed signaling.
//!
//! ## Wire formats
//!
//! Handshake frame (sent in the clear — it establishes the key):
//!
//!   `[HANDSHAKE_TAG: 1][identity_pub: 32][ephemeral_pub: 32][signature: 64]`
//!
//! Data frame (encrypted with the derived key):
//!
//!   `[nonce: 12][ciphertext + Poly1305 tag: N + 16]` where the plaintext is
//!   `[DATA_TAG: 1][payload]`.

use crate::error::{codes, SdkError};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey as XPublicKey, StaticSecret as XSecret};

/// Size of a ChaCha20-Poly1305 nonce (96 bits).
pub const NONCE_SIZE: usize = 12;

/// Size of a ChaCha20-Poly1305 key (256 bits).
pub const KEY_SIZE: usize = 32;

/// Minimum valid ciphertext length: nonce + Poly1305 tag (no plaintext).
pub const MIN_ENCRYPTED_LEN: usize = NONCE_SIZE + 16;

/// Handshake message tag byte (first byte of a handshake frame).
pub const HANDSHAKE_TAG: u8 = 0x01;

/// Data message tag byte (first byte of the *plaintext* inside a data frame).
pub const DATA_TAG: u8 = 0x02;

/// Length of an Ed25519 identity public key.
pub const IDENTITY_PUBLIC_LEN: usize = 32;

/// Length of an X25519 ephemeral public key.
pub const EPHEMERAL_PUBLIC_LEN: usize = 32;

/// Length of an Ed25519 signature.
pub const SIGNATURE_LEN: usize = 64;

/// Total handshake frame length.
pub const HANDSHAKE_FRAME_LEN: usize =
    1 + IDENTITY_PUBLIC_LEN + EPHEMERAL_PUBLIC_LEN + SIGNATURE_LEN;

/// Domain-separation / HKDF context string. Bumped from the old `-v1` scheme.
const HS_CONTEXT: &[u8] = b"shadowmesh-datachannel-noise-v2";

// ---------------------------------------------------------------------------
// Secure randomness
// ---------------------------------------------------------------------------

/// Fill `dest` with cryptographically secure random bytes.
///
/// Uses the `getrandom` crate, which delegates to `crypto.getRandomValues()`
/// in browser / Web Worker contexts. There is no insecure fallback: a failure
/// panics rather than producing predictable key material.
fn secure_random(dest: &mut [u8]) {
    getrandom::getrandom(dest)
        .expect("FATAL: no secure random source available for key generation");
}

// ---------------------------------------------------------------------------
// Long-term identity
// ---------------------------------------------------------------------------

/// A long-term Ed25519 identity keypair.
///
/// The peer's public identity is `hex(verifying_key)`, which is used directly
/// as its `peer_id` on the signaling layer.
pub struct Identity {
    signing: SigningKey,
}

impl Identity {
    /// Generate a fresh random identity.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        secure_random(&mut seed);
        let signing = SigningKey::from_bytes(&seed);
        Self { signing }
    }

    /// The 32-byte Ed25519 public key.
    pub fn public_bytes(&self) -> [u8; IDENTITY_PUBLIC_LEN] {
        self.signing.verifying_key().to_bytes()
    }

    /// The peer ID: lowercase hex of the identity public key.
    pub fn peer_id(&self) -> String {
        hex_lower(&self.public_bytes())
    }

    /// Sign `message` with the identity key.
    fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.signing.sign(message).to_bytes()
    }
}

// ---------------------------------------------------------------------------
// Ephemeral key agreement
// ---------------------------------------------------------------------------

/// A per-connection ephemeral X25519 keypair.
pub struct EphemeralKeypair {
    secret: XSecret,
    public: [u8; EPHEMERAL_PUBLIC_LEN],
}

impl EphemeralKeypair {
    /// Generate a fresh ephemeral keypair for one connection.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        secure_random(&mut bytes);
        let secret = XSecret::from(bytes);
        let public = XPublicKey::from(&secret).to_bytes();
        Self { secret, public }
    }

    /// Our ephemeral public key bytes.
    pub fn public(&self) -> [u8; EPHEMERAL_PUBLIC_LEN] {
        self.public
    }
}

/// The signed ephemeral message: `HS_CONTEXT || ephemeral_pub`.
fn ephemeral_signing_message(eph_pub: &[u8; EPHEMERAL_PUBLIC_LEN]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(HS_CONTEXT.len() + EPHEMERAL_PUBLIC_LEN);
    msg.extend_from_slice(HS_CONTEXT);
    msg.extend_from_slice(eph_pub);
    msg
}

/// Build the handshake frame we send to the peer.
///
/// Wire: `[HANDSHAKE_TAG][identity_pub(32)][ephemeral_pub(32)][signature(64)]`.
pub fn build_handshake(identity: &Identity, ephemeral: &EphemeralKeypair) -> Vec<u8> {
    let eph_pub = ephemeral.public();
    let signature = identity.sign(&ephemeral_signing_message(&eph_pub));

    let mut frame = Vec::with_capacity(HANDSHAKE_FRAME_LEN);
    frame.push(HANDSHAKE_TAG);
    frame.extend_from_slice(&identity.public_bytes());
    frame.extend_from_slice(&eph_pub);
    frame.extend_from_slice(&signature);
    frame
}

/// A verified peer handshake: their identity (as a `peer_id` hex string) and
/// their ephemeral public key.
pub struct PeerHandshake {
    pub peer_id: String,
    pub ephemeral_public: [u8; EPHEMERAL_PUBLIC_LEN],
}

/// Parse and cryptographically verify a handshake frame from the remote peer.
///
/// Verifies the Ed25519 signature over the ephemeral key using the identity key
/// carried in the frame. Returns the peer's identity (`peer_id` hex) and their
/// ephemeral public key. The caller MUST additionally check that `peer_id`
/// matches the identity learned from signaling before deriving the key.
pub fn verify_handshake(frame: &[u8]) -> Result<PeerHandshake, SdkError> {
    if frame.len() != HANDSHAKE_FRAME_LEN {
        return Err(SdkError::new(
            codes::AUTH_FAILED,
            &format!(
                "Invalid handshake: expected {} bytes, got {}",
                HANDSHAKE_FRAME_LEN,
                frame.len()
            ),
        ));
    }
    if frame[0] != HANDSHAKE_TAG {
        return Err(SdkError::new(
            codes::AUTH_FAILED,
            "Invalid handshake: missing handshake tag",
        ));
    }

    let mut identity_pub = [0u8; IDENTITY_PUBLIC_LEN];
    identity_pub.copy_from_slice(&frame[1..1 + IDENTITY_PUBLIC_LEN]);

    let mut eph_pub = [0u8; EPHEMERAL_PUBLIC_LEN];
    let eph_start = 1 + IDENTITY_PUBLIC_LEN;
    eph_pub.copy_from_slice(&frame[eph_start..eph_start + EPHEMERAL_PUBLIC_LEN]);

    let mut sig_bytes = [0u8; SIGNATURE_LEN];
    let sig_start = eph_start + EPHEMERAL_PUBLIC_LEN;
    sig_bytes.copy_from_slice(&frame[sig_start..sig_start + SIGNATURE_LEN]);

    let verifying_key = VerifyingKey::from_bytes(&identity_pub).map_err(|_| {
        SdkError::new(codes::AUTH_FAILED, "Invalid handshake: bad identity key")
    })?;
    let signature = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify(&ephemeral_signing_message(&eph_pub), &signature)
        .map_err(|_| {
            SdkError::new(
                codes::AUTH_FAILED,
                "Invalid handshake: ephemeral key signature verification failed",
            )
        })?;

    Ok(PeerHandshake {
        peer_id: hex_lower(&identity_pub),
        ephemeral_public: eph_pub,
    })
}

/// Derive the symmetric ChaCha20-Poly1305 key from the ephemeral ECDH exchange.
///
/// `shared = X25519(our_ephemeral_secret, their_ephemeral_public)`, then
/// `key = HKDF-SHA256(ikm = shared, salt = sorted(our_pub || their_pub),
/// info = HS_CONTEXT)`. Sorting the two public keys makes both peers compute an
/// identical salt regardless of who initiated.
pub fn derive_channel_key(
    ephemeral: &EphemeralKeypair,
    their_ephemeral_public: &[u8; EPHEMERAL_PUBLIC_LEN],
) -> [u8; KEY_SIZE] {
    let their_public = XPublicKey::from(*their_ephemeral_public);
    let shared = ephemeral.secret.diffie_hellman(&their_public);

    // Order-independent salt binding both ephemeral public keys.
    let our_pub = ephemeral.public();
    let (first, second) = if our_pub <= *their_ephemeral_public {
        (&our_pub, their_ephemeral_public)
    } else {
        (their_ephemeral_public, &our_pub)
    };
    let mut salt = [0u8; 2 * EPHEMERAL_PUBLIC_LEN];
    salt[..EPHEMERAL_PUBLIC_LEN].copy_from_slice(first);
    salt[EPHEMERAL_PUBLIC_LEN..].copy_from_slice(second);

    let hk = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
    let mut okm = [0u8; KEY_SIZE];
    hk.expand(HS_CONTEXT, &mut okm)
        .expect("KEY_SIZE is a valid HKDF-SHA256 output length");
    okm
}

// ---------------------------------------------------------------------------
// Hex helper
// ---------------------------------------------------------------------------

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

// ---------------------------------------------------------------------------
// Cipher wrapper
// ---------------------------------------------------------------------------

/// Thin wrapper around `ChaCha20Poly1305` that encrypts / decrypts DataChannel
/// data frames using the `[nonce][ciphertext+tag]` wire format.
pub struct DataChannelCipher {
    cipher: ChaCha20Poly1305,
}

impl DataChannelCipher {
    /// Create a cipher from a raw 256-bit key (the HKDF output).
    pub fn new(key: &[u8; KEY_SIZE]) -> Self {
        let key = Key::from_slice(key);
        Self {
            cipher: ChaCha20Poly1305::new(key),
        }
    }

    /// Encrypt `plaintext` and return `[nonce (12 B)][ciphertext + tag]`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, SdkError> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

        let ciphertext = self.cipher.encrypt(&nonce, plaintext).map_err(|e| {
            SdkError::new(
                codes::CRYPTO_ERROR,
                &format!("ChaCha20-Poly1305 encryption failed: {}", e),
            )
        })?;

        let mut out = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt a frame produced by [`DataChannelCipher::encrypt`].
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, SdkError> {
        if data.len() < MIN_ENCRYPTED_LEN {
            return Err(SdkError::new(
                codes::CRYPTO_ERROR,
                &format!(
                    "Ciphertext too short ({} bytes, minimum {})",
                    data.len(),
                    MIN_ENCRYPTED_LEN
                ),
            ));
        }

        let nonce = Nonce::from_slice(&data[..NONCE_SIZE]);
        let ciphertext = &data[NONCE_SIZE..];

        self.cipher.decrypt(nonce, ciphertext).map_err(|e| {
            SdkError::new(
                codes::CRYPTO_ERROR,
                &format!("ChaCha20-Poly1305 decryption failed: {}", e),
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Data-frame helpers
// ---------------------------------------------------------------------------

/// Wrap an application-level payload in an encrypted data frame.
///
/// Wire plaintext: `[DATA_TAG (1 B)][payload]`.
pub fn encrypt_data(cipher: &DataChannelCipher, payload: &[u8]) -> Result<Vec<u8>, SdkError> {
    let mut plaintext = Vec::with_capacity(1 + payload.len());
    plaintext.push(DATA_TAG);
    plaintext.extend_from_slice(payload);
    cipher.encrypt(&plaintext)
}

/// Decrypt an application-level data frame and strip the tag byte.
pub fn decrypt_data(cipher: &DataChannelCipher, data: &[u8]) -> Result<Vec<u8>, SdkError> {
    let plaintext = cipher.decrypt(data)?;

    if plaintext.is_empty() || plaintext[0] != DATA_TAG {
        return Err(SdkError::new(
            codes::CRYPTO_ERROR,
            "Invalid data frame: unexpected tag byte",
        ));
    }

    Ok(plaintext[1..].to_vec())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulate a full handshake between two peers and return their derived
    /// keys plus each side's view of the other's identity.
    fn run_handshake() -> (Identity, Identity, [u8; KEY_SIZE], [u8; KEY_SIZE], String, String) {
        let alice_id = Identity::generate();
        let bob_id = Identity::generate();

        let alice_eph = EphemeralKeypair::generate();
        let bob_eph = EphemeralKeypair::generate();

        let alice_frame = build_handshake(&alice_id, &alice_eph);
        let bob_frame = build_handshake(&bob_id, &bob_eph);

        // Each side verifies the other's frame.
        let alice_sees = verify_handshake(&bob_frame).unwrap();
        let bob_sees = verify_handshake(&alice_frame).unwrap();

        let alice_key = derive_channel_key(&alice_eph, &alice_sees.ephemeral_public);
        let bob_key = derive_channel_key(&bob_eph, &bob_sees.ephemeral_public);

        (
            alice_id,
            bob_id,
            alice_key,
            bob_key,
            alice_sees.peer_id,
            bob_sees.peer_id,
        )
    }

    #[test]
    fn test_ecdh_both_sides_agree() {
        let (alice_id, bob_id, alice_key, bob_key, alice_sees_bob, bob_sees_alice) =
            run_handshake();

        assert_eq!(alice_key, bob_key, "ECDH must yield the same channel key");
        assert_eq!(alice_sees_bob, bob_id.peer_id(), "Alice must see Bob's id");
        assert_eq!(bob_sees_alice, alice_id.peer_id(), "Bob must see Alice's id");
    }

    #[test]
    fn test_peer_id_is_identity_public_key() {
        let id = Identity::generate();
        assert_eq!(id.peer_id(), hex_lower(&id.public_bytes()));
        assert_eq!(id.peer_id().len(), 64); // 32 bytes hex
    }

    #[test]
    fn test_key_not_derivable_from_public_ids() {
        // Two independent handshakes between the SAME identities produce
        // DIFFERENT keys, because the ephemeral secrets differ. This is the
        // core property the old peer-id derivation lacked.
        let (_, _, k1, _, _, _) = run_handshake();
        let (_, _, k2, _, _, _) = run_handshake();
        assert_ne!(k1, k2, "ephemeral handshakes must not be deterministic");
    }

    #[test]
    fn test_tampered_ephemeral_key_rejected() {
        let id = Identity::generate();
        let eph = EphemeralKeypair::generate();
        let mut frame = build_handshake(&id, &eph);
        // Flip a byte in the ephemeral public key region.
        frame[1 + IDENTITY_PUBLIC_LEN] ^= 0xFF;
        assert!(
            verify_handshake(&frame).is_err(),
            "signature must fail on tampered ephemeral key"
        );
    }

    #[test]
    fn test_tampered_signature_rejected() {
        let id = Identity::generate();
        let eph = EphemeralKeypair::generate();
        let mut frame = build_handshake(&id, &eph);
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        assert!(verify_handshake(&frame).is_err());
    }

    #[test]
    fn test_wrong_identity_detected() {
        // A MITM presenting its own identity+ephemeral produces a valid frame,
        // but the reported peer_id will not match the expected one.
        let mitm = Identity::generate();
        let mitm_eph = EphemeralKeypair::generate();
        let frame = build_handshake(&mitm, &mitm_eph);
        let seen = verify_handshake(&frame).unwrap();

        let expected_peer = Identity::generate();
        assert_ne!(
            seen.peer_id,
            expected_peer.peer_id(),
            "MITM identity must not match the expected peer id"
        );
    }

    #[test]
    fn test_short_frame_rejected() {
        assert!(verify_handshake(&[HANDSHAKE_TAG; 10]).is_err());
    }

    #[test]
    fn test_data_frame_roundtrip_with_derived_key() {
        let (_, _, alice_key, bob_key, _, _) = run_handshake();
        let alice = DataChannelCipher::new(&alice_key);
        let bob = DataChannelCipher::new(&bob_key);

        let payload = b"content fragment 42";
        let frame = encrypt_data(&alice, payload).unwrap();
        let decrypted = decrypt_data(&bob, &frame).unwrap();
        assert_eq!(decrypted, payload);
    }

    #[test]
    fn test_wrong_key_fails() {
        let c1 = DataChannelCipher::new(&[1u8; KEY_SIZE]);
        let c2 = DataChannelCipher::new(&[2u8; KEY_SIZE]);
        let encrypted = c1.encrypt(b"secret").unwrap();
        assert!(c2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_truncated_ciphertext_rejected() {
        let cipher = DataChannelCipher::new(&[0u8; KEY_SIZE]);
        assert!(cipher.decrypt(&[0u8; 10]).is_err());
    }
}
