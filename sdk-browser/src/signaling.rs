//! WebSocket signaling client for WebRTC peer discovery

use crate::error::{codes, SdkError};
use crate::utils::generate_session_id;
use futures::channel::mpsc;
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

/// Signaling message types (mirrors protocol crate)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalingMessage {
    Announce(AnnounceMessage),
    Discover(DiscoverMessage),
    Peers(PeersMessage),
    Offer(OfferMessage),
    Answer(AnswerMessage),
    IceCandidate(IceCandidateMessage),
    Heartbeat(HeartbeatMessage),
    Error(ErrorMessage),
    PeerDisconnected(PeerDisconnectedMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnounceMessage {
    pub peer_id: String,
    pub multiaddrs: Vec<String>,
    #[serde(default)]
    pub transports: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverMessage {
    pub peer_id: String,
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,
    #[serde(default)]
    pub transport_filter: Option<String>,
    #[serde(default)]
    pub region_filter: Option<String>,
}

fn default_max_peers() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeersMessage {
    pub peers: Vec<PeerInfo>,
    pub total_peers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub multiaddrs: Vec<String>,
    pub transports: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    #[serde(default)]
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferMessage {
    pub from: String,
    pub to: String,
    pub sdp: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerMessage {
    pub from: String,
    pub to: String,
    pub sdp: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceCandidateMessage {
    pub from: String,
    pub to: String,
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    pub peer_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDisconnectedMessage {
    pub peer_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// Signaling client for WebRTC peer discovery
pub struct SignalingClient {
    url: String,
    peer_id: String,
    ws: Rc<RefCell<Option<WebSocket>>>,
    state: Rc<RefCell<ConnectionState>>,
    message_tx: mpsc::UnboundedSender<SignalingMessage>,
    message_rx: Rc<RefCell<Option<mpsc::UnboundedReceiver<SignalingMessage>>>>,
}

impl SignalingClient {
    /// Create a new signaling client
    pub fn new(url: &str, peer_id: &str) -> Self {
        let (tx, rx) = mpsc::unbounded();

        Self {
            url: url.to_string(),
            peer_id: peer_id.to_string(),
            ws: Rc::new(RefCell::new(None)),
            state: Rc::new(RefCell::new(ConnectionState::Disconnected)),
            message_tx: tx,
            message_rx: Rc::new(RefCell::new(Some(rx))),
        }
    }

    /// Connect to the signaling server
    pub async fn connect(&self) -> Result<(), SdkError> {
        *self.state.borrow_mut() = ConnectionState::Connecting;

        // Create WebSocket
        let ws = WebSocket::new(&self.url).map_err(|e| {
            SdkError::new(
                codes::CONNECTION_FAILED,
                &format!("Failed to create WebSocket: {:?}", e),
            )
        })?;

        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // Set up event handlers
        let state = self.state.clone();
        let tx = self.message_tx.clone();

        // onopen
        let state_open = state.clone();
        let onopen = Closure::wrap(Box::new(move |_: web_sys::Event| {
            *state_open.borrow_mut() = ConnectionState::Connected;
            tracing::info!("WebSocket connected");
        }) as Box<dyn FnMut(_)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        // onclose
        let state_close = state.clone();
        let onclose = Closure::wrap(Box::new(move |_: CloseEvent| {
            *state_close.borrow_mut() = ConnectionState::Disconnected;
            tracing::info!("WebSocket closed");
        }) as Box<dyn FnMut(_)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        // onerror
        let state_error = state.clone();
        let onerror = Closure::wrap(Box::new(move |e: ErrorEvent| {
            *state_error.borrow_mut() = ConnectionState::Error;
            tracing::error!("WebSocket error: {:?}", e.message());
        }) as Box<dyn FnMut(_)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        // onmessage
        let tx_msg = tx.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                if let Ok(msg) = serde_json::from_str::<SignalingMessage>(&text) {
                    let _ = tx_msg.unbounded_send(msg);
                }
            }
        }) as Box<dyn FnMut(_)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        *self.ws.borrow_mut() = Some(ws);

        // Wait for connection (with timeout)
        let timeout = gloo_timers::future::TimeoutFuture::new(5000);
        let state_wait = state.clone();

        futures::select! {
            _ = timeout.fuse() => {
                return Err(SdkError::new(codes::TIMEOUT, "Connection timeout"));
            }
            _ = async {
                while *state_wait.borrow() == ConnectionState::Connecting {
                    gloo_timers::future::TimeoutFuture::new(100).await;
                }
            }.fuse() => {}
        }

        if *state.borrow() != ConnectionState::Connected {
            return Err(SdkError::new(codes::CONNECTION_FAILED, "Failed to connect"));
        }

        Ok(())
    }

    /// Send a signaling message
    pub fn send(&self, message: &SignalingMessage) -> Result<(), SdkError> {
        let ws = self.ws.borrow();
        let ws = ws.as_ref().ok_or_else(|| {
            SdkError::new(codes::NOT_CONNECTED, "Not connected to signaling server")
        })?;

        let json = serde_json::to_string(message).map_err(|e| {
            SdkError::new(
                codes::SIGNALING_ERROR,
                &format!("Serialization error: {}", e),
            )
        })?;

        ws.send_with_str(&json)
            .map_err(|e| SdkError::new(codes::SIGNALING_ERROR, &format!("Send error: {:?}", e)))?;

        Ok(())
    }

    /// Announce presence to the signaling server
    pub fn announce(&self) -> Result<(), SdkError> {
        let msg = SignalingMessage::Announce(AnnounceMessage {
            peer_id: self.peer_id.clone(),
            multiaddrs: vec![],
            transports: vec!["webrtc".to_string()],
            metadata: HashMap::new(),
        });
        self.send(&msg)
    }

    /// Discover available peers
    pub fn discover(&self, max_peers: usize) -> Result<(), SdkError> {
        let msg = SignalingMessage::Discover(DiscoverMessage {
            peer_id: self.peer_id.clone(),
            max_peers,
            transport_filter: Some("webrtc".to_string()),
            region_filter: None,
        });
        self.send(&msg)
    }

    /// Send WebRTC offer
    pub fn send_offer(&self, to: &str, sdp: &str, session_id: &str) -> Result<(), SdkError> {
        let msg = SignalingMessage::Offer(OfferMessage {
            from: self.peer_id.clone(),
            to: to.to_string(),
            sdp: sdp.to_string(),
            session_id: session_id.to_string(),
        });
        self.send(&msg)
    }

    /// Send WebRTC answer
    pub fn send_answer(&self, to: &str, sdp: &str, session_id: &str) -> Result<(), SdkError> {
        let msg = SignalingMessage::Answer(AnswerMessage {
            from: self.peer_id.clone(),
            to: to.to_string(),
            sdp: sdp.to_string(),
            session_id: session_id.to_string(),
        });
        self.send(&msg)
    }

    /// Send ICE candidate
    pub fn send_ice_candidate(
        &self,
        to: &str,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
        session_id: &str,
    ) -> Result<(), SdkError> {
        let msg = SignalingMessage::IceCandidate(IceCandidateMessage {
            from: self.peer_id.clone(),
            to: to.to_string(),
            candidate: candidate.to_string(),
            sdp_mid: sdp_mid.map(|s| s.to_string()),
            sdp_mline_index,
            session_id: session_id.to_string(),
        });
        self.send(&msg)
    }

    /// Get connection state
    pub fn state(&self) -> ConnectionState {
        *self.state.borrow()
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        *self.state.borrow() == ConnectionState::Connected
    }

    /// Disconnect from signaling server
    pub fn disconnect(&self) {
        if let Some(ws) = self.ws.borrow().as_ref() {
            let _ = ws.close();
        }
        *self.ws.borrow_mut() = None;
        *self.state.borrow_mut() = ConnectionState::Disconnected;
    }

    /// Take the message receiver (can only be called once)
    pub fn take_message_receiver(&self) -> Option<mpsc::UnboundedReceiver<SignalingMessage>> {
        self.message_rx.borrow_mut().take()
    }
}

impl Drop for SignalingClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}
