use protocol::StorageLayer;
use std::sync::Arc;

pub struct BlobStorage {
    storage: Arc<StorageLayer>,
}

impl BlobStorage {
    pub fn new(storage: Arc<StorageLayer>) -> Self {
        Self { storage }
    }

    /// Store a blob, returns its CID.
    pub async fn put(&self, data: &[u8]) -> Result<String, String> {
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
