//! Cryptographic primitives for ShadowMesh
//! 
//! Provides encryption, decryption, and key management for:
//! - Fragment encryption (content privacy)
//! - Onion routing layers (multi-hop privacy)
//! - Peer authentication

use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

/// Size of encryption keys (256 bits)
pub const KEY_SIZE: usize = 32;

/// Size of nonces (96 bits for ChaCha20Poly1305)
pub const NONCE_SIZE: usize = 12;

/// Custom error type for crypto operations
#[derive(Debug)]
pub enum CryptoError {
    EncryptionFailed(String),
    DecryptionFailed(String),
    InvalidKeySize,
    InvalidNonce,
    InvalidCiphertext,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CryptoError::EncryptionFailed(msg) => write!(f, "Encryption failed: {}", msg),
            CryptoError::DecryptionFailed(msg) => write!(f, "Decryption failed: {}", msg),
            CryptoError::InvalidKeySize => write!(f, "Invalid key size"),
            CryptoError::InvalidNonce => write!(f, "Invalid nonce"),
            CryptoError::InvalidCiphertext => write!(f, "Invalid ciphertext format"),
        }
    }
}

impl Error for CryptoError {}

/// Encrypted data with embedded nonce
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    /// Nonce used for encryption (12 bytes)
    pub nonce: Vec<u8>,
    /// Encrypted ciphertext
    pub ciphertext: Vec<u8>,
}

/// Cryptographic key manager
pub struct CryptoManager {
    cipher: ChaCha20Poly1305,
}

impl CryptoManager {
    /// Create a new crypto manager with the given key
    pub fn new(key: &[u8; KEY_SIZE]) -> Self {
        let key = Key::from_slice(key);
        let cipher = ChaCha20Poly1305::new(key);
        
        Self { cipher }
    }
    
    /// Create a new crypto manager with a random key
    pub fn new_random() -> (Self, [u8; KEY_SIZE]) {
        let key = ChaCha20Poly1305::generate_key(&mut OsRng);
        let key_bytes: [u8; KEY_SIZE] = key.as_slice().try_into().unwrap();
        
        (Self::new(&key_bytes), key_bytes)
    }
    
    /// Encrypt data and return EncryptedData structure
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedData, CryptoError> {
        // Generate random nonce
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        
        // Encrypt
        let ciphertext = self.cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;
        
        Ok(EncryptedData {
            nonce: nonce.to_vec(),
            ciphertext,
        })
    }
    
    /// Decrypt EncryptedData structure
    pub fn decrypt(&self, encrypted: &EncryptedData) -> Result<Vec<u8>, CryptoError> {
        // Validate nonce size
        if encrypted.nonce.len() != NONCE_SIZE {
            return Err(CryptoError::InvalidNonce);
        }
        
        // Convert to Nonce type
        let nonce = Nonce::from_slice(&encrypted.nonce);
        
        // Decrypt
        let plaintext = self.cipher
            .decrypt(nonce, encrypted.ciphertext.as_ref())
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;
        
        Ok(plaintext)
    }
    
    /// Encrypt data and return raw bytes (nonce prepended to ciphertext)
    /// 
    /// Format: [nonce (12 bytes)][ciphertext]
    pub fn encrypt_raw(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let encrypted = self.encrypt(plaintext)?;
        
        // Prepend nonce to ciphertext
        let mut result = encrypted.nonce;
        result.extend(encrypted.ciphertext);
        
        Ok(result)
    }
    
    /// Decrypt raw bytes (expecting nonce prepended format)
    pub fn decrypt_raw(&self, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if data.len() < NONCE_SIZE {
            return Err(CryptoError::InvalidCiphertext);
        }
        
        let encrypted = EncryptedData {
            nonce: data[..NONCE_SIZE].to_vec(),
            ciphertext: data[NONCE_SIZE..].to_vec(),
        };
        
        self.decrypt(&encrypted)
    }
}

/// Onion routing layer for multi-hop encryption
pub struct OnionRouter {
    layers: Vec<CryptoManager>,
}

impl OnionRouter {
    /// Create a new onion router with the given keys (one per hop)
    pub fn new(keys: &[[u8; KEY_SIZE]]) -> Self {
        let layers = keys.iter()
            .map(|key| CryptoManager::new(key))
            .collect();
        
        Self { layers }
    }
    
    /// Wrap data in multiple encryption layers (for sending)
    /// 
    /// Encrypts from innermost to outermost layer
    pub fn wrap(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut data = plaintext.to_vec();
        
        // Encrypt in reverse order (innermost first)
        for manager in self.layers.iter().rev() {
            data = manager.encrypt_raw(&data)?;
        }
        
        Ok(data)
    }
    
    /// Unwrap one layer of encryption (for relaying)
    /// 
    /// Returns the decrypted data and the index of the layer that was unwrapped
    pub fn unwrap_layer(&self, data: &[u8], layer: usize) -> Result<Vec<u8>, CryptoError> {
        if layer >= self.layers.len() {
            return Err(CryptoError::DecryptionFailed("Invalid layer index".to_string()));
        }
        
        self.layers[layer].decrypt_raw(data)
    }
    
    /// Fully unwrap all layers (for receiving at destination)
    pub fn unwrap_all(&self, mut data: Vec<u8>) -> Result<Vec<u8>, CryptoError> {
        // Decrypt from outermost to innermost
        for manager in &self.layers {
            data = manager.decrypt_raw(&data)?;
        }
        
        Ok(data)
    }
}

/// Key derivation from a master secret
pub struct KeyDerivation;

impl KeyDerivation {
    /// Derive a key from a password/secret using BLAKE3
    pub fn derive_key(secret: &[u8], context: &str) -> [u8; KEY_SIZE] {
        let mut hasher = Hasher::new();
        hasher.update(context.as_bytes());
        hasher.update(secret);
        
        let hash = hasher.finalize();
        let mut key = [0u8; KEY_SIZE];
        key.copy_from_slice(&hash.as_bytes()[..KEY_SIZE]);
        
        key
    }
    
    /// Derive multiple keys for onion routing
    pub fn derive_route_keys(secret: &[u8], num_hops: usize) -> Vec<[u8; KEY_SIZE]> {
        (0..num_hops)
            .map(|i| {
                let context = format!("shadowmesh-hop-{}", i);
                Self::derive_key(secret, &context)
            })
            .collect()
    }
}

/// Hash content using BLAKE3
pub fn hash_content(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}

/// Verify content hash
pub fn verify_content_hash(data: &[u8], expected_hash: &str) -> bool {
    let actual_hash = hash_content(data);
    actual_hash == expected_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_decryption() {
        let key = [0u8; KEY_SIZE];
        let manager = CryptoManager::new(&key);
        
        let plaintext = b"Hello, ShadowMesh!";
        
        // Encrypt
        let encrypted = manager.encrypt(plaintext).unwrap();
        assert_ne!(encrypted.ciphertext, plaintext);
        
        // Decrypt
        let decrypted = manager.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }
    
    #[test]
    fn test_encryption_raw() {
        let key = [1u8; KEY_SIZE];
        let manager = CryptoManager::new(&key);
        
        let plaintext = b"Raw format test";
        
        let encrypted = manager.encrypt_raw(plaintext).unwrap();
        let decrypted = manager.decrypt_raw(&encrypted).unwrap();
        
        assert_eq!(decrypted, plaintext);
    }
    
    #[test]
    fn test_random_key() {
        let (manager, _key) = CryptoManager::new_random();
        
        let plaintext = b"Random key test";
        let encrypted = manager.encrypt(plaintext).unwrap();
        let decrypted = manager.decrypt(&encrypted).unwrap();
        
        assert_eq!(decrypted, plaintext);
    }
    
    #[test]
    fn test_onion_routing() {
        // Generate 3 keys for 3 hops
        let keys = [
            [0u8; KEY_SIZE],
            [1u8; KEY_SIZE],
            [2u8; KEY_SIZE],
        ];
        
        let router = OnionRouter::new(&keys);
        let plaintext = b"Secret message";
        
        // Wrap in all layers
        let wrapped = router.wrap(plaintext).unwrap();
        assert_ne!(wrapped, plaintext);
        
        // Unwrap all layers
        let unwrapped = router.unwrap_all(wrapped).unwrap();
        assert_eq!(unwrapped, plaintext);
    }
    
    #[test]
    fn test_onion_layer_unwrap() {
        let keys = [
            [0u8; KEY_SIZE],
            [1u8; KEY_SIZE],
        ];
        
        let router = OnionRouter::new(&keys);
        let plaintext = b"Layer test";
        
        // Wrap
        let wrapped = router.wrap(plaintext).unwrap();
        
        // Unwrap first layer (outermost)
        let layer1 = router.unwrap_layer(&wrapped, 0).unwrap();
        
        // Unwrap second layer
        let layer2 = router.unwrap_layer(&layer1, 1).unwrap();
        
        assert_eq!(layer2, plaintext);
    }
    
    #[test]
    fn test_key_derivation() {
        let secret = b"my-master-secret";
        
        let key1 = KeyDerivation::derive_key(secret, "context-1");
        let key2 = KeyDerivation::derive_key(secret, "context-2");
        
        // Same secret, different context = different keys
        assert_ne!(key1, key2);
        
        // Same secret, same context = same key
        let key1_again = KeyDerivation::derive_key(secret, "context-1");
        assert_eq!(key1, key1_again);
    }
    
    #[test]
    fn test_derive_route_keys() {
        let secret = b"route-secret";
        let keys = KeyDerivation::derive_route_keys(secret, 5);
        
        assert_eq!(keys.len(), 5);
        
        // All keys should be different
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(keys[i], keys[j]);
            }
        }
    }
    
    #[test]
    fn test_content_hashing() {
        let data = b"Content to hash";
        
        let hash1 = hash_content(data);
        let hash2 = hash_content(data);
        
        // Same data = same hash
        assert_eq!(hash1, hash2);
        
        // Verify hash
        assert!(verify_content_hash(data, &hash1));
        
        // Different data = different hash
        let hash3 = hash_content(b"Different content");
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_decryption_with_wrong_key() {
        let key1 = [0u8; KEY_SIZE];
        let key2 = [1u8; KEY_SIZE];
        
        let manager1 = CryptoManager::new(&key1);
        let manager2 = CryptoManager::new(&key2);
        
        let plaintext = b"Secret data";
        let encrypted = manager1.encrypt(plaintext).unwrap();
        
        // Try to decrypt with wrong key
        let result = manager2.decrypt(&encrypted);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_tampered_ciphertext() {
        let key = [0u8; KEY_SIZE];
        let manager = CryptoManager::new(&key);
        
        let plaintext = b"Original message";
        let mut encrypted = manager.encrypt(plaintext).unwrap();
        
        // Tamper with ciphertext
        if let Some(byte) = encrypted.ciphertext.first_mut() {
            *byte ^= 0xFF; // Flip bits
        }
        
        // Decryption should fail
        let result = manager.decrypt(&encrypted);
        assert!(result.is_err());
    }
}
