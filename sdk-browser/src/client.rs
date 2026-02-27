//! ShadowMesh browser client

use crate::error::{codes, SdkError};
use crate::signaling::{PeerInfo, SignalingClient, SignalingMessage};
use crate::utils::{console_log, generate_session_id};
use crate::webrtc::WebRtcConnection;
use base64::Engine;
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

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
                // Reliable public STUN servers (DNS-based)
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
                "stun:stun2.l.google.com:19302".to_string(),
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
    resolver: Option<futures::channel::oneshot::Sender<Result<Vec<u8>, SdkError>>>,
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
    content_cache: Rc<RefCell<HashMap<String, Vec<u8>>>>,
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
            content_cache: Rc::new(RefCell::new(HashMap::new())),
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

        // Start background signaling message loop
        self.start_message_loop();

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

        // Set up data channel message handler
        let pending = self.pending_fetches.clone();
        let cache = self.content_cache.clone();
        let connections_for_msg = self.connections.clone();
        let peer_id_for_msg = peer_id.to_string();

        conn.on_message(move |data: Vec<u8>| {
            let text = match String::from_utf8(data) {
                Ok(t) => t,
                Err(_) => return,
            };

            if let Ok(response) = serde_json::from_str::<ContentResponse>(&text) {
                handle_content_response(&pending, &cache, response);
                return;
            }

            if let Ok(request) = serde_json::from_str::<ContentRequest>(&text) {
                handle_content_request(&cache, &connections_for_msg, &peer_id_for_msg, request);
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

    /// Fetch content by CID using P2P or HTTP fallback.
    ///
    /// The fetch strategy is:
    /// 1. Check the local in-memory cache.
    /// 2. If connected to the P2P network, attempt a WebRTC fetch from peers.
    /// 3. If P2P fails (timeout, no peers, connection error, etc.) **and**
    ///    HTTP fallback is enabled (`use_fallback` + `gateway_url` configured),
    ///    fall back to fetching the content from the HTTP gateway.
    /// 4. Content fetched via HTTP is BLAKE3-verified and cached locally so it
    ///    can be served to other peers over P2P.
    #[wasm_bindgen]
    pub async fn fetch(&self, cid: &str) -> Result<js_sys::Uint8Array, JsValue> {
        tracing::info!("Fetching content: {}", cid);
        console_log(&format!("[ShadowMesh] fetch requested for CID: {}", cid));

        // ── 1. Local cache ──────────────────────────────────────────────
        if let Some(data) = self.content_cache.borrow().get(cid) {
            console_log(&format!(
                "[ShadowMesh] cache hit for CID {} ({} bytes)",
                cid,
                data.len()
            ));
            let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
            array.copy_from(data);
            return Ok(array);
        }

        // ── 2. P2P fetch (only when connected) ─────────────────────────
        let mut p2p_error: Option<SdkError> = None;

        if *self.connected.borrow() {
            console_log("[ShadowMesh] attempting P2P fetch...");
            match self.fetch_p2p(cid).await {
                Ok(data) => {
                    tracing::info!("Content fetched via P2P: {} bytes", data.len());
                    console_log(&format!(
                        "[ShadowMesh] P2P fetch succeeded: {} bytes",
                        data.len()
                    ));
                    let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
                    array.copy_from(&data);
                    return Ok(array);
                }
                Err(e) => {
                    tracing::warn!("P2P fetch failed: {}", e);
                    console_log(&format!("[ShadowMesh] P2P fetch failed: {}", e));
                    p2p_error = Some(e);
                }
            }
        } else {
            console_log("[ShadowMesh] not connected to P2P network, skipping P2P fetch");
        }

        // ── 3. HTTP gateway fallback ────────────────────────────────────
        if self.config.use_fallback {
            if let Some(ref gateway_url) = self.config.gateway_url {
                console_log(&format!(
                    "[ShadowMesh] falling back to HTTP gateway: {}",
                    gateway_url
                ));

                match self.fetch_http_with_timeout(gateway_url, cid).await {
                    Ok(data) => {
                        // Verify content integrity with BLAKE3
                        if !verify_cid(&data, cid) {
                            let msg = format!(
                                "HTTP fallback: BLAKE3 hash mismatch for CID {}",
                                cid
                            );
                            tracing::error!("{}", msg);
                            console_log(&format!("[ShadowMesh] {}", msg));
                            return Err(JsValue::from_str(&msg));
                        }

                        tracing::info!(
                            "Content fetched via HTTP fallback: {} bytes",
                            data.len()
                        );
                        console_log(&format!(
                            "[ShadowMesh] HTTP fallback succeeded: {} bytes (integrity verified)",
                            data.len()
                        ));

                        // Cache so we can serve it to other peers via P2P
                        self.content_cache
                            .borrow_mut()
                            .insert(cid.to_string(), data.clone());

                        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
                        array.copy_from(&data);
                        return Ok(array);
                    }
                    Err(e) => {
                        tracing::error!("HTTP fallback failed: {}", e);
                        console_log(&format!("[ShadowMesh] HTTP fallback failed: {}", e));
                    }
                }
            } else {
                console_log(
                    "[ShadowMesh] HTTP fallback enabled but no gateway_url configured",
                );
            }
        } else {
            console_log("[ShadowMesh] HTTP fallback is disabled");
        }

        // ── 4. All sources exhausted ────────────────────────────────────
        let detail = match p2p_error {
            Some(e) => format!("P2P error: {}", e),
            None => "no P2P connection available".to_string(),
        };
        let msg = format!(
            "Failed to fetch content from any source ({})",
            detail
        );
        console_log(&format!("[ShadowMesh] {}", msg));
        Err(JsValue::from_str(&msg))
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
        // Check local cache first
        if let Some(data) = self.content_cache.borrow().get(cid) {
            tracing::info!("Content {} found in local cache", cid);
            return Ok(data.clone());
        }

        // Find a connected peer to request from
        let request_json = {
            let connections = self.connections.borrow();
            if connections.is_empty() {
                return Err(SdkError::new(codes::NOT_CONNECTED, "No connected peers"));
            }

            let request = ContentRequest {
                request_type: "content_request".to_string(),
                cid: cid.to_string(),
                fragment_index: None,
            };

            let request_json = serde_json::to_string(&request)
                .map_err(|e| SdkError::new(codes::SIGNALING_ERROR, &e.to_string()))?;

            // Send to first connected peer
            let mut sent = false;
            for (peer_id, conn) in connections.iter() {
                if conn.is_connected() {
                    tracing::info!("Requesting content {} from peer {}", cid, peer_id);
                    conn.send(request_json.as_bytes())
                        .map_err(|e| SdkError::new(codes::WEBRTC_ERROR, &e.to_string()))?;
                    sent = true;
                    break;
                }
            }

            if !sent {
                return Err(SdkError::new(
                    codes::NOT_CONNECTED,
                    "No connected peers with open data channel",
                ));
            }

            request_json
        };
        let _ = request_json; // consumed above

        // Create oneshot channel for async response
        let (tx, rx) = futures::channel::oneshot::channel::<Result<Vec<u8>, SdkError>>();

        self.pending_fetches.borrow_mut().insert(
            cid.to_string(),
            PendingFetch {
                cid: cid.to_string(),
                fragments: HashMap::new(),
                total_fragments: None,
                resolver: Some(tx),
            },
        );

        // Race response against timeout
        let timeout = gloo_timers::future::TimeoutFuture::new(self.config.timeout_ms);

        let result = futures::select! {
            response = rx.fuse() => {
                match response {
                    Ok(inner) => inner,
                    Err(_) => Err(SdkError::new(codes::WEBRTC_ERROR, "Response channel closed")),
                }
            }
            _ = timeout.fuse() => {
                // Clean up the pending fetch on timeout
                self.pending_fetches.borrow_mut().remove(cid);
                Err(SdkError::new(codes::TIMEOUT, "P2P fetch timed out"))
            }
        };

        // Cache successful result
        if let Ok(ref data) = result {
            self.content_cache
                .borrow_mut()
                .insert(cid.to_string(), data.clone());
        }

        result
    }

    /// Fetch content via HTTP gateway (raw, no timeout).
    async fn fetch_http(&self, gateway_url: &str, cid: &str) -> Result<Vec<u8>, SdkError> {
        let url = format!("{}/ipfs/{}", gateway_url.trim_end_matches('/'), cid);

        // Use web_sys fetch API
        let window = web_sys::window()
            .ok_or_else(|| SdkError::new(codes::CONNECTION_FAILED, "No window object"))?;

        let promise = window.fetch_with_str(&url);
        let response = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| SdkError::new(codes::CONNECTION_FAILED, &format!("HTTP fetch error: {:?}", e)))?;

        let response: web_sys::Response = response
            .dyn_into()
            .map_err(|_| SdkError::new(codes::CONNECTION_FAILED, "Invalid HTTP response object"))?;

        if !response.ok() {
            return Err(SdkError::new(
                codes::CONTENT_NOT_FOUND,
                &format!("HTTP {} from gateway", response.status()),
            ));
        }

        let array_buffer = wasm_bindgen_futures::JsFuture::from(
            response
                .array_buffer()
                .map_err(|_| SdkError::new(codes::CONNECTION_FAILED, "Failed to read response body"))?,
        )
        .await
        .map_err(|e| SdkError::new(codes::CONNECTION_FAILED, &format!("Body read error: {:?}", e)))?;

        let uint8_array = js_sys::Uint8Array::new(&array_buffer);
        Ok(uint8_array.to_vec())
    }

    /// Fetch content via HTTP gateway with a timeout.
    ///
    /// Uses the same `timeout_ms` setting from the client config.
    /// The timeout protects against unresponsive gateways so the caller
    /// gets a clear error rather than hanging indefinitely.
    async fn fetch_http_with_timeout(
        &self,
        gateway_url: &str,
        cid: &str,
    ) -> Result<Vec<u8>, SdkError> {
        let timeout = gloo_timers::future::TimeoutFuture::new(self.config.timeout_ms);

        let result = futures::select! {
            res = self.fetch_http(gateway_url, cid).fuse() => res,
            _ = timeout.fuse() => {
                Err(SdkError::new(
                    codes::TIMEOUT,
                    &format!(
                        "HTTP gateway fetch timed out after {}ms",
                        self.config.timeout_ms
                    ),
                ))
            }
        };

        result
    }

    /// Verify content hash using BLAKE3
    fn verify_content(&self, data: &[u8], expected_cid: &str) -> bool {
        verify_cid(data, expected_cid)
    }

    /// Start a background signaling message loop.
    ///
    /// Handles incoming WebRTC offers, answers, ICE candidates, peer discovery
    /// responses, and disconnect notifications.
    fn start_message_loop(&self) {
        let signaling = self.signaling.clone();
        let connections = self.connections.clone();
        let available_peers = self.available_peers.clone();
        let stun_servers = self.config.stun_servers.clone();
        let pending = self.pending_fetches.clone();
        let cache = self.content_cache.clone();

        // Take the message receiver from signaling (can only be called once)
        let rx = {
            let sig = signaling.borrow();
            sig.as_ref().and_then(|s| s.take_message_receiver())
        };

        let Some(mut rx) = rx else {
            tracing::warn!("Signaling message receiver already taken");
            return;
        };

        wasm_bindgen_futures::spawn_local(async move {
            while let Some(msg) = rx.next().await {
                match msg {
                    SignalingMessage::Peers(peers_msg) => {
                        *available_peers.borrow_mut() = peers_msg.peers;
                        tracing::info!(
                            "Discovered {} peers",
                            available_peers.borrow().len()
                        );
                    }
                    SignalingMessage::Offer(offer) => {
                        tracing::info!("Received offer from peer {}", offer.from);
                        let conn = match WebRtcConnection::new(&stun_servers) {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::error!("Failed to create WebRTC connection: {}", e);
                                continue;
                            }
                        };

                        // Wire ICE candidate handler
                        let sig_for_ice = signaling.clone();
                        let target = offer.from.clone();
                        let sid = offer.session_id.clone();
                        conn.on_ice_candidate(move |candidate, sdp_mid, sdp_mline_index| {
                            if let Some(ref sig) = *sig_for_ice.borrow() {
                                let _ = sig.send_ice_candidate(
                                    &target,
                                    &candidate,
                                    sdp_mid.as_deref(),
                                    sdp_mline_index,
                                    &sid,
                                );
                            }
                        });

                        // Wire data channel message handler
                        let p = pending.clone();
                        let c = cache.clone();
                        let conns_for_msg = connections.clone();
                        let pid = offer.from.clone();
                        conn.on_message(move |data: Vec<u8>| {
                            let text = match String::from_utf8(data) {
                                Ok(t) => t,
                                Err(_) => return,
                            };
                            if let Ok(response) =
                                serde_json::from_str::<ContentResponse>(&text)
                            {
                                handle_content_response(&p, &c, response);
                                return;
                            }
                            if let Ok(request) =
                                serde_json::from_str::<ContentRequest>(&text)
                            {
                                handle_content_request(
                                    &c,
                                    &conns_for_msg,
                                    &pid,
                                    request,
                                );
                            }
                        });

                        // Create answer
                        match conn.create_answer(&offer.sdp).await {
                            Ok(answer_sdp) => {
                                if let Some(ref sig) = *signaling.borrow() {
                                    let _ = sig.send_answer(
                                        &offer.from,
                                        &answer_sdp,
                                        &offer.session_id,
                                    );
                                }
                                connections
                                    .borrow_mut()
                                    .insert(offer.from.clone(), conn);
                                tracing::info!(
                                    "Accepted connection from peer {}",
                                    offer.from
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to create answer for {}: {}",
                                    offer.from,
                                    e
                                );
                            }
                        }
                    }
                    SignalingMessage::Answer(answer) => {
                        tracing::info!("Received answer from peer {}", answer.from);
                        let conns = connections.borrow();
                        if let Some(conn) = conns.get(&answer.from) {
                            if let Err(e) = conn.set_remote_answer(&answer.sdp).await {
                                tracing::error!(
                                    "Failed to set remote answer from {}: {}",
                                    answer.from,
                                    e
                                );
                            }
                        }
                    }
                    SignalingMessage::IceCandidate(ice) => {
                        let conns = connections.borrow();
                        if let Some(conn) = conns.get(&ice.from) {
                            if let Err(e) = conn
                                .add_ice_candidate(
                                    &ice.candidate,
                                    ice.sdp_mid.as_deref(),
                                    ice.sdp_mline_index,
                                )
                                .await
                            {
                                tracing::warn!(
                                    "Failed to add ICE candidate from {}: {}",
                                    ice.from,
                                    e
                                );
                            }
                        }
                    }
                    SignalingMessage::PeerDisconnected(disc) => {
                        tracing::info!("Peer disconnected: {}", disc.peer_id);
                        if let Some(conn) =
                            connections.borrow_mut().remove(&disc.peer_id)
                        {
                            conn.close();
                        }
                    }
                    _ => {}
                }
            }
            tracing::info!("Signaling message loop ended");
        });
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

/// BLAKE3 content verification (standalone for use in free functions)
fn verify_cid(data: &[u8], expected_cid: &str) -> bool {
    let hash = blake3::hash(data);
    let computed_cid = format!("baf{}", hex_encode(&hash.as_bytes()[..32]));
    computed_cid.starts_with(&expected_cid[..expected_cid.len().min(10)])
}

/// Handle an incoming content response from a peer.
///
/// Collects fragments into the pending fetch, and when all fragments have
/// arrived, reassembles, verifies, caches, and resolves the oneshot.
fn handle_content_response(
    pending: &Rc<RefCell<HashMap<String, PendingFetch>>>,
    cache: &Rc<RefCell<HashMap<String, Vec<u8>>>>,
    response: ContentResponse,
) {
    let cid = response.cid.clone();

    match response.response_type.as_str() {
        "content_response" => {
            let fragment_index = match response.fragment_index {
                Some(idx) => idx,
                None => return,
            };
            let total = match response.total_fragments {
                Some(t) => t,
                None => return,
            };
            let data_b64 = match response.data {
                Some(ref d) => d,
                None => return,
            };
            let fragment_data =
                match base64::engine::general_purpose::STANDARD.decode(data_b64) {
                    Ok(d) => d,
                    Err(_) => {
                        tracing::warn!("Failed to decode base64 fragment data for {}", cid);
                        return;
                    }
                };

            let mut pending_map = pending.borrow_mut();
            let fetch = match pending_map.get_mut(&cid) {
                Some(f) => f,
                None => return, // No pending fetch for this CID
            };

            fetch.total_fragments = Some(total);
            fetch.fragments.insert(fragment_index, fragment_data);

            tracing::debug!(
                "Received fragment {}/{} for {}",
                fetch.fragments.len(),
                total,
                cid
            );

            // Check if all fragments have arrived
            if fetch.fragments.len() == total as usize {
                // Reassemble in order
                let mut assembled = Vec::new();
                for i in 0..total {
                    match fetch.fragments.get(&i) {
                        Some(frag) => assembled.extend_from_slice(frag),
                        None => {
                            // Missing fragment — resolve with error
                            if let Some(resolver) = fetch.resolver.take() {
                                let _ = resolver.send(Err(SdkError::new(
                                    codes::CONTENT_NOT_FOUND,
                                    "Missing fragment during reassembly",
                                )));
                            }
                            pending_map.remove(&cid);
                            return;
                        }
                    }
                }

                // Verify BLAKE3 hash
                if !verify_cid(&assembled, &cid) {
                    if let Some(resolver) = fetch.resolver.take() {
                        let _ = resolver.send(Err(SdkError::new(
                            codes::CONTENT_NOT_FOUND,
                            "Content hash verification failed",
                        )));
                    }
                    pending_map.remove(&cid);
                    return;
                }

                // Cache and resolve
                cache
                    .borrow_mut()
                    .insert(cid.clone(), assembled.clone());

                if let Some(resolver) = fetch.resolver.take() {
                    let _ = resolver.send(Ok(assembled));
                }
                pending_map.remove(&cid);
            }
        }
        "content_not_found" => {
            let mut pending_map = pending.borrow_mut();
            if let Some(mut fetch) = pending_map.remove(&cid) {
                if let Some(resolver) = fetch.resolver.take() {
                    let _ = resolver.send(Err(SdkError::new(
                        codes::CONTENT_NOT_FOUND,
                        "Peer does not have the requested content",
                    )));
                }
            }
        }
        "content_error" => {
            let msg = response
                .error
                .unwrap_or_else(|| "Unknown peer error".to_string());
            let mut pending_map = pending.borrow_mut();
            if let Some(mut fetch) = pending_map.remove(&cid) {
                if let Some(resolver) = fetch.resolver.take() {
                    let _ = resolver.send(Err(SdkError::new(codes::WEBRTC_ERROR, &msg)));
                }
            }
        }
        _ => {}
    }
}

/// Handle an incoming content request from a peer — serve from cache.
fn handle_content_request(
    cache: &Rc<RefCell<HashMap<String, Vec<u8>>>>,
    connections: &Rc<RefCell<HashMap<String, WebRtcConnection>>>,
    peer_id: &str,
    request: ContentRequest,
) {
    let cache_map = cache.borrow();
    let conns = connections.borrow();

    let conn = match conns.get(peer_id) {
        Some(c) if c.is_connected() => c,
        _ => return,
    };

    match cache_map.get(&request.cid) {
        Some(data) => {
            // Chunk into FRAGMENT_SIZE pieces and send each as a response
            let chunks: Vec<&[u8]> = data.chunks(FRAGMENT_SIZE).collect();
            let total_fragments = chunks.len() as u32;

            for (i, chunk) in chunks.iter().enumerate() {
                let encoded = base64::engine::general_purpose::STANDARD.encode(chunk);
                let response = ContentResponse {
                    response_type: "content_response".to_string(),
                    cid: request.cid.clone(),
                    fragment_index: Some(i as u32),
                    total_fragments: Some(total_fragments),
                    data: Some(encoded),
                    error: None,
                };

                if let Ok(json) = serde_json::to_string(&response) {
                    if let Err(e) = conn.send(json.as_bytes()) {
                        tracing::warn!("Failed to send fragment {} to {}: {}", i, peer_id, e);
                        return;
                    }
                }
            }
            tracing::info!(
                "Served {} fragments for {} to peer {}",
                total_fragments,
                request.cid,
                peer_id
            );
        }
        None => {
            let response = ContentResponse {
                response_type: "content_not_found".to_string(),
                cid: request.cid.clone(),
                fragment_index: None,
                total_fragments: None,
                data: None,
                error: None,
            };
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = conn.send(json.as_bytes());
            }
        }
    }
}
