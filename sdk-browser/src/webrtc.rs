//! WebRTC peer connection management

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
    Disconnected,
    Failed,
}

/// Type alias for ICE candidate callback
type IceCandidateCallback = Rc<RefCell<Option<Box<dyn Fn(String, Option<String>, Option<u16>)>>>>;

/// Type alias for message callback
type MessageCallback = Rc<RefCell<Option<Box<dyn Fn(Vec<u8>)>>>>;

/// WebRTC peer connection wrapper
pub struct WebRtcConnection {
    pc: RtcPeerConnection,
    data_channel: Rc<RefCell<Option<RtcDataChannel>>>,
    state: Rc<RefCell<WebRtcState>>,
    on_ice_candidate: IceCandidateCallback,
    on_message: MessageCallback,
}

impl WebRtcConnection {
    /// Create a new WebRTC connection
    pub fn new(stun_servers: &[String]) -> Result<Self, SdkError> {
        // Create RTC configuration with STUN servers
        let config = RtcConfiguration::new();

        // Create ICE servers array
        let ice_servers = js_sys::Array::new();
        for server in stun_servers {
            let ice_server = js_sys::Object::new();
            js_sys::Reflect::set(&ice_server, &"urls".into(), &JsValue::from_str(server)).map_err(
                |e| {
                    SdkError::new(
                        codes::WEBRTC_ERROR,
                        &format!("Failed to set ICE server: {:?}", e),
                    )
                },
            )?;
            ice_servers.push(&ice_server);
        }
        config.set_ice_servers(&ice_servers);

        // Create peer connection
        let pc = RtcPeerConnection::new_with_configuration(&config).map_err(|e| {
            SdkError::new(
                codes::WEBRTC_ERROR,
                &format!("Failed to create PeerConnection: {:?}", e),
            )
        })?;

        let state = Rc::new(RefCell::new(WebRtcState::New));
        let data_channel = Rc::new(RefCell::new(None));
        let on_ice_candidate: IceCandidateCallback = Rc::new(RefCell::new(None));
        let on_message: MessageCallback = Rc::new(RefCell::new(None));

        // Set up ICE candidate handler
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

        // Set up data channel handler (for answering peer)
        let dc_ref = data_channel.clone();
        let state_dc = state.clone();
        let on_msg = on_message.clone();
        let ondatachannel = Closure::wrap(Box::new(move |e: RtcDataChannelEvent| {
            let channel = e.channel();
            Self::setup_data_channel_handlers(&channel, state_dc.clone(), on_msg.clone());
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
        })
    }

    /// Set up data channel event handlers
    fn setup_data_channel_handlers(
        channel: &RtcDataChannel,
        state: Rc<RefCell<WebRtcState>>,
        on_message: MessageCallback,
    ) {
        let state_open = state.clone();
        let onopen = Closure::wrap(Box::new(move |_: web_sys::Event| {
            *state_open.borrow_mut() = WebRtcState::Connected;
            tracing::info!("DataChannel opened");
        }) as Box<dyn FnMut(_)>);
        channel.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let state_close = state.clone();
        let onclose = Closure::wrap(Box::new(move |_: web_sys::Event| {
            *state_close.borrow_mut() = WebRtcState::Disconnected;
            tracing::info!("DataChannel closed");
        }) as Box<dyn FnMut(_)>);
        channel.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        let state_error = state.clone();
        let onerror = Closure::wrap(Box::new(move |_: web_sys::Event| {
            *state_error.borrow_mut() = WebRtcState::Failed;
            tracing::error!("DataChannel error");
        }) as Box<dyn FnMut(_)>);
        channel.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let onmessage = Closure::wrap(Box::new(move |e: web_sys::MessageEvent| {
            // Handle binary data
            if let Ok(array_buffer) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                let array = js_sys::Uint8Array::new(&array_buffer);
                let data = array.to_vec();
                if let Some(ref callback) = *on_message.borrow() {
                    callback(data);
                }
            }
        }) as Box<dyn FnMut(_)>);
        channel.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();
    }

    /// Set ICE candidate callback
    pub fn on_ice_candidate<F>(&self, callback: F)
    where
        F: Fn(String, Option<String>, Option<u16>) + 'static,
    {
        *self.on_ice_candidate.borrow_mut() = Some(Box::new(callback));
    }

    /// Set message received callback
    pub fn on_message<F>(&self, callback: F)
    where
        F: Fn(Vec<u8>) + 'static,
    {
        *self.on_message.borrow_mut() = Some(Box::new(callback));
    }

    /// Create an offer (initiator)
    pub async fn create_offer(&self) -> Result<String, SdkError> {
        *self.state.borrow_mut() = WebRtcState::Connecting;

        // Create data channel
        let channel = self.pc.create_data_channel("shadowmesh");
        Self::setup_data_channel_handlers(&channel, self.state.clone(), self.on_message.clone());
        *self.data_channel.borrow_mut() = Some(channel);

        // Create offer
        let offer = wasm_bindgen_futures::JsFuture::from(self.pc.create_offer())
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to create offer: {:?}", e),
                )
            })?;

        // Set local description
        let offer_obj = offer.unchecked_into::<RtcSessionDescriptionInit>();
        wasm_bindgen_futures::JsFuture::from(self.pc.set_local_description(&offer_obj))
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to set local description: {:?}", e),
                )
            })?;

        // Get SDP string
        let local_desc = self
            .pc
            .local_description()
            .ok_or_else(|| SdkError::new(codes::WEBRTC_ERROR, "No local description"))?;

        Ok(local_desc.sdp())
    }

    /// Create an answer (responder)
    pub async fn create_answer(&self, offer_sdp: &str) -> Result<String, SdkError> {
        *self.state.borrow_mut() = WebRtcState::Connecting;

        // Set remote description (the offer)
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

        // Create answer
        let answer = wasm_bindgen_futures::JsFuture::from(self.pc.create_answer())
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to create answer: {:?}", e),
                )
            })?;

        // Set local description
        let answer_obj = answer.unchecked_into::<RtcSessionDescriptionInit>();
        wasm_bindgen_futures::JsFuture::from(self.pc.set_local_description(&answer_obj))
            .await
            .map_err(|e| {
                SdkError::new(
                    codes::WEBRTC_ERROR,
                    &format!("Failed to set local description: {:?}", e),
                )
            })?;

        // Get SDP string
        let local_desc = self
            .pc
            .local_description()
            .ok_or_else(|| SdkError::new(codes::WEBRTC_ERROR, "No local description"))?;

        Ok(local_desc.sdp())
    }

    /// Set remote answer (for initiator)
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

    /// Add ICE candidate
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

    /// Send data over the data channel
    pub fn send(&self, data: &[u8]) -> Result<(), SdkError> {
        let channel = self.data_channel.borrow();
        let channel = channel
            .as_ref()
            .ok_or_else(|| SdkError::new(codes::NOT_CONNECTED, "Data channel not ready"))?;

        if channel.ready_state() != RtcDataChannelState::Open {
            return Err(SdkError::new(codes::NOT_CONNECTED, "Data channel not open"));
        }

        channel
            .send_with_u8_array(data)
            .map_err(|e| SdkError::new(codes::WEBRTC_ERROR, &format!("Send failed: {:?}", e)))?;

        Ok(())
    }

    /// Get connection state
    pub fn state(&self) -> WebRtcState {
        *self.state.borrow()
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        *self.state.borrow() == WebRtcState::Connected
    }

    /// Close the connection
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
