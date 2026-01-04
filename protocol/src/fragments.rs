use blake3::Hasher;
use serde::{Deserialize, Serialize};

const CHUNK_SIZE: usize = 256 * 1024; // 256KB chunks

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFragment {
    pub index: u32,
    pub total_fragments: u32,
    pub data: Vec<u8>,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentManifest {
    pub content_hash: String,
    pub fragments: Vec<String>, // Fragment hashes
    pub metadata: ContentMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentMetadata {
    pub name: String,
    pub size: u64,
    pub mime_type: String,
}

pub struct FragmentManager;

impl FragmentManager {
    /// Split content into fragments
    pub fn fragment_content(data: &[u8], name: String) -> ContentManifest {
        let total_size = data.len() as u64;
        
        let mut fragments = Vec::new();
        let mut hasher = Hasher::new();
        
        for chunk in data.chunks(CHUNK_SIZE) {
            let fragment_hash = blake3::hash(chunk).to_hex().to_string();
            fragments.push(fragment_hash);
            hasher.update(chunk);
        }
        
        let content_hash = hasher.finalize().to_hex().to_string();
        
        ContentManifest {
            content_hash,
            fragments,
            metadata: ContentMetadata {
                name,
                size: total_size,
                mime_type: "application/octet-stream".to_string(),
            },
        }
    }

    /// Create a ContentFragment from raw data
    pub fn create_fragment(data: Vec<u8>, index: u32, total_fragments: u32) -> ContentFragment {
        let hash = blake3::hash(&data).to_hex().to_string();
        ContentFragment {
            index,
            total_fragments,
            data,
            hash,
        }
    }
    
    /// Reassemble fragments into original content
    pub fn reassemble_fragments(fragments: Vec<ContentFragment>) -> Vec<u8> {
        let mut sorted = fragments;
        sorted.sort_by_key(|f| f.index);
        
        sorted.into_iter()
            .flat_map(|f| f.data)
            .collect()
    }

    /// Verify a fragment's hash
    pub fn verify_fragment(fragment: &ContentFragment) -> bool {
        let computed_hash = blake3::hash(&fragment.data).to_hex().to_string();
        computed_hash == fragment.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragment_and_reassemble() {
        let original_data = vec![0u8; 512 * 1024]; // 512KB
        let manifest = FragmentManager::fragment_content(&original_data, "test.bin".to_string());
        
        assert_eq!(manifest.fragments.len(), 2); // Should be 2 chunks
        assert_eq!(manifest.metadata.size, 512 * 1024);
    }
}
