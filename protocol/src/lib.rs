//! ShadowMesh Protocol Library
//! 
//! Provides core functionality for the ShadowMesh decentralized CDN network.
//! 
//! # Modules
//! 
//! - `crypto` - Encryption, decryption, and key management
//! - `fragments` - Content chunking and reassembly
//! - `routing` - Multi-hop onion routing
//! - `node` - P2P network node implementation
//! - `storage` - IPFS/Filecoin integration

pub mod crypto;
pub mod fragments;
pub mod node;
pub mod routing;
pub mod storage;

// Re-export crypto types
pub use crypto::{
    CryptoManager, CryptoError, EncryptedData,
    OnionRouter, KeyDerivation,
    hash_content, verify_content_hash,
    KEY_SIZE, NONCE_SIZE,
};

// Re-export fragment types
pub use fragments::{ContentFragment, ContentManifest, ContentMetadata, FragmentManager};

// Re-export routing types
pub use routing::RoutingLayer;

// Re-export node types
pub use node::ShadowNode;

// Re-export storage types
pub use storage::{StorageLayer, StorageConfig};
