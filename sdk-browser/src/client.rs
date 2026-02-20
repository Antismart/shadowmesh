//! ShadowMesh browser client

use crate::error::{codes, SdkError};
use crate::signaling::{PeerInfo, SignalingClient, SignalingMessage};
use crate::utils::generate_session_id;
use crate::webrtc::WebRtcConnection;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// Fragment size in bytes (256 KB)
const FRAGMENT_SIZE: usize = 256 * 1024;

/// Content request message sent over WebRTC data channel
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentRequest {
    request_type: String,
    cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment_index: Option<u32>,
}

/// Content response message received over WebRTC data channel
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentResponse {
    response_type: String,
    cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_fragments: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>, // Base64 encoded
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Client configuration
#[wasm_bindgen]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Signaling server WebSocket URL
    signaling_url: String,

    /// Gateway URL for HTTP fallback
    gateway_url: Option<String>,

    /// STUN servers for NAT traversal
    stun_servers: Vec<String>,

    /// Maximum peers to connect to
    max_peers: usize,

    /// Connection timeout in milliseconds
    timeout_ms: u32,

    /// Use HTTP fallback if P2P fails
    use_fallback: bool,
}

#[wasm_bindgen]
impl ClientConfig {
    /// Create a new client configuration
    #[wasm_bindgen(constructor)]
    pub fn new(signaling_url: &str) -> Self {
        Self {
            signaling_url: signaling_url.to_string(),
            gateway_url: None,
            stun_servers: vec![
                // ShadowMesh-operated STUN (IP-only, no DNS dependency)
                "stun:45.33.32.156:3478".to_string(),
                "stun:178.62.8.237:3478".to_string(),
                // Fallback (DNS-based)
                "stun:stun.l.google.com:19302".to_string(),
            ],
            max_peers: 5,
            timeout_ms: 30000,
            use_fallback: true,
        }
    }

    /// Set gateway URL for HTTP fallback
    #[wasm_bindgen(js_name = setGatewayUrl)]
    pub fn set_gateway_url(&mut self, url: &str) {
        self.gateway_url = Some(url.to_string());
    }

    /// Add a STUN server
    #[wasm_bindgen(js_name = addStunServer)]
    pub fn add_stun_server(&mut self, server: &str) {
        self.stun_servers.push(server.to_string());
    }

    /// Set maximum peers
    #[wasm_bindgen(js_name = setMaxPeers)]
    pub fn set_max_peers(&mut self, max: usize) {
        self.max_peers = max;
    }

    /// Set connection timeout
    #[wasm_bindgen(js_name = setTimeout)]
    pub fn set_timeout(&mut self, timeout_ms: u32) {
        self.timeout_ms = timeout_ms;
    }

    /// Enable/disable HTTP fallback
    #[wasm_bindgen(js_name = setUseFallback)]
    pub fn set_use_fallback(&mut self, use_fallback: bool) {
        self.use_fallback = use_fallback;
    }
}

/// Pending content fetch state
struct PendingFetch {
    cid: String,
    fragments: HashMap<u32, Vec<u8>>,
    total_fragments: Option<u32>,
}

/// ShadowMesh browser client
#[wasm_bindgen]
pub struct ShadowMeshClient {
    config: ClientConfig,
    peer_id: String,
    signaling: Rc<RefCell<Option<SignalingClient>>>,
    connections: Rc<RefCell<HashMap<String, WebRtcConnection>>>,
    available_peers: Rc<RefCell<Vec<PeerInfo>>>,
    connected: Rc<RefCell<bool>>,
    pending_fetches: Rc<RefCell<HashMap<String, PendingFetch>>>,
}

#[wasm_bindgen]
impl ShadowMeshClient {
    /// Create a new ShadowMesh client
    #[wasm_bindgen(constructor)]
    pub fn new(config: ClientConfig) -> Result<ShadowMeshClient, JsValue> {
        let peer_id = generate_session_id();

        Ok(Self {
            config,
            peer_id,
            signaling: Rc::new(RefCell::new(None)),
            connections: Rc::new(RefCell::new(HashMap::new())),
            available_peers: Rc::new(RefCell::new(Vec::new())),
            connected: Rc::new(RefCell::new(false)),
            pending_fetches: Rc::new(RefCell::new(HashMap::new())),
        })
    }

    /// Get the local peer ID
    #[wasm_bindgen(getter, js_name = peerId)]
    pub fn peer_id(&self) -> String {
        self.peer_id.clone()
    }

    /// Check if connected to the network
    #[wasm_bindgen(getter, js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        *self.connected.borrow()
    }

    /// Get the number of connected peers
    #[wasm_bindgen(getter, js_name = peerCount)]
    pub fn peer_count(&self) -> usize {
        self.connections.borrow().len()
    }

    /// Connect to the ShadowMesh network
    #[wasm_bindgen]
    pub async fn connect(&self) -> Result<(), JsValue> {
        tracing::info!("Connecting to ShadowMesh network...");

        // Create signaling client
        let signaling = SignalingClient::new(&self.config.signaling_url, &self.peer_id);

        // Connect to signaling server
        signaling
            .connect()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Announce presence
        signaling
            .announce()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Store signaling client
        *self.signaling.borrow_mut() = Some(signaling);
        *self.connected.borrow_mut() = true;

        tracing::info!("Connected to ShadowMesh network as {}", self.peer_id);

        Ok(())
    }

    /// Discover available peers
    #[wasm_bindgen(js_name = discoverPeers)]
    pub async fn discover_peers(&self) -> Result<JsValue, JsValue> {
        let signaling = self.signaling.borrow();
        let signaling = signaling
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Not connected"))?;

        signaling
            .discover(self.config.max_peers)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Return current known peers
        let peers = self.available_peers.borrow();
        serde_wasm_bindgen::to_value(&*peers).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Connect to a specific peer via WebRTC
    #[wasm_bindgen(js_name = connectToPeer)]
    pub async fn connect_to_peer(&self, peer_id: &str) -> Result<(), JsValue> {
        tracing::info!("Connecting to peer: {}", peer_id);

        // Check if already connected
        if self.connections.borrow().contains_key(peer_id) {
            return Ok(());
        }

        // Create WebRTC connection
        let conn = WebRtcConnection::new(&self.config.stun_servers)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Set up ICE candidate handler
        let signaling = self.signaling.clone();
        let target_peer = peer_id.to_string();
        let session_id = generate_session_id();
        let session_id_clone = session_id.clone();

        conn.on_ice_candidate(move |candidate, sdp_mid, sdp_mline_index| {
            if let Some(ref sig) = *signaling.borrow() {
                let _ = sig.send_ice_candidate(
                    &target_peer,
                    &candidate,
                    sdp_mid.as_deref(),
                    sdp_mline_index,
                    &session_id_clone,
                );
            }
        });

        // Create offer
        let offer_sdp = conn
            .create_offer()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Send offer via signaling
        {
            let signaling = self.signaling.borrow();
            let signaling = signaling
                .as_ref()
                .ok_or_else(|| JsValue::from_str("Not connected"))?;

            signaling
                .send_offer(peer_id, &offer_sdp, &session_id)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
        }

        // Store connection
        self.connections
            .borrow_mut()
            .insert(peer_id.to_string(), conn);

        tracing::info!("Initiated connection to peer: {}", peer_id);

        Ok(())
    }

    /// Fetch content by CID using P2P or HTTP fallback
    #[wasm_bindgen]
    pub async fn fetch(&self, cid: &str) -> Result<js_sys::Uint8Array, JsValue> {
        tracing::info!("Fetching content: {}", cid);

        if !*self.connected.borrow() {
            return Err(JsValue::from_str("Not connected to network"));
        }

        // Try P2P fetch first
        match self.fetch_p2p(cid).await {
            Ok(data) => {
                tracing::info!("Content fetched via P2P: {} bytes", data.len());
                let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
                array.copy_from(&data);
                return Ok(array);
            }
            Err(e) => {
                tracing::warn!("P2P fetch failed: {:?}, trying fallback", e);
            }
        }

        // Try HTTP fallback if enabled
        if self.config.use_fallback {
            if let Some(ref gateway_url) = self.config.gateway_url {
                match self.fetch_http(gateway_url, cid).await {
                    Ok(data) => {
                        tracing::info!("Content fetched via HTTP: {} bytes", data.len());
                        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
                        array.copy_from(&data);
                        return Ok(array);
                    }
                    Err(e) => {
                        tracing::error!("HTTP fallback failed: {:?}", e);
                    }
                }
            }
        }

        Err(JsValue::from_str("Failed to fetch content from any source"))
    }

    /// Fetch content via HTTP gateway
    #[wasm_bindgen(js_name = fetchHttp)]
    pub async fn fetch_http_public(
        &self,
        gateway_url: &str,
        cid: &str,
    ) -> Result<js_sys::Uint8Array, JsValue> {
        let data = self
            .fetch_http(gateway_url, cid)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(&data);
        Ok(array)
    }

    /// Disconnect from the network
    #[wasm_bindgen]
    pub fn disconnect(&self) {
        tracing::info!("Disconnecting from ShadowMesh network...");

        // Close all WebRTC connections
        for (_, conn) in self.connections.borrow().iter() {
            conn.close();
        }
        self.connections.borrow_mut().clear();

        // Disconnect from signaling
        if let Some(signaling) = self.signaling.borrow().as_ref() {
            signaling.disconnect();
        }
        *self.signaling.borrow_mut() = None;

        *self.connected.borrow_mut() = false;

        tracing::info!("Disconnected from ShadowMesh network");
    }

    /// Get SDK version
    #[wasm_bindgen]
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

// Private implementation methods
impl ShadowMeshClient {
    /// Fetch content via P2P WebRTC
    async fn fetch_p2p(&self, cid: &str) -> Result<Vec<u8>, SdkError> {
        // Check if we have any connected peers
        let connections = self.connections.borrow();
        if connections.is_empty() {
            return Err(SdkError::new(codes::NOT_CONNECTED, "No connected peers"));
        }

        // Create content request
        let request = ContentRequest {
            request_type: "content_request".to_string(),
            cid: cid.to_string(),
            fragment_index: None, // Request all fragments
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| SdkError::new(codes::SIGNALING_ERROR, &e.to_string()))?;

        // Send request to first available peer
        for (peer_id, conn) in connections.iter() {
            if conn.is_connected() {
                tracing::info!("Requesting content from peer: {}", peer_id);
                conn.send(request_json.as_bytes())
                    .map_err(|e| SdkError::new(codes::WEBRTC_ERROR, &e.to_string()))?;

                // Wait for response (simplified - in production would use async channels)
                // For now, return error as full implementation requires message handling loop
                return Err(SdkError::new(
                    codes::TIMEOUT,
                    "P2P content fetching requires message handling - use HTTP fallback",
                ));
            }
        }

        Err(SdkError::new(codes::NOT_CONNECTED, "No connected peers"))
    }

    /// Fetch content via HTTP gateway
    async fn fetch_http(&self, gateway_url: &str, cid: &str) -> Result<Vec<u8>, SdkError> {
        let url = format!("{}/ipfs/{}", gateway_url.trim_end_matches('/'), cid);

        // Use web_sys fetch API
        let window = web_sys::window()
            .ok_or_else(|| SdkError::new(codes::WEBRTC_ERROR, "No window object"))?;

        let promise = window.fetch_with_str(&url);
        let response = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| SdkError::new(codes::CONNECTION_FAILED, &format!("{:?}", e)))?;

        let response: web_sys::Response = response
            .dyn_into()
            .map_err(|_| SdkError::new(codes::CONNECTION_FAILED, "Invalid response"))?;

        if !response.ok() {
            return Err(SdkError::new(
                codes::CONTENT_NOT_FOUND,
                &format!("HTTP {}", response.status()),
            ));
        }

        let array_buffer = wasm_bindgen_futures::JsFuture::from(
            response
                .array_buffer()
                .map_err(|_| SdkError::new(codes::CONNECTION_FAILED, "Failed to get body"))?,
        )
        .await
        .map_err(|e| SdkError::new(codes::CONNECTION_FAILED, &format!("{:?}", e)))?;

        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        Ok(uint8_array.to_vec())
    }

    /// Verify content hash using BLAKE3
    fn verify_content(&self, data: &[u8], expected_cid: &str) -> bool {
        let hash = blake3::hash(data);
        let computed_cid = format!("baf{}", hex_encode(&hash.as_bytes()[..32]));
        computed_cid.starts_with(&expected_cid[..expected_cid.len().min(10)])
    }
}

impl Drop for ShadowMeshClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Simple hex encoding
fn hex_encode(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX_CHARS[(byte >> 4) as usize] as char);
        result.push(HEX_CHARS[(byte & 0x0f) as usize] as char);
    }
    result
}
