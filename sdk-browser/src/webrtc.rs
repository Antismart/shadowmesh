//! WebRTC peer connection management.
//!
//! Every DataChannel data message is encrypted with ChaCha20-Poly1305 using a
//! key established by an **authenticated ephemeral X25519 ECDH handshake** (see
//! [`crate::crypto`]). The key is derived from ephemeral private keys that never
//! leave either peer, so an observer of the signaling path cannot recover it.
//!
//! When a DataChannel opens each side sends a handshake frame containing its
//! Ed25519 identity public key, a fresh ephemeral X25519 public key, and a
//! signature binding the two. Each side verifies the signature, checks the
//! identity matches the `peer_id` learned from signaling, and only then derives
//! the channel key and marks the peer authenticated. If verification fails or
//! the identity does not match, the connection is closed immediately.

use crate::crypto::{
    build_handshake, decrypt_data, derive_channel_key, encrypt_data, verify_handshake,
    DataChannelCipher, EphemeralKeypair, Identity,
};
use crate::error::{codes, SdkError};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelState, RtcIceCandidate,
    RtcIceCandidateInit, RtcPeerConnection, RtcPeerConnectionIceEvent, RtcSdpType,
    RtcSessionDescriptionInit,
};

/// WebRTC connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebRtcState {
    New,
    Connecting,
    Connected,
    Authenticated,
    Disconnected,
    Failed,
}

/// Type alias for ICE candidate callback
type IceCandidateCallback = Rc<RefCell<Option<Box<dyn Fn(String, Option<String>, Option<u16>)>>>>;

/// Type alias for message callback (receives *decrypted* application data)
type MessageCallback = Rc<RefCell<Option<Box<dyn Fn(Vec<u8>)>>>>;

/// The channel cipher, established once the handshake completes.
type ChannelCipher = Rc<RefCell<Option<DataChannelCipher>>>;

/// WebRTC peer connection wrapper with authenticated, encrypted DataChannels.
pub struct WebRtcConnection {
    pc: RtcPeerConnection,
    data_channel: Rc<RefCell<Option<RtcDataChannel>>>,
    state: Rc<RefCell<WebRtcState>>,
    on_ice_candidate: IceCandidateCallback,
    on_message: MessageCallback,

    // ── Key-agreement state ─────────────────────────────────────────────
    /// Our long-term identity keypair (shared across connections).
    identity: Rc<Identity>,
    /// Per-connection ephemeral X25519 keypair.
    ephemeral: Rc<EphemeralKeypair>,
    /// ChaCha20-Poly1305 cipher derived from the ECDH handshake (None until
    /// the peer's handshake has been received and verified).
    cipher: ChannelCipher,
    /// The expected remote peer ID (identity hex, from signaling).
    expected_remote_id: Rc<RefCell<String>>,
    /// Whether the remote peer has been authenticated via the handshake.
    peer_authenticated: Rc<RefCell<bool>>,
}

impl WebRtcConnection {
    /// Create a new WebRTC connection.
    ///
    /// `identity`       – our long-term Ed25519 identity (its public key hex is
    ///                    our own `peer_id`).
    /// `remote_peer_id` – the identity (hex) we expect on the other end, as
    ///                    learned from signaling.
    /// `stun_servers`   – list of STUN server URIs for ICE.
    pub fn new(
        identity: Rc<Identity>,
        remote_peer_id: &str,
        stun_servers: &[String],
    ) -> Result<Self, SdkError> {
        // Create RTC configuration with STUN servers
        let config = RtcConfiguration::new();

        let ice_servers = js_sys::Array::new();
        for server in stun_servers {
            let ice_server = js_sys::Object::new();
            js_sys::Reflect::set(&ice_server, &"urls".into(), &JsValue::from_str(server))
                .map_err(|e| {
                    SdkError::new(
                        codes::WEBRTC_ERROR,
                        &format!("Failed to set ICE server: {:?}", e),
                    )
                })?;
            ice_servers.push(&ice_server);
        }
        config.set_ice_servers(&ice_servers);

        let pc = RtcPeerConnection::new_with_configuration(&config).map_err(|e| {
            SdkError::new(
                codes::WEBRTC_ERROR,
                &format!("Failed to create PeerConnection: {:?}", e),
            )
        })?;

        let state = Rc::new(RefCell::new(WebRtcState::New));
        let data_channel: Rc<RefCell<Option<RtcDataChannel>>> = Rc::new(RefCell::new(None));
        let on_ice_candidate: IceCandidateCallback = Rc::new(RefCell::new(None));
        let on_message: MessageCallback = Rc::new(RefCell::new(None));

        // Fresh ephemeral keypair for this connection; cipher is established
        // only after the authenticated handshake completes.
        let ephemeral = Rc::new(EphemeralKeypair::generate());
        let cipher: ChannelCipher = Rc::new(RefCell::new(None));
        let peer_authenticated = Rc::new(RefCell::new(false));
        let expected_remote_id = Rc::new(RefCell::new(remote_peer_id.to_string()));

        // -----------------------------------------------------------------
        // ICE candidate handler
        // -----------------------------------------------------------------
        let on_ice = on_ice_candidate.clone();
        let onicecandidate = Closure::wrap(Box::new(move |e: RtcPeerConnectionIceEvent| {
            if let Some(candidate) = e.candidate() {
                let candidate_str = candidate.candidate();
                let sdp_mid = candidate.sdp_mid();
                let sdp_mline_index = candidate.sdp_m_line_index();

                if let Some(ref callback) = *on_ice.borrow() {
                    callback(candidate_str, sdp_mid, sdp_mline_index);
                }
            }
        }) as Box<dyn FnMut(_)>);
        pc.set_onicecandidate(Some(onicecandidate.as_ref().unchecked_ref()));
        onicecandidate.forget();

        // -----------------------------------------------------------------
        // ondatachannel handler (answering / receiving side)
        // -----------------------------------------------------------------
        let dc_ref = data_channel.clone();
        let state_dc = state.clone();
        let on_msg = on_message.clone();
        let identity_dc = identity.clone();
        let ephemeral_dc = ephemeral.clone();
        let cipher_dc = cipher.clone();
        let expected_dc = expected_remote_id.clone();
        let auth_dc = peer_authenticated.clone();

        let ondatachannel = Closure::wrap(Box::new(move |e: RtcDataChannelEvent| {
            let channel = e.channel();
            Self::setup_data_channel_handlers(
                &channel,
                state_dc.clone(),
                on_msg.clone(),
                identity_dc.clone(),
                ephemeral_dc.clone(),
                cipher_dc.clone(),
                expected_dc.clone(),
                auth_dc.clone(),
            );
            *dc_ref.borrow_mut() = Some(channel);
        }) as Box<dyn FnMut(_)>);
        pc.set_ondatachannel(Some(ondatachannel.as_ref().unchecked_ref()));
        ondatachannel.forget();

        Ok(Self {
            pc,
            data_channel,
            state,
            on_ice_candidate,
            on_message,
            identity,
            ephemeral,
            cipher,
            expected_remote_id,
            peer_authenticated,
        })
    }

    // -----------------------------------------------------------------
    // Data-channel event wiring (shared by initiator & answerer)
    // -----------------------------------------------------------------
    #[allow(clippy::too_many_arguments)]
    fn setup_data_channel_handlers(
        channel: &RtcDataChannel,
        state: Rc<RefCell<WebRtcState>>,
        on_message: MessageCallback,
        identity: Rc<Identity>,
        ephemeral: Rc<EphemeralKeypair>,
        cipher: ChannelCipher,
        expected_remote_id: Rc<RefCell<String>>,
        peer_authenticated: Rc<RefCell<bool>>,
    ) {
        // ── onopen: send our (plaintext, signed) handshake ──────────────
        let state_open = state.clone();
        let identity_open = identity.clone();
        let ephemeral_open = ephemeral.clone();
        let channel_for_open = channel.clone();

        let onopen = Closure::wrap(Box::new(move |_: web_sys::Event| {
            *state_open.borrow_mut() = WebRtcState::Connected;
            tracing::info!("DataChannel opened – sending authenticated handshake");

            let hs_frame = build_handshake(&identity_open, &ephemeral_open);
            if let Err(e) = channel_for_open.send_with_u8_array(&hs_frame) {
                tracing::error!("Failed to send handshake: {:?}", e);
            }
        }) as Box<dyn FnMut(_)>);
        channel.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        // ── onclose ─────────────────────────────────────────────────────
        let state_close = state.clone();
        let onclose = Closure::wrap(Box::new(move |_: web_sys::Event| {
            *state_close.borrow_mut() = WebRtcState::Disconnected;
            tracing::info!("DataChannel closed");
        }) as Box<dyn FnMut(_)>);
        channel.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        // ── onerror ─────────────────────────────────────────────────────
        let state_error = state.clone();
        let onerror = Closure::wrap(Box::new(move |_: web_sys::Event| {
            *state_error.borrow_mut() = WebRtcState::Failed;
            tracing::error!("DataChannel error");
        }) as Box<dyn FnMut(_)>);
        channel.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        // ── onmessage: handshake, then decrypt & demux ──────────────────
        let ephemeral_msg = ephemeral.clone();
        let cipher_msg = cipher.clone();
        let expected_msg = expected_remote_id.clone();
        let auth_msg = peer_authenticated.clone();
        let state_msg = state.clone();
        let channel_for_msg = channel.clone();

        let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
            // Extract raw bytes from the JS MessageEvent.
            let raw = if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                js_sys::Uint8Array::new(&buf).to_vec()
            } else {
                return;
            };

            // Until the peer is authenticated, the first message MUST be a
            // valid, correctly-signed handshake frame from the expected peer.
            if !*auth_msg.borrow() {
                match verify_handshake(&raw) {
                    Ok(peer) => {
                        let expected = expected_msg.borrow();
                        if peer.peer_id != *expected {
                            tracing::error!(
                                "Peer authentication failed: expected '{}', got '{}'",
                                *expected,
                                peer.peer_id
                            );
                            channel_for_msg.close();
                            *state_msg.borrow_mut() = WebRtcState::Failed;
                            return;
                        }

                        // Derive the channel key from the ephemeral ECDH.
                        let key = derive_channel_key(&ephemeral_msg, &peer.ephemeral_public);
                        *cipher_msg.borrow_mut() = Some(DataChannelCipher::new(&key));

                        tracing::info!(
                            "Peer '{}' authenticated; channel key established via ECDH",
                            peer.peer_id
                        );
                        *auth_msg.borrow_mut() = true;
                        *state_msg.borrow_mut() = WebRtcState::Authenticated;
                    }
                    Err(e) => {
                        tracing::error!("Handshake verification failed: {}", e);
                        channel_for_msg.close();
                        *state_msg.borrow_mut() = WebRtcState::Failed;
                    }
                }
                return;
            }

            // Normal data frame – decrypt with the derived key and deliver.
            let cipher_ref = cipher_msg.borrow();
            let Some(active_cipher) = cipher_ref.as_ref() else {
                tracing::warn!("Data frame received before channel key was established");
                return;
            };
            match decrypt_data(active_cipher, &raw) {
                Ok(plaintext) => {
                    if let Some(ref callback) = *on_message.borrow() {
                        callback(plaintext);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to decrypt DataChannel message: {}", e);
                }
            }
        }) as Box<dyn FnMut(_)>);
        channel.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();
    }

    // -----------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------

    /// Set ICE candidate callback.
    pub fn on_ice_candidate<F>(&self, callback: F)
    where
        F: Fn(String, Option<String>, Option<u16>) + 'static,
    {
        *self.on_ice_candidate.borrow_mut() = Some(Box::new(callback));
    }

    /// Set message received callback (receives *decrypted* application data).
    pub fn on_message<F>(&self, callback: F)
    where
        F: Fn(Vec<u8>) + 'static,
    {
        *self.on_message.borrow_mut() = Some(Box::new(callback));
    }

    /// Create an offer (initiator).
    pub async fn create_offer(&self) -> Result<String, SdkError> {
        *self.state.borrow_mut() = WebRtcState::Connecting;

        // Create data channel.
        let channel = self.pc.create_data_channel("shadowmesh");
        Self::setup_data_channel_handlers(
            &channel,
            self.state.clone(),
            self.on_message.clone(),
            self.identity.clone(),
            self.ephemeral.clone(),
            self.cipher.clone(),
            self.expected_remote_id.clone(),
            self.peer_authenticated.clone(),
        );
        *self.data_channel.borrow_mut() = Some(channel);

        // Create offer.
        let offer = wasm_bindgen_futures::JsFuture::from(self.pc.create_offer())
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to create offer: {:?}", e),
                )
            })?;

        let offer_obj = offer.unchecked_into::<RtcSessionDescriptionInit>();
        wasm_bindgen_futures::JsFuture::from(self.pc.set_local_description(&offer_obj))
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to set local description: {:?}", e),
                )
            })?;

        let local_desc = self
            .pc
            .local_description()
            .ok_or_else(|| SdkError::new(codes::WEBRTC_ERROR, "No local description"))?;

        Ok(local_desc.sdp())
    }

    /// Create an answer (responder).
    pub async fn create_answer(&self, offer_sdp: &str) -> Result<String, SdkError> {
        *self.state.borrow_mut() = WebRtcState::Connecting;

        let remote_desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        remote_desc.set_sdp(offer_sdp);
        wasm_bindgen_futures::JsFuture::from(self.pc.set_remote_description(&remote_desc))
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to set remote description: {:?}", e),
                )
            })?;

        let answer = wasm_bindgen_futures::JsFuture::from(self.pc.create_answer())
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to create answer: {:?}", e),
                )
            })?;

        let answer_obj = answer.unchecked_into::<RtcSessionDescriptionInit>();
        wasm_bindgen_futures::JsFuture::from(self.pc.set_local_description(&answer_obj))
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to set local description: {:?}", e),
                )
            })?;

        let local_desc = self
            .pc
            .local_description()
            .ok_or_else(|| SdkError::new(codes::WEBRTC_ERROR, "No local description"))?;

        Ok(local_desc.sdp())
    }

    /// Set remote answer (for initiator).
    pub async fn set_remote_answer(&self, answer_sdp: &str) -> Result<(), SdkError> {
        let remote_desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        remote_desc.set_sdp(answer_sdp);
        wasm_bindgen_futures::JsFuture::from(self.pc.set_remote_description(&remote_desc))
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to set remote description: {:?}", e),
                )
            })?;
        Ok(())
    }

    /// Add ICE candidate.
    pub async fn add_ice_candidate(
        &self,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
    ) -> Result<(), SdkError> {
        let init = RtcIceCandidateInit::new(candidate);
        if let Some(mid) = sdp_mid {
            init.set_sdp_mid(Some(mid));
        }
        if let Some(index) = sdp_mline_index {
            init.set_sdp_m_line_index(Some(index));
        }

        let ice_candidate = RtcIceCandidate::new(&init).map_err(|e| {
            SdkError::new(
                codes::WEBRTC_ERROR,
                &format!("Invalid ICE candidate: {:?}", e),
            )
        })?;

        wasm_bindgen_futures::JsFuture::from(
            self.pc
                .add_ice_candidate_with_opt_rtc_ice_candidate(Some(&ice_candidate)),
        )
        .await
        .map_err(|e| {
            SdkError::new(
                codes::WEBRTC_ERROR,
                &format!("Failed to add ICE candidate: {:?}", e),
            )
        })?;

        Ok(())
    }

    /// Send application data over the encrypted DataChannel.
    ///
    /// The payload is wrapped in a `DATA_TAG` frame and encrypted with the
    /// ECDH-derived ChaCha20-Poly1305 key. Fails if the authenticated handshake
    /// has not yet completed (no channel key established).
    pub fn send(&self, data: &[u8]) -> Result<(), SdkError> {
        let channel = self.data_channel.borrow();
        let channel = channel
            .as_ref()
            .ok_or_else(|| SdkError::new(codes::NOT_CONNECTED, "Data channel not ready"))?;

        if channel.ready_state() != RtcDataChannelState::Open {
            return Err(SdkError::new(codes::NOT_CONNECTED, "Data channel not open"));
        }

        // The channel key is only available after the authenticated handshake.
        let cipher_ref = self.cipher.borrow();
        let cipher = cipher_ref.as_ref().ok_or_else(|| {
            SdkError::new(
                codes::NOT_CONNECTED,
                "Cannot send: authenticated handshake not yet complete",
            )
        })?;

        let encrypted = encrypt_data(cipher, data)?;

        channel
            .send_with_u8_array(&encrypted)
            .map_err(|e| SdkError::new(codes::WEBRTC_ERROR, &format!("Send failed: {:?}", e)))?;

        Ok(())
    }

    /// Get connection state.
    pub fn state(&self) -> WebRtcState {
        *self.state.borrow()
    }

    /// Check if connected (DataChannel open, regardless of handshake).
    pub fn is_connected(&self) -> bool {
        let s = *self.state.borrow();
        s == WebRtcState::Connected || s == WebRtcState::Authenticated
    }

    /// Check if the remote peer has been authenticated via the handshake.
    pub fn is_authenticated(&self) -> bool {
        *self.peer_authenticated.borrow()
    }

    /// Close the connection.
    pub fn close(&self) {
        if let Some(channel) = self.data_channel.borrow().as_ref() {
            channel.close();
        }
        self.pc.close();
        *self.state.borrow_mut() = WebRtcState::Disconnected;
    }
}

impl Drop for WebRtcConnection {
    fn drop(&mut self) {
        self.close();
    }
}
