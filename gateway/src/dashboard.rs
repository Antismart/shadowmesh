//! Dashboard for ShadowMesh Gateway
//!
//! Provides a Vercel-like web interface for deploying and managing sites.

use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use crate::AppState;

/// Main dashboard page - Vercel-like UI
pub async fn dashboard_handler(State(state): State<AppState>) -> impl IntoResponse {
    let deployments = state.deployments.read().unwrap();
    let deployment_rows: String = deployments.iter()
        .map(|d| format!(r#"
            <tr>
                <td>
                    <div class="deploy-name">{}</div>
                    <div class="deploy-cid">{}</div>
                </td>
                <td><span class="status ready">● Ready</span></td>
                <td>{}</td>
                <td>
                    <a href="/ipfs/{}/index.html" target="_blank" class="btn btn-sm">Visit</a>
                    <button onclick="copyUrl('{}')" class="btn btn-sm btn-secondary">Copy URL</button>
                </td>
            </tr>
        "#, 
            d.name,
            &d.cid[..12],
            d.created_at,
            d.cid,
            d.cid
        ))
        .collect();

    let html = format!(r##"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ShadowMesh Dashboard</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}
        
        :root {{
            --bg-primary: #000;
            --bg-secondary: #111;
            --bg-card: #1a1a1a;
            --text-primary: #fff;
            --text-secondary: #888;
            --accent: #0070f3;
            --accent-hover: #0060df;
            --success: #50e3c2;
            --border: #333;
        }}
        
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            background: var(--bg-primary);
            color: var(--text-primary);
            min-height: 100vh;
        }}
        
        /* Navigation */
        .navbar {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 0 24px;
            height: 64px;
            border-bottom: 1px solid var(--border);
            background: var(--bg-secondary);
        }}
        
        .logo {{
            display: flex;
            align-items: center;
            gap: 12px;
            font-size: 18px;
            font-weight: 600;
        }}
        
        .logo-icon {{
            width: 32px;
            height: 32px;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            border-radius: 8px;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        
        .nav-links {{
            display: flex;
            gap: 24px;
        }}
        
        .nav-links a {{
            color: var(--text-secondary);
            text-decoration: none;
            font-size: 14px;
            transition: color 0.2s;
        }}
        
        .nav-links a:hover, .nav-links a.active {{
            color: var(--text-primary);
        }}
        
        /* Main Content */
        .container {{
            max-width: 1200px;
            margin: 0 auto;
            padding: 40px 24px;
        }}
        
        .header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 32px;
        }}
        
        .header h1 {{
            font-size: 32px;
            font-weight: 600;
        }}
        
        /* Deploy Button */
        .btn {{
            background: var(--accent);
            color: white;
            border: none;
            padding: 10px 20px;
            border-radius: 6px;
            font-size: 14px;
            font-weight: 500;
            cursor: pointer;
            transition: all 0.2s;
            text-decoration: none;
            display: inline-flex;
            align-items: center;
            gap: 8px;
        }}
        
        .btn:hover {{
            background: var(--accent-hover);
        }}
        
        .btn-sm {{
            padding: 6px 12px;
            font-size: 13px;
        }}
        
        .btn-secondary {{
            background: transparent;
            border: 1px solid var(--border);
        }}
        
        .btn-secondary:hover {{
            background: var(--bg-card);
            border-color: var(--text-secondary);
        }}
        
        /* Upload Area */
        .upload-area {{
            background: var(--bg-card);
            border: 2px dashed var(--border);
            border-radius: 12px;
            padding: 60px;
            text-align: center;
            margin-bottom: 40px;
            transition: all 0.3s;
            cursor: pointer;
        }}
        
        .upload-area:hover, .upload-area.dragover {{
            border-color: var(--accent);
            background: rgba(0, 112, 243, 0.05);
        }}
        
        .upload-area h2 {{
            font-size: 20px;
            margin-bottom: 12px;
        }}
        
        .upload-area p {{
            color: var(--text-secondary);
            margin-bottom: 20px;
        }}
        
        .upload-area input[type="file"] {{
            display: none;
        }}
        
        /* Deployments Table */
        .deployments-section h2 {{
            font-size: 20px;
            margin-bottom: 20px;
        }}
        
        .deployments-table {{
            width: 100%;
            border-collapse: collapse;
            background: var(--bg-card);
            border-radius: 12px;
            overflow: hidden;
        }}
        
        .deployments-table th,
        .deployments-table td {{
            padding: 16px 20px;
            text-align: left;
        }}
        
        .deployments-table th {{
            background: var(--bg-secondary);
            color: var(--text-secondary);
            font-size: 12px;
            font-weight: 500;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }}
        
        .deployments-table tr {{
            border-bottom: 1px solid var(--border);
        }}
        
        .deployments-table tr:last-child {{
            border-bottom: none;
        }}
        
        .deployments-table tr:hover {{
            background: rgba(255, 255, 255, 0.02);
        }}
        
        .deploy-name {{
            font-weight: 500;
            margin-bottom: 4px;
        }}
        
        .deploy-cid {{
            font-family: monospace;
            font-size: 12px;
            color: var(--text-secondary);
        }}
        
        .status {{
            display: inline-flex;
            align-items: center;
            gap: 6px;
            font-size: 13px;
        }}
        
        .status.ready {{
            color: var(--success);
        }}
        
        /* Empty State */
        .empty-state {{
            text-align: center;
            padding: 60px;
            color: var(--text-secondary);
        }}
        
        .empty-state svg {{
            width: 64px;
            height: 64px;
            margin-bottom: 20px;
            opacity: 0.5;
        }}
        
        /* Progress Bar */
        .progress-container {{
            display: none;
            margin-top: 20px;
        }}
        
        .progress-bar {{
            height: 4px;
            background: var(--border);
            border-radius: 2px;
            overflow: hidden;
        }}
        
        .progress-fill {{
            height: 100%;
            background: var(--accent);
            width: 0%;
            transition: width 0.3s;
        }}
        
        .progress-text {{
            margin-top: 10px;
            font-size: 14px;
            color: var(--text-secondary);
        }}
        
        /* Toast Notification */
        .toast {{
            position: fixed;
            bottom: 24px;
            right: 24px;
            background: var(--bg-card);
            border: 1px solid var(--border);
            padding: 16px 24px;
            border-radius: 8px;
            display: none;
            animation: slideIn 0.3s ease;
        }}
        
        .toast.success {{
            border-color: var(--success);
        }}
        
        @keyframes slideIn {{
            from {{ transform: translateY(20px); opacity: 0; }}
            to {{ transform: translateY(0); opacity: 1; }}
        }}
        
        /* Stats Cards */
        .stats {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin-bottom: 40px;
        }}
        
        .stat-card {{
            background: var(--bg-card);
            border: 1px solid var(--border);
            border-radius: 12px;
            padding: 24px;
        }}
        
        .stat-card h3 {{
            font-size: 12px;
            color: var(--text-secondary);
            text-transform: uppercase;
            letter-spacing: 0.5px;
            margin-bottom: 8px;
        }}
        
        .stat-card .value {{
            font-size: 32px;
            font-weight: 600;
        }}
        
        /* Modal */
        .modal-overlay {{
            display: none;
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            bottom: 0;
            background: rgba(0, 0, 0, 0.8);
            z-index: 1000;
            align-items: center;
            justify-content: center;
        }}
        
        .modal {{
            background: var(--bg-card);
            border-radius: 12px;
            padding: 32px;
            max-width: 500px;
            width: 90%;
        }}
        
        .modal h2 {{
            margin-bottom: 20px;
        }}
        
        .modal-close {{
            float: right;
            background: none;
            border: none;
            color: var(--text-secondary);
            font-size: 24px;
            cursor: pointer;
        }}
    </style>
</head>
<body>
    <nav class="navbar">
        <div class="logo">
            <div class="logo-icon">◈</div>
            ShadowMesh
        </div>
        <div class="nav-links">
            <a href="/dashboard" class="active">Deployments</a>
            <a href="/dashboard/domains">Domains</a>
            <a href="/dashboard/settings">Settings</a>
            <a href="/metrics">Metrics</a>
        </div>
    </nav>
    
    <div class="container">
        <div class="header">
            <h1>Deployments</h1>
            <button class="btn" onclick="document.getElementById('fileInput').click()">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M12 5v14M5 12h14"/>
                </svg>
                New Deployment
            </button>
        </div>
        
        <!-- Stats -->
        <div class="stats">
            <div class="stat-card">
                <h3>Total Deployments</h3>
                <div class="value">{}</div>
            </div>
            <div class="stat-card">
                <h3>Total Size</h3>
                <div class="value">{}</div>
            </div>
            <div class="stat-card">
                <h3>Status</h3>
                <div class="value" style="color: var(--success);">● Online</div>
            </div>
        </div>
        
        <!-- Upload Area -->
        <div class="upload-area" id="uploadArea">
            <h2>Deploy your project</h2>
            <p>Drag and drop a ZIP file here, or click to browse</p>
            <button class="btn">Select ZIP File</button>
            <input type="file" id="fileInput" accept=".zip" />
            
            <div class="progress-container" id="progressContainer">
                <div class="progress-bar">
                    <div class="progress-fill" id="progressFill"></div>
                </div>
                <div class="progress-text" id="progressText">Uploading...</div>
            </div>
        </div>
        
        <!-- Deployments List -->
        <div class="deployments-section">
            <h2>Recent Deployments</h2>
            {}
        </div>
    </div>
    
    <!-- Toast -->
    <div class="toast" id="toast"></div>
    
    <script>
        const uploadArea = document.getElementById('uploadArea');
        const fileInput = document.getElementById('fileInput');
        const progressContainer = document.getElementById('progressContainer');
        const progressFill = document.getElementById('progressFill');
        const progressText = document.getElementById('progressText');
        const toast = document.getElementById('toast');
        
        // Drag and drop
        uploadArea.addEventListener('dragover', (e) => {{
            e.preventDefault();
            uploadArea.classList.add('dragover');
        }});
        
        uploadArea.addEventListener('dragleave', () => {{
            uploadArea.classList.remove('dragover');
        }});
        
        uploadArea.addEventListener('drop', (e) => {{
            e.preventDefault();
            uploadArea.classList.remove('dragover');
            const files = e.dataTransfer.files;
            if (files.length > 0) {{
                uploadFile(files[0]);
            }}
        }});
        
        uploadArea.addEventListener('click', () => {{
            fileInput.click();
        }});
        
        fileInput.addEventListener('change', (e) => {{
            if (e.target.files.length > 0) {{
                uploadFile(e.target.files[0]);
            }}
        }});
        
        async function uploadFile(file) {{
            if (!file.name.endsWith('.zip')) {{
                showToast('Please upload a ZIP file', 'error');
                return;
            }}
            
            progressContainer.style.display = 'block';
            progressFill.style.width = '0%';
            progressText.textContent = 'Uploading...';
            
            const formData = new FormData();
            formData.append('file', file);
            
            try {{
                // Simulate progress
                let progress = 0;
                const progressInterval = setInterval(() => {{
                    progress += 10;
                    if (progress <= 90) {{
                        progressFill.style.width = progress + '%';
                    }}
                }}, 200);
                
                const response = await fetch('/api/deploy', {{
                    method: 'POST',
                    body: formData
                }});
                
                clearInterval(progressInterval);
                progressFill.style.width = '100%';
                
                const result = await response.json();
                
                if (result.success) {{
                    progressText.textContent = 'Deployed successfully!';
                    showToast('🎉 Deployment successful! CID: ' + result.cid.substring(0, 12) + '...', 'success');
                    
                    // Reload after 1.5 seconds
                    setTimeout(() => {{
                        window.location.reload();
                    }}, 1500);
                }} else {{
                    progressText.textContent = 'Deployment failed: ' + result.error;
                    showToast('Deployment failed: ' + result.error, 'error');
                }}
            }} catch (err) {{
                progressText.textContent = 'Error: ' + err.message;
                showToast('Error: ' + err.message, 'error');
            }}
        }}
        
        function showToast(message, type) {{
            toast.textContent = message;
            toast.className = 'toast ' + type;
            toast.style.display = 'block';
            
            setTimeout(() => {{
                toast.style.display = 'none';
            }}, 5000);
        }}
        
        function copyUrl(cid) {{
            const url = window.location.origin + '/ipfs/' + cid + '/index.html';
            navigator.clipboard.writeText(url).then(() => {{
                showToast('URL copied to clipboard!', 'success');
            }});
        }}
    </script>
</body>
</html>
    "##,
        deployments.len(),
        format_size(deployments.iter().map(|d| d.size).sum()),
        if deployment_rows.is_empty() {
            r#"<div class="empty-state">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1">
                    <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/>
                    <polyline points="3.27 6.96 12 12.01 20.73 6.96"/>
                    <line x1="12" y1="22.08" x2="12" y2="12"/>
                </svg>
                <h3>No deployments yet</h3>
                <p>Deploy your first project by uploading a ZIP file above</p>
            </div>"#.to_string()
        } else {
            format!(r#"<table class="deployments-table">
                <thead>
                    <tr>
                        <th>Project</th>
                        <th>Status</th>
                        <th>Created</th>
                        <th>Actions</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>"#, deployment_rows)
        }
    );

    Html(html)
}

/// Format bytes to human readable
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Deployment record
#[derive(Debug, Clone)]
pub struct Deployment {
    pub name: String,
    pub cid: String,
    pub size: u64,
    pub file_count: usize,
    pub created_at: String,
}

impl Deployment {
    pub fn new(name: String, cid: String, size: u64, file_count: usize) -> Self {
        let created_at = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
        Self {
            name,
            cid,
            size,
            file_count,
            created_at,
        }
    }
}
