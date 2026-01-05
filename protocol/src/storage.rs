use ipfs_api_backend_hyper::{IpfsApi, IpfsClient};
use std::io::Cursor;
use futures::TryStreamExt;

pub struct StorageLayer {
    ipfs_client: IpfsClient,
}

impl StorageLayer {
    /// Create a new StorageLayer with default IPFS configuration (localhost:5001)
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_url("http://127.0.0.1:5001").await
    }

    /// Create a new StorageLayer with a custom IPFS API URL
    pub async fn with_url(api_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let client = if api_url == "http://127.0.0.1:5001" || api_url == "http://localhost:5001" {
            IpfsClient::default()
        } else {
            // Parse the URL and create client with custom address
            let url = api_url.parse::<http::Uri>()?;
            let host = url.host().unwrap_or("127.0.0.1");
            let port = url.port_u16().unwrap_or(5001);
            IpfsClient::from_host_and_port(
                url.scheme_str().unwrap_or("http"),
                host,
                port
            )?
        };

        match client.version().await {
            Ok(version) => {
                println!("✓ Connected to IPFS version: {}", version.version);
            }
            Err(e) => {
                println!("✗ IPFS version check failed: {}", e);
            }
        }

        Ok(Self { ipfs_client: client })
    }
    
    /// Upload content to IPFS
    pub async fn store_content(&self, data: Vec<u8>) -> Result<String, Box<dyn std::error::Error>> {
        let cursor = Cursor::new(data);
        let response = self.ipfs_client.add(cursor).await?;
        
        Ok(response.hash)
    }
    
    /// Retrieve content from IPFS
    pub async fn retrieve_content(&self, cid: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        println!("→ Attempting to retrieve CID: {}", cid);

        let client = self.ipfs_client.clone();
        let cid = cid.to_string();

        // Use block_in_place to handle the non-Send IPFS stream
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                // Collect all chunks
                let chunks: Vec<_> = client
                    .cat(&cid)
                    .try_collect()
                    .await
                    .map_err(|e| {
                println!("✗ IPFS cat error: {}", e);
                e
            })?;

                // Concatenate all chunks into single Vec
                let data: Vec<u8> = chunks.into_iter().flatten().collect();
                println!("✓ Retrieved {} bytes", data.len());


                Ok::<Vec<u8>, Box<dyn std::error::Error>>(data)
            })
        })?;

        Ok(result)
    }
}