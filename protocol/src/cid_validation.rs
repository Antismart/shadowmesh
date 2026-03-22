//! Strict CID validation for the ShadowMesh protocol.
//!
//! All CID inputs — from URL paths, pin endpoints, and IPFS API calls — MUST
//! be validated through [`validate_cid`] (bare CID) or [`validate_cid_path`]
//! (CID with optional subpath) before being forwarded to IPFS or used in
//! responses.
//!
//! ## Format rules
//!
//! | Version | Prefix | Length | Alphabet               |
//! |---------|--------|--------|------------------------|
//! | CIDv0   | `Qm`   | 46     | Base58 (no `0OIl`)     |
//! | CIDv1   | `bafy`  | 59     | Base32lower (`a-z2-7`) |
//!
//! ## Path rules
//!
//! - Full path (CID + subpath) must be <= 512 bytes.
//! - No `.` or `..` path segments (directory traversal).
//! - No control characters (bytes < 0x20).

/// Maximum total length for a CID path (CID + `/` + subpath).
const MAX_CID_PATH_LEN: usize = 512;

/// Base58 alphabet used by CIDv0 — standard Bitcoin alphabet.
/// Excludes `0`, `O`, `I`, `l`.
const BASE58_CHARS: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Base32-lower alphabet used by CIDv1 multibase encoding.
const BASE32_LOWER_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

// ── helpers ────────────────────────────────────────────────────────────

fn is_base58(c: u8) -> bool {
    BASE58_CHARS.contains(&c)
}

fn is_base32_lower(c: u8) -> bool {
    BASE32_LOWER_CHARS.contains(&c)
}

/// Returns `true` when every byte in `s` belongs to the Base58 alphabet.
fn all_base58(s: &str) -> bool {
    s.bytes().all(is_base58)
}

/// Returns `true` when every byte in `s` belongs to the Base32-lower alphabet.
fn all_base32_lower(s: &str) -> bool {
    s.bytes().all(is_base32_lower)
}

// ── public API ─────────────────────────────────────────────────────────

/// Validate a bare CID (no subpath).
///
/// Returns `true` when `cid` is a well-formed CIDv0 or CIDv1 string.
pub fn validate_cid(cid: &str) -> bool {
    if cid.is_empty() {
        return false;
    }

    // CIDv0: starts with "Qm", exactly 46 chars, all base58
    if cid.starts_with("Qm") {
        return cid.len() == 46 && all_base58(cid);
    }

    // CIDv1: starts with "bafy", exactly 59 chars, all base32-lower
    if cid.starts_with("bafy") {
        return cid.len() == 59 && all_base32_lower(cid);
    }

    false
}

/// Validate a CID path that may contain an optional subpath after the CID.
///
/// Accepts:
/// - `"QmXXX..."` (bare CID)
/// - `"QmXXX.../some/subpath"` (CID with directory subpath)
///
/// Rejects:
/// - Paths longer than 512 bytes
/// - Paths containing `.` or `..` segments
/// - Paths containing control characters (bytes < 0x20)
/// - CIDs that don't match CIDv0 or CIDv1 format
///
/// On success returns `(cid, optional_subpath)`.
pub fn validate_cid_path(path: &str) -> Result<(&str, Option<&str>), CidValidationError> {
    if path.is_empty() {
        return Err(CidValidationError::Empty);
    }

    if path.len() > MAX_CID_PATH_LEN {
        return Err(CidValidationError::TooLong);
    }

    // Reject control characters anywhere in the path
    if path.bytes().any(|b| b < 0x20) {
        return Err(CidValidationError::ControlCharacter);
    }

    // Split into CID and optional subpath
    let (cid, subpath) = match path.find('/') {
        Some(pos) => (&path[..pos], Some(&path[pos + 1..])),
        None => (path, None),
    };

    // Validate the CID portion
    if !validate_cid(cid) {
        return Err(CidValidationError::InvalidCid);
    }

    // Validate subpath segments: reject `.` and `..`
    if let Some(sub) = subpath {
        for segment in sub.split('/') {
            if segment == "." || segment == ".." {
                return Err(CidValidationError::TraversalAttempt);
            }
        }
    }

    Ok((cid, subpath))
}

/// Validate a content path used by the gateway's unified `/ipfs/*` endpoint.
///
/// This is intentionally more permissive than [`validate_cid_path`] because
/// the gateway resolves content from multiple backends (IPFS, P2P, node-runners),
/// some of which use non-CID identifiers (e.g. blake3 hex hashes).
///
/// Accepts:
/// - Standard CIDs (CIDv0/CIDv1)
/// - Hex content hashes (alphanumeric, 1-128 chars)
/// - Optional subpaths after the identifier
///
/// Rejects:
/// - Paths longer than 512 bytes
/// - `.` or `..` segments (directory traversal)
/// - Control characters (bytes < 0x20)
/// - Non-alphanumeric characters in the content identifier portion
///
/// On success returns `(content_id, optional_subpath)`.
pub fn validate_content_path(path: &str) -> Result<(&str, Option<&str>), CidValidationError> {
    if path.is_empty() {
        return Err(CidValidationError::Empty);
    }

    if path.len() > MAX_CID_PATH_LEN {
        return Err(CidValidationError::TooLong);
    }

    // Reject control characters anywhere in the path
    if path.bytes().any(|b| b < 0x20) {
        return Err(CidValidationError::ControlCharacter);
    }

    // Split into content ID and optional subpath
    let (content_id, subpath) = match path.find('/') {
        Some(pos) => (&path[..pos], Some(&path[pos + 1..])),
        None => (path, None),
    };

    // Content ID must be non-empty, alphanumeric only, and reasonable length
    if content_id.is_empty()
        || content_id.len() > 128
        || !content_id.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(CidValidationError::InvalidCid);
    }

    // Validate subpath segments: reject `.` and `..`
    if let Some(sub) = subpath {
        for segment in sub.split('/') {
            if segment == "." || segment == ".." {
                return Err(CidValidationError::TraversalAttempt);
            }
        }
    }

    Ok((content_id, subpath))
}

/// Errors returned by CID validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CidValidationError {
    /// Input was empty.
    Empty,
    /// Full path exceeds 512 bytes.
    TooLong,
    /// Path contains control characters (bytes < 0x20).
    ControlCharacter,
    /// CID does not match CIDv0 or CIDv1 format.
    InvalidCid,
    /// Subpath contains `.` or `..` segments (directory traversal).
    TraversalAttempt,
}

impl std::fmt::Display for CidValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "CID path is empty"),
            Self::TooLong => write!(f, "CID path exceeds maximum length of {MAX_CID_PATH_LEN}"),
            Self::ControlCharacter => write!(f, "CID path contains control characters"),
            Self::InvalidCid => write!(f, "Invalid CID format (expected CIDv0 Qm... or CIDv1 bafy...)"),
            Self::TraversalAttempt => write!(f, "CID subpath contains directory traversal (. or ..)"),
        }
    }
}

impl std::error::Error for CidValidationError {}

// ── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Valid CIDv0 (46 chars, starts with Qm, base58)
    const VALID_V0: &str = "QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
    // Valid CIDv1 (59 chars, starts with bafy, base32lower)
    const VALID_V1: &str = "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi";

    #[test]
    fn valid_cidv0() {
        assert!(validate_cid(VALID_V0));
    }

    #[test]
    fn valid_cidv1() {
        assert!(validate_cid(VALID_V1));
    }

    #[test]
    fn reject_empty() {
        assert!(!validate_cid(""));
    }

    #[test]
    fn reject_cidv0_wrong_length() {
        // 45 chars
        assert!(!validate_cid("QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbd"));
        // 47 chars
        assert!(!validate_cid("QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdGx"));
    }

    #[test]
    fn reject_cidv0_invalid_chars() {
        // 'O' is not in base58 — replace last char
        let mut bad = VALID_V0.to_string();
        bad.replace_range(45..46, "O");
        assert!(!validate_cid(&bad));

        // '0' is not in base58
        let mut bad = VALID_V0.to_string();
        bad.replace_range(45..46, "0");
        assert!(!validate_cid(&bad));

        // 'I' is not in base58
        let mut bad = VALID_V0.to_string();
        bad.replace_range(45..46, "I");
        assert!(!validate_cid(&bad));

        // 'l' is not in base58
        let mut bad = VALID_V0.to_string();
        bad.replace_range(45..46, "l");
        assert!(!validate_cid(&bad));
    }

    #[test]
    fn reject_cidv1_wrong_length() {
        assert!(!validate_cid(&VALID_V1[..58]));
        assert!(!validate_cid(&format!("{}a", VALID_V1)));
    }

    #[test]
    fn reject_cidv1_invalid_chars() {
        // Uppercase is not base32-lower
        let mut bad = VALID_V1.to_string();
        bad.replace_range(58..59, "A");
        assert!(!validate_cid(&bad));

        // '8' is not in base32-lower
        let mut bad = VALID_V1.to_string();
        bad.replace_range(58..59, "8");
        assert!(!validate_cid(&bad));
    }

    #[test]
    fn reject_slash_in_bare_cid() {
        assert!(!validate_cid("Qm/wAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbd"));
    }

    #[test]
    fn reject_dot_in_bare_cid() {
        assert!(!validate_cid("Qm.wAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbd"));
    }

    #[test]
    fn reject_dash_underscore() {
        // These were previously allowed by the loose validation — now rejected
        assert!(!validate_cid("Qm-wAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPb"));
        assert!(!validate_cid("Qm_wAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPb"));
    }

    #[test]
    fn validate_cid_path_bare() {
        let (cid, sub) = validate_cid_path(VALID_V0).unwrap();
        assert_eq!(cid, VALID_V0);
        assert_eq!(sub, None);
    }

    #[test]
    fn validate_cid_path_with_subpath() {
        let input = format!("{}/index.html", VALID_V0);
        let (cid, sub) = validate_cid_path(&input).unwrap();
        assert_eq!(cid, VALID_V0);
        assert_eq!(sub, Some("index.html"));
    }

    #[test]
    fn validate_cid_path_nested_subpath() {
        let input = format!("{}/assets/css/style.css", VALID_V1);
        let (cid, sub) = validate_cid_path(&input).unwrap();
        assert_eq!(cid, VALID_V1);
        assert_eq!(sub, Some("assets/css/style.css"));
    }

    #[test]
    fn reject_traversal_dot_dot() {
        let input = format!("{}/../etc/passwd", VALID_V0);
        assert_eq!(
            validate_cid_path(&input).unwrap_err(),
            CidValidationError::TraversalAttempt
        );
    }

    #[test]
    fn reject_traversal_single_dot() {
        let input = format!("{}/./index.html", VALID_V0);
        assert_eq!(
            validate_cid_path(&input).unwrap_err(),
            CidValidationError::TraversalAttempt
        );
    }

    #[test]
    fn reject_control_characters() {
        let input = format!("{}/\x00evil", VALID_V0);
        assert_eq!(
            validate_cid_path(&input).unwrap_err(),
            CidValidationError::ControlCharacter
        );
    }

    #[test]
    fn reject_too_long_path() {
        let long_subpath = "a".repeat(MAX_CID_PATH_LEN);
        let input = format!("{}/{}", VALID_V0, long_subpath);
        assert_eq!(
            validate_cid_path(&input).unwrap_err(),
            CidValidationError::TooLong
        );
    }

    #[test]
    fn reject_unknown_prefix() {
        assert!(!validate_cid("zdpuAnmqATw4M9U3Y7b"));
        assert!(!validate_cid("hello-world"));
    }
}
