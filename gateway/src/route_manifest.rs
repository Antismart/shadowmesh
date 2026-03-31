use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteManifest {
    pub version: u32,
    pub routes: Vec<RouteEntry>,
    #[serde(default)]
    pub static_patterns: Vec<String>,
    #[serde(default)]
    pub capabilities: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteEntry {
    pub path: String,
    pub handler: String,
    #[serde(default = "default_methods")]
    pub methods: Vec<String>,
}

fn default_methods() -> Vec<String> {
    vec!["GET".into()]
}

const VALID_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

pub fn parse_manifest(data: &[u8]) -> Result<RouteManifest, String> {
    let manifest: RouteManifest =
        serde_json::from_slice(data).map_err(|e| format!("Invalid manifest JSON: {}", e))?;

    if manifest.version != 1 {
        return Err(format!(
            "Unsupported manifest version: {} (expected 1)",
            manifest.version
        ));
    }

    for route in &manifest.routes {
        // Validate handler — no path traversal
        if route.handler.contains("..")
            || route.handler.contains('/')
            || route.handler.contains('\\')
            || route.handler.is_empty()
        {
            return Err(format!(
                "Invalid handler '{}': must be a filename without path separators",
                route.handler
            ));
        }
        // Validate methods
        for method in &route.methods {
            if !VALID_METHODS.contains(&method.to_uppercase().as_str()) {
                return Err(format!("Invalid HTTP method: {}", method));
            }
        }
    }

    Ok(manifest)
}

pub fn match_route<'a>(manifest: &'a RouteManifest, method: &str, path: &str) -> Option<&'a RouteEntry> {
    let path = path.trim_end_matches('/');
    let path = if path.is_empty() { "/" } else { path };
    let method_upper = method.to_uppercase();

    let mut best_match: Option<&RouteEntry> = None;
    let mut best_score: u8 = 0; // 0=none, 1=glob, 2=param, 3=exact

    for route in &manifest.routes {
        // Check method
        if !route.methods.iter().any(|m| m.to_uppercase() == method_upper) {
            continue;
        }

        let score = match_path_score(&route.path, path);
        if score > best_score {
            best_score = score;
            best_match = Some(route);
        }
    }

    best_match
}

fn match_path_score(pattern: &str, path: &str) -> u8 {
    let pattern = pattern.trim_end_matches('/');
    let pattern = if pattern.is_empty() { "/" } else { pattern };
    let path = if path.is_empty() { "/" } else { path };

    // Exact match
    if pattern == path {
        return 3;
    }

    let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Wildcard: /api/* matches /api/anything/nested
    if let Some(last) = pattern_parts.last() {
        if *last == "*" {
            let prefix_parts = &pattern_parts[..pattern_parts.len() - 1];
            if path_parts.len() >= prefix_parts.len()
                && prefix_parts
                    .iter()
                    .zip(path_parts.iter())
                    .all(|(a, b)| a == b)
            {
                return 1;
            }
        }
    }

    // Param segments: /ssr/:page matches /ssr/about
    if pattern_parts.len() == path_parts.len() {
        let all_match = pattern_parts.iter().zip(path_parts.iter()).all(|(p, v)| {
            p.starts_with(':') || *p == *v
        });
        if all_match {
            return 2;
        }
    }

    0
}

pub fn load_manifest_from_dir(dir: &Path) -> Option<RouteManifest> {
    let manifest_path = dir.join("_shadowmesh").join("routes.json");
    let data = std::fs::read(&manifest_path).ok()?;
    match parse_manifest(&data) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(path = %manifest_path.display(), error = %e, "failed to parse route manifest");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest_json() -> &'static [u8] {
        br#"{
            "version": 1,
            "routes": [
                {"path": "/api/*", "handler": "api.wasm", "methods": ["GET", "POST"]},
                {"path": "/ssr/:page", "handler": "ssr.wasm"},
                {"path": "/health", "handler": "health.wasm"}
            ],
            "static": ["/**"],
            "capabilities": {"api.wasm": ["net:connect", "kv:read"]}
        }"#
    }

    #[test]
    fn parse_valid_manifest() {
        let m = parse_manifest(valid_manifest_json()).unwrap();
        assert_eq!(m.version, 1);
        assert_eq!(m.routes.len(), 3);
        assert_eq!(m.routes[0].handler, "api.wasm");
        assert_eq!(m.routes[1].methods, vec!["GET"]);
    }

    #[test]
    fn reject_invalid_version() {
        let data = br#"{"version": 2, "routes": []}"#;
        assert!(parse_manifest(data).is_err());
    }

    #[test]
    fn reject_path_traversal_handler() {
        let data = br#"{"version": 1, "routes": [{"path": "/", "handler": "../evil.wasm"}]}"#;
        assert!(parse_manifest(data).is_err());
    }

    #[test]
    fn reject_handler_with_slash() {
        let data = br#"{"version": 1, "routes": [{"path": "/", "handler": "sub/mod.wasm"}]}"#;
        assert!(parse_manifest(data).is_err());
    }

    #[test]
    fn reject_empty_handler() {
        let data = br#"{"version": 1, "routes": [{"path": "/", "handler": ""}]}"#;
        assert!(parse_manifest(data).is_err());
    }

    #[test]
    fn match_exact_route() {
        let m = parse_manifest(valid_manifest_json()).unwrap();
        let r = match_route(&m, "GET", "/health").unwrap();
        assert_eq!(r.handler, "health.wasm");
    }

    #[test]
    fn match_wildcard_route() {
        let m = parse_manifest(valid_manifest_json()).unwrap();
        let r = match_route(&m, "GET", "/api/users").unwrap();
        assert_eq!(r.handler, "api.wasm");
    }

    #[test]
    fn match_wildcard_nested() {
        let m = parse_manifest(valid_manifest_json()).unwrap();
        let r = match_route(&m, "POST", "/api/users/123/edit").unwrap();
        assert_eq!(r.handler, "api.wasm");
    }

    #[test]
    fn match_param_route() {
        let m = parse_manifest(valid_manifest_json()).unwrap();
        let r = match_route(&m, "GET", "/ssr/about").unwrap();
        assert_eq!(r.handler, "ssr.wasm");
    }

    #[test]
    fn no_match_returns_none() {
        let m = parse_manifest(valid_manifest_json()).unwrap();
        assert!(match_route(&m, "GET", "/unknown").is_none());
    }

    #[test]
    fn method_mismatch_returns_none() {
        let m = parse_manifest(valid_manifest_json()).unwrap();
        assert!(match_route(&m, "DELETE", "/health").is_none());
    }

    #[test]
    fn exact_beats_wildcard() {
        let data = br#"{
            "version": 1,
            "routes": [
                {"path": "/api/*", "handler": "catch.wasm", "methods": ["GET"]},
                {"path": "/api/special", "handler": "special.wasm", "methods": ["GET"]}
            ]
        }"#;
        let m = parse_manifest(data).unwrap();
        let r = match_route(&m, "GET", "/api/special").unwrap();
        assert_eq!(r.handler, "special.wasm");
    }

    #[test]
    fn load_from_dir_nonexistent() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(load_manifest_from_dir(dir.path()).is_none());
    }

    #[test]
    fn load_from_dir_valid() {
        let dir = tempfile::TempDir::new().unwrap();
        let sm_dir = dir.path().join("_shadowmesh");
        std::fs::create_dir(&sm_dir).unwrap();
        std::fs::write(sm_dir.join("routes.json"), valid_manifest_json()).unwrap();
        let m = load_manifest_from_dir(dir.path()).unwrap();
        assert_eq!(m.routes.len(), 3);
    }
}
