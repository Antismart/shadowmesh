//! Utility functions for the browser SDK

use wasm_bindgen::prelude::*;
use web_sys::Window;

/// Get the browser window object
pub fn window() -> Result<Window, JsValue> {
    web_sys::window().ok_or_else(|| JsValue::from_str("No window object available"))
}

/// Generate a random session ID
pub fn generate_session_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom(&mut bytes);
    hex::encode(&bytes)
}

/// Get random bytes using Web Crypto API
fn getrandom(dest: &mut [u8]) {
    let win = match window() {
        Ok(w) => w,
        Err(_) => {
            // Fallback: use getrandom crate which works in WASM contexts
            // without window (e.g. web workers, Node.js)
            tracing::warn!("No window object, using getrandom fallback");
            if getrandom::getrandom(dest).is_err() {
                tracing::error!("All random sources failed — session IDs will be insecure");
                // Last resort: use index-based fill (NOT cryptographically secure)
                for (i, byte) in dest.iter_mut().enumerate() {
                    *byte = (i as u8).wrapping_mul(137).wrapping_add(43);
                }
            }
            return;
        }
    };

    let crypto = match win.crypto() {
        Ok(c) => c,
        Err(_) => {
            tracing::warn!("No Web Crypto API available");
            return;
        }
    };

    if let Err(e) = crypto.get_random_values_with_u8_array(dest) {
        tracing::warn!("get_random_values failed: {:?}", e);
    }
}

/// Hex encoding (simple implementation)
mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut result = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            result.push(HEX_CHARS[(byte >> 4) as usize] as char);
            result.push(HEX_CHARS[(byte & 0x0f) as usize] as char);
        }
        result
    }
}

/// Log to browser console
pub fn console_log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// Log error to browser console
pub fn console_error(msg: &str) {
    web_sys::console::error_1(&JsValue::from_str(msg));
}
