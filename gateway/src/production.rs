//! Production Mode Validation
//!
//! Validates that security settings are properly configured for production.
//! Fails startup if critical security is misconfigured in production mode.

use crate::config::Config;
use std::env;

/// Production validation errors and warnings
#[derive(Debug)]
pub struct ProductionValidation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ProductionValidation {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn add_error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    pub fn add_warning(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}

impl Default for ProductionValidation {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if running in production mode
pub fn is_production_mode() -> bool {
    env::var("SHADOWMESH_PRODUCTION")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
        || env::var("SHADOWMESH_ENV")
            .map(|v| v == "production" || v == "prod")
            .unwrap_or(false)
}

/// Validate configuration for production readiness
pub fn validate_production_config(config: &Config) -> ProductionValidation {
    let mut validation = ProductionValidation::new();

    // Critical: CORS must not be wildcard in production
    if config.security.is_cors_permissive() {
        validation.add_error(
            "CORS allows all origins (*). Set SHADOWMESH_SECURITY_ALLOWED_ORIGINS to specific domains"
        );
    }

    // Critical: API keys must be configured in production
    let api_keys = env::var("SHADOWMESH_API_KEYS").unwrap_or_default();
    if api_keys.is_empty() {
        validation.add_error(
            "No API keys configured. Set SHADOWMESH_API_KEYS environment variable"
        );
    }

    // Warning: Binding to 0.0.0.0 exposes to all interfaces
    if config.server.host == "0.0.0.0" {
        validation.add_warning(
            "Binding to 0.0.0.0 - ensure firewall/network security is configured"
        );
    }

    // Warning: GitHub OAuth for dashboard
    let github_id = env::var("GITHUB_CLIENT_ID").unwrap_or_default();
    if github_id.is_empty() {
        validation.add_warning(
            "GitHub OAuth not configured - dashboard GitHub features disabled"
        );
    }

    // Warning: TLS reminder
    validation.add_warning(
        "Ensure TLS termination is handled by reverse proxy (nginx/traefik/ingress)"
    );

    validation
}

/// Enforce production validation - fails if critical errors found in production mode
pub fn enforce_production_validation(config: &Config) -> Result<(), String> {
    let in_production = is_production_mode();
    let validation = validate_production_config(config);

    // Always print warnings
    for warning in &validation.warnings {
        if in_production {
            tracing::warn!("Production warning: {}", warning);
        } else {
            println!("ℹ️  {}", warning);
        }
    }

    // In production mode, fail on errors
    if in_production && validation.has_errors() {
        for error in &validation.errors {
            tracing::error!("Production validation error: {}", error);
        }
        return Err(format!(
            "Production validation failed with {} error(s):\n  - {}",
            validation.errors.len(),
            validation.errors.join("\n  - ")
        ));
    }

    // In non-production mode, just warn about errors
    if !in_production && validation.has_errors() {
        for error in &validation.errors {
            println!("⚠️  Would fail in production: {}", error);
        }
    }

    if in_production {
        tracing::info!("Production validation passed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_production_validation_with_permissive_cors() {
        let mut config = Config::default();
        config.security.allowed_origins = vec!["*".to_string()];

        let validation = validate_production_config(&config);
        assert!(validation.has_errors());
        assert!(validation.errors.iter().any(|e| e.contains("CORS")));
    }

    #[test]
    fn test_production_validation_with_empty_cors() {
        let config = Config::default();
        // Default is now empty, so no CORS error
        let validation = validate_production_config(&config);
        // Will have error about API keys (unless env var set), but not CORS
        let has_cors_error = validation.errors.iter().any(|e| e.contains("CORS"));
        assert!(!has_cors_error);
    }

    #[test]
    fn test_is_production_mode() {
        // Clear any existing env vars
        env::remove_var("SHADOWMESH_PRODUCTION");
        env::remove_var("SHADOWMESH_ENV");

        assert!(!is_production_mode());

        env::set_var("SHADOWMESH_PRODUCTION", "true");
        assert!(is_production_mode());

        env::remove_var("SHADOWMESH_PRODUCTION");
        env::set_var("SHADOWMESH_ENV", "production");
        assert!(is_production_mode());

        // Cleanup
        env::remove_var("SHADOWMESH_ENV");
    }
}
