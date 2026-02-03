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
    let crypto = window()
        .expect("window")
        .crypto()
        .expect("crypto");

    crypto
        .get_random_values_with_u8_array(dest)
        .expect("get_random_values");
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
#[allow(dead_code)]
pub fn console_log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

/// Log error to browser console
#[allow(dead_code)]
pub fn console_error(msg: &str) {
    web_sys::console::error_1(&JsValue::from_str(msg));
}
