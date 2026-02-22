//! Dashboard for ShadowMesh Gateway - Tailwind CSS Version

use crate::{
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

#[allow(dead_code)]
pub async fn dashboard_handler(State(state): State<AppState>) -> impl IntoResponse {
    let deployments = read_lock(&state.deployments);
    let mut grouped: BTreeMap<String, Vec<&Deployment>> = BTreeMap::new();
    for deployment in deployments.iter() {
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
        let redeploy_button = if d.source == "github" {
            format!(
                r##"<button onclick=\"event.stopPropagation();redeployDeployment('{}')\" class=\"btn-ghost\">Redeploy</button>"##,
                d.cid
            )
        } else {
            String::new()
        };
        format!(
            r##"<div class="card deploy-card" onclick="window.open('/ipfs/{}/index.html','_blank')">
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
                        <button onclick="event.stopPropagation();navigator.clipboard.writeText(location.origin+'/ipfs/{}/index.html');alert('URL copied!')" class="btn-ghost">Copy URL</button>
                        {}
                        <button onclick="event.stopPropagation();deleteDeployment('{}')" class="btn-ghost btn-danger">Delete</button>
                    </div>
                </div>
            </div>"##,
            d.cid,
            d.name,
            source_label,
            branch_display,
            d.created_at,
            status_class,
            status_icon,
            d.status,
            build_class,
            build_status,
            d.cid,
            &d.cid[..8.min(d.cid.len())],
            d.cid,
            redeploy_button,
            d.cid
        )
    };

    let deployment_rows: String = grouped
        .iter()
        .map(|(key, items)| {
            let title = if key == "Uploads" {
                "Uploads".to_string()
            } else {
                format!("GitHub · {}", key)
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
}

pub async fn deploy_from_github(
    State(state): State<AppState>,
    Json(request): Json<GithubDeployRequest>,
) -> impl IntoResponse {
    let url = request.url.trim();
    let branch = request.branch.as_deref().unwrap_or("main");

    match deploy_github_project(&state, url, branch).await {
        Ok(payload) => (StatusCode::OK, Json(payload)).into_response(),
        Err((status, message)) => {
            (status, Json(json!({"success": false, "error": message}))).into_response()
        }
    }
}

pub async fn redeploy_github(
    State(state): State<AppState>,
    AxumPath(cid): AxumPath<String>,
) -> impl IntoResponse {
    let deployment = {
        let deployments = read_lock(&state.deployments);
        deployments.iter().find(|d| d.cid == cid).cloned()
    };

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

    match deploy_github_project(&state, &repo_url, &branch).await {
        Ok(payload) => {
            {
                let mut deployments = write_lock(&state.deployments);
                deployments.retain(|d| d.cid != cid);
            }
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

pub async fn get_deployments(State(state): State<AppState>) -> impl IntoResponse {
    let deployments = read_lock(&state.deployments);
    Json(deployments.clone())
}

pub async fn delete_deployment(
    State(state): State<AppState>,
    AxumPath(cid): AxumPath<String>,
) -> impl IntoResponse {
    let found = {
        let mut deployments = write_lock(&state.deployments);
        let before = deployments.len();
        deployments.retain(|d| d.cid != cid);
        deployments.len() != before
    };

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
        Json(json!({"success": true})).into_response()
    }
}

pub async fn deployment_logs(
    State(state): State<AppState>,
    AxumPath(cid): AxumPath<String>,
) -> impl IntoResponse {
    let deployments = read_lock(&state.deployments);
    if let Some(deployment) = deployments.iter().find(|d| d.cid == cid) {
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

pub async fn github_login(State(state): State<AppState>) -> impl IntoResponse {
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

    let redirect_uri = std::env::var("GITHUB_REDIRECT_URL").unwrap_or_else(|_| {
        format!(
            "http://localhost:{}/api/github/callback",
            state.config.server.port
        )
    });

    let csrf_state = Uuid::new_v4().to_string();
    *write_lock(&state.github_oauth_state) = Some(csrf_state.clone());

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

    let redirect_uri = std::env::var("GITHUB_REDIRECT_URL").unwrap_or_else(|_| {
        format!(
            "http://localhost:{}/api/github/callback",
            state.config.server.port
        )
    });

    if let Some(expected_state) = read_lock(&state.github_oauth_state).clone() {
        if Some(expected_state) != query.state {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"success": false, "error": "Invalid OAuth state"})),
            )
                .into_response();
        }
    }

    let client = reqwest::Client::new();
    let token_response = match client
        .post("https://github.com/login/oauth/access_token")
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

    let user_response = match client
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

    *write_lock(&state.github_auth) = Some(GithubAuth {
        token: token_body.access_token,
        user,
    });
    *write_lock(&state.github_oauth_state) = None;

    Redirect::to("/").into_response()
}

pub async fn github_status(State(state): State<AppState>) -> impl IntoResponse {
    let auth = read_lock(&state.github_auth).clone();
    if let Some(auth) = auth {
        Json(json!({"connected": true, "user": auth.user}))
    } else {
        Json(json!({"connected": false, "user": null}))
    }
}

pub async fn github_repos(State(state): State<AppState>) -> impl IntoResponse {
    let auth = read_lock(&state.github_auth).clone();
    let auth = match auth {
        Some(auth) => auth,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "GitHub not connected"})),
            )
                .into_response()
        }
    };

    let client = reqwest::Client::new();
    let response = match client
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

struct RepoInfo {
    owner: String,
    repo: String,
}

fn parse_github_url(url: &str) -> Option<RepoInfo> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    if let Some(path) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return Some(RepoInfo {
                owner: parts[0].to_string(),
                repo: parts[1].to_string(),
            });
        }
    }
    if let Some(path) = url.strip_prefix("github.com/") {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return Some(RepoInfo {
                owner: parts[0].to_string(),
                repo: parts[1].to_string(),
            });
        }
    }
    None
}

fn extract_github_zip(data: &[u8]) -> Result<tempfile::TempDir, String> {
    use std::io::{Read, Write};
    let temp_dir = tempfile::TempDir::new().map_err(|e| format!("Temp dir failed: {}", e))?;
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("Invalid ZIP: {}", e))?;
    let mut root_prefix = String::new();
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
        let out_path = temp_dir.path().join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| format!("Dir failed: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut outfile =
                std::fs::File::create(&out_path).map_err(|e| format!("File failed: {}", e))?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)
                .map_err(|e| format!("Read failed: {}", e))?;
            outfile
                .write_all(&contents)
                .map_err(|e| format!("Write failed: {}", e))?;
        }
    }
    Ok(temp_dir)
}

async fn deploy_github_project(
    state: &AppState,
    url: &str,
    branch: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let repo_info =
        parse_github_url(url).ok_or((StatusCode::BAD_REQUEST, "Invalid GitHub URL".to_string()))?;

    let zip_url = format!(
        "https://github.com/{}/{}/archive/refs/heads/{}.zip",
        repo_info.owner, repo_info.repo, branch
    );

    let client = reqwest::Client::new();
    let response = client.get(&zip_url).send().await.map_err(|e| {
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

    let zip_bytes = response
        .bytes()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Failed to read: {}", e)))?;

    let temp_dir =
        extract_github_zip(&zip_bytes).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let build_outcome =
        prepare_deploy_dir(temp_dir.path()).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

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
    );

    // Auto-assign .shadow domain
    deployment.domain = crate::auto_assign_domain(state, &repo_info.repo, &cid);

    // Save to Redis if available
    if let Some(ref redis) = state.redis {
        if let Err(e) = deployment.save_to_redis(redis).await {
            tracing::warn!("Failed to save GitHub deployment to Redis: {}", e);
        }
    }

    metrics::record_deployment(true);

    let domain = deployment.domain.clone();
    write_lock(&state.deployments).insert(0, deployment);

    Ok(json!({
        "success": true,
        "cid": cid,
        "domain": domain,
        "repo": format!("{}/{}", repo_info.owner, repo_info.repo),
        "branch": branch,
        "url": format!("http://localhost:8081/ipfs/{}/index.html", cid)
    }))
}

struct BuildOutcome {
    deploy_root: PathBuf,
    build_status: String,
    build_logs: String,
}

fn prepare_deploy_dir(root: &Path) -> Result<BuildOutcome, String> {
    let package_json = root.join("package.json");
    if package_json.exists() {
        let build_script = std::fs::read_to_string(&package_json)
            .ok()
            .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
            .and_then(|value| value.get("scripts").cloned())
            .and_then(|scripts| scripts.get("build").cloned());

        if build_script.is_some() {
            let mut logs = String::new();
            logs.push_str("$ npm install\n");
            let output = Command::new("npm")
                .arg("install")
                .current_dir(root)
                .output()
                .map_err(|e| format!("Failed to run npm install: {}", e))?;
            logs.push_str(&String::from_utf8_lossy(&output.stdout));
            logs.push_str(&String::from_utf8_lossy(&output.stderr));
            if !output.status.success() {
                return Err(format!("npm install failed\n{}", trim_logs(&logs)));
            }

            logs.push_str("\n$ npm run build\n");
            let output = Command::new("npm")
                .arg("run")
                .arg("build")
                .current_dir(root)
                .output()
                .map_err(|e| format!("Failed to run npm build: {}", e))?;
            logs.push_str(&String::from_utf8_lossy(&output.stdout));
            logs.push_str(&String::from_utf8_lossy(&output.stderr));
            if !output.status.success() {
                return Err(format!("npm run build failed\n{}", trim_logs(&logs)));
            }

            let dist_dir = root.join("dist");
            if dist_dir.exists() {
                return Ok(BuildOutcome {
                    deploy_root: dist_dir,
                    build_status: "Built".to_string(),
                    build_logs: trim_logs(&logs),
                });
            }
            let build_dir = root.join("build");
            if build_dir.exists() {
                return Ok(BuildOutcome {
                    deploy_root: build_dir,
                    build_status: "Built".to_string(),
                    build_logs: trim_logs(&logs),
                });
            }

            return Err(format!(
                "Build finished but no dist/ or build/ folder found.\n{}",
                trim_logs(&logs)
            ));
        }
    }

    Ok(BuildOutcome {
        deploy_root: root.to_path_buf(),
        build_status: "Skipped".to_string(),
        build_logs: "No build script detected. Deployed repository as-is.".to_string(),
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
