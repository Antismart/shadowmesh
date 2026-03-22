//! ChaCha20-Poly1305 encryption for WebRTC DataChannel messages.
//!
//! Every message on the wire is formatted as:
//!
//!   `[nonce: 12 bytes][ciphertext + Poly1305 tag: N + 16 bytes]`
//!
//! The symmetric key is derived deterministically from the two peer IDs so
//! both sides of a connection compute the same key without a round-trip:
//!
//!   `key = BLAKE3::derive_key("shadowmesh-datachannel-v1", sorted(peer_a || peer_b))`
//!
//! Nonces are 96-bit random values sourced from the Web Crypto API via
//! `getrandom`.

use crate::error::{codes, SdkError};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};

/// Size of a ChaCha20-Poly1305 nonce (96 bits).
pub const NONCE_SIZE: usize = 12;

/// Size of a ChaCha20-Poly1305 key (256 bits).
pub const KEY_SIZE: usize = 32;

/// Minimum valid ciphertext length: nonce + Poly1305 tag (no plaintext).
pub const MIN_ENCRYPTED_LEN: usize = NONCE_SIZE + 16;

/// Handshake message tag byte (first byte of the plaintext after decryption).
/// Used to distinguish handshake frames from data frames.
pub const HANDSHAKE_TAG: u8 = 0x01;

/// Data message tag byte.
pub const DATA_TAG: u8 = 0x02;

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derive a symmetric 256-bit key from two peer IDs.
///
/// The IDs are sorted lexicographically before concatenation so that both
/// sides of the connection arrive at the same key regardless of who
/// initiated the connection.
pub fn derive_peer_key(peer_a: &str, peer_b: &str) -> [u8; KEY_SIZE] {
    let (first, second) = if peer_a <= peer_b {
        (peer_a, peer_b)
    } else {
        (peer_b, peer_a)
    };

    // BLAKE3 keyed-derivation context string.
    let context = "shadowmesh-datachannel-v1";

    let mut material = Vec::with_capacity(first.len() + second.len());
    material.extend_from_slice(first.as_bytes());
    material.extend_from_slice(second.as_bytes());

    // blake3::derive_key produces a 32-byte output deterministically.
    blake3::derive_key(context, &material)
}

// ---------------------------------------------------------------------------
// Cipher wrapper
// ---------------------------------------------------------------------------

/// Thin wrapper around `ChaCha20Poly1305` that encrypts / decrypts
/// DataChannel frames using the `[nonce][ciphertext+tag]` wire format.
pub struct DataChannelCipher {
    cipher: ChaCha20Poly1305,
}

impl DataChannelCipher {
    /// Create a cipher from a raw 256-bit key.
    pub fn new(key: &[u8; KEY_SIZE]) -> Self {
        let key = Key::from_slice(key);
        Self {
            cipher: ChaCha20Poly1305::new(key),
        }
    }

    /// Create a cipher derived from two peer IDs (convenience constructor).
    pub fn from_peer_ids(local_id: &str, remote_id: &str) -> Self {
        let key = derive_peer_key(local_id, remote_id);
        Self::new(&key)
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

    /// Decrypt a frame produced by [`encrypt`].
    ///
    /// Expects the `[nonce (12 B)][ciphertext + tag]` wire format.
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
// Handshake helpers
// ---------------------------------------------------------------------------

/// Build an encrypted handshake frame containing our peer ID.
///
/// Wire plaintext: `[HANDSHAKE_TAG (1 B)][peer_id UTF-8 bytes]`
pub fn build_handshake(cipher: &DataChannelCipher, local_peer_id: &str) -> Result<Vec<u8>, SdkError> {
    let mut plaintext = Vec::with_capacity(1 + local_peer_id.len());
    plaintext.push(HANDSHAKE_TAG);
    plaintext.extend_from_slice(local_peer_id.as_bytes());
    cipher.encrypt(&plaintext)
}

/// Decrypt and validate a handshake frame from the remote peer.
///
/// Returns the peer ID contained in the handshake. The caller MUST verify
/// that this matches the peer ID learned from signaling.
pub fn verify_handshake(
    cipher: &DataChannelCipher,
    data: &[u8],
) -> Result<String, SdkError> {
    let plaintext = cipher.decrypt(data)?;

    if plaintext.is_empty() || plaintext[0] != HANDSHAKE_TAG {
        return Err(SdkError::new(
            codes::AUTH_FAILED,
            "Invalid handshake: missing handshake tag",
        ));
    }

    let peer_id = String::from_utf8(plaintext[1..].to_vec()).map_err(|_| {
        SdkError::new(
            codes::AUTH_FAILED,
            "Invalid handshake: peer ID is not valid UTF-8",
        )
    })?;

    if peer_id.is_empty() {
        return Err(SdkError::new(
            codes::AUTH_FAILED,
            "Invalid handshake: empty peer ID",
        ));
    }

    Ok(peer_id)
}

/// Wrap an application-level payload in an encrypted data frame.
///
/// Wire plaintext: `[DATA_TAG (1 B)][payload]`
pub fn encrypt_data(cipher: &DataChannelCipher, payload: &[u8]) -> Result<Vec<u8>, SdkError> {
    let mut plaintext = Vec::with_capacity(1 + payload.len());
    plaintext.push(DATA_TAG);
    plaintext.extend_from_slice(payload);
    cipher.encrypt(&plaintext)
}

/// Decrypt an application-level data frame and strip the tag byte.
///
/// Returns `Err` if decryption fails or the tag byte is not `DATA_TAG`.
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

    #[test]
    fn test_derive_peer_key_symmetric() {
        let k1 = derive_peer_key("alice", "bob");
        let k2 = derive_peer_key("bob", "alice");
        assert_eq!(k1, k2, "key derivation must be order-independent");
    }

    #[test]
    fn test_derive_peer_key_different_peers_different_keys() {
        let k1 = derive_peer_key("alice", "bob");
        let k2 = derive_peer_key("alice", "carol");
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let cipher = DataChannelCipher::from_peer_ids("peer-a", "peer-b");
        let plaintext = b"hello shadowmesh";

        let encrypted = cipher.encrypt(plaintext).unwrap();
        assert!(encrypted.len() >= MIN_ENCRYPTED_LEN);

        let decrypted = cipher.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let c1 = DataChannelCipher::from_peer_ids("peer-a", "peer-b");
        let c2 = DataChannelCipher::from_peer_ids("peer-a", "peer-c");

        let encrypted = c1.encrypt(b"secret").unwrap();
        assert!(c2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_truncated_ciphertext_rejected() {
        let cipher = DataChannelCipher::from_peer_ids("a", "b");
        assert!(cipher.decrypt(&[0u8; 10]).is_err());
    }

    #[test]
    fn test_handshake_roundtrip() {
        let cipher = DataChannelCipher::from_peer_ids("peer-a", "peer-b");

        let hs = build_handshake(&cipher, "peer-a").unwrap();
        let id = verify_handshake(&cipher, &hs).unwrap();
        assert_eq!(id, "peer-a");
    }

    #[test]
    fn test_data_frame_roundtrip() {
        let cipher = DataChannelCipher::from_peer_ids("x", "y");
        let payload = b"content fragment 42";

        let frame = encrypt_data(&cipher, payload).unwrap();
        let decrypted = decrypt_data(&cipher, &frame).unwrap();
        assert_eq!(decrypted, payload);
    }
}
