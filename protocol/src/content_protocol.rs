//! ShadowMesh Content Serving Protocol
//!
//! Defines request/response messages and a length-prefixed JSON codec
//! for P2P fragment exchange between ShadowMesh nodes.

use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};

/// Protocol identifier.
pub const CONTENT_PROTOCOL: StreamProtocol = StreamProtocol::new("/shadowmesh/content/1.0.0");

/// Maximum wire message size (512 KB — covers 256 KB fragments + base64/JSON overhead).
const MAX_MESSAGE_SIZE: usize = 512 * 1024;

// ─── Request types ───────────────────────────────────────────────

/// A request sent to a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentRequest {
    /// Request a single fragment by its BLAKE3 hash.
    GetFragment { fragment_hash: String },

    /// Request the manifest (fragment list) for a content item.
    GetManifest { content_hash: String },

    /// Liveness probe.
    Ping,
}

// ─── Response types ──────────────────────────────────────────────

/// A response to a `ContentRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentResponse {
    /// Fragment data.
    Fragment {
        fragment_hash: String,
        data: Vec<u8>,
    },

    /// Content manifest.
    Manifest {
        content_hash: String,
        fragment_hashes: Vec<String>,
        total_size: u64,
        mime_type: String,
    },

    /// Pong reply.
    Pong,

    /// The requested content was not found on this peer.
    NotFound { key: String },

    /// An error occurred while processing the request.
    Error { message: String },
}

// ─── Codec ───────────────────────────────────────────────────────

/// Length-prefixed JSON codec for the content protocol.
///
/// Wire format: `[4-byte big-endian length][JSON payload]`
#[derive(Debug, Clone, Default)]
pub struct ContentCodec;

#[async_trait::async_trait]
impl libp2p::request_response::Codec for ContentCodec {
    type Protocol = StreamProtocol;
    type Request = ContentRequest;
    type Response = ContentResponse;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Request>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        let buf = read_length_prefixed(io).await?;
        serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> std::io::Result<Self::Response>
    where
        T: futures::AsyncRead + Unpin + Send,
    {
        let buf = read_length_prefixed(io).await?;
        serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        let bytes = serde_json::to_vec(&req)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_length_prefixed(io, &bytes).await
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> std::io::Result<()>
    where
        T: futures::AsyncWrite + Unpin + Send,
    {
        let bytes = serde_json::to_vec(&res)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_length_prefixed(io, &bytes).await
    }
}

/// Read a 4-byte big-endian length prefix, then read that many bytes.
async fn read_length_prefixed<T>(io: &mut T) -> std::io::Result<Vec<u8>>
where
    T: futures::AsyncRead + Unpin + Send,
{
    use futures::AsyncReadExt;

    let mut len_buf = [0u8; 4];
    io.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > MAX_MESSAGE_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("message too large: {} bytes (max {})", len, MAX_MESSAGE_SIZE),
        ));
    }

    let mut buf = vec![0u8; len];
    io.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Write a 4-byte big-endian length prefix followed by the payload.
async fn write_length_prefixed<T>(io: &mut T, data: &[u8]) -> std::io::Result<()>
where
    T: futures::AsyncWrite + Unpin + Send,
{
    use futures::AsyncWriteExt;

    let len = data.len() as u32;
    io.write_all(&len.to_be_bytes()).await?;
    io.write_all(data).await?;
    io.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let requests = vec![
            ContentRequest::GetFragment {
                fragment_hash: "abc123".to_string(),
            },
            ContentRequest::GetManifest {
                content_hash: "def456".to_string(),
            },
            ContentRequest::Ping,
        ];

        for req in requests {
            let bytes = serde_json::to_vec(&req).unwrap();
            let decoded: ContentRequest = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(format!("{:?}", req), format!("{:?}", decoded));
        }
    }

    #[test]
    fn response_roundtrip() {
        let responses = vec![
            ContentResponse::Fragment {
                fragment_hash: "abc123".to_string(),
                data: vec![1, 2, 3, 4],
            },
            ContentResponse::Manifest {
                content_hash: "root".to_string(),
                fragment_hashes: vec!["a".to_string(), "b".to_string()],
                total_size: 1024,
                mime_type: "text/html".to_string(),
            },
            ContentResponse::Pong,
            ContentResponse::NotFound {
                key: "missing".to_string(),
            },
            ContentResponse::Error {
                message: "oops".to_string(),
            },
        ];

        for resp in responses {
            let bytes = serde_json::to_vec(&resp).unwrap();
            let decoded: ContentResponse = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(format!("{:?}", resp), format!("{:?}", decoded));
        }
    }
}
