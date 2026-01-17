use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, TryFromUri};
use std::io::Cursor;
use std::time::Duration;
use std::path::Path;
use futures::TryStreamExt;

/// Configuration for StorageLayer
#[derive(Clone, Debug)]
pub struct StorageConfig {
    pub api_url: String,
    pub timeout: Duration,
    pub retry_attempts: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            api_url: "http://127.0.0.1:5001".to_string(),
            timeout: Duration::from_secs(30),
            retry_attempts: 3,
        }
    }
}

pub struct StorageLayer {
    ipfs_client: IpfsClient,
    config: StorageConfig,
}

impl StorageLayer {
    /// Create a new StorageLayer with default IPFS configuration (localhost:5001)
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_config(StorageConfig::default()).await
    }

    /// Create a new StorageLayer with a custom IPFS API URL
    pub async fn with_url(api_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_config(StorageConfig {
            api_url: api_url.to_string(),
            ..Default::default()
        }).await
    }

    /// Create a new StorageLayer with full configuration
    pub async fn with_config(config: StorageConfig) -> Result<Self, Box<dyn std::error::Error>> {
        // Parse custom URL using TryFromUri trait
        let client = if config.api_url == "http://127.0.0.1:5001" {
            // Use default for standard localhost connection
            IpfsClient::default()
        } else {
            // Parse custom URL
            IpfsClient::from_str(&config.api_url)
                .map_err(|e| format!("Invalid IPFS API URL '{}': {}", config.api_url, e))?
        };

        match client.version().await {
            Ok(version) => {
                println!("✓ Connected to IPFS at {} (version: {})", config.api_url, version.version);
            }
            Err(e) => {
                return Err(format!("Failed to connect to IPFS at {}: {}", config.api_url, e).into());
            }
        }

        Ok(Self { 
            ipfs_client: client,
            config,
        })
    }
    
    /// Upload content to IPFS with retry
    pub async fn store_content(&self, data: Vec<u8>) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut last_error: Option<Box<dyn std::error::Error + Send + Sync>> = None;
        
        for attempt in 1..=self.config.retry_attempts {
            let cursor = Cursor::new(data.clone());
            
            match tokio::time::timeout(
                self.config.timeout,
                self.ipfs_client.add(cursor)
            ).await {
                Ok(Ok(response)) => return Ok(response.hash),
                Ok(Err(e)) => {
                    println!("✗ IPFS add attempt {}/{} failed: {}", attempt, self.config.retry_attempts, e);
                    last_error = Some(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string()
                    )));
                }
                Err(_) => {
                    println!("✗ IPFS add attempt {}/{} timed out", attempt, self.config.retry_attempts);
                    last_error = Some(Box::new(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "IPFS operation timed out"
                    )));
                }
            }
            
            // Brief delay before retry
            if attempt < self.config.retry_attempts {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
        }
        
        Err(last_error.unwrap_or_else(|| Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Unknown error"
        ))))
    }
    
    /// Retrieve content from IPFS with timeout and retry
    pub async fn retrieve_content(&self, cid: &str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        println!("→ Attempting to retrieve CID: {}", cid);

        let mut last_error: Option<Box<dyn std::error::Error + Send + Sync>> = None;

        for attempt in 1..=self.config.retry_attempts {
            let client = self.ipfs_client.clone();
            let cid_owned = cid.to_string();
            let timeout = self.config.timeout;

            // Use block_in_place to handle the non-Send IPFS stream
            let result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async move {
                    tokio::time::timeout(timeout, async {
                        // Collect all chunks
                        let chunks: Vec<_> = client
                            .cat(&cid_owned)
                            .try_collect()
                            .await?;

                        // Concatenate all chunks into single Vec
                        let data: Vec<u8> = chunks.into_iter().flatten().collect();
                        Ok::<Vec<u8>, ipfs_api_backend_hyper::Error>(data)
                    }).await
                })
            });

            match result {
                Ok(Ok(data)) => {
                    println!("✓ Retrieved {} bytes", data.len());
                    return Ok(data);
                }
                Ok(Err(e)) => {
                    println!("✗ IPFS cat attempt {}/{} failed: {}", attempt, self.config.retry_attempts, e);
                    last_error = Some(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string()
                    )) as Box<dyn std::error::Error + Send + Sync>);
                }
                Err(_) => {
                    println!("✗ IPFS cat attempt {}/{} timed out", attempt, self.config.retry_attempts);
                    last_error = Some(Box::new(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("IPFS operation timed out after {:?}", self.config.timeout)
                    )) as Box<dyn std::error::Error + Send + Sync>);
                }
            }

            // Brief delay before retry
            if attempt < self.config.retry_attempts {
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Unknown error"
        )) as Box<dyn std::error::Error + Send + Sync>))
    }

    /// Upload a directory to IPFS from a local path
    /// Returns the root CID that can be used to access the entire directory
    pub async fn store_directory<P: AsRef<Path> + Send>(
        &self, 
        path: P
    ) -> Result<DirectoryUploadResult, Box<dyn std::error::Error + Send + Sync>> {
        let path_ref = path.as_ref();
        
        if !path_ref.exists() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Directory not found: {:?}", path_ref)
            )));
        }

        if !path_ref.is_dir() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Path is not a directory: {:?}", path_ref)
            )));
        }

        let client = self.ipfs_client.clone();
        let path_owned = path_ref.to_path_buf();
        let timeout = self.config.timeout * 5; // Allow more time for directories

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                tokio::time::timeout(timeout, async {
                    client.add_path(&path_owned).await
                }).await
            })
        });

        match result {
            Ok(Ok(responses)) => {
                // The last response is typically the root directory
                let mut files = Vec::new();
                let mut root_cid = String::new();
                let mut total_size = 0u64;

                for response in &responses {
                    files.push(UploadedFile {
                        name: response.name.clone(),
                        hash: response.hash.clone(),
                        size: response.size.parse().unwrap_or(0),
                    });
                    total_size += response.size.parse::<u64>().unwrap_or(0);
                }

                // The root is the last entry (the directory itself)
                if let Some(last) = responses.last() {
                    root_cid = last.hash.clone();
                }

                Ok(DirectoryUploadResult {
                    root_cid,
                    files,
                    total_size,
                })
            }
            Ok(Err(e)) => {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to add directory: {}", e)
                )) as Box<dyn std::error::Error + Send + Sync>)
            }
            Err(_) => {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Directory upload timed out"
                )) as Box<dyn std::error::Error + Send + Sync>)
            }
        }
    }

    /// Get the IPFS client for advanced operations
    pub fn client(&self) -> &IpfsClient {
        &self.ipfs_client
    }
}

/// Result of uploading a directory to IPFS
#[derive(Debug, Clone)]
pub struct DirectoryUploadResult {
    /// The root CID for the directory (use this to access the site)
    pub root_cid: String,
    /// List of all files uploaded
    pub files: Vec<UploadedFile>,
    /// Total size in bytes
    pub total_size: u64,
}

/// Information about an uploaded file
#[derive(Debug, Clone)]
pub struct UploadedFile {
    /// File name/path within the directory
    pub name: String,
    /// IPFS CID of this file
    pub hash: String,
    /// Size in bytes
    pub size: u64,
}