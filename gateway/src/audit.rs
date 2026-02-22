//! Audit Logging for ShadowMesh Gateway
//!
//! Provides structured audit logging for security-sensitive operations.
//! All audit events are logged in a machine-parseable format for compliance
//! and security monitoring.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Audit event types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// Deployment created
    DeployCreate,
    /// Deployment deleted
    DeployDelete,
    /// Deployment redeployed
    DeployRedeploy,
    /// File uploaded
    FileUpload,
    /// Authentication success
    AuthSuccess,
    /// Authentication failure
    AuthFailure,
    /// Rate limit exceeded
    RateLimitExceeded,
    /// GitHub OAuth connected
    GithubConnect,
    /// GitHub OAuth disconnected
    GithubDisconnect,
    /// Configuration changed
    ConfigChange,
}

/// Audit event outcome
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

/// A single audit event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event ID
    pub id: String,
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Type of action
    pub action: AuditAction,
    /// Outcome of the action
    pub outcome: AuditOutcome,
    /// Actor identifier (API key prefix, IP, or "system")
    pub actor: String,
    /// Request ID for correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Client IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    /// Resource affected (e.g., CID, deployment name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// Additional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Size in bytes (for uploads/deploys)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

impl AuditEvent {
    /// Create a new audit event
    pub fn new(action: AuditAction, outcome: AuditOutcome, actor: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            action,
            outcome,
            actor: actor.into(),
            request_id: None,
            client_ip: None,
            resource: None,
            details: None,
            size_bytes: None,
        }
    }

    /// Add request ID
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Add client IP
    pub fn with_client_ip(mut self, ip: impl Into<String>) -> Self {
        self.client_ip = Some(ip.into());
        self
    }

    /// Add resource
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    /// Add details
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Add size
    pub fn with_size(mut self, size_bytes: u64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }
}

/// Audit logger that stores events and logs them
#[derive(Clone)]
pub struct AuditLogger {
    /// In-memory event buffer (last N events)
    events: Arc<RwLock<Vec<AuditEvent>>>,
    /// Maximum events to keep in memory
    max_events: usize,
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl AuditLogger {
    /// Create a new audit logger
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::with_capacity(max_events))),
            max_events,
        }
    }

    /// Log an audit event
    pub async fn log(&self, event: AuditEvent) {
        // Log to tracing (will be captured by structured logging)
        tracing::info!(
            target: "audit",
            event_id = %event.id,
            action = ?event.action,
            outcome = ?event.outcome,
            actor = %event.actor,
            request_id = ?event.request_id,
            client_ip = ?event.client_ip,
            resource = ?event.resource,
            details = ?event.details,
            size_bytes = ?event.size_bytes,
            "audit_event"
        );

        // Store in memory buffer
        let mut events = self.events.write().await;
        if events.len() >= self.max_events {
            events.remove(0);
        }
        events.push(event);
    }

    /// Get recent audit events
    pub async fn get_recent(&self, limit: usize) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        events.iter().rev().take(limit).cloned().collect()
    }

    /// Get events for a specific resource
    pub async fn get_for_resource(&self, resource: &str, limit: usize) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        events
            .iter()
            .rev()
            .filter(|e| e.resource.as_deref() == Some(resource))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get events by action type
    pub async fn get_by_action(&self, action: AuditAction, limit: usize) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        events
            .iter()
            .rev()
            .filter(|e| e.action == action)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Clear all events (for testing)
    #[cfg(test)]
    pub async fn clear(&self) {
        let mut events = self.events.write().await;
        events.clear();
    }
}

// Convenience functions for common audit events

/// Log a successful deployment
pub async fn log_deploy_success(
    logger: &AuditLogger,
    actor: &str,
    cid: &str,
    size_bytes: u64,
    request_id: Option<&str>,
    client_ip: Option<&str>,
) {
    let mut event = AuditEvent::new(AuditAction::DeployCreate, AuditOutcome::Success, actor)
        .with_resource(cid)
        .with_size(size_bytes);

    if let Some(rid) = request_id {
        event = event.with_request_id(rid);
    }
    if let Some(ip) = client_ip {
        event = event.with_client_ip(ip);
    }

    logger.log(event).await;
}

/// Log a failed deployment
pub async fn log_deploy_failure(
    logger: &AuditLogger,
    actor: &str,
    error: &str,
    request_id: Option<&str>,
    client_ip: Option<&str>,
) {
    let mut event = AuditEvent::new(AuditAction::DeployCreate, AuditOutcome::Failure, actor)
        .with_details(error);

    if let Some(rid) = request_id {
        event = event.with_request_id(rid);
    }
    if let Some(ip) = client_ip {
        event = event.with_client_ip(ip);
    }

    logger.log(event).await;
}

/// Log a deployment deletion
pub async fn log_deploy_delete(
    logger: &AuditLogger,
    actor: &str,
    cid: &str,
    request_id: Option<&str>,
    client_ip: Option<&str>,
) {
    let mut event =
        AuditEvent::new(AuditAction::DeployDelete, AuditOutcome::Success, actor).with_resource(cid);

    if let Some(rid) = request_id {
        event = event.with_request_id(rid);
    }
    if let Some(ip) = client_ip {
        event = event.with_client_ip(ip);
    }

    logger.log(event).await;
}

/// Log an authentication failure
pub async fn log_auth_failure(
    logger: &AuditLogger,
    reason: &str,
    request_id: Option<&str>,
    client_ip: Option<&str>,
) {
    let mut event = AuditEvent::new(AuditAction::AuthFailure, AuditOutcome::Denied, "anonymous")
        .with_details(reason);

    if let Some(rid) = request_id {
        event = event.with_request_id(rid);
    }
    if let Some(ip) = client_ip {
        event = event.with_client_ip(ip);
    }

    logger.log(event).await;
}

/// Log a file upload
pub async fn log_file_upload(
    logger: &AuditLogger,
    actor: &str,
    cid: &str,
    size_bytes: u64,
    filename: Option<&str>,
    request_id: Option<&str>,
    client_ip: Option<&str>,
) {
    let mut event = AuditEvent::new(AuditAction::FileUpload, AuditOutcome::Success, actor)
        .with_resource(cid)
        .with_size(size_bytes);

    if let Some(name) = filename {
        event = event.with_details(format!("filename: {}", name));
    }
    if let Some(rid) = request_id {
        event = event.with_request_id(rid);
    }
    if let Some(ip) = client_ip {
        event = event.with_client_ip(ip);
    }

    logger.log(event).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audit_event_creation() {
        let event = AuditEvent::new(
            AuditAction::DeployCreate,
            AuditOutcome::Success,
            "test-actor",
        )
        .with_resource("QmTest123")
        .with_size(1024)
        .with_request_id("req-123");

        assert_eq!(event.action, AuditAction::DeployCreate);
        assert_eq!(event.outcome, AuditOutcome::Success);
        assert_eq!(event.actor, "test-actor");
        assert_eq!(event.resource, Some("QmTest123".to_string()));
        assert_eq!(event.size_bytes, Some(1024));
    }

    #[tokio::test]
    async fn test_audit_logger() {
        let logger = AuditLogger::new(10);

        // Log some events
        for i in 0..5 {
            let event = AuditEvent::new(
                AuditAction::FileUpload,
                AuditOutcome::Success,
                format!("actor-{}", i),
            )
            .with_resource(format!("cid-{}", i));

            logger.log(event).await;
        }

        // Get recent events
        let recent = logger.get_recent(3).await;
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].actor, "actor-4"); // Most recent first
    }

    #[tokio::test]
    async fn test_audit_logger_max_events() {
        let logger = AuditLogger::new(3);

        // Log more events than max
        for i in 0..5 {
            let event = AuditEvent::new(
                AuditAction::FileUpload,
                AuditOutcome::Success,
                format!("actor-{}", i),
            );
            logger.log(event).await;
        }

        // Should only have 3 events
        let all = logger.get_recent(10).await;
        assert_eq!(all.len(), 3);

        // Should be the most recent 3
        assert_eq!(all[0].actor, "actor-4");
        assert_eq!(all[1].actor, "actor-3");
        assert_eq!(all[2].actor, "actor-2");
    }

    #[tokio::test]
    async fn test_get_for_resource() {
        let logger = AuditLogger::new(100);

        // Log events for different resources
        logger
            .log(
                AuditEvent::new(AuditAction::DeployCreate, AuditOutcome::Success, "a")
                    .with_resource("cid-1"),
            )
            .await;
        logger
            .log(
                AuditEvent::new(AuditAction::DeployDelete, AuditOutcome::Success, "b")
                    .with_resource("cid-1"),
            )
            .await;
        logger
            .log(
                AuditEvent::new(AuditAction::DeployCreate, AuditOutcome::Success, "c")
                    .with_resource("cid-2"),
            )
            .await;

        let cid1_events = logger.get_for_resource("cid-1", 10).await;
        assert_eq!(cid1_events.len(), 2);

        let cid2_events = logger.get_for_resource("cid-2", 10).await;
        assert_eq!(cid2_events.len(), 1);
    }
}
