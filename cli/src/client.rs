//! HTTP client wrapper for the ShadowMesh node-runner API.

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::path::Path;

/// Thin HTTP client that maps each node-runner API endpoint to a method.
pub struct NodeClient {
    base_url: String,
    http: Client,
}

impl NodeClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: Client::new(),
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────

    fn url(&self, path: &str) -> String {
        format!("{}/api{}", self.base_url, path)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let resp = self
            .http
            .get(self.url(path))
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            bail!("HTTP {} — {}", status.as_u16(), body);
        }

        serde_json::from_str(&body).with_context(|| "Invalid JSON response from node")
    }

    async fn post_empty(&self, path: &str) -> Result<Value> {
        let resp = self
            .http
            .post(self.url(path))
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            bail!("HTTP {} — {}", status.as_u16(), body);
        }

        if body.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_str(&body).with_context(|| "Invalid JSON response from node")
        }
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let resp = self
            .http
            .delete(self.url(path))
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await?;
            bail!("HTTP {} — {}", status.as_u16(), body);
        }

        Ok(())
    }

    // ── Status & Health ─────────────────────────────────────────────

    pub async fn get_status(&self) -> Result<Value> {
        self.get_json("/status").await
    }

    pub async fn get_health(&self) -> Result<Value> {
        self.get_json("/health").await
    }

    pub async fn get_ready(&self) -> Result<Value> {
        self.get_json("/ready").await
    }

    pub async fn get_metrics(&self) -> Result<Value> {
        self.get_json("/metrics").await
    }

    // ── Configuration ───────────────────────────────────────────────

    pub async fn get_config(&self) -> Result<Value> {
        self.get_json("/config").await
    }

    pub async fn update_config(&self, body: Value) -> Result<Value> {
        let resp = self
            .http
            .put(self.url("/config"))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            bail!("HTTP {} — {}", status.as_u16(), text);
        }

        serde_json::from_str(&text).with_context(|| "Invalid JSON response from node")
    }

    // ── Storage ─────────────────────────────────────────────────────

    pub async fn get_storage_stats(&self) -> Result<Value> {
        self.get_json("/storage").await
    }

    pub async fn list_content(&self) -> Result<Value> {
        self.get_json("/storage/content").await
    }

    pub async fn get_content(&self, cid: &str) -> Result<Value> {
        self.get_json(&format!("/storage/content/{cid}")).await
    }

    pub async fn delete_content(&self, cid: &str) -> Result<()> {
        self.delete(&format!("/storage/content/{cid}")).await
    }

    pub async fn pin(&self, cid: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/storage/pin/{cid}")))
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await?;
            bail!("HTTP {} — {}", status.as_u16(), body);
        }
        Ok(())
    }

    pub async fn unpin(&self, cid: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/storage/unpin/{cid}")))
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await?;
            bail!("HTTP {} — {}", status.as_u16(), body);
        }
        Ok(())
    }

    pub async fn upload(&self, file_path: &Path) -> Result<Value> {
        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());

        let file_bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("Cannot read file: {}", file_path.display()))?;

        let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name);
        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = self
            .http
            .post(self.url("/storage/upload"))
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            bail!("HTTP {} — {}", status.as_u16(), text);
        }

        serde_json::from_str(&text).with_context(|| "Invalid JSON response from node")
    }

    pub async fn fetch_remote(&self, cid: &str) -> Result<Value> {
        self.post_empty(&format!("/storage/fetch/{cid}")).await
    }

    /// Download raw content bytes. Returns (bytes, content_type, content_name).
    pub async fn download(
        &self,
        cid: &str,
    ) -> Result<(Vec<u8>, Option<String>, Option<String>)> {
        let resp = self
            .http
            .get(self.url(&format!("/storage/download/{cid}")))
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await?;
            bail!("HTTP {} — {}", status.as_u16(), body);
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let content_name = resp
            .headers()
            .get("x-content-cid")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let bytes = resp.bytes().await?.to_vec();
        Ok((bytes, content_type, content_name))
    }

    pub async fn run_gc(&self, target_gb: f64) -> Result<Value> {
        let resp = self
            .http
            .post(self.url("/storage/gc"))
            .json(&serde_json::json!({ "target_free_gb": target_gb }))
            .send()
            .await
            .with_context(|| format!("Cannot connect to node at {}", self.base_url))?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            bail!("HTTP {} — {}", status.as_u16(), text);
        }

        serde_json::from_str(&text).with_context(|| "Invalid JSON response from node")
    }

    // ── Network ─────────────────────────────────────────────────────

    pub async fn get_peers(&self) -> Result<Value> {
        self.get_json("/network/peers").await
    }

    pub async fn get_bandwidth(&self) -> Result<Value> {
        self.get_json("/network/bandwidth").await
    }

    // ── Replication ─────────────────────────────────────────────────

    pub async fn get_replication_health(&self) -> Result<Value> {
        self.get_json("/replication/health").await
    }

    // ── Node Control ────────────────────────────────────────────────

    pub async fn shutdown(&self) -> Result<Value> {
        self.post_empty("/node/shutdown").await
    }
}
