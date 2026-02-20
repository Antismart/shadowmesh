//! ShadowMesh Browser SDK
//!
//! WebRTC-enabled browser client for the ShadowMesh decentralized CDN.

// Allow dead code - this is a library where many items are exported for JS use
#![allow(dead_code)]
#![allow(unused_imports)]
//!
//! # Example
//!
//! ```javascript
//! import init, { ShadowMeshClient } from '@shadowmesh/browser';
//!
//! await init();
//!
//! const client = new ShadowMeshClient({
//!   signalingUrl: 'wss://gateway.shadowmesh.network/signaling/ws',
//!   stunServers: ['stun:stun.l.google.com:19302']
//! });
//!
//! await client.connect();
//! const content = await client.fetch('QmXxx...');
//! await client.disconnect();
//! ```

use wasm_bindgen::prelude::*;

mod client;
mod error;
pub mod naming;
mod signaling;
mod utils;
mod webrtc;

pub use client::ShadowMeshClient;
pub use error::SdkError;
pub use naming::WasmNameResolver;

/// Initialize the SDK (call once before using)
#[wasm_bindgen(start)]
pub fn init() {
    // Set up better panic messages
    console_error_panic_hook::set_once();

    // Initialize tracing for WASM
    tracing_wasm::set_as_global_default();

    tracing::info!("ShadowMesh Browser SDK initialized");
}

/// SDK version
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
