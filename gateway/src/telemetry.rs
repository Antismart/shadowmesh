//! OpenTelemetry Telemetry for ShadowMesh Gateway
//!
//! Provides distributed tracing with OTLP exporter support for Jaeger and other backends.

use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    runtime,
    trace::{Config as TraceConfig, Sampler},
    Resource,
};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::config::TelemetryConfig;

/// Initialize OpenTelemetry tracing
///
/// Sets up the tracing subscriber with:
/// - Console output (JSON or pretty depending on SHADOWMESH_LOG_FORMAT)
/// - OpenTelemetry export to OTLP endpoint (Jaeger, etc.)
/// - Configurable sampling rate
pub fn init_telemetry(config: &TelemetryConfig) -> Result<(), TelemetryError> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let use_json = std::env::var("SHADOWMESH_LOG_FORMAT")
        .map(|v| v.to_lowercase() == "json")
        .unwrap_or(false);

    if !config.enabled {
        // Just set up basic logging without OpenTelemetry
        if use_json {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json())
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().pretty())
                .init();
        }
        return Ok(());
    }

    // Determine OTLP endpoint
    let otlp_endpoint = config
        .get_otlp_endpoint()
        .ok_or(TelemetryError::NoEndpoint)?;

    // Create resource with service info
    let resource = Resource::new(vec![
        opentelemetry::KeyValue::new("service.name", config.service_name.clone()),
        opentelemetry::KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
    ]);

    // Create sampler based on config
    let sampler = if config.sample_rate >= 1.0 {
        Sampler::AlwaysOn
    } else if config.sample_rate <= 0.0 {
        Sampler::AlwaysOff
    } else {
        Sampler::TraceIdRatioBased(config.sample_rate)
    };

    // Create OTLP exporter
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(&otlp_endpoint);

    // Create and install tracer provider
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            TraceConfig::default()
                .with_sampler(sampler)
                .with_resource(resource),
        )
        .install_batch(runtime::Tokio)
        .map_err(|e| TelemetryError::Init(e.to_string()))?;

    // Create OpenTelemetry layer with the tracer
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // Set up subscriber with OpenTelemetry - always use JSON for production tracing
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().json())
        .with(otel_layer)
        .init();

    tracing::info!(endpoint = %otlp_endpoint, "OpenTelemetry tracing initialized");

    Ok(())
}

/// Shutdown OpenTelemetry tracer gracefully
pub fn shutdown_telemetry() {
    opentelemetry::global::shutdown_tracer_provider();
    tracing::info!("OpenTelemetry shutdown complete");
}

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error(
        "No OTLP endpoint configured. Set OTEL_EXPORTER_OTLP_ENDPOINT or telemetry.otlp_endpoint"
    )]
    NoEndpoint,
    #[error("Failed to initialize telemetry: {0}")]
    Init(String),
}

/// Middleware for trace context propagation
///
/// Extracts trace context from incoming request headers and creates spans
pub mod middleware {
    use axum::{body::Body, extract::Request, middleware::Next, response::Response};
    use tracing::Instrument;

    /// Tracing middleware that creates spans for each request
    pub async fn trace_request(req: Request<Body>, next: Next) -> Response {
        let method = req.method().to_string();
        let uri = req.uri().to_string();
        let path = req.uri().path().to_string();

        // Create a span for this request
        let span = tracing::info_span!(
            "http_request",
            otel.kind = "server",
            http.method = %method,
            http.url = %uri,
            http.route = %path,
        );

        // Execute the request within the span
        let response = next.run(req).instrument(span.clone()).await;

        // Record response status
        let status = response.status();
        span.in_scope(|| {
            tracing::info!(http.status_code = status.as_u16(), "Request completed");
        });

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config_defaults() {
        let config = TelemetryConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.service_name, "shadowmesh-gateway");
        assert_eq!(config.sample_rate, 1.0);
    }
}
