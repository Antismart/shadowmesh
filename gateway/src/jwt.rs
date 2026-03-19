//! Lightweight JWT (HS256) implementation for cross-gateway auth tokens.
//!
//! Tokens are signed with HMAC-SHA256 using `SHADOWMESH_AUTH_SECRET`.
//! Any gateway that shares the same secret can verify tokens without
//! contacting the auth gateway.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Claims embedded in every auth JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// GitHub username
    pub sub: String,
    /// GitHub login handle
    pub login: String,
    /// Avatar URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// Encrypted GitHub access token (so gateways can call GitHub API)
    pub github_token: String,
    /// Issued-at (unix seconds)
    pub iat: u64,
    /// Expiry (unix seconds)
    pub exp: u64,
}

/// Create a signed JWT from the given claims.
pub fn encode(claims: &Claims, secret: &[u8]) -> Result<String, String> {
    let header = B64.encode(b"{\"alg\":\"HS256\",\"typ\":\"JWT\"}");
    let payload = B64.encode(
        serde_json::to_vec(claims).map_err(|e| format!("serialize claims: {e}"))?,
    );

    let signing_input = format!("{header}.{payload}");

    let mut mac =
        HmacSha256::new_from_slice(secret).map_err(|e| format!("hmac init: {e}"))?;
    mac.update(signing_input.as_bytes());
    let signature = B64.encode(mac.finalize().into_bytes());

    Ok(format!("{signing_input}.{signature}"))
}

/// Verify a JWT and return the decoded claims.
pub fn decode(token: &str, secret: &[u8]) -> Result<Claims, JwtError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(JwtError::MalformedToken);
    }

    // Verify signature
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| JwtError::InvalidSecret)?;
    mac.update(signing_input.as_bytes());

    let sig_bytes = B64.decode(parts[2]).map_err(|_| JwtError::MalformedToken)?;
    mac.verify_slice(&sig_bytes)
        .map_err(|_| JwtError::InvalidSignature)?;

    // Decode payload
    let payload_bytes = B64.decode(parts[1]).map_err(|_| JwtError::MalformedToken)?;
    let claims: Claims =
        serde_json::from_slice(&payload_bytes).map_err(|_| JwtError::InvalidPayload)?;

    // Check expiry
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if claims.exp < now {
        return Err(JwtError::Expired);
    }

    Ok(claims)
}

/// Get the auth secret from environment, if configured.
pub fn auth_secret() -> Option<Vec<u8>> {
    std::env::var("SHADOWMESH_AUTH_SECRET")
        .ok()
        .filter(|s| s.len() >= 32)
        .map(|s| s.into_bytes())
}

#[derive(Debug)]
pub enum JwtError {
    MalformedToken,
    InvalidSecret,
    InvalidSignature,
    InvalidPayload,
    Expired,
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JwtError::MalformedToken => write!(f, "Malformed token"),
            JwtError::InvalidSecret => write!(f, "Invalid secret"),
            JwtError::InvalidSignature => write!(f, "Invalid signature"),
            JwtError::InvalidPayload => write!(f, "Invalid payload"),
            JwtError::Expired => write!(f, "Token expired"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let secret = b"test-secret-that-is-at-least-32-bytes-long!!";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let claims = Claims {
            sub: "testuser".into(),
            login: "testuser".into(),
            avatar_url: Some("https://example.com/avatar.png".into()),
            github_token: "gho_xxxx".into(),
            iat: now,
            exp: now + 3600,
        };

        let token = encode(&claims, secret).unwrap();
        let decoded = decode(&token, secret).unwrap();

        assert_eq!(decoded.sub, "testuser");
        assert_eq!(decoded.github_token, "gho_xxxx");
    }

    #[test]
    fn rejects_bad_signature() {
        let secret = b"test-secret-that-is-at-least-32-bytes-long!!";
        let wrong = b"wrong-secret-that-is-at-least-32-bytes-long!";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let claims = Claims {
            sub: "testuser".into(),
            login: "testuser".into(),
            avatar_url: None,
            github_token: "gho_xxxx".into(),
            iat: now,
            exp: now + 3600,
        };

        let token = encode(&claims, secret).unwrap();
        assert!(decode(&token, wrong).is_err());
    }

    #[test]
    fn rejects_expired() {
        let secret = b"test-secret-that-is-at-least-32-bytes-long!!";
        let claims = Claims {
            sub: "testuser".into(),
            login: "testuser".into(),
            avatar_url: None,
            github_token: "gho_xxxx".into(),
            iat: 1000,
            exp: 1001, // expired long ago
        };

        let token = encode(&claims, secret).unwrap();
        assert!(matches!(decode(&token, secret), Err(JwtError::Expired)));
    }
}
