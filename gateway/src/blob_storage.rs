use protocol::StorageLayer;
use std::sync::Arc;

const MAX_BLOB_SIZE: usize = 100 * 1024 * 1024; // 100 MB

pub struct BlobStorage {
    storage: Arc<StorageLayer>,
}

impl BlobStorage {
    pub fn new(storage: Arc<StorageLayer>) -> Self {
        Self { storage }
    }

    /// Store a blob, returns its CID. Max 100MB.
    pub async fn put(&self, data: &[u8]) -> Result<String, String> {
        if data.len() > MAX_BLOB_SIZE {
            return Err(format!("Blob too large: {} bytes (max {}MB)", data.len(), MAX_BLOB_SIZE / (1024 * 1024)));
        }
        let cid = self
            .storage
            .store_content(data.to_vec())
            .await
            .map_err(|e| format!("Blob storage failed: {}", e))?;
        Ok(cid)
    }

    /// Retrieve a blob by CID.
    pub async fn get(&self, cid: &str) -> Result<Vec<u8>, String> {
        self.storage
            .retrieve_content(cid)
            .await
            .map_err(|e| format!("Blob retrieval failed: {}", e))
    }

    /// Check if a blob exists.
    pub async fn exists(&self, cid: &str) -> bool {
        self.storage.retrieve_content(cid).await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_size_limit_constant() {
        assert_eq!(MAX_BLOB_SIZE, 100 * 1024 * 1024);
    }
}
