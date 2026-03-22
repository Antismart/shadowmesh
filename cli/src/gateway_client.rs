//! HTTP client for the ShadowMesh Gateway API (deploy, deployments, build logs).

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::io::{BufRead, BufReader};

/// Client for the ShadowMesh gateway (default `http://localhost:8081`).
pub struct GatewayClient {
    base_url: String,
    http: Client,
}

impl GatewayClient {
    pub fn new(gateway_url: &str) -> Self {
        Self {
            base_url: gateway_url.trim_end_matches('/').to_string(),
            http: Client::new(),
        }
    }

    // ── Deploy ZIP ──────────────────────────────────────────────────

    /// Upload a ZIP archive to `/api/deploy` (multipart).
    /// Returns the deploy response JSON from the gateway.
    pub async fn deploy_zip(&self, zip_data: Vec<u8>) -> Result<Value> {
        let part = reqwest::multipart::Part::bytes(zip_data)
            .file_name("deploy.zip")
            .mime_str("application/zip")?;
        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = self
            .http
            .post(format!("{}/api/deploy", self.base_url))
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("Cannot connect to gateway at {}", self.base_url))?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            bail!("Gateway HTTP {} — {}", status.as_u16(), body);
        }

        serde_json::from_str(&body).with_context(|| "Invalid JSON response from gateway")
    }

    // ── Deploy from GitHub ──────────────────────────────────────────

    /// POST JSON to `/api/deploy/github`. Returns the async deploy response
    /// which includes `deploy_id` and `stream_url`.
    pub async fn deploy_github(
        &self,
        url: &str,
        branch: &str,
        root_dir: Option<&str>,
    ) -> Result<Value> {
        let mut body = serde_json::json!({
            "url": url,
            "branch": branch,
        });

        if let Some(rd) = root_dir {
            body["root_directory"] = serde_json::Value::String(rd.to_string());
        }

        let resp = self
            .http
            .post(format!("{}/api/deploy/github", self.base_url))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Cannot connect to gateway at {}", self.base_url))?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            bail!("Gateway HTTP {} — {}", status.as_u16(), text);
        }

        serde_json::from_str(&text).with_context(|| "Invalid JSON response from gateway")
    }

    // ── Stream build logs (SSE) ─────────────────────────────────────

    /// Connect to an SSE stream URL and print build logs to stdout.
    /// Handles `log`, `complete`, and `error` events.
    /// Returns `Ok(())` when the stream ends or an error/complete event is received.
    pub async fn stream_build_logs(&self, stream_url: &str) -> Result<()> {
        let full_url = if stream_url.starts_with("http") {
            stream_url.to_string()
        } else {
            format!("{}{}", self.base_url, stream_url)
        };

        let resp = self
            .http
            .get(&full_url)
            .send()
            .await
            .with_context(|| format!("Cannot connect to build log stream at {}", full_url))?;

        if !resp.status().is_success() {
            bail!(
                "Failed to connect to build log stream: HTTP {}",
                resp.status().as_u16()
            );
        }

        // Read the SSE stream as bytes and parse event/data lines.
        let bytes = resp.bytes().await?;
        let reader = BufReader::new(bytes.as_ref());

        let mut current_event = String::new();
        let mut current_data = String::new();

        for line in reader.lines() {
            let line = line?;

            if line.starts_with("event:") {
                current_event = line.trim_start_matches("event:").trim().to_string();
            } else if line.starts_with("data:") {
                current_data = line.trim_start_matches("data:").trim().to_string();
            } else if line.is_empty() {
                // Empty line = end of SSE message, dispatch event
                if !current_event.is_empty() || !current_data.is_empty() {
                    match current_event.as_str() {
                        "log" => {
                            println!("  {}", current_data);
                        }
                        "complete" => {
                            // Try to parse as JSON for a nice summary
                            if let Ok(val) = serde_json::from_str::<Value>(&current_data) {
                                if let Some(cid) = val.get("cid").and_then(|c| c.as_str()) {
                                    println!("\n  Build complete! CID: {}", cid);
                                }
                                if let Some(url) = val.get("url").and_then(|u| u.as_str()) {
                                    println!("  URL: {}", url);
                                }
                            } else {
                                println!("\n  Build complete: {}", current_data);
                            }
                            return Ok(());
                        }
                        "error" => {
                            bail!("Build failed: {}", current_data);
                        }
                        _ => {
                            // Unknown event type, print as log
                            if !current_data.is_empty() {
                                println!("  {}", current_data);
                            }
                        }
                    }
                }
                current_event.clear();
                current_data.clear();
            }
        }

        Ok(())
    }

    // ── List deployments ────────────────────────────────────────────

    /// GET `/api/deployments` — returns an array of deployment objects.
    pub async fn list_deployments(&self) -> Result<Vec<Value>> {
        let resp = self
            .http
            .get(format!("{}/api/deployments", self.base_url))
            .send()
            .await
            .with_context(|| format!("Cannot connect to gateway at {}", self.base_url))?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            bail!("Gateway HTTP {} — {}", status.as_u16(), body);
        }

        serde_json::from_str(&body).with_context(|| "Invalid JSON response from gateway")
    }
}
