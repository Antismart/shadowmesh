//! ShadowMesh browser client

use crate::signaling::{PeerInfo, SignalingClient};
use crate::utils::generate_session_id;
use crate::webrtc::WebRtcConnection;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// Client configuration
#[wasm_bindgen]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Signaling server WebSocket URL
    signaling_url: String,

    /// STUN servers for NAT traversal
    stun_servers: Vec<String>,

    /// Maximum peers to connect to
    max_peers: usize,

    /// Connection timeout in milliseconds
    timeout_ms: u32,
}

#[wasm_bindgen]
impl ClientConfig {
    /// Create a new client configuration
    #[wasm_bindgen(constructor)]
    pub fn new(signaling_url: &str) -> Self {
        Self {
            signaling_url: signaling_url.to_string(),
            stun_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            max_peers: 5,
            timeout_ms: 30000,
        }
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

    /// Fetch content by CID
    #[wasm_bindgen]
    pub async fn fetch(&self, cid: &str) -> Result<js_sys::Uint8Array, JsValue> {
        tracing::info!("Fetching content: {}", cid);

        if !*self.connected.borrow() {
            return Err(JsValue::from_str("Not connected to network"));
        }

        // For now, return a placeholder
        // In a full implementation, this would:
        // 1. Query DHT for content providers
        // 2. Connect to providers via WebRTC
        // 3. Request and receive content fragments
        // 4. Reassemble and verify content

        Err(JsValue::from_str("Content fetching not yet implemented"))
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

impl Drop for ShadowMeshClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}
