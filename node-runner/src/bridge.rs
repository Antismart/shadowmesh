//! WebRTC Bridge: Node-Runner ↔ Browser
//!
//! Connects to the gateway signaling server, accepts WebRTC offers from browsers,
//! and serves content over data channels using the browser SDK's JSON protocol.

use base64::Engine;
use futures::{SinkExt, StreamExt};
use shadowmesh_protocol::{
    AnnounceMessage, AnswerMessage, HeartbeatMessage, IceCandidateMessage, OfferMessage,
    SignalingMessage,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_tungstenite::tungstenite;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use crate::config::BridgeConfig;
use crate::metrics::MetricsCollector;
use crate::storage::StorageManager;

/// Fragment size in bytes (256 KB) — matches browser SDK
const FRAGMENT_SIZE: usize = 256 * 1024;

// ── Browser Protocol Types ───────────────────────────────────────

/// Content request from browser (mirrors sdk-browser/src/client.rs)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentRequest {
    request_type: String,
    cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment_index: Option<u32>,
}

/// Content response to browser (mirrors sdk-browser/src/client.rs)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentResponse {
    response_type: String,
    cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_fragments: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ── Bridge State ─────────────────────────────────────────────────

/// Shared bridge state accessible from API handlers.
pub struct BridgeState {
    /// Number of currently connected browser peers
    pub connected_browsers: RwLock<usize>,
    /// Whether the bridge is enabled
    pub enabled: bool,
}

impl BridgeState {
    pub fn new(enabled: bool) -> Self {
        Self {
            connected_browsers: RwLock::new(0),
            enabled,
        }
    }
}

// ── Main Bridge Event Loop ───────────────────────────────────────

/// Run the bridge event loop.
///
/// Connects to the gateway signaling server via WebSocket, announces as a
/// WebRTC peer, and accepts browser connections. Reconnects on failure.
pub async fn run_bridge(
    config: BridgeConfig,
    peer_id: String,
    storage: Arc<StorageManager>,
    metrics: Arc<MetricsCollector>,
    bridge_state: Arc<BridgeState>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let signaling_url = match &config.signaling_url {
        Some(url) => url.clone(),
        None => {
            tracing::warn!("Bridge enabled but no signaling_url configured — skipping");
            return;
        }
    };

    tracing::info!(%signaling_url, "WebRTC bridge starting");

    // Outer reconnect loop
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!("Bridge shutting down");
                return;
            }
            _ = run_signaling_session(
                &config,
                &signaling_url,
                &peer_id,
                &storage,
                &metrics,
                &bridge_state,
            ) => {
                tracing::warn!("Signaling session ended, reconnecting in {}s", config.reconnect_delay_secs);
            }
        }

        // Wait before reconnecting, but allow shutdown to cancel
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!("Bridge shutting down during reconnect wait");
                return;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(config.reconnect_delay_secs)) => {}
        }
    }
}

/// Run a single signaling session (connect, announce, handle messages).
async fn run_signaling_session(
    config: &BridgeConfig,
    signaling_url: &str,
    peer_id: &str,
    storage: &Arc<StorageManager>,
    metrics: &Arc<MetricsCollector>,
    bridge_state: &Arc<BridgeState>,
) {
    // 1. Connect to signaling server
    let (ws_stream, _) = match tokio_tungstenite::connect_async(signaling_url).await {
        Ok(conn) => {
            tracing::info!("Connected to signaling server");
            conn
        }
        Err(e) => {
            tracing::warn!(%e, "Failed to connect to signaling server");
            return;
        }
    };

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // 2. Announce ourselves with webrtc transport
    let announce = SignalingMessage::Announce(AnnounceMessage {
        peer_id: peer_id.to_string(),
        multiaddrs: vec![],
        transports: vec!["webrtc".to_string()],
        metadata: {
            let mut m = HashMap::new();
            m.insert("peer_type".to_string(), "node-runner".to_string());
            m
        },
    });

    if let Err(e) = send_signaling_msg(&mut ws_tx, &announce).await {
        tracing::warn!(%e, "Failed to send announce");
        return;
    }

    tracing::info!(%peer_id, "Announced to signaling server as WebRTC peer");

    // Channel for per-connection tasks to send signaling messages back through the WS
    let (sig_tx, mut sig_rx) = mpsc::channel::<SignalingMessage>(64);

    // Track active sessions for ICE candidate relay
    let active_sessions: Arc<RwLock<HashMap<String, mpsc::Sender<IceCandidateMessage>>>> =
        Arc::new(RwLock::new(HashMap::new()));

    // Heartbeat timer
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(
        config.heartbeat_interval_secs,
    ));
    heartbeat.tick().await; // skip immediate first tick

    // 3. Message loop
    loop {
        tokio::select! {
            // Incoming signaling message
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(tungstenite::Message::Text(text))) => {
                        handle_signaling_message(
                            &text,
                            peer_id,
                            config,
                            storage,
                            metrics,
                            bridge_state,
                            &sig_tx,
                            &active_sessions,
                        ).await;
                    }
                    Some(Ok(tungstenite::Message::Close(_))) | None => {
                        tracing::info!("Signaling WebSocket closed");
                        return;
                    }
                    Some(Err(e)) => {
                        tracing::warn!(%e, "Signaling WebSocket error");
                        return;
                    }
                    _ => {} // ping/pong/binary — ignore
                }
            }

            // Outgoing signaling messages from per-connection tasks
            Some(msg) = sig_rx.recv() => {
                if let Err(e) = send_signaling_msg(&mut ws_tx, &msg).await {
                    tracing::warn!(%e, "Failed to send signaling message");
                    return;
                }
            }

            // Heartbeat
            _ = heartbeat.tick() => {
                let hb = SignalingMessage::Heartbeat(HeartbeatMessage {
                    peer_id: peer_id.to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                });
                if let Err(e) = send_signaling_msg(&mut ws_tx, &hb).await {
                    tracing::warn!(%e, "Failed to send heartbeat");
                    return;
                }
            }
        }
    }
}

/// Handle a single signaling message from the WebSocket.
async fn handle_signaling_message(
    text: &str,
    our_peer_id: &str,
    config: &BridgeConfig,
    storage: &Arc<StorageManager>,
    metrics: &Arc<MetricsCollector>,
    bridge_state: &Arc<BridgeState>,
    sig_tx: &mpsc::Sender<SignalingMessage>,
    active_sessions: &Arc<RwLock<HashMap<String, mpsc::Sender<IceCandidateMessage>>>>,
) {
    let msg: SignalingMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!(%e, "Ignoring unparseable signaling message");
            return;
        }
    };

    match msg {
        SignalingMessage::Offer(offer) => {
            // Check connection limit
            let current = *bridge_state.connected_browsers.read().await;
            if current >= config.max_connections {
                tracing::warn!(
                    from = %offer.from,
                    "Rejecting WebRTC offer — max connections ({}) reached",
                    config.max_connections
                );
                return;
            }

            tracing::info!(from = %offer.from, session = %offer.session_id, "Received WebRTC offer from browser");

            // Create ICE candidate channel for this session
            let (ice_tx, ice_rx) = mpsc::channel::<IceCandidateMessage>(32);
            active_sessions
                .write()
                .await
                .insert(offer.session_id.clone(), ice_tx);

            // Spawn connection handler
            let storage = storage.clone();
            let metrics = metrics.clone();
            let bridge_state = bridge_state.clone();
            let sig_tx = sig_tx.clone();
            let active_sessions = active_sessions.clone();
            let stun_servers = config.stun_servers.clone();
            let our_peer_id = our_peer_id.to_string();

            tokio::spawn(async move {
                handle_browser_offer(
                    offer,
                    ice_rx,
                    &our_peer_id,
                    &stun_servers,
                    &storage,
                    &metrics,
                    &bridge_state,
                    &sig_tx,
                )
                .await;

                // Cleanup session on disconnect
                active_sessions
                    .write()
                    .await
                    .remove(&our_peer_id);
            });
        }

        SignalingMessage::IceCandidate(candidate) => {
            // Relay to the appropriate session
            let sessions = active_sessions.read().await;
            if let Some(tx) = sessions.get(&candidate.session_id) {
                let _ = tx.send(candidate).await;
            }
        }

        SignalingMessage::Peers(_) => {
            tracing::debug!("Received peers response (announce ack)");
        }

        SignalingMessage::Error(err) => {
            tracing::warn!(code = ?err.code, msg = %err.message, "Signaling server error");
        }

        _ => {
            tracing::debug!("Ignoring signaling message");
        }
    }
}

// ── WebRTC Connection Handling ───────────────────────────────────

/// Handle a WebRTC offer from a browser peer.
async fn handle_browser_offer(
    offer: OfferMessage,
    ice_rx: mpsc::Receiver<IceCandidateMessage>,
    our_peer_id: &str,
    stun_servers: &[String],
    storage: &Arc<StorageManager>,
    metrics: &Arc<MetricsCollector>,
    bridge_state: &Arc<BridgeState>,
    sig_tx: &mpsc::Sender<SignalingMessage>,
) {
    // Increment connected browsers
    {
        let mut count = bridge_state.connected_browsers.write().await;
        *count += 1;
        tracing::info!(from = %offer.from, browsers = *count, "Browser connecting");
    }

    // Run the connection (errors are logged internally)
    let result = handle_browser_offer_inner(
        &offer,
        ice_rx,
        our_peer_id,
        stun_servers,
        storage,
        metrics,
        sig_tx,
    )
    .await;

    if let Err(e) = result {
        tracing::warn!(from = %offer.from, %e, "Browser connection failed");
    }

    // Decrement connected browsers
    {
        let mut count = bridge_state.connected_browsers.write().await;
        *count = count.saturating_sub(1);
        tracing::info!(from = %offer.from, browsers = *count, "Browser disconnected");
    }
}

async fn handle_browser_offer_inner(
    offer: &OfferMessage,
    mut ice_rx: mpsc::Receiver<IceCandidateMessage>,
    our_peer_id: &str,
    stun_servers: &[String],
    storage: &Arc<StorageManager>,
    metrics: &Arc<MetricsCollector>,
    sig_tx: &mpsc::Sender<SignalingMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Build WebRTC API
    let mut media_engine = MediaEngine::default();
    media_engine.register_default_codecs()?;

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media_engine)?;

    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();

    // 2. Create peer connection with STUN config
    let ice_servers: Vec<RTCIceServer> = stun_servers
        .iter()
        .map(|s| RTCIceServer {
            urls: vec![s.clone()],
            ..Default::default()
        })
        .collect();

    let rtc_config = RTCConfiguration {
        ice_servers,
        ..Default::default()
    };

    let pc = Arc::new(api.new_peer_connection(rtc_config).await?);

    // 3. Set up ICE candidate gathering — send candidates back to browser
    let sig_tx_ice = sig_tx.clone();
    let from_peer = offer.from.clone();
    let session_id = offer.session_id.clone();
    let our_id = our_peer_id.to_string();
    pc.on_ice_candidate(Box::new(move |candidate| {
        let sig_tx = sig_tx_ice.clone();
        let from_peer = from_peer.clone();
        let session_id = session_id.clone();
        let our_id = our_id.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                let candidate_json = match candidate.to_json() {
                    Ok(j) => j,
                    Err(_) => return,
                };
                let msg = SignalingMessage::IceCandidate(IceCandidateMessage {
                    from: our_id,
                    to: from_peer,
                    candidate: candidate_json.candidate,
                    sdp_mid: candidate_json.sdp_mid,
                    sdp_mline_index: candidate_json.sdp_mline_index,
                    session_id,
                });
                let _ = sig_tx.send(msg).await;
            }
        })
    }));

    // 4. Set remote description (the offer)
    let remote_desc = RTCSessionDescription::offer(offer.sdp.clone())?;
    pc.set_remote_description(remote_desc).await?;

    // 5. Create and set answer
    let answer = pc.create_answer(None).await?;
    pc.set_local_description(answer.clone()).await?;

    // 6. Send answer back to browser
    let answer_msg = SignalingMessage::Answer(AnswerMessage {
        from: our_peer_id.to_string(),
        to: offer.from.clone(),
        sdp: answer.sdp,
        session_id: offer.session_id.clone(),
    });
    sig_tx.send(answer_msg).await.map_err(|e| format!("Failed to send answer: {}", e))?;

    // 7. Wait for data channel from browser
    let (dc_tx, mut dc_rx) = mpsc::channel::<Arc<RTCDataChannel>>(1);
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_tx = dc_tx.clone();
        Box::pin(async move {
            tracing::info!(label = %dc.label(), "Data channel opened by browser");
            let _ = dc_tx.send(dc).await;
        })
    }));

    // Spawn a task to relay incoming ICE candidates
    let pc_for_ice = pc.clone();
    let ice_relay = tokio::spawn(async move {
        while let Some(candidate) = ice_rx.recv().await {
            let init = RTCIceCandidateInit {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid,
                sdp_mline_index: candidate.sdp_mline_index,
                ..Default::default()
            };
            if let Err(e) = pc_for_ice.add_ice_candidate(init).await {
                tracing::debug!(%e, "Failed to add ICE candidate");
            }
        }
    });

    // Wait for data channel with a timeout
    let dc = tokio::time::timeout(std::time::Duration::from_secs(30), dc_rx.recv())
        .await
        .map_err(|_| "Timeout waiting for data channel")?
        .ok_or("Data channel sender dropped")?;

    tracing::info!(label = %dc.label(), from = %offer.from, "WebRTC data channel ready");

    // 8. Serve content over the data channel
    serve_data_channel(dc, storage, metrics).await;

    // Cleanup
    ice_relay.abort();
    pc.close().await?;

    Ok(())
}

// ── Data Channel Content Serving ─────────────────────────────────

/// Serve content requests on a data channel until it closes.
async fn serve_data_channel(
    dc: Arc<RTCDataChannel>,
    storage: &Arc<StorageManager>,
    metrics: &Arc<MetricsCollector>,
) {
    let (msg_tx, mut msg_rx) = mpsc::channel::<Vec<u8>>(64);

    // Register onmessage handler to forward to our channel
    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let msg_tx = msg_tx.clone();
        Box::pin(async move {
            let _ = msg_tx.send(msg.data.to_vec()).await;
        })
    }));

    // Wait for the data channel to open
    let (open_tx, open_rx) = tokio::sync::oneshot::channel::<()>();
    let open_tx = Arc::new(std::sync::Mutex::new(Some(open_tx)));
    dc.on_open(Box::new(move || {
        let open_tx = open_tx.clone();
        Box::pin(async move {
            if let Ok(mut guard) = open_tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(());
                }
            }
        })
    }));

    // Wait for open with timeout
    if tokio::time::timeout(std::time::Duration::from_secs(15), open_rx)
        .await
        .is_err()
    {
        tracing::warn!("Data channel did not open in time");
        return;
    }

    // Process messages
    while let Some(data) = msg_rx.recv().await {
        // Parse as UTF-8 string, then as JSON
        let text = match String::from_utf8(data) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let request: ContentRequest = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(%e, "Ignoring non-content message on data channel");
                continue;
            }
        };

        if request.request_type == "content_request" {
            handle_content_request(&dc, storage, metrics, &request).await;
        }
    }

    tracing::debug!("Data channel message stream ended");
}

/// Handle a content request: look up in storage, chunk, and send fragments.
async fn handle_content_request(
    dc: &Arc<RTCDataChannel>,
    storage: &Arc<StorageManager>,
    metrics: &Arc<MetricsCollector>,
    request: &ContentRequest,
) {
    let cid = &request.cid;

    // Look up content metadata
    let content = match storage.get_content(cid).await {
        Some(c) => c,
        None => {
            // Content not found
            let resp = ContentResponse {
                response_type: "content_not_found".to_string(),
                cid: cid.clone(),
                fragment_index: None,
                total_fragments: None,
                data: None,
                error: None,
            };
            send_response(dc, &resp).await;
            return;
        }
    };

    // Collect all fragment data
    let mut all_data = Vec::with_capacity(content.total_size as usize);
    for frag_hash in &content.fragments {
        match storage.get_fragment(frag_hash).await {
            Ok(data) => all_data.extend_from_slice(&data),
            Err(e) => {
                tracing::warn!(%cid, hash = %frag_hash, %e, "Missing fragment for content");
                let resp = ContentResponse {
                    response_type: "content_error".to_string(),
                    cid: cid.clone(),
                    fragment_index: None,
                    total_fragments: None,
                    data: None,
                    error: Some(format!("Missing fragment: {}", frag_hash)),
                };
                send_response(dc, &resp).await;
                return;
            }
        }
    }

    // Chunk into FRAGMENT_SIZE pieces and send
    let chunks: Vec<&[u8]> = all_data.chunks(FRAGMENT_SIZE).collect();
    let total_fragments = chunks.len() as u32;
    let mut total_bytes_sent = 0u64;

    for (i, chunk) in chunks.iter().enumerate() {
        let encoded = base64::engine::general_purpose::STANDARD.encode(chunk);
        total_bytes_sent += chunk.len() as u64;

        let resp = ContentResponse {
            response_type: "content_response".to_string(),
            cid: cid.clone(),
            fragment_index: Some(i as u32),
            total_fragments: Some(total_fragments),
            data: Some(encoded),
            error: None,
        };

        send_response(dc, &resp).await;
    }

    metrics.record_served(total_bytes_sent);
    tracing::debug!(
        %cid,
        fragments = total_fragments,
        bytes = total_bytes_sent,
        "Served content to browser via WebRTC"
    );
}

/// Send a JSON response over the data channel.
async fn send_response(dc: &Arc<RTCDataChannel>, resp: &ContentResponse) {
    match serde_json::to_string(resp) {
        Ok(json) => {
            if let Err(e) = dc.send_text(json).await {
                tracing::debug!(%e, "Failed to send on data channel");
            }
        }
        Err(e) => {
            tracing::warn!(%e, "Failed to serialize content response");
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

/// Serialize and send a signaling message over WebSocket.
async fn send_signaling_msg<S>(
    ws_tx: &mut futures::stream::SplitSink<S, tungstenite::Message>,
    msg: &SignalingMessage,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: futures::Sink<tungstenite::Message, Error = tungstenite::Error> + Unpin,
{
    let json = serde_json::to_string(msg)?;
    ws_tx.send(tungstenite::Message::Text(json.into())).await?;
    Ok(())
}
