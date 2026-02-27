//! Error types for the browser SDK

use wasm_bindgen::prelude::*;

/// SDK error type
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct SdkError {
    code: String,
    message: String,
}

#[wasm_bindgen]
impl SdkError {
    /// Create a new error
    #[wasm_bindgen(constructor)]
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    /// Get error code
    #[wasm_bindgen(getter)]
    pub fn code(&self) -> String {
        self.code.clone()
    }

    /// Get error message
    #[wasm_bindgen(getter)]
    pub fn message(&self) -> String {
        self.message.clone()
    }
}

impl std::fmt::Display for SdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SdkError {}

impl From<JsValue> for SdkError {
    fn from(value: JsValue) -> Self {
        let message = value
            .as_string()
            .unwrap_or_else(|| "Unknown JS error".to_string());
        Self::new("JS_ERROR", &message)
    }
}

impl SdkError {
    /// Convert to JsValue
    pub fn to_js(&self) -> JsValue {
        JsValue::from_str(&self.to_string())
    }
}

/// Error codes
pub mod codes {
    pub const CONNECTION_FAILED: &str = "CONNECTION_FAILED";
    pub const SIGNALING_ERROR: &str = "SIGNALING_ERROR";
    pub const WEBRTC_ERROR: &str = "WEBRTC_ERROR";
    pub const CONTENT_NOT_FOUND: &str = "CONTENT_NOT_FOUND";
    pub const TIMEOUT: &str = "TIMEOUT";
    pub const INVALID_CONFIG: &str = "INVALID_CONFIG";
    pub const NOT_CONNECTED: &str = "NOT_CONNECTED";
    pub const INTEGRITY_ERROR: &str = "INTEGRITY_ERROR";
}
