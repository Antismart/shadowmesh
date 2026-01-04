//! Multi-hop routing for ShadowMesh
//! 
//! Provides onion-encrypted routing for privacy-preserving content delivery.

use crate::crypto::{CryptoManager, OnionRouter, KeyDerivation};
use libp2p::PeerId;
use rand::seq::SliceRandom;

/// Route through the network with encryption
#[derive(Debug, Clone)]
pub struct Route {
    pub hops: Vec<PeerId>,
    pub encrypted_data: Vec<u8>,
}

/// Routing layer for multi-hop content delivery
pub struct RoutingLayer {
    secret: Vec<u8>,
}

impl RoutingLayer {
    /// Create a new routing layer with a secret
    pub fn new(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }
    
    /// Create a new routing layer with random secret
    pub fn new_random() -> Self {
        use rand::Rng;
        let secret: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen()).collect();
        Self { secret }
    }
    
    /// Build a route through the network
    pub fn build_route(&self, available_peers: &[PeerId], num_hops: usize) -> Vec<PeerId> {
        let mut rng = rand::thread_rng();
        let mut peers = available_peers.to_vec();
        peers.shuffle(&mut rng);
        peers.truncate(num_hops);
        peers
    }
    
    /// Encrypt data for multi-hop routing (onion encryption)
    pub fn encrypt_for_route(
        &self,
        data: &[u8],
        route: &[PeerId],
    ) -> Result<Vec<u8>, String> {
        // Derive keys for each hop
        let keys = KeyDerivation::derive_route_keys(&self.secret, route.len());
        
        // Create onion router
        let router = OnionRouter::new(&keys);
        
        // Wrap in encryption layers
        router.wrap(data)
            .map_err(|e| format!("Encryption failed: {}", e))
    }
    
    /// Decrypt one layer (for relay nodes)
    pub fn decrypt_layer(
        &self,
        data: &[u8],
        layer_index: usize,
        num_hops: usize,
    ) -> Result<Vec<u8>, String> {
        let keys = KeyDerivation::derive_route_keys(&self.secret, num_hops);
        let router = OnionRouter::new(&keys);
        
        router.unwrap_layer(data, layer_index)
            .map_err(|e| format!("Decryption failed: {}", e))
    }
    
    /// Decrypt all layers (for destination)
    pub fn decrypt_all(&self, data: &[u8], num_hops: usize) -> Result<Vec<u8>, String> {
        let keys = KeyDerivation::derive_route_keys(&self.secret, num_hops);
        let router = OnionRouter::new(&keys);
        
        router.unwrap_all(data.to_vec())
            .map_err(|e| format!("Decryption failed: {}", e))
    }
    
    /// Encrypt a single fragment (not for routing, just content privacy)
    pub fn encrypt_fragment(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let key = KeyDerivation::derive_key(&self.secret, "fragment");
        let manager = CryptoManager::new(&key);
        
        manager.encrypt_raw(data)
            .map_err(|e| format!("Fragment encryption failed: {}", e))
    }
    
    /// Decrypt a single fragment
    pub fn decrypt_fragment(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let key = KeyDerivation::derive_key(&self.secret, "fragment");
        let manager = CryptoManager::new(&key);
        
        manager.decrypt_raw(data)
            .map_err(|e| format!("Fragment decryption failed: {}", e))
    }
    
    /// Get the secret (for key sharing)
    pub fn secret(&self) -> &[u8] {
        &self.secret
    }
}

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

    #[test]
    fn test_build_route() {
        let layer = RoutingLayer::new_random();
        let peers = create_test_peers(10);
        
        let route = layer.build_route(&peers, 5);
        assert_eq!(route.len(), 5);
    }

    #[test]
    fn test_onion_encryption() {
        let layer = RoutingLayer::new(b"test-secret");
        let peers = create_test_peers(3);
        
        let plaintext = b"Secret message through the network";
        
        // Encrypt for route
        let encrypted = layer.encrypt_for_route(plaintext, &peers).unwrap();
        assert_ne!(encrypted, plaintext);
        
        // Decrypt all layers
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
    fn test_layer_by_layer_decryption() {
        let layer = RoutingLayer::new(b"layer-test-secret");
        let peers = create_test_peers(3);
        
        let plaintext = b"Multi-layer message";
        
        // Encrypt for route
        let encrypted = layer.encrypt_for_route(plaintext, &peers).unwrap();
        
        // Decrypt layer by layer
        let after_layer0 = layer.decrypt_layer(&encrypted, 0, 3).unwrap();
        let after_layer1 = layer.decrypt_layer(&after_layer0, 1, 3).unwrap();
        let after_layer2 = layer.decrypt_layer(&after_layer1, 2, 3).unwrap();
        
        assert_eq!(after_layer2, plaintext);
    }
}
