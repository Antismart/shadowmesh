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

/// Get random bytes securely.
///
/// Uses `crypto.getRandomValues()` via the Web Crypto API when a `window`
/// object is available (main thread). Falls back to the `getrandom` crate
/// (configured with `features = ["js"]`) which also calls
/// `crypto.getRandomValues()` under the hood but works in Web Workers and
/// other non-window contexts.
///
/// **No insecure fallback** — if both sources fail the function panics,
/// because generating predictable "random" bytes silently would be a
/// security vulnerability (session IDs, encryption nonces, etc.).
fn getrandom(dest: &mut [u8]) {
    // Try the Web Crypto API directly (available on the main thread)
    if let Ok(win) = window() {
        if let Ok(crypto) = win.crypto() {
            if crypto.get_random_values_with_u8_array(dest).is_ok() {
                return;
            }
        }
    }

    // Fallback: `getrandom` crate with `js` feature — delegates to
    // `crypto.getRandomValues()` via wasm-bindgen, works in Web Workers.
    getrandom::getrandom(dest).expect(
        "FATAL: no secure random source available — \
         crypto.getRandomValues() is required in this browser environment",
    );
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
