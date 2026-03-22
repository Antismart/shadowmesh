//! Dashboard for ShadowMesh Gateway - Tailwind CSS Version

use crate::{
    audit,
    lock_utils::{read_lock, write_lock},
    metrics,
    redis_client::RedisClient,
    AppState,
};
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json, Redirect},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

// ── Build session for SSE log streaming ─────────────────────────────

/// Tracks an in-progress build for SSE streaming.
pub struct BuildSession {
    pub logs: std::sync::Mutex<Vec<String>>,
    pub status: std::sync::Mutex<BuildSessionStatus>,
    pub result: std::sync::Mutex<Option<serde_json::Value>>,
    pub notify: tokio::sync::broadcast::Sender<BuildEvent>,
    /// When this session was created (for TTL-based cleanup).
    pub created_at: std::time::Instant,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum BuildSessionStatus {
    Building,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug)]
pub struct BuildEvent {
    pub event_type: String,
    pub data: String,
}

impl BuildSession {
    pub fn new() -> (Self, tokio::sync::broadcast::Receiver<BuildEvent>) {
        let (tx, rx) = tokio::sync::broadcast::channel(512);
        (
            Self {
                logs: std::sync::Mutex::new(Vec::new()),
                status: std::sync::Mutex::new(BuildSessionStatus::Building),
                result: std::sync::Mutex::new(None),
                notify: tx,
                created_at: std::time::Instant::now(),
            },
            rx,
        )
    }

    pub fn push_log(&self, line: &str) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(line.to_string());
        }
        let _ = self.notify.send(BuildEvent {
            event_type: "log".to_string(),
            data: line.to_string(),
        });
    }

    pub fn complete(&self, result: serde_json::Value) {
        match self.status.lock() {
            Ok(mut s) => *s = BuildSessionStatus::Succeeded,
            Err(e) => {
                tracing::warn!("BuildSession status lock poisoned in complete(), recovering");
                *e.into_inner() = BuildSessionStatus::Succeeded;
            }
        }
        match self.result.lock() {
            Ok(mut r) => *r = Some(result.clone()),
            Err(e) => {
                tracing::warn!("BuildSession result lock poisoned in complete(), recovering");
                *e.into_inner() = Some(result.clone());
            }
        }
        let _ = self.notify.send(BuildEvent {
            event_type: "complete".to_string(),
            data: result.to_string(),
        });
    }

    pub fn fail(&self, error: &str) {
        let error_value = json!({"success": false, "error": error});
        match self.status.lock() {
            Ok(mut s) => *s = BuildSessionStatus::Failed,
            Err(e) => {
                tracing::warn!("BuildSession status lock poisoned in fail(), recovering");
                *e.into_inner() = BuildSessionStatus::Failed;
            }
        }
        match self.result.lock() {
            Ok(mut r) => *r = Some(error_value),
            Err(e) => {
                tracing::warn!("BuildSession result lock poisoned in fail(), recovering");
                *e.into_inner() = Some(json!({"success": false, "error": error}));
            }
        }
        let _ = self.notify.send(BuildEvent {
            event_type: "error".to_string(),
            data: error.to_string(),
        });
    }
}

/// Escape a string for safe embedding in HTML content.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Escape a string for safe embedding inside a JavaScript single-quoted literal
/// within an HTML attribute (e.g., onclick="fn('VALUE')").
fn escape_js_attr(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "&quot;")
        .replace('<', "\\x3c")
        .replace('>', "\\x3e")
        .replace('&', "\\x26")
}

pub async fn dashboard_handler(State(state): State<AppState>) -> impl IntoResponse {
    let all_deployments: Vec<Deployment> = state.deployments.iter().map(|r| r.value().clone()).collect();
    let mut grouped: BTreeMap<String, Vec<&Deployment>> = BTreeMap::new();
    for deployment in all_deployments.iter() {
        let key = if deployment.source == "github" {
            deployment
                .repo_url
                .clone()
                .unwrap_or_else(|| deployment.name.clone())
        } else {
            "Uploads".to_string()
        };
        grouped.entry(key).or_default().push(deployment);
    }

    let render_deployment = |d: &Deployment| {
        let status_class = if d.status == "Ready" {
            "status-ready"
        } else {
            "status-pending"
        };
        let status_icon = if d.status == "Ready" { "✓" } else { "◐" };
        let source_label = if d.source == "github" {
            "GitHub"
        } else {
            "Upload"
        };
        let branch_display = d.branch.as_deref().unwrap_or("main");
        let build_status = d.build_status.as_str();
        let build_class = if build_status.contains("Built") {
            "build-ready"
        } else if build_status.contains("Failed") {
            "build-failed"
        } else {
            "build-skipped"
        };

        // Escape all user-controlled values for safe HTML/JS embedding
        let cid_js = escape_js_attr(&d.cid);
        let name_html = escape_html(&d.name);
        let branch_html = escape_html(branch_display);
        let created_html = escape_html(&d.created_at);
        let status_html = escape_html(&d.status);
        let build_status_html = escape_html(build_status);
        let cid_short = escape_html(&d.cid[..8.min(d.cid.len())]);

        let redeploy_button = if d.source == "github" {
            format!(
                r##"<button onclick="event.stopPropagation();redeployDeployment('{}')" class="btn-ghost">Redeploy</button>"##,
                cid_js
            )
        } else {
            String::new()
        };
        format!(
            r##"<div class="card deploy-card" onclick="window.open('/{}','_blank')">
                <div class="deploy-top">
                    <div>
                        <div class="deploy-name">{}</div>
                        <div class="deploy-meta">{} · {} · {}</div>
                    </div>
                    <div class="deploy-status {}">
                        <span class="status-dot">{}</span>
                        <span>{}</span>
                    </div>
                </div>
                <div class="deploy-status-row">
                    <span class="build-status {}">Build: {}</span>
                    <button onclick="event.stopPropagation();showLogs('{}')" class="btn-ghost">View logs</button>
                </div>
                <div class="deploy-bottom">
                    <span class="deploy-url">shadowmesh.local/{}...</span>
                    <div style="display:flex; gap:8px;">
                        <button onclick="event.stopPropagation();navigator.clipboard.writeText(location.origin+'/{}');alert('URL copied!')" class="btn-ghost">Copy URL</button>
                        {}
                        <button onclick="event.stopPropagation();deleteDeployment('{}')" class="btn-ghost btn-danger">Delete</button>
                    </div>
                </div>
            </div>"##,
            cid_js,
            name_html,
            source_label,
            branch_html,
            created_html,
            status_class,
            status_icon,
            status_html,
            build_class,
            build_status_html,
            cid_js,
            cid_short,
            cid_js,
            redeploy_button,
            cid_js
        )
    };

    let deployment_rows: String = grouped
        .iter()
        .map(|(key, items)| {
            let title = if key == "Uploads" {
                "Uploads".to_string()
            } else {
                format!("GitHub · {}", escape_html(key))
            };
            let count = items.len();
            let list = items
                .iter()
                .map(|d| render_deployment(d))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                r##"<section class="repo-group">
                        <div class="repo-header">
                            <div class="repo-title">{}</div>
                            <div class="repo-count">{} deploy{} </div>
                        </div>
                        <div class="repo-list">{}</div>
                    </section>"##,
                title,
                count,
                if count == 1 { "ment" } else { "ments" },
                list
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let empty_state = r##"<div class="card empty-state">
        <div class="empty-icon">📦</div>
        <div class="empty-title">No deployments yet</div>
        <div class="empty-subtitle">Import a project from GitHub or upload a ZIP to get started</div>
        <button onclick="showGithubModal()" class="btn-primary">Import from GitHub</button>
    </div>"##;

    let deployments_content = if deployment_rows.is_empty() {
        empty_state.to_string()
    } else {
        deployment_rows
    };

    let html = format!(
        r###"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ShadowMesh Dashboard</title>
    <style>
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, sans-serif;
            background: radial-gradient(circle at top, #1b1f3a 0%, #0b0b14 45%, #050508 100%);
            color: #f8fafc;
            min-height: 100vh;
        }}
        a {{ color: inherit; text-decoration: none; }}
        button {{ font: inherit; }}
        .navbar {{
            position: sticky;
            top: 0;
            z-index: 50;
            background: rgba(5, 5, 8, 0.8);
            backdrop-filter: blur(16px);
            border-bottom: 1px solid rgba(255, 255, 255, 0.08);
        }}
        .nav-inner {{
            max-width: 980px;
            margin: 0 auto;
            padding: 0 24px;
            height: 64px;
            display: flex;
            align-items: center;
            justify-content: space-between;
        }}
        .brand {{ display: flex; align-items: center; gap: 12px; }}
        .brand-icon {{
            width: 40px;
            height: 40px;
            border-radius: 12px;
            background: linear-gradient(135deg, #7c3aed, #38bdf8);
            display: grid;
            place-items: center;
            font-weight: 700;
            box-shadow: 0 0 24px rgba(99, 102, 241, 0.5);
        }}
        .status-pill {{
            display: flex;
            align-items: center;
            gap: 8px;
            font-size: 12px;
            color: #4ade80;
        }}
        .status-dot-live {{
            width: 8px;
            height: 8px;
            border-radius: 999px;
            background: #4ade80;
            box-shadow: 0 0 8px rgba(74, 222, 128, 0.8);
        }}
        .page {{ max-width: 980px; margin: 0 auto; padding: 36px 24px 72px; }}
        .page-header {{ display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 32px; }}
        .title {{ font-size: 32px; font-weight: 700; display: flex; align-items: center; gap: 10px; }}
        .subtitle {{ color: #94a3b8; margin-top: 6px; }}
        .btn-primary {{
            background: linear-gradient(135deg, #6366f1, #8b5cf6);
            border: none;
            color: #fff;
            padding: 12px 22px;
            border-radius: 12px;
            font-weight: 600;
            cursor: pointer;
            transition: transform 0.2s ease, box-shadow 0.2s ease;
        }}
        .btn-primary:hover {{ transform: translateY(-1px); box-shadow: 0 12px 24px rgba(99, 102, 241, 0.3); }}
        .btn-secondary {{
            padding: 10px 18px;
            border-radius: 10px;
            background: rgba(255, 255, 255, 0.08);
            color: #e2e8f0;
            border: 1px solid rgba(255, 255, 255, 0.1);
            transition: background 0.2s ease;
        }}
        .btn-secondary:hover {{ background: rgba(255, 255, 255, 0.16); }}
        .cards {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 20px; margin-bottom: 40px; }}
        .card {{
            background: rgba(15, 23, 42, 0.55);
            border: 1px solid rgba(148, 163, 184, 0.15);
            border-radius: 18px;
            padding: 24px;
            box-shadow: 0 12px 30px rgba(15, 23, 42, 0.35);
        }}
        .card:hover {{ border-color: rgba(99, 102, 241, 0.5); }}
        .card-icon {{
            width: 44px;
            height: 44px;
            border-radius: 14px;
            display: grid;
            place-items: center;
            margin-bottom: 16px;
            background: rgba(255, 255, 255, 0.08);
        }}
        .card-icon.gradient {{ background: linear-gradient(135deg, #6366f1, #ec4899); }}
        .card-title {{ font-size: 18px; font-weight: 600; margin-bottom: 6px; }}
        .card-desc {{ color: #94a3b8; font-size: 14px; }}
        .section-title {{ font-size: 12px; text-transform: uppercase; letter-spacing: 0.2em; color: #64748b; margin-bottom: 16px; }}
        .deploy-card {{ cursor: pointer; padding: 18px 20px; }}
        .deploy-top {{ display: flex; justify-content: space-between; align-items: flex-start; gap: 16px; margin-bottom: 12px; }}
        .deploy-name {{ font-size: 16px; font-weight: 600; margin-bottom: 4px; }}
        .deploy-meta {{ font-size: 13px; color: #94a3b8; }}
        .deploy-status {{
            display: inline-flex;
            align-items: center;
            gap: 6px;
            font-size: 12px;
            padding: 6px 12px;
            border-radius: 999px;
            background: rgba(99, 102, 241, 0.15);
        }}
        .status-ready {{ color: #4ade80; }}
        .status-pending {{ color: #fbbf24; }}
        .status-dot {{ font-size: 12px; }}
        .deploy-bottom {{ display: flex; justify-content: space-between; align-items: center; }}
        .deploy-url {{ font-family: "SFMono-Regular", ui-monospace, monospace; font-size: 12px; color: #64748b; }}
    .deploy-status-row {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 12px; }}
    .build-status {{ font-size: 12px; padding: 4px 10px; border-radius: 999px; background: rgba(148, 163, 184, 0.1); }}
    .build-ready {{ color: #4ade80; background: rgba(74, 222, 128, 0.12); }}
    .build-failed {{ color: #f87171; background: rgba(248, 113, 113, 0.12); }}
    .build-skipped {{ color: #fbbf24; background: rgba(251, 191, 36, 0.12); }}
        .btn-ghost {{
            border: none;
            background: rgba(255, 255, 255, 0.08);
            color: #e2e8f0;
            padding: 6px 12px;
            border-radius: 8px;
            font-size: 12px;
            cursor: pointer;
            transition: background 0.2s ease;
        }}
        .btn-ghost:hover {{ background: rgba(255, 255, 255, 0.16); }}
        .btn-danger {{
            background: rgba(248, 113, 113, 0.16);
            color: #fecaca;
        }}
        .btn-danger:hover {{ background: rgba(248, 113, 113, 0.3); }}
        .repo-group {{ display: grid; gap: 14px; margin-bottom: 24px; }}
        .repo-header {{ display: flex; align-items: center; justify-content: space-between; }}
        .repo-title {{ font-size: 14px; font-weight: 600; color: #e2e8f0; }}
        .repo-count {{ font-size: 12px; color: #94a3b8; }}
        .repo-list {{ display: grid; gap: 16px; }}
        .empty-state {{ display: grid; place-items: center; text-align: center; padding: 48px; gap: 12px; }}
        .empty-icon {{ width: 72px; height: 72px; border-radius: 20px; background: rgba(255, 255, 255, 0.08); display: grid; place-items: center; font-size: 32px; }}
        .empty-title {{ font-size: 20px; font-weight: 600; }}
        .empty-subtitle {{ color: #94a3b8; max-width: 360px; }}
        .modal-overlay {{
            position: fixed;
            inset: 0;
            display: none;
            align-items: center;
            justify-content: center;
            background: rgba(3, 6, 12, 0.75);
            backdrop-filter: blur(6px);
            z-index: 100;
            padding: 24px;
        }}
        .modal-overlay.active {{ display: flex; }}
        .modal {{ width: 100%; max-width: 520px; }}
        .modal-header {{ display: flex; justify-content: space-between; align-items: center; padding: 20px 24px; border-bottom: 1px solid rgba(255, 255, 255, 0.08); }}
    .modal-body {{ padding: 24px; display: grid; gap: 16px; }}
        .modal-footer {{ display: flex; justify-content: flex-end; gap: 12px; padding: 18px 24px; border-top: 1px solid rgba(255, 255, 255, 0.08); }}
        .form-label {{ font-size: 13px; color: #94a3b8; margin-bottom: 8px; display: block; }}
        .form-input {{
            width: 100%;
            padding: 12px 14px;
            border-radius: 12px;
            border: 1px solid rgba(148, 163, 184, 0.2);
            background: rgba(15, 23, 42, 0.7);
            color: #f8fafc;
        }}
        .form-input:focus {{ outline: none; border-color: rgba(99, 102, 241, 0.7); }}
        .upload-area {{
            border: 2px dashed rgba(148, 163, 184, 0.3);
            border-radius: 16px;
            padding: 36px;
            text-align: center;
            cursor: pointer;
            transition: border 0.2s ease, background 0.2s ease;
        }}
        .upload-area:hover {{ border-color: rgba(99, 102, 241, 0.7); background: rgba(15, 23, 42, 0.4); }}
        .progress {{ margin-top: 12px; display: none; }}
        .progress.active {{ display: block; }}
        .progress-bar {{ height: 6px; border-radius: 999px; background: rgba(255, 255, 255, 0.08); overflow: hidden; }}
        .progress-fill {{ height: 100%; width: 0%; background: linear-gradient(135deg, #6366f1, #8b5cf6); transition: width 0.3s ease; }}
        .progress-text {{ margin-top: 10px; text-align: center; font-size: 13px; color: #94a3b8; }}
        .github-panel {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 12px;
            padding: 14px 16px;
            border-radius: 14px;
            background: rgba(15, 23, 42, 0.6);
            border: 1px solid rgba(148, 163, 184, 0.2);
        }}
        .github-status {{ font-size: 13px; color: #94a3b8; }}
        .repo-select {{
            width: 100%;
            padding: 12px 14px;
            border-radius: 12px;
            border: 1px solid rgba(148, 163, 184, 0.2);
            background: rgba(15, 23, 42, 0.7);
            color: #f8fafc;
        }}
        @media (max-width: 720px) {{
            .page-header {{ flex-direction: column; align-items: flex-start; }}
        }}
    </style>
</head>
<body>
    <!-- Navbar -->
    <nav class="navbar">
        <div class="nav-inner">
            <div class="brand">
                <div class="brand-icon">◈</div>
                <span style="font-size: 18px; font-weight: 600;">ShadowMesh</span>
            </div>
            <div style="display: flex; align-items: center; gap: 16px;">
                <div class="status-pill">
                    <span class="status-dot-live"></span>
                    <span>Online</span>
                </div>
                <a href="/metrics" class="btn-secondary">Metrics</a>
            </div>
        </div>
    </nav>
    
    <main class="page">
        <!-- Header -->
        <div class="page-header">
            <div>
                <h1 class="title">
                    <span>🚀</span>
                    Deployments
                </h1>
                <p class="subtitle">Deploy your projects to the decentralized web</p>
            </div>
            <button onclick="showUploadModal()" class="btn-primary">
                + New Project
            </button>
        </div>
        
        <!-- Action Cards -->
        <div class="cards">
            <div class="card" style="cursor:pointer;" onclick="showGithubModal()">
                <div class="card-icon">
                    <svg width="22" height="22" viewBox="0 0 16 16" fill="white"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
                </div>
                <div class="card-title">Import from GitHub</div>
                <div class="card-desc">Deploy directly from a GitHub repository</div>
            </div>
            <div class="card" style="cursor:pointer;" onclick="showUploadModal()">
                <div class="card-icon gradient">
                    <svg width="22" height="22" fill="none" stroke="white" stroke-width="2" viewBox="0 0 24 24"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                </div>
                <div class="card-title">Upload ZIP</div>
                <div class="card-desc">Deploy by uploading a ZIP file</div>
            </div>
        </div>
        
        <!-- Recent Deployments -->
        <div>
            <h2 class="section-title">Recent Deployments</h2>
            <div style="display:grid; gap:16px;">
                {}
            </div>
        </div>
    </main>
    
    <!-- Upload Modal -->
    <div id="uploadModal" class="modal-overlay">
        <div class="card modal">
            <div class="modal-header">
                <h2>Upload Project</h2>
                <button onclick="hideUploadModal()" class="btn-ghost">&times;</button>
            </div>
            <div class="modal-body">
                <div id="uploadArea" class="upload-area">
                    <div class="card-icon gradient" style="margin: 0 auto 16px;">
                        <svg width="22" height="22" fill="none" stroke="white" stroke-width="2" viewBox="0 0 24 24"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                    </div>
                    <div style="font-size:16px; font-weight: 600; margin-bottom: 6px;">Drop your ZIP here</div>
                    <div style="font-size:13px; color:#94a3b8;">or click to browse</div>
                    <input type="file" id="fileInput" accept=".zip" style="display:none;">
                </div>
                <div class="progress" id="uploadProgress">
                    <div class="progress-bar"><div class="progress-fill" id="progressBar"></div></div>
                    <div class="progress-text" id="progressText">Uploading...</div>
                </div>
            </div>
        </div>
    </div>
    
    <!-- GitHub Modal -->
    <div id="githubModal" class="modal-overlay">
        <div class="card modal">
            <div class="modal-header">
                <h2>Import from GitHub</h2>
                <button onclick="hideGithubModal()" class="btn-ghost">&times;</button>
            </div>
            <div class="modal-body">
                <div class="github-panel">
                    <div>
                        <div style="font-weight: 600;">GitHub Connection</div>
                        <div class="github-status" id="githubStatus">Not connected</div>
                    </div>
                    <button type="button" class="btn-secondary" id="githubConnectBtn" onclick="connectGithub()">Connect</button>
                </div>
                <div id="githubRepoBlock" style="display:none;">
                    <label class="form-label">Choose a repository</label>
                    <select id="githubRepoSelect" class="repo-select"></select>
                </div>
                <div>
                    <label class="form-label">GitHub Repository URL</label>
                    <input type="text" id="githubUrl" placeholder="https://github.com/username/repo" class="form-input">
                </div>
                <div>
                    <label class="form-label">Branch</label>
                    <input type="text" id="githubBranch" value="main" placeholder="main" class="form-input">
                </div>
                <div class="progress" id="githubProgress">
                    <div class="progress-bar"><div id="githubProgressBar" class="progress-fill"></div></div>
                    <div id="githubProgressText" class="progress-text">Cloning...</div>
                </div>
            </div>
            <div class="modal-footer">
                <button onclick="hideGithubModal()" class="btn-secondary">Cancel</button>
                <button onclick="deployFromGithub()" class="btn-primary">Deploy</button>
            </div>
        </div>
    </div>

    <div id="logsModal" class="modal-overlay">
        <div class="card modal">
            <div class="modal-header">
                <h2>Build Logs</h2>
                <button onclick="hideLogs()" class="btn-ghost">&times;</button>
            </div>
            <div class="modal-body">
                <div id="logsStatus" class="build-status build-skipped">Build</div>
                <pre id="logsContent" style="white-space:pre-wrap; font-size:12px; color:#e2e8f0; background:rgba(15,23,42,0.6); padding:12px; border-radius:12px; max-height:360px; overflow:auto;"></pre>
            </div>
        </div>
    </div>
    
    <script>
    function showUploadModal() {{ document.getElementById('uploadModal').classList.add('active'); }}
        function hideUploadModal() {{ document.getElementById('uploadModal').classList.remove('active'); }}
        function showGithubModal() {{ document.getElementById('githubModal').classList.add('active'); }}
        function hideGithubModal() {{ document.getElementById('githubModal').classList.remove('active'); }}
        
        document.querySelectorAll('.modal-overlay').forEach(function(m) {{
            m.onclick = function(e) {{ if (e.target === m) {{ m.classList.remove('active'); }} }};
        }});
        
        var uploadArea = document.getElementById('uploadArea');
        var fileInput = document.getElementById('fileInput');
        
        uploadArea.onclick = function() {{ fileInput.click(); }};
        uploadArea.ondragover = function(e) {{ e.preventDefault(); uploadArea.classList.add('border-indigo-500'); }};
        uploadArea.ondragleave = function() {{ uploadArea.classList.remove('border-indigo-500'); }};
        uploadArea.ondrop = function(e) {{ e.preventDefault(); uploadArea.classList.remove('border-indigo-500'); if (e.dataTransfer.files.length) uploadFile(e.dataTransfer.files[0]); }};
        fileInput.onchange = function(e) {{ if (e.target.files.length) uploadFile(e.target.files[0]); }};

        function connectGithub() {{ window.location.href = '/api/github/login'; }}

        async function loadGithubStatus() {{
            try {{
                var res = await fetch('/api/github/status');
                if (!res.ok) return;
                var data = await res.json();
                var status = document.getElementById('githubStatus');
                var btn = document.getElementById('githubConnectBtn');
                var repoBlock = document.getElementById('githubRepoBlock');
                if (data.connected) {{
                    var name = data.user && data.user.login ? data.user.login : 'GitHub';
                    status.textContent = 'Connected as @' + name;
                    status.style.color = '#4ade80';
                    btn.textContent = 'Connected';
                    btn.disabled = true;
                    repoBlock.style.display = 'block';
                    await loadGithubRepos();
                }} else {{
                    status.textContent = 'Not connected';
                    status.style.color = '#94a3b8';
                    btn.textContent = 'Connect';
                    btn.disabled = false;
                    repoBlock.style.display = 'none';
                }}
            }} catch (err) {{}}
        }}

        async function loadGithubRepos() {{
            try {{
                var res = await fetch('/api/github/repos');
                if (!res.ok) return;
                var repos = await res.json();
                var select = document.getElementById('githubRepoSelect');
                select.innerHTML = '';
                repos.forEach(function(repo) {{
                    var option = document.createElement('option');
                    option.value = repo.html_url;
                    option.textContent = repo.full_name + (repo.private ? ' (private)' : '');
                    select.appendChild(option);
                }});
                if (repos.length > 0) {{
                    document.getElementById('githubUrl').value = repos[0].html_url;
                }}
                select.onchange = function(e) {{ document.getElementById('githubUrl').value = e.target.value; }};
            }} catch (err) {{}}
    }}
        
        function uploadFile(file) {{
            if (!file.name.endsWith('.zip')) {{ alert('Please upload a ZIP file'); return; }}
            document.getElementById('uploadProgress').classList.add('active');
            var bar = document.getElementById('progressBar');
            var text = document.getElementById('progressText');
            bar.style.width = '0%';
            var fd = new FormData(); fd.append('file', file);
            var p = 0, iv = setInterval(function() {{ p = Math.min(p + 10, 90); bar.style.width = p + '%'; }}, 200);
            fetch('/api/deploy', {{ method: 'POST', body: fd }})
                .then(function(r) {{ return r.json(); }})
                .then(function(res) {{
                    clearInterval(iv); bar.style.width = '100%';
                    if (res.success) {{ text.textContent = 'Deployed!'; text.style.color = '#4ade80'; setTimeout(function() {{ location.reload(); }}, 1500); }}
                    else {{ text.textContent = 'Error: ' + res.error; text.style.color = '#f87171'; }}
                }})
                .catch(function(e) {{ clearInterval(iv); text.textContent = 'Error'; text.style.color = '#f87171'; }});
        }}
        
        function deployFromGithub() {{
            var url = document.getElementById('githubUrl').value.trim();
            var branch = document.getElementById('githubBranch').value.trim() || 'main';
            if (!url) {{ alert('Enter a GitHub URL'); return; }}
            document.getElementById('githubProgress').classList.add('active');
            var bar = document.getElementById('githubProgressBar');
            var text = document.getElementById('githubProgressText');
            bar.style.width = '0%';
            var p = 0, iv = setInterval(function() {{ p = Math.min(p + 5, 90); bar.style.width = p + '%'; if (p < 30) text.textContent = 'Cloning...'; else if (p < 60) text.textContent = 'Extracting...'; else text.textContent = 'Deploying...'; }}, 300);
            fetch('/api/deploy/github', {{ method: 'POST', headers: {{ 'Content-Type': 'application/json' }}, body: JSON.stringify({{ url: url, branch: branch }}) }})
                .then(function(r) {{ return r.json(); }})
                .then(function(res) {{
                    clearInterval(iv); bar.style.width = '100%';
                    if (res.success) {{ text.textContent = 'Deployed!'; text.style.color = '#4ade80'; setTimeout(function() {{ location.reload(); }}, 1500); }}
                    else {{ text.textContent = 'Error: ' + res.error; text.style.color = '#f87171'; }}
                }})
                .catch(function(e) {{ clearInterval(iv); text.textContent = 'Error'; text.style.color = '#f87171'; }});
        }}

        async function deleteDeployment(cid) {{
            if (!confirm('Delete this deployment?')) return;
            try {{
                var res = await fetch('/api/deployments/' + cid, {{ method: 'DELETE' }});
                if (!res.ok) {{ alert('Failed to delete deployment'); return; }}
                location.reload();
            }} catch (err) {{
                alert('Failed to delete deployment');
            }}
        }}

        async function redeployDeployment(cid) {{
            if (!confirm('Redeploy this project?')) return;
            try {{
                var res = await fetch('/api/deployments/' + cid + '/redeploy', {{ method: 'POST' }});
                var data = await res.json();
                if (!res.ok || !data.success) {{
                    alert(data.error || 'Redeploy failed');
                    return;
                }}
                location.reload();
            }} catch (err) {{
                alert('Redeploy failed');
            }}
        }}

        document.addEventListener('DOMContentLoaded', function() {{ loadGithubStatus(); }});

        function showLogs(cid) {{
            fetch('/api/deployments/' + cid + '/logs')
                .then(function(res) {{ return res.json(); }})
                .then(function(data) {{
                    if (!data.success) {{ alert(data.error || 'Failed to load logs'); return; }}
                    var status = document.getElementById('logsStatus');
                    status.textContent = 'Build: ' + data.status;
                    status.className = 'build-status ' + (data.status.includes('Built') ? 'build-ready' : data.status.includes('Failed') ? 'build-failed' : 'build-skipped');
                    document.getElementById('logsContent').textContent = data.logs || 'No logs available.';
                    document.getElementById('logsModal').classList.add('active');
                }})
                .catch(function() {{ alert('Failed to load logs'); }});
        }}

        function hideLogs() {{ document.getElementById('logsModal').classList.remove('active'); }}
    </script>
</body>
</html>"###,
        deployments_content
    );

    Html(html)
}

#[derive(Debug, Deserialize)]
pub struct GithubDeployRequest {
    pub url: String,
    pub branch: Option<String>,
    /// Subdirectory within the repo containing the project (e.g. "frontend", "packages/app")
    #[serde(default)]
    pub root_directory: Option<String>,
}

pub async fn deploy_from_github(
    State(state): State<AppState>,
    Json(request): Json<GithubDeployRequest>,
) -> impl IntoResponse {
    let url = request.url.trim().to_string();
    let branch = request.branch.as_deref().unwrap_or("main").to_string();
    let root_dir = request.root_directory.clone();

    // Create a build session for SSE streaming
    let deploy_id = Uuid::new_v4().to_string();
    let (session, _rx) = BuildSession::new();
    let session = Arc::new(session);

    {
        let mut sessions = write_lock(&state.build_sessions);
        sessions.insert(deploy_id.clone(), session.clone());
    }

    // Spawn background build task
    let state_clone = state.clone();
    let deploy_id_clone = deploy_id.clone();
    let session_clone = session.clone();
    tokio::spawn(async move {
        session_clone.push_log(&format!("Starting deployment from {}", url));

        match deploy_github_project_streaming(
            &state_clone,
            &url,
            &branch,
            root_dir.as_deref(),
            &session_clone,
        )
        .await
        {
            Ok(payload) => {
                session_clone.complete(payload);
            }
            Err((_status, message)) => {
                session_clone.fail(&message);
            }
        }

        // Clean up session after 5 minutes
        let sessions = state_clone.build_sessions.clone();
        let id = deploy_id_clone;
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            write_lock(&sessions).remove(&id);
        });
    });

    (
        StatusCode::OK,
        Json(json!({
            "deploy_id": deploy_id,
            "stream_url": format!("/api/deploy/{}/stream", deploy_id)
        })),
    )
        .into_response()
}

/// SSE endpoint for streaming build logs in real time.
pub async fn deploy_stream(
    State(state): State<AppState>,
    AxumPath(deploy_id): AxumPath<String>,
) -> impl IntoResponse {
    use axum::response::sse::{Event, Sse};

    let session = {
        let sessions = read_lock(&state.build_sessions);
        sessions.get(&deploy_id).cloned()
    };

    let Some(session) = session else {
        let stream = async_stream::stream! {
            yield Ok::<_, std::convert::Infallible>(
                Event::default().event("error").data("Build session not found")
            );
        };
        return Sse::new(stream)
            .keep_alive(axum::response::sse::KeepAlive::default())
            .into_response();
    };

    // Get buffered logs and subscribe for new events
    let buffered = match session.logs.lock() {
        Ok(l) => l.clone(),
        Err(e) => {
            tracing::warn!("BuildSession logs lock poisoned, recovering");
            e.into_inner().clone()
        }
    };
    let status = match session.status.lock() {
        Ok(s) => s.clone(),
        Err(e) => {
            tracing::warn!("BuildSession status lock poisoned, recovering");
            e.into_inner().clone()
        }
    };
    let result = match session.result.lock() {
        Ok(r) => r.clone(),
        Err(e) => {
            tracing::warn!("BuildSession result lock poisoned, recovering");
            e.into_inner().clone()
        }
    };
    let mut rx = session.notify.subscribe();

    let stream = async_stream::stream! {
        // Replay buffered logs
        for line in buffered {
            yield Ok::<_, std::convert::Infallible>(
                Event::default().event("log").data(line)
            );
        }

        // If already complete, send final event and stop
        if status == BuildSessionStatus::Succeeded {
            if let Some(r) = result {
                yield Ok(Event::default().event("complete").data(r.to_string()));
            }
            return;
        } else if status == BuildSessionStatus::Failed {
            if let Some(r) = result {
                yield Ok(Event::default().event("error").data(
                    r.get("error").and_then(|e| e.as_str()).unwrap_or("Build failed").to_string()
                ));
            }
            return;
        }

        // Stream new events
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let sse_event = Event::default()
                        .event(&event.event_type)
                        .data(&event.data);
                    yield Ok(sse_event);

                    if event.event_type == "complete" || event.event_type == "error" {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    };

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

pub async fn redeploy_github(
    State(state): State<AppState>,
    AxumPath(cid): AxumPath<String>,
) -> impl IntoResponse {
    let deployment = state.deployments.get(&cid).map(|r| r.value().clone());

    let Some(deployment) = deployment else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"success": false, "error": "Deployment not found"})),
        )
            .into_response();
    };

    if deployment.source != "github" {
        return (StatusCode::BAD_REQUEST, Json(json!({"success": false, "error": "Redeploy only supported for GitHub deployments"}))).into_response();
    }

    let repo_url = match deployment.repo_url.clone() {
        Some(url) => url,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"success": false, "error": "Missing repo URL"})),
            )
                .into_response()
        }
    };
    let branch = deployment
        .branch
        .clone()
        .unwrap_or_else(|| "main".to_string());
    let root_dir = deployment.root_directory.clone();

    match deploy_github_project(&state, &repo_url, &branch, root_dir.as_deref()).await {
        Ok(payload) => {
            state.deployments.remove(&cid);
            // Clean old deployment from Redis
            if let Some(ref redis) = state.redis {
                if let Err(e) = Deployment::delete_from_redis(&cid, redis).await {
                    tracing::warn!("Failed to delete old deployment from Redis during redeploy: {}", e);
                }
            }
            (StatusCode::OK, Json(payload)).into_response()
        }
        Err((status, message)) => {
            (status, Json(json!({"success": false, "error": message}))).into_response()
        }
    }
}

/// GitHub webhook handler for auto-redeploy on push
///
/// Set `GITHUB_WEBHOOK_SECRET` env var to validate webhook signatures.
/// Configure on GitHub: Settings → Webhooks → Add webhook
///   URL: https://your-gateway/api/webhook/github
///   Content type: application/json
///   Secret: same as GITHUB_WEBHOOK_SECRET
pub async fn github_webhook(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Validate signature if secret is configured
    if let Ok(secret) = std::env::var("GITHUB_WEBHOOK_SECRET") {
        let signature = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_webhook_signature(&secret, &body, signature) {
            tracing::warn!("GitHub webhook signature validation failed");
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"success": false, "error": "Invalid signature"})),
            )
                .into_response();
        }
    }

    // Only handle push events
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if event != "push" {
        return (
            StatusCode::OK,
            Json(json!({"success": true, "message": format!("Ignored event: {}", event)})),
        )
            .into_response();
    }

    // Parse push payload
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"success": false, "error": format!("Invalid JSON: {}", e)})),
            )
                .into_response();
        }
    };

    let repo_url = payload["repository"]["html_url"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let git_ref = payload["ref"].as_str().unwrap_or("refs/heads/main");
    let branch = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);

    if repo_url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "No repository URL in payload"})),
        )
            .into_response();
    }

    tracing::info!(repo = %repo_url, branch = %branch, "GitHub webhook: auto-redeploy triggered");

    // Find matching deployment
    let (old_cid, root_dir) = {
        let found = state.deployments
            .iter()
            .find(|r| {
                let d = r.value();
                d.source == "github"
                    && d.repo_url.as_deref() == Some(&repo_url)
                    && d.branch.as_deref() == Some(branch)
            })
            .map(|r| (r.value().cid.clone(), r.value().root_directory.clone()));
        match found {
            Some((cid, root_dir)) => (Some(cid), root_dir),
            None => (None, None),
        }
    };

    // Redeploy
    match deploy_github_project(&state, &repo_url, branch, root_dir.as_deref()).await {
        Ok(payload) => {
            // Remove old deployment if exists
            if let Some(ref old_cid) = old_cid {
                state.deployments.remove(old_cid);
                if let Some(ref redis) = state.redis {
                    if let Err(e) = Deployment::delete_from_redis(old_cid, redis).await {
                        tracing::warn!("Failed to delete old deployment from Redis: {}", e);
                    }
                }
            }

            tracing::info!(repo = %repo_url, "GitHub webhook: auto-redeploy succeeded");
            (StatusCode::OK, Json(payload)).into_response()
        }
        Err((_status, message)) => {
            tracing::error!(repo = %repo_url, error = %message, "GitHub webhook: auto-redeploy failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"success": false, "error": message})),
            )
                .into_response()
        }
    }
}

/// Verify GitHub webhook HMAC-SHA256 signature
fn verify_webhook_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let expected = match signature.strip_prefix("sha256=") {
        Some(hex) => hex,
        None => return false,
    };

    let mut mac = match Hmac::<Sha256>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(body);
    let result = mac.finalize();
    let computed = hex::encode(result.into_bytes());

    // Constant-time comparison
    subtle::ConstantTimeEq::ct_eq(computed.as_bytes(), expected.as_bytes()).into()
}

pub async fn get_deployments(State(state): State<AppState>) -> impl IntoResponse {
    let deployments: Vec<Deployment> = state.deployments.iter().map(|r| r.value().clone()).collect();
    Json(deployments)
}

pub async fn delete_deployment(
    State(state): State<AppState>,
    AxumPath(cid): AxumPath<String>,
) -> impl IntoResponse {
    let found = state.deployments.remove(&cid).is_some();

    if !found {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"success": false, "error": "Deployment not found"})),
        )
            .into_response()
    } else {
        // Delete from Redis if available
        if let Some(ref redis) = state.redis {
            if let Err(e) = Deployment::delete_from_redis(&cid, redis).await {
                tracing::warn!("Failed to delete deployment from Redis: {}", e);
            }
        }
        audit::log_deploy_delete(&state.audit_logger, "api", &cid, None, None).await;
        Json(json!({"success": true})).into_response()
    }
}

pub async fn deployment_logs(
    State(state): State<AppState>,
    AxumPath(cid): AxumPath<String>,
) -> impl IntoResponse {
    if let Some(deployment) = state.deployments.get(&cid) {
        (StatusCode::OK, Json(json!({
            "success": true,
            "status": deployment.build_status,
            "logs": deployment.build_logs.clone().unwrap_or_else(|| "No logs available".to_string())
        }))).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"success": false, "error": "Deployment not found"})),
        )
            .into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubUser {
    pub login: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubRepo {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub html_url: String,
    pub private: bool,
    pub default_branch: String,
}

#[derive(Debug, Clone)]
pub struct GithubAuth {
    pub token: String,
    pub user: GithubUser,
}

#[derive(Debug, Deserialize)]
pub struct GithubCallbackQuery {
    pub code: String,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubLoginQuery {
    /// Where to redirect the user after OAuth completes (the dashboard URL).
    /// When omitted the callback redirects to "/" on the same gateway.
    pub redirect_to: Option<String>,
}

pub async fn github_login(
    State(state): State<AppState>,
    Query(params): Query<GithubLoginQuery>,
) -> impl IntoResponse {
    let client_id = match std::env::var("GITHUB_CLIENT_ID") {
        Ok(val) => val,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "error": "GITHUB_CLIENT_ID not set"
                })),
            )
                .into_response();
        }
    };

    let server_port = state.config.read().await.server.port;
    let redirect_uri = std::env::var("GITHUB_REDIRECT_URL").unwrap_or_else(|_| {
        format!(
            "http://localhost:{}/api/github/callback",
            server_port
        )
    });

    let csrf_state = Uuid::new_v4().to_string();
    {
        let mut states = write_lock(&state.github_oauth_states);
        // Always evict expired states (>10 min) to prevent unbounded growth.
        let cutoff = std::time::Duration::from_secs(600);
        states.retain(|_, (created, _)| created.elapsed() <= cutoff);
        // Hard cap as a safety net after eviction.
        if states.len() >= 10_000 {
            tracing::warn!(count = states.len(), "OAuth state map at capacity after eviction");
        }
        states.insert(csrf_state.clone(), (std::time::Instant::now(), params.redirect_to));
    }

    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user%20repo&state={}",
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&csrf_state)
    );

    Redirect::to(&url).into_response()
}

pub async fn github_callback(
    State(state): State<AppState>,
    Query(query): Query<GithubCallbackQuery>,
) -> impl IntoResponse {
    let client_id = match std::env::var("GITHUB_CLIENT_ID") {
        Ok(val) => val,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"success": false, "error": "GITHUB_CLIENT_ID not set"})),
            )
                .into_response()
        }
    };
    let client_secret = match std::env::var("GITHUB_CLIENT_SECRET") {
        Ok(val) => val,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"success": false, "error": "GITHUB_CLIENT_SECRET not set"})),
            )
                .into_response()
        }
    };

    let server_port = state.config.read().await.server.port;
    let redirect_uri = std::env::var("GITHUB_REDIRECT_URL").unwrap_or_else(|_| {
        format!(
            "http://localhost:{}/api/github/callback",
            server_port
        )
    });

    // Validate and consume the CSRF state token, recovering the redirect_to URL.
    let redirect_to: Option<String>;
    match &query.state {
        Some(state_param) => {
            let mut states = write_lock(&state.github_oauth_states);
            match states.remove(state_param.as_str()) {
                Some((created_at, stored_redirect)) => {
                    // Reject tokens older than 10 minutes
                    if created_at.elapsed() > std::time::Duration::from_secs(600) {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"success": false, "error": "OAuth state expired"})),
                        )
                            .into_response();
                    }
                    redirect_to = stored_redirect;
                }
                None => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"success": false, "error": "Invalid OAuth state"})),
                    )
                        .into_response();
                }
            }
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"success": false, "error": "Missing OAuth state parameter"})),
            )
                .into_response();
        }
    }

    let token_response = match state.http_client
        .post("https://github.com/login/oauth/access_token")
        .timeout(std::time::Duration::from_secs(30))
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", query.code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"success": false, "error": format!("Token exchange failed: {}", e)})),
            )
                .into_response()
        }
    };

    let token_body: GithubTokenResponse = match token_response.json().await {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"success": false, "error": "Invalid token response"})),
            )
                .into_response()
        }
    };

    let user_response = match state.http_client
        .get("https://api.github.com/user")
        .header("User-Agent", "shadowmesh-gateway")
        .bearer_auth(&token_body.access_token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"success": false, "error": format!("Failed to fetch user: {}", e)})),
            )
                .into_response()
        }
    };

    let user: GithubUser = match user_response.json().await {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"success": false, "error": "Failed to parse user"})),
            )
                .into_response()
        }
    };

    // Store auth locally (for same-gateway usage)
    *write_lock(&state.github_auth) = Some(GithubAuth {
        token: token_body.access_token.clone(),
        user: user.clone(),
    });

    // If an auth secret is configured, issue a JWT and redirect with a
    // short-lived authorization code instead of embedding the JWT in the
    // URL.  The dashboard exchanges the code for the JWT via a POST to
    // /api/github/exchange, keeping the token out of browser history,
    // Referer headers, and server access logs.
    if let Some(secret) = crate::jwt::auth_secret() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let claims = crate::jwt::Claims {
            sub: user.login.clone(),
            login: user.login,
            avatar_url: user.avatar_url,
            github_token: token_body.access_token,
            iat: now,
            exp: now + 7 * 24 * 3600, // 7 days
        };

        if let Ok(jwt) = crate::jwt::encode(&claims, &secret) {
            // Store a single-use auth code that maps to this JWT.
            let auth_code = Uuid::new_v4().to_string();
            {
                let mut codes = write_lock(&state.auth_codes);
                // Evict expired codes (>60 s old) and cap to prevent unbounded growth.
                if codes.len() >= 10_000 {
                    let cutoff = std::time::Duration::from_secs(60);
                    codes.retain(|_, (_, created)| created.elapsed() <= cutoff);
                }
                codes.insert(auth_code.clone(), (jwt, std::time::Instant::now()));
            }

            let base = redirect_to.as_deref().unwrap_or("/");
            let sep = if base.contains('?') { "&" } else { "?" };
            let target = format!("{base}{sep}code={auth_code}");
            return Redirect::to(&target).into_response();
        }
    }

    // Fallback: redirect to dashboard origin or "/" (legacy same-gateway flow)
    let target = redirect_to.as_deref().unwrap_or("/");
    Redirect::to(target).into_response()
}

// ── Auth code exchange endpoint ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ExchangeCodeRequest {
    pub code: String,
}

/// POST /api/github/exchange
/// Exchange a short-lived authorization code for the JWT.
/// The code is single-use and expires after 60 seconds.
pub async fn exchange_auth_code(
    State(state): State<AppState>,
    Json(body): Json<ExchangeCodeRequest>,
) -> impl IntoResponse {
    let mut codes = write_lock(&state.auth_codes);

    // Evict expired codes on every call to bound memory growth.
    let cutoff = std::time::Duration::from_secs(60);
    codes.retain(|_, (_, created)| created.elapsed() <= cutoff);

    match codes.remove(&body.code) {
        Some((jwt, created)) => {
            if created.elapsed() > cutoff {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "error": "Authorization code expired"})),
                )
                    .into_response()
            } else {
                Json(json!({"success": true, "token": jwt})).into_response()
            }
        }
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "error": "Invalid or already-used authorization code"})),
        )
            .into_response(),
    }
}

/// Extract GitHub auth from server-side state or a JWT Bearer token.
/// This allows authenticated endpoints to work on any gateway that
/// shares the `SHADOWMESH_AUTH_SECRET`.
fn resolve_auth(state: &AppState, headers: &axum::http::HeaderMap) -> Option<GithubAuth> {
    // 1. Try server-side session (legacy same-gateway flow)
    if let Some(auth) = read_lock(&state.github_auth).clone() {
        return Some(auth);
    }

    // 2. Try JWT from Authorization: Bearer <token>
    let header_val = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = header_val.strip_prefix("Bearer ")?;
    let secret = crate::jwt::auth_secret()?;
    let claims = crate::jwt::decode(token, &secret).ok()?;

    Some(GithubAuth {
        token: claims.github_token,
        user: GithubUser {
            login: claims.login,
            avatar_url: claims.avatar_url,
        },
    })
}

pub async fn github_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    if let Some(auth) = resolve_auth(&state, &headers) {
        Json(json!({"connected": true, "user": auth.user}))
    } else {
        Json(json!({"connected": false, "user": null}))
    }
}

pub async fn github_repos(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let auth = match resolve_auth(&state, &headers) {
        Some(auth) => auth,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "GitHub not connected"})),
            )
                .into_response()
        }
    };

    let response = match state.http_client
        .get("https://api.github.com/user/repos?per_page=100&sort=updated")
        .header("User-Agent", "shadowmesh-gateway")
        .bearer_auth(&auth.token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("Failed to fetch repos: {}", e)})),
            )
                .into_response()
        }
    };

    let repos: Vec<GithubRepo> = match response.json().await {
        Ok(list) => list,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "Failed to parse repos"})),
            )
                .into_response()
        }
    };

    Json(repos).into_response()
}

#[derive(Debug, Deserialize)]
pub struct RepoTreeQuery {
    pub repo: String,
    pub branch: String,
}

/// Proxy GitHub's tree API to list repo folder structure.
/// Used by the dashboard folder browser for root directory selection.
pub async fn github_repo_tree(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<RepoTreeQuery>,
) -> impl IntoResponse {
    let auth = match resolve_auth(&state, &headers) {
        Some(auth) => auth,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "GitHub not connected"})),
            )
                .into_response()
        }
    };

    if !is_valid_branch_name(&query.branch) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid branch name"})),
        )
            .into_response();
    }

    // Validate repo format "owner/repo"
    let parts: Vec<&str> = query.repo.splitn(2, '/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid repo format, expected owner/repo"})),
        )
            .into_response();
    }

    let url = format!(
        "https://api.github.com/repos/{}/git/trees/{}?recursive=1",
        query.repo, query.branch
    );

    let response = match state
        .http_client
        .get(&url)
        .header("User-Agent", "otter-gateway")
        .bearer_auth(&auth.token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("Failed to fetch tree: {}", e)})),
            )
                .into_response()
        }
    };

    let body: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "Failed to parse tree response"})),
            )
                .into_response()
        }
    };

    // Extract directory entries from the tree
    let entries: Vec<serde_json::Value> = body
        .get("tree")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let path = item.get("path")?.as_str()?;
                    let item_type = item.get("type")?.as_str()?;
                    if item_type == "tree" {
                        Some(json!({"path": path, "type": "tree"}))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Json(json!({ "entries": entries })).into_response()
}

struct RepoInfo {
    owner: String,
    repo: String,
}

/// Validate that a GitHub owner or repo name contains only allowed characters.
/// GitHub allows alphanumeric, hyphens, dots, and underscores.
fn is_valid_github_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
}

fn parse_github_url(url: &str) -> Option<RepoInfo> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("github.com/"))?;

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let owner = parts[0];
    let repo = parts[1];

    // GitHub limits: owner max 39 chars, repo max 100 chars
    if owner.len() > 39 || repo.len() > 100 {
        return None;
    }

    // Validate character sets
    if !is_valid_github_name(owner) || !is_valid_github_name(repo) {
        return None;
    }

    Some(RepoInfo {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

fn extract_github_zip(data: &[u8]) -> Result<tempfile::TempDir, String> {
    use std::io::{Read, Write};

    /// Maximum total uncompressed size (512 MB)
    const MAX_UNCOMPRESSED_SIZE: u64 = 512 * 1024 * 1024;
    /// Maximum number of entries in the ZIP
    const MAX_FILE_COUNT: usize = 10_000;

    let temp_dir = tempfile::TempDir::new().map_err(|e| format!("Temp dir failed: {}", e))?;
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid ZIP: {}", e))?;

    if archive.len() > MAX_FILE_COUNT {
        return Err(format!(
            "ZIP contains too many entries ({}, max {})",
            archive.len(),
            MAX_FILE_COUNT
        ));
    }

    let mut root_prefix = String::new();
    let mut total_size: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("ZIP error: {}", e))?;
        let name = file.name().to_string();
        if i == 0 && name.ends_with('/') {
            root_prefix = name.clone();
            continue;
        }
        let relative_path = if !root_prefix.is_empty() && name.starts_with(&root_prefix) {
            &name[root_prefix.len()..]
        } else {
            &name
        };
        if relative_path.is_empty() {
            continue;
        }
        // Reject path traversal attempts (e.g. "../../../etc/passwd")
        if relative_path.contains("..") {
            return Err(format!("ZIP contains path traversal: {}", relative_path));
        }
        let out_path = temp_dir.path().join(relative_path);
        // Double-check the resolved path stays within temp_dir
        if !out_path.starts_with(temp_dir.path()) {
            return Err(format!("ZIP path escapes target directory: {}", relative_path));
        }
        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| format!("Dir failed: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut outfile =
                std::fs::File::create(&out_path).map_err(|e| format!("File failed: {}", e))?;

            // Stream the file in chunks to enforce size limit without buffering entire file
            let mut buf = [0u8; 8192];
            loop {
                let n = file.read(&mut buf).map_err(|e| format!("Read failed: {}", e))?;
                if n == 0 {
                    break;
                }
                total_size += n as u64;
                if total_size > MAX_UNCOMPRESSED_SIZE {
                    return Err(format!(
                        "ZIP uncompressed size exceeds limit ({} bytes)",
                        MAX_UNCOMPRESSED_SIZE
                    ));
                }
                outfile
                    .write_all(&buf[..n])
                    .map_err(|e| format!("Write failed: {}", e))?;
            }
        }
    }
    Ok(temp_dir)
}

/// Validate that a Git branch name contains only safe characters.
fn is_valid_branch_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !s.starts_with('.')
        && !s.starts_with('-')
        && !s.contains("..")
        && !s.contains("//")
        && !s.contains('\\')
        && !s.contains(' ')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '/')
}

async fn deploy_github_project(
    state: &AppState,
    url: &str,
    branch: &str,
    root_directory: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let repo_info =
        parse_github_url(url).ok_or((StatusCode::BAD_REQUEST, "Invalid GitHub URL".to_string()))?;

    if !is_valid_branch_name(branch) {
        return Err((StatusCode::BAD_REQUEST, "Invalid branch name".to_string()));
    }

    let zip_url = format!(
        "https://github.com/{}/{}/archive/refs/heads/{}.zip",
        repo_info.owner, repo_info.repo, branch
    );

    let mut response = state.http_client.get(&zip_url)
        .timeout(std::time::Duration::from_secs(60))
        .send().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to download: {}", e),
        )
    })?;

    if !response.status().is_success() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Repository or branch '{}' not found", branch),
        ));
    }

    // Stream the download with a size limit to prevent memory exhaustion.
    const MAX_ZIP_DOWNLOAD: usize = 512 * 1024 * 1024; // 512 MB
    let mut zip_bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Failed to read: {}", e)))?
    {
        if zip_bytes.len() + chunk.len() > MAX_ZIP_DOWNLOAD {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "Repository archive exceeds 512 MB limit".to_string(),
            ));
        }
        zip_bytes.extend_from_slice(&chunk);
    }

    let temp_dir =
        extract_github_zip(&zip_bytes).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // If root_directory is specified, resolve build root to that subdirectory
    let build_root = if let Some(rd) = root_directory.filter(|s| !s.is_empty()) {
        // Sanitize: strip leading/trailing slashes, reject path traversal
        let cleaned = rd.trim_matches('/');
        if cleaned.contains("..") {
            return Err((StatusCode::BAD_REQUEST, "root_directory must not contain '..'".to_string()));
        }
        let sub = temp_dir.path().join(cleaned);
        if !sub.exists() || !sub.is_dir() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Root directory '{}' not found in repository", cleaned),
            ));
        }
        sub
    } else {
        temp_dir.path().to_path_buf()
    };

    let build_outcome =
        prepare_deploy_dir(&build_root).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let storage = state.storage.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "IPFS not available".to_string(),
    ))?;

    fn count_files(dir: &std::path::Path) -> (u64, usize) {
        let mut size = 0u64;
        let mut count = 0usize;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let (s, c) = count_files(&path);
                    size += s;
                    count += c;
                } else if let Ok(meta) = path.metadata() {
                    size += meta.len();
                    count += 1;
                }
            }
        }
        (size, count)
    }

    let (total_size, file_count) = count_files(&build_outcome.deploy_root);

    let result = storage
        .store_directory(&build_outcome.deploy_root)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("IPFS upload failed: {}", e),
            )
        })?;

    let cid = result.root_cid;

    let mut deployment = Deployment::new_github(
        repo_info.repo.clone(),
        cid.clone(),
        total_size,
        file_count,
        branch.to_string(),
        Some(url.to_string()),
        build_outcome.build_status,
        Some(build_outcome.build_logs),
        root_directory.map(|s| s.to_string()),
    );

    // Auto-assign .shadow domain
    deployment.domain = crate::auto_assign_domain(state, &repo_info.repo, &cid).await;

    // Save to Redis if available
    if let Some(ref redis) = state.redis {
        if let Err(e) = deployment.save_to_redis(redis).await {
            tracing::warn!("Failed to save GitHub deployment to Redis: {}", e);
        }
    }

    metrics::record_deployment(true);
    audit::log_deploy_success(
        &state.audit_logger,
        "github",
        &cid,
        total_size,
        None,
        None,
    )
    .await;

    let domain = deployment.domain.clone();
    state.deployments.insert(deployment.cid.clone(), deployment);

    Ok(json!({
        "success": true,
        "cid": cid,
        "domain": domain,
        "repo": format!("{}/{}", repo_info.owner, repo_info.repo),
        "branch": branch,
        "url": format!("/{}", cid)
    }))
}

/// Streaming variant that pushes log lines to a BuildSession for SSE.
async fn deploy_github_project_streaming(
    state: &AppState,
    url: &str,
    branch: &str,
    root_directory: Option<&str>,
    session: &Arc<BuildSession>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let repo_info =
        parse_github_url(url).ok_or((StatusCode::BAD_REQUEST, "Invalid GitHub URL".to_string()))?;

    if !is_valid_branch_name(branch) {
        return Err((StatusCode::BAD_REQUEST, "Invalid branch name".to_string()));
    }

    session.push_log(&format!("Cloning {}/{} (branch: {})", repo_info.owner, repo_info.repo, branch));

    let zip_url = format!(
        "https://github.com/{}/{}/archive/refs/heads/{}.zip",
        repo_info.owner, repo_info.repo, branch
    );

    let mut response = state.http_client.get(&zip_url)
        .timeout(std::time::Duration::from_secs(60))
        .send().await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, format!("Failed to download: {}", e))
    })?;

    if !response.status().is_success() {
        return Err((StatusCode::NOT_FOUND, format!("Repository or branch '{}' not found", branch)));
    }

    const MAX_ZIP_DOWNLOAD: usize = 512 * 1024 * 1024;
    let mut zip_bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Failed to read: {}", e)))?
    {
        if zip_bytes.len() + chunk.len() > MAX_ZIP_DOWNLOAD {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "Repository archive exceeds 512 MB limit".to_string()));
        }
        zip_bytes.extend_from_slice(&chunk);
    }

    session.push_log(&format!("Downloaded {:.1} MB, extracting...", zip_bytes.len() as f64 / (1024.0 * 1024.0)));

    let temp_dir = extract_github_zip(&zip_bytes).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let build_root = if let Some(rd) = root_directory.filter(|s| !s.is_empty()) {
        let cleaned = rd.trim_matches('/');
        if cleaned.contains("..") {
            return Err((StatusCode::BAD_REQUEST, "root_directory must not contain '..'".to_string()));
        }
        let sub = temp_dir.path().join(cleaned);
        if !sub.exists() || !sub.is_dir() {
            return Err((StatusCode::BAD_REQUEST, format!("Root directory '{}' not found in repository", cleaned)));
        }
        session.push_log(&format!("Using root directory: {}", cleaned));
        sub
    } else {
        temp_dir.path().to_path_buf()
    };

    // Run build in blocking thread, streaming logs to session
    let build_root_clone = build_root.clone();
    let session_clone = Arc::clone(session);
    let build_outcome = tokio::task::spawn_blocking(move || {
        prepare_deploy_dir_streaming(&build_root_clone, &session_clone)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Build task failed: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let storage = state.storage.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE, "IPFS not available".to_string(),
    ))?;

    fn count_files(dir: &std::path::Path) -> (u64, usize) {
        let mut size = 0u64;
        let mut count = 0usize;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let (s, c) = count_files(&path);
                    size += s;
                    count += c;
                } else if let Ok(meta) = path.metadata() {
                    size += meta.len();
                    count += 1;
                }
            }
        }
        (size, count)
    }

    let (total_size, file_count) = count_files(&build_outcome.deploy_root);
    session.push_log(&format!("Uploading {} files ({:.1} KB) to IPFS...", file_count, total_size as f64 / 1024.0));

    let result = storage.store_directory(&build_outcome.deploy_root).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("IPFS upload failed: {}", e))
    })?;

    let cid = result.root_cid;
    session.push_log(&format!("Uploaded to IPFS: {}", cid));

    let mut deployment = Deployment::new_github(
        repo_info.repo.clone(), cid.clone(), total_size, file_count,
        branch.to_string(), Some(url.to_string()),
        build_outcome.build_status, Some(build_outcome.build_logs),
        root_directory.map(|s| s.to_string()),
    );

    deployment.domain = crate::auto_assign_domain(state, &repo_info.repo, &cid).await;

    if let Some(ref redis) = state.redis {
        if let Err(e) = deployment.save_to_redis(redis).await {
            tracing::warn!("Failed to save GitHub deployment to Redis: {}", e);
        }
    }

    metrics::record_deployment(true);
    audit::log_deploy_success(&state.audit_logger, "github", &cid, total_size, None, None).await;

    let domain = deployment.domain.clone();
    state.deployments.insert(deployment.cid.clone(), deployment);

    if let Some(ref d) = domain {
        session.push_log(&format!("Domain: {}", d));
    }
    session.push_log("Deployment complete!");

    Ok(json!({
        "success": true,
        "cid": cid,
        "domain": domain,
        "repo": format!("{}/{}", repo_info.owner, repo_info.repo),
        "branch": branch,
        "url": format!("/{}", cid)
    }))
}

struct BuildOutcome {
    deploy_root: PathBuf,
    build_status: String,
    build_logs: String,
}

/// Maximum time allowed for each npm command (5 minutes)
const BUILD_TIMEOUT_SECS: u64 = 300;

/// Allowed environment variables to pass through to npm builds.
/// Everything else is stripped to prevent leaking secrets.
const BUILD_ENV_ALLOWLIST: &[&str] = &[
    "PATH", "HOME", "USER", "LANG", "LC_ALL", "NODE_ENV",
    "npm_config_cache", "npm_config_loglevel",
];

/// Detect the package manager from lock files in priority order.
/// Returns (binary_name, install_args, build_args).
fn detect_package_manager(root: &Path) -> (&'static str, &'static [&'static str], &'static [&'static str]) {
    if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
        ("bun", &["install"], &["run", "build"])
    } else if root.join("pnpm-lock.yaml").exists() {
        ("pnpm", &["install", "--frozen-lockfile"], &["run", "build"])
    } else if root.join("yarn.lock").exists() {
        ("yarn", &["install", "--frozen-lockfile"], &["build"])
    } else {
        ("npm", &["install"], &["run", "build"])
    }
}

/// Run a shell command with a timeout. Kills the process if it exceeds the limit.
fn run_cmd_with_timeout(
    cmd: &str,
    args: &[&str],
    dir: &Path,
    env: &[(String, String)],
    timeout_secs: u64,
) -> Result<std::process::Output, String> {
    use std::process::Stdio;
    let mut child = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", cmd, e))?;

    let timeout = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|e| format!("Failed to read {} output: {}", cmd, e));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} {} timed out after {}s in {} — killed",
                        cmd,
                        args.first().unwrap_or(&"<unknown>"),
                        timeout_secs,
                        dir.display()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(e) => return Err(format!("Failed to wait for {}: {}", cmd, e)),
        }
    }
}

/// Detect framework and return a human-readable name for build logs.
fn detect_framework(root: &Path) -> &'static str {
    if root.join("next.config.js").exists()
        || root.join("next.config.ts").exists()
        || root.join("next.config.mjs").exists()
    {
        return "Next.js";
    }
    if root.join("nuxt.config.ts").exists() || root.join("nuxt.config.js").exists() {
        return "Nuxt";
    }
    if root.join("astro.config.mjs").exists() || root.join("astro.config.ts").exists() {
        return "Astro";
    }
    if root.join("svelte.config.js").exists() || root.join("svelte.config.ts").exists() {
        return "SvelteKit";
    }
    if root.join("vite.config.ts").exists()
        || root.join("vite.config.js").exists()
        || root.join("vite.config.mjs").exists()
    {
        return "Vite";
    }
    if root.join("gatsby-config.js").exists() || root.join("gatsby-config.ts").exists() {
        return "Gatsby";
    }
    if root.join("angular.json").exists() {
        return "Angular";
    }
    if root.join("package.json").exists() {
        return "Node.js";
    }
    if root.join("index.html").exists() {
        return "Static HTML";
    }
    "Unknown"
}

/// Known output directories to check after build, in priority order.
const OUTPUT_DIRS: &[&str] = &[
    "dist",      // Vite, Vue CLI, Svelte, Astro
    "build",     // Create React App, Angular
    "out",       // Next.js static export
    ".output/public", // Nuxt 3 static
    "public",    // Hugo, Gatsby (if no build)
    "_site",     // Jekyll, Eleventy
    "storybook-static", // Storybook
];

/// For Next.js projects, ensure `output: 'export'` is in the config so that
/// `next build` produces a static `out/` directory instead of `.next/` (server mode).
/// Otter serves content from IPFS, so only static exports work.
fn ensure_nextjs_static_export(root: &Path) {
    let config_files = ["next.config.mjs", "next.config.js", "next.config.ts"];
    for name in &config_files {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return,
        };
        // Already has output configured — don't override
        if content.contains("output") {
            return;
        }
        // Find the config object opening brace and inject `output: 'export'`
        // Works for: `const nextConfig = {`, `const config: NextConfig = {`, etc.
        if let Some(brace_pos) = content.rfind("= {").map(|p| p + 2) {
            let mut patched = String::with_capacity(content.len() + 30);
            patched.push_str(&content[..=brace_pos]);
            patched.push_str("\n  output: 'export',");
            patched.push_str(&content[brace_pos + 1..]);
            if std::fs::write(&path, &patched).is_ok() {
                tracing::info!(file = %name, "Injected output: 'export' into Next.js config for static hosting");
            }
        }
        return;
    }
}

fn prepare_deploy_dir(root: &Path) -> Result<BuildOutcome, String> {
    let framework = detect_framework(root);
    let package_json = root.join("package.json");

    if package_json.exists() {
        let pkg_contents = std::fs::read_to_string(&package_json).ok();
        let pkg_json = pkg_contents
            .as_ref()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());

        let has_build_script = pkg_json
            .as_ref()
            .and_then(|v| v.get("scripts"))
            .and_then(|s| s.get("build"))
            .is_some();

        if has_build_script {
            // For Next.js: ensure static export is configured so `next build`
            // produces an `out/` directory with static HTML files.
            // Without this, Next.js only creates `.next/` (server mode).
            if framework == "Next.js" {
                ensure_nextjs_static_export(root);
            }

            // Detect package manager from lock files
            let (pm, install_args, build_args) = detect_package_manager(root);

            // Collect only safe env vars to prevent leaking secrets to build scripts
            let mut safe_env: Vec<(String, String)> = BUILD_ENV_ALLOWLIST
                .iter()
                .filter_map(|key| std::env::var(key).ok().map(|val| (key.to_string(), val)))
                .collect();

            // Nuxt 3: force static preset so `nuxi build` produces .output/public/
            if framework == "Nuxt" {
                safe_env.push(("NITRO_PRESET".to_string(), "static".to_string()));
            }

            let mut logs = format!("Detected framework: {} ({})\n\n", framework, pm);

            let install_cmd = format!("$ {} {}", pm, install_args.join(" "));
            logs.push_str(&install_cmd);
            logs.push('\n');
            let output =
                run_cmd_with_timeout(pm, install_args, root, &safe_env, BUILD_TIMEOUT_SECS)?;
            logs.push_str(&String::from_utf8_lossy(&output.stdout));
            logs.push_str(&String::from_utf8_lossy(&output.stderr));
            if !output.status.success() {
                return Err(format!("{} install failed\n{}", pm, trim_logs(&logs)));
            }

            let build_cmd = format!("$ {} {}", pm, build_args.join(" "));
            logs.push_str(&format!("\n{}\n", build_cmd));
            let output =
                run_cmd_with_timeout(pm, build_args, root, &safe_env, BUILD_TIMEOUT_SECS)?;
            logs.push_str(&String::from_utf8_lossy(&output.stdout));
            logs.push_str(&String::from_utf8_lossy(&output.stderr));
            if !output.status.success() {
                return Err(format!("{} build failed\n{}", pm, trim_logs(&logs)));
            }

            // Check all known output directories
            for dir_name in OUTPUT_DIRS {
                // Skip 'public' for Next.js — it's the static assets dir, not build output
                if *dir_name == "public" && framework == "Next.js" {
                    continue;
                }
                let dir = root.join(dir_name);
                if dir.exists() && dir.is_dir() {
                    logs.push_str(&format!("\nOutput directory: {}/\n", dir_name));
                    return Ok(BuildOutcome {
                        deploy_root: dir,
                        build_status: format!("Built ({})", framework),
                        build_logs: trim_logs(&logs),
                    });
                }
            }

            return Err(format!(
                "Build finished but no output folder found.\nLooked for: {}\n{}",
                OUTPUT_DIRS.join(", "),
                trim_logs(&logs)
            ));
        }
    }

    // No package.json or no build script — check for static site generators
    // that don't use npm (Hugo, Jekyll, etc.)
    if root.join("index.html").exists() {
        return Ok(BuildOutcome {
            deploy_root: root.to_path_buf(),
            build_status: format!("Static ({})", framework),
            build_logs: format!("Detected framework: {}\nNo build needed — deploying as-is.", framework),
        });
    }

    // Check if there's a public/ folder with static content
    let public_dir = root.join("public");
    if public_dir.exists() && public_dir.join("index.html").exists() {
        return Ok(BuildOutcome {
            deploy_root: public_dir,
            build_status: format!("Static ({})", framework),
            build_logs: format!("Detected framework: {}\nDeploying public/ directory.", framework),
        });
    }

    Ok(BuildOutcome {
        deploy_root: root.to_path_buf(),
        build_status: "Skipped".to_string(),
        build_logs: format!("Detected framework: {}\nNo build script detected. Deployed repository as-is.", framework),
    })
}

fn trim_logs(logs: &str) -> String {
    const MAX: usize = 12000;
    if logs.len() <= MAX {
        logs.to_string()
    } else {
        let start = logs.len().saturating_sub(MAX);
        format!("…\n{}", &logs[start..])
    }
}

/// Run a command and stream stdout/stderr lines to a BuildSession.
fn run_cmd_streaming(
    cmd: &str,
    args: &[&str],
    dir: &Path,
    env: &[(String, String)],
    timeout_secs: u64,
    session: &Arc<BuildSession>,
) -> Result<(bool, String), String> {
    use std::io::BufRead;
    use std::process::Stdio;

    let mut child = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", cmd, e))?;

    let stdout = child.stdout.take()
        .ok_or_else(|| format!("Failed to capture stdout from {}", cmd))?;
    let stderr = child.stderr.take()
        .ok_or_else(|| format!("Failed to capture stderr from {}", cmd))?;

    // Collect output for build logs while also streaming to session
    let session_out = Arc::clone(session);
    let out_handle = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut lines = Vec::new();
        for line in reader.lines() {
            if let Ok(line) = line {
                session_out.push_log(&line);
                lines.push(line);
            }
        }
        lines.join("\n")
    });

    let session_err = Arc::clone(session);
    let err_handle = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        let mut lines = Vec::new();
        for line in reader.lines() {
            if let Ok(line) = line {
                session_err.push_log(&line);
                lines.push(line);
            }
        }
        lines.join("\n")
    });

    // Wait with timeout
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout_text = out_handle.join().unwrap_or_default();
                let stderr_text = err_handle.join().unwrap_or_default();
                let mut all = stdout_text;
                if !stderr_text.is_empty() {
                    if !all.is_empty() { all.push('\n'); }
                    all.push_str(&stderr_text);
                }
                return Ok((status.success(), all));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} timed out after {}s", cmd, timeout_secs));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Failed to wait for {}: {}", cmd, e)),
        }
    }
}

/// Streaming variant of prepare_deploy_dir that pushes log lines to a BuildSession.
fn prepare_deploy_dir_streaming(root: &Path, session: &Arc<BuildSession>) -> Result<BuildOutcome, String> {
    let framework = detect_framework(root);
    let package_json = root.join("package.json");

    if package_json.exists() {
        let pkg_contents = std::fs::read_to_string(&package_json).ok();
        let pkg_json = pkg_contents
            .as_ref()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());

        let has_build_script = pkg_json
            .as_ref()
            .and_then(|v| v.get("scripts"))
            .and_then(|s| s.get("build"))
            .is_some();

        if has_build_script {
            if framework == "Next.js" {
                ensure_nextjs_static_export(root);
            }

            let (pm, install_args, build_args) = detect_package_manager(root);

            let mut safe_env: Vec<(String, String)> = BUILD_ENV_ALLOWLIST
                .iter()
                .filter_map(|key| std::env::var(key).ok().map(|val| (key.to_string(), val)))
                .collect();

            if framework == "Nuxt" {
                safe_env.push(("NITRO_PRESET".to_string(), "static".to_string()));
            }

            session.push_log(&format!("Detected framework: {} ({})", framework, pm));

            let install_cmd = format!("$ {} {}", pm, install_args.join(" "));
            session.push_log(&install_cmd);
            let mut logs = format!("{}\n", install_cmd);

            let (success, output) = run_cmd_streaming(pm, install_args, root, &safe_env, BUILD_TIMEOUT_SECS, session)?;
            logs.push_str(&output);
            if !success {
                return Err(format!("{} install failed", pm));
            }

            let build_cmd = format!("$ {} {}", pm, build_args.join(" "));
            session.push_log(&build_cmd);
            logs.push_str(&format!("\n{}\n", build_cmd));

            let (success, output) = run_cmd_streaming(pm, build_args, root, &safe_env, BUILD_TIMEOUT_SECS, session)?;
            logs.push_str(&output);
            if !success {
                return Err(format!("{} build failed", pm));
            }

            for dir_name in OUTPUT_DIRS {
                if *dir_name == "public" && framework == "Next.js" {
                    continue;
                }
                let dir = root.join(dir_name);
                if dir.exists() && dir.is_dir() {
                    session.push_log(&format!("Output directory: {}/", dir_name));
                    return Ok(BuildOutcome {
                        deploy_root: dir,
                        build_status: format!("Built ({})", framework),
                        build_logs: trim_logs(&logs),
                    });
                }
            }

            return Err(format!(
                "Build finished but no output folder found. Looked for: {}",
                OUTPUT_DIRS.join(", ")
            ));
        }
    }

    if root.join("index.html").exists() {
        session.push_log(&format!("Detected framework: {} — deploying static site", framework));
        return Ok(BuildOutcome {
            deploy_root: root.to_path_buf(),
            build_status: format!("Static ({})", framework),
            build_logs: format!("Detected framework: {}\nNo build needed.", framework),
        });
    }

    let public_dir = root.join("public");
    if public_dir.exists() && public_dir.join("index.html").exists() {
        session.push_log("Deploying public/ directory");
        return Ok(BuildOutcome {
            deploy_root: public_dir,
            build_status: format!("Static ({})", framework),
            build_logs: format!("Detected framework: {}\nDeploying public/ directory.", framework),
        });
    }

    session.push_log("No build script detected, deploying as-is");
    Ok(BuildOutcome {
        deploy_root: root.to_path_buf(),
        build_status: "Skipped".to_string(),
        build_logs: format!("Detected framework: {}\nNo build script detected.", framework),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub name: String,
    pub cid: String,
    pub size: u64,
    pub file_count: usize,
    pub created_at: String,
    pub source: String,
    pub branch: Option<String>,
    pub repo_url: Option<String>,
    pub build_status: String,
    pub build_logs: Option<String>,
    pub status: String,
    #[serde(default)]
    pub domain: Option<String>,
    /// Subdirectory within the repo containing the project (e.g. "frontend")
    #[serde(default)]
    pub root_directory: Option<String>,
}

impl Deployment {
    pub fn new(name: String, cid: String, size: u64, file_count: usize) -> Self {
        Self {
            name,
            cid,
            size,
            file_count,
            created_at: chrono::Utc::now().format("%b %d, %H:%M").to_string(),
            source: "upload".to_string(),
            branch: None,
            repo_url: None,
            build_status: "Uploaded".to_string(),
            build_logs: Some("Uploaded ZIP without build step".to_string()),
            status: "Ready".to_string(),
            domain: None,
            root_directory: None,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new_github(
        name: String,
        cid: String,
        size: u64,
        file_count: usize,
        branch: String,
        repo_url: Option<String>,
        build_status: String,
        build_logs: Option<String>,
        root_directory: Option<String>,
    ) -> Self {
        Self {
            name,
            cid,
            size,
            file_count,
            created_at: chrono::Utc::now().format("%b %d, %H:%M").to_string(),
            source: "github".to_string(),
            branch: Some(branch),
            repo_url,
            build_status,
            build_logs,
            status: "Ready".to_string(),
            domain: None,
            root_directory,
        }
    }

    /// Save deployment to Redis
    pub async fn save_to_redis(
        &self,
        redis: &RedisClient,
    ) -> Result<(), crate::redis_client::RedisClientError> {
        // Store the deployment JSON
        redis
            .set_json(&format!("deployment:{}", self.cid), self, None)
            .await?;

        // Add to sorted set for ordering (by timestamp)
        let timestamp = chrono::Utc::now().timestamp() as f64;
        redis.zadd("deployments", &self.cid, timestamp).await?;

        Ok(())
    }

    /// Load all deployments from Redis
    pub async fn load_all_from_redis(
        redis: &Arc<RedisClient>,
    ) -> Result<Vec<Deployment>, crate::redis_client::RedisClientError> {
        // Get all deployment CIDs from sorted set (newest first)
        let cids = redis.zrevrange("deployments", 0, -1).await?;

        let mut deployments = Vec::with_capacity(cids.len());
        for cid in cids {
            if let Some(deployment) = redis
                .get_json::<Deployment>(&format!("deployment:{}", cid))
                .await?
            {
                deployments.push(deployment);
            }
        }

        Ok(deployments)
    }

    /// Delete deployment from Redis
    pub async fn delete_from_redis(
        cid: &str,
        redis: &RedisClient,
    ) -> Result<(), crate::redis_client::RedisClientError> {
        redis.delete(&format!("deployment:{}", cid)).await?;
        redis.zrem("deployments", cid).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Deployment::new ─────────────────────────────────────────────

    #[test]
    fn deployment_new_sets_basic_fields() {
        let d = Deployment::new("my-site".into(), "QmABC123".into(), 4096, 5);
        assert_eq!(d.name, "my-site");
        assert_eq!(d.cid, "QmABC123");
        assert_eq!(d.size, 4096);
        assert_eq!(d.file_count, 5);
    }

    #[test]
    fn deployment_new_defaults() {
        let d = Deployment::new("app".into(), "QmXYZ".into(), 100, 1);
        assert_eq!(d.source, "upload");
        assert_eq!(d.status, "Ready");
        assert_eq!(d.build_status, "Uploaded");
        assert!(d.branch.is_none());
        assert!(d.repo_url.is_none());
        assert!(d.domain.is_none());
        assert!(d.build_logs.is_some());
    }

    #[test]
    fn deployment_new_created_at_is_nonempty() {
        let d = Deployment::new("x".into(), "QmZ".into(), 0, 0);
        assert!(!d.created_at.is_empty());
    }

    // ── Deployment::new_github ──────────────────────────────────────

    #[test]
    fn deployment_new_github_sets_source() {
        let d = Deployment::new_github(
            "repo".into(),
            "QmGH".into(),
            2048,
            3,
            "main".into(),
            Some("https://github.com/user/repo".into()),
            "Built".into(),
            Some("build logs here".into()),
            None,
        );
        assert_eq!(d.source, "github");
        assert_eq!(d.branch, Some("main".to_string()));
        assert_eq!(
            d.repo_url,
            Some("https://github.com/user/repo".to_string())
        );
        assert_eq!(d.build_status, "Built");
        assert_eq!(d.status, "Ready");
        assert!(d.domain.is_none());
        assert!(d.root_directory.is_none());
    }

    #[test]
    fn deployment_new_github_optional_repo_url() {
        let d = Deployment::new_github(
            "repo".into(),
            "QmXX".into(),
            0,
            0,
            "dev".into(),
            None,
            "Skipped".into(),
            None,
            Some("packages/frontend".into()),
        );
        assert!(d.repo_url.is_none());
        assert!(d.build_logs.is_none());
        assert_eq!(d.root_directory, Some("packages/frontend".to_string()));
    }

    // ── Serialization / Deserialization ─────────────────────────────

    #[test]
    fn deployment_serializes_to_json() {
        let d = Deployment::new("test".into(), "QmT".into(), 1024, 2);
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["name"], "test");
        assert_eq!(json["cid"], "QmT");
        assert_eq!(json["size"], 1024);
        assert_eq!(json["file_count"], 2);
        assert_eq!(json["source"], "upload");
        assert_eq!(json["status"], "Ready");
        assert!(json["domain"].is_null());
    }

    #[test]
    fn deployment_roundtrip_json() {
        let original = Deployment::new_github(
            "my-repo".into(),
            "QmRoundTrip".into(),
            8192,
            10,
            "feature/x".into(),
            Some("https://github.com/user/my-repo".into()),
            "Built".into(),
            Some("npm install\nnpm run build".into()),
            Some("packages/web".into()),
        );

        let json_str = serde_json::to_string(&original).unwrap();
        let restored: Deployment = serde_json::from_str(&json_str).unwrap();

        assert_eq!(restored.name, original.name);
        assert_eq!(restored.cid, original.cid);
        assert_eq!(restored.size, original.size);
        assert_eq!(restored.file_count, original.file_count);
        assert_eq!(restored.source, original.source);
        assert_eq!(restored.branch, original.branch);
        assert_eq!(restored.repo_url, original.repo_url);
        assert_eq!(restored.build_status, original.build_status);
        assert_eq!(restored.build_logs, original.build_logs);
        assert_eq!(restored.status, original.status);
        assert_eq!(restored.domain, original.domain);
    }

    #[test]
    fn deployment_domain_default_is_none() {
        // Ensure serde(default) works: domain absent in JSON -> None
        let json_str = r#"{
            "name": "test",
            "cid": "QmTest",
            "size": 100,
            "file_count": 1,
            "created_at": "Jan 01, 00:00",
            "source": "upload",
            "branch": null,
            "repo_url": null,
            "build_status": "Uploaded",
            "build_logs": null,
            "status": "Ready"
        }"#;

        let d: Deployment = serde_json::from_str(json_str).unwrap();
        assert!(d.domain.is_none());
    }

    #[test]
    fn deployment_domain_deserializes_when_present() {
        let json_str = r#"{
            "name": "test",
            "cid": "QmTest",
            "size": 100,
            "file_count": 1,
            "created_at": "Jan 01, 00:00",
            "source": "upload",
            "branch": null,
            "repo_url": null,
            "build_status": "Uploaded",
            "build_logs": null,
            "status": "Ready",
            "domain": "my-app.shadow"
        }"#;

        let d: Deployment = serde_json::from_str(json_str).unwrap();
        assert_eq!(d.domain, Some("my-app.shadow".to_string()));
    }

    // ── parse_github_url ────────────────────────────────────────────

    #[test]
    fn parse_github_url_https() {
        let info = parse_github_url("https://github.com/user/repo").unwrap();
        assert_eq!(info.owner, "user");
        assert_eq!(info.repo, "repo");
    }

    #[test]
    fn parse_github_url_trailing_slash() {
        let info = parse_github_url("https://github.com/user/repo/").unwrap();
        assert_eq!(info.owner, "user");
        assert_eq!(info.repo, "repo");
    }

    #[test]
    fn parse_github_url_dot_git_suffix() {
        let info = parse_github_url("https://github.com/user/repo.git").unwrap();
        assert_eq!(info.owner, "user");
        assert_eq!(info.repo, "repo");
    }

    #[test]
    fn parse_github_url_without_scheme() {
        let info = parse_github_url("github.com/user/repo").unwrap();
        assert_eq!(info.owner, "user");
        assert_eq!(info.repo, "repo");
    }

    #[test]
    fn parse_github_url_invalid() {
        assert!(parse_github_url("https://gitlab.com/user/repo").is_none());
        assert!(parse_github_url("not-a-url").is_none());
        assert!(parse_github_url("https://github.com/solo").is_none());
    }

    // ── trim_logs ───────────────────────────────────────────────────

    #[test]
    fn trim_logs_short_unchanged() {
        let short = "hello world";
        assert_eq!(trim_logs(short), short);
    }

    #[test]
    fn trim_logs_long_truncated() {
        let long = "x".repeat(20_000);
        let trimmed = trim_logs(&long);
        // Should start with the ellipsis marker
        assert!(trimmed.starts_with('\u{2026}'));
        // Total length should be 12000 + a few bytes for prefix
        assert!(trimmed.len() <= 12_010);
    }
}
