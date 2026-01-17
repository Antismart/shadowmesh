//! Dashboard for ShadowMesh Gateway
//!
//! Provides a Vercel-like web interface for deploying and managing sites.

use axum::{
    extract::State,
    response::{Html, IntoResponse, Json},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use crate::AppState;

/// Main dashboard page - Vercel-like UI
pub async fn dashboard_handler(State(state): State<AppState>) -> impl IntoResponse {
    let deployments = state.deployments.read().unwrap();
    let deployment_rows: String = deployments.iter()
        .map(|d| {
            let status_class = if d.status == "Ready" { "ready" } else { "building" };
            let status_icon = if d.status == "Ready" { "●" } else { "◐" };
            let source_icon = if d.source == "github" { 
                r#"<svg class="source-icon" viewBox="0 0 16 16" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>"#
            } else { 
                r#"<svg class="source-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>"#
            };
            let branch_display = d.branch.as_deref().unwrap_or("main");
            format!(r#"
            <div class="deployment-card" onclick="window.open('/ipfs/{cid}/index.html', '_blank')">
                <div class="deployment-header">
                    <div class="deployment-info">
                        <div class="deployment-name">
                            {source_icon}
                            <span>{name}</span>
                        </div>
                        <div class="deployment-meta">
                            <span class="deployment-branch">{branch}</span>
                            <span class="deployment-time">{time}</span>
                        </div>
                    </div>
                    <div class="deployment-status">
                        <span class="status {status_class}">{status_icon} {status}</span>
                    </div>
                </div>
                <div class="deployment-footer">
                    <div class="deployment-url">
                        <a href="/ipfs/{cid}/index.html" target="_blank" onclick="event.stopPropagation()">
                            shadowmesh.local/{cid_short}
                        </a>
                    </div>
                    <div class="deployment-actions">
                        <button class="btn-icon" onclick="event.stopPropagation(); copyUrl('{cid}')" title="Copy URL">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                        </button>
                        <button class="btn-icon" onclick="event.stopPropagation(); window.open('/ipfs/{cid}/index.html', '_blank')" title="Visit">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
                        </button>
                    </div>
                </div>
            </div>
        "#, 
                cid = d.cid,
                name = d.name,
                branch = branch_display,
                time = d.created_at,
                status_class = status_class,
                status_icon = status_icon,
                status = d.status,
                cid_short = &d.cid[..12.min(d.cid.len())],
                source_icon = source_icon
            )
        })
        .collect();

    let html = format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ShadowMesh</title>
    <style>
        *{{margin:0;padding:0;box-sizing:border-box}}
        :root{{
            --bg-100:#0a0a0a;--bg-200:#111;--bg-300:#1a1a1a;--bg-400:#252525;
            --text-100:#fafafa;--text-200:#a1a1a1;--text-300:#6b6b6b;
            --success:#50e3c2;--warning:#f5a623;--error:#e00;
            --border:#2e2e2e;--border-hover:#444;
            --gradient:linear-gradient(135deg,#667eea 0%,#764ba2 100%)
        }}
        body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:var(--bg-100);color:var(--text-100);min-height:100vh;line-height:1.5;-webkit-font-smoothing:antialiased}}
        a{{color:inherit;text-decoration:none}}
        
        .navbar{{position:sticky;top:0;z-index:100;display:flex;align-items:center;justify-content:space-between;padding:0 24px;height:64px;background:rgba(10,10,10,0.8);backdrop-filter:blur(12px);border-bottom:1px solid var(--border)}}
        .nav-left{{display:flex;align-items:center;gap:24px}}
        .logo{{display:flex;align-items:center;gap:10px;font-size:18px;font-weight:600;letter-spacing:-0.5px}}
        .logo-icon{{width:28px;height:28px;background:var(--gradient);border-radius:6px;display:flex;align-items:center;justify-content:center;font-size:14px}}
        .nav-tabs{{display:flex;gap:4px}}
        .nav-tab{{padding:8px 12px;border-radius:6px;font-size:14px;color:var(--text-200);transition:all 0.15s}}
        .nav-tab:hover,.nav-tab.active{{color:var(--text-100);background:var(--bg-300)}}
        
        .main{{max-width:1200px;margin:0 auto;padding:32px 24px}}
        .page-header{{display:flex;justify-content:space-between;align-items:flex-start;margin-bottom:32px}}
        .page-title{{font-size:28px;font-weight:600;letter-spacing:-0.5px;margin-bottom:8px}}
        .page-subtitle{{color:var(--text-200);font-size:14px}}
        
        .btn{{display:inline-flex;align-items:center;justify-content:center;gap:8px;padding:0 16px;height:40px;border-radius:8px;font-size:14px;font-weight:500;cursor:pointer;transition:all 0.15s;border:none}}
        .btn-primary{{background:var(--text-100);color:var(--bg-100)}}
        .btn-primary:hover{{background:var(--text-200)}}
        .btn-secondary{{background:transparent;color:var(--text-100);border:1px solid var(--border)}}
        .btn-secondary:hover{{background:var(--bg-300);border-color:var(--border-hover)}}
        .btn-icon{{display:inline-flex;align-items:center;justify-content:center;width:32px;height:32px;border-radius:6px;background:transparent;border:none;color:var(--text-200);cursor:pointer;transition:all 0.15s}}
        .btn-icon:hover{{background:var(--bg-400);color:var(--text-100)}}
        .btn-icon svg,.btn svg{{width:16px;height:16px}}
        
        .import-section{{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:16px;margin-bottom:48px}}
        .import-card{{background:var(--bg-200);border:1px solid var(--border);border-radius:12px;padding:24px;cursor:pointer;transition:all 0.2s}}
        .import-card:hover{{border-color:var(--border-hover);background:var(--bg-300)}}
        .import-card-icon{{width:48px;height:48px;border-radius:10px;display:flex;align-items:center;justify-content:center;margin-bottom:16px}}
        .import-card-icon.github{{background:#24292e}}
        .import-card-icon.upload{{background:var(--gradient)}}
        .import-card-icon svg{{width:24px;height:24px}}
        .import-card h3{{font-size:16px;font-weight:600;margin-bottom:8px}}
        .import-card p{{font-size:14px;color:var(--text-200)}}
        
        .section-header{{display:flex;justify-content:space-between;align-items:center;margin-bottom:20px}}
        .section-title{{font-size:14px;font-weight:500;color:var(--text-200);text-transform:uppercase;letter-spacing:0.5px}}
        
        .deployments-grid{{display:flex;flex-direction:column;gap:1px;background:var(--border);border:1px solid var(--border);border-radius:12px;overflow:hidden}}
        .deployment-card{{background:var(--bg-200);padding:16px 20px;cursor:pointer;transition:background 0.15s}}
        .deployment-card:hover{{background:var(--bg-300)}}
        .deployment-header{{display:flex;justify-content:space-between;align-items:flex-start;margin-bottom:12px}}
        .deployment-name{{display:flex;align-items:center;gap:8px;font-size:14px;font-weight:500;margin-bottom:4px}}
        .source-icon{{width:16px;height:16px;color:var(--text-200)}}
        .deployment-meta{{display:flex;align-items:center;gap:12px;font-size:13px;color:var(--text-300)}}
        .deployment-branch{{display:flex;align-items:center;gap:4px}}
        .status{{display:inline-flex;align-items:center;gap:6px;padding:4px 10px;border-radius:9999px;font-size:12px;font-weight:500}}
        .status.ready{{background:rgba(80,227,194,0.1);color:var(--success)}}
        .status.building{{background:rgba(245,166,35,0.1);color:var(--warning)}}
        .deployment-footer{{display:flex;justify-content:space-between;align-items:center}}
        .deployment-url a{{font-size:13px;color:var(--text-200);transition:color 0.15s}}
        .deployment-url a:hover{{color:var(--text-100)}}
        .deployment-actions{{display:flex;gap:4px}}
        
        .empty-state{{text-align:center;padding:80px 40px;background:var(--bg-200);border:1px solid var(--border);border-radius:12px}}
        .empty-state-icon{{width:64px;height:64px;margin:0 auto 24px;background:var(--bg-300);border-radius:16px;display:flex;align-items:center;justify-content:center}}
        .empty-state-icon svg{{width:32px;height:32px;color:var(--text-300)}}
        .empty-state h3{{font-size:18px;font-weight:600;margin-bottom:8px}}
        .empty-state p{{color:var(--text-200);font-size:14px;margin-bottom:24px}}
        
        .modal-overlay{{display:none;position:fixed;inset:0;background:rgba(0,0,0,0.7);backdrop-filter:blur(4px);z-index:1000;align-items:center;justify-content:center}}
        .modal-overlay.active{{display:flex}}
        .modal{{background:var(--bg-200);border:1px solid var(--border);border-radius:16px;width:90%;max-width:520px;max-height:90vh;overflow:hidden}}
        .modal-header{{display:flex;justify-content:space-between;align-items:center;padding:20px 24px;border-bottom:1px solid var(--border)}}
        .modal-title{{font-size:18px;font-weight:600}}
        .modal-close{{width:32px;height:32px;border-radius:6px;background:transparent;border:none;color:var(--text-200);cursor:pointer;display:flex;align-items:center;justify-content:center}}
        .modal-close:hover{{background:var(--bg-400);color:var(--text-100)}}
        .modal-body{{padding:24px}}
        .modal-footer{{padding:16px 24px;border-top:1px solid var(--border);display:flex;justify-content:flex-end;gap:12px}}
        
        .form-group{{margin-bottom:20px}}
        .form-label{{display:block;font-size:13px;font-weight:500;color:var(--text-200);margin-bottom:8px}}
        .form-input{{width:100%;padding:12px 14px;background:var(--bg-100);border:1px solid var(--border);border-radius:8px;color:var(--text-100);font-size:14px}}
        .form-input:focus{{outline:none;border-color:var(--text-300)}}
        .form-input::placeholder{{color:var(--text-300)}}
        
        .upload-area{{border:2px dashed var(--border);border-radius:12px;padding:48px;text-align:center;cursor:pointer;transition:all 0.2s}}
        .upload-area:hover,.upload-area.dragover{{border-color:var(--text-300);background:var(--bg-300)}}
        .upload-area-icon{{width:56px;height:56px;margin:0 auto 16px;background:var(--gradient);border-radius:14px;display:flex;align-items:center;justify-content:center}}
        .upload-area-icon svg{{width:28px;height:28px;color:white}}
        .upload-area h3{{font-size:16px;margin-bottom:8px}}
        .upload-area p{{font-size:14px;color:var(--text-200)}}
        
        .progress-container{{display:none;margin-top:24px}}
        .progress-container.active{{display:block}}
        .progress-bar{{height:4px;background:var(--bg-400);border-radius:2px;overflow:hidden}}
        .progress-fill{{height:100%;background:var(--gradient);width:0%;transition:width 0.3s}}
        .progress-text{{margin-top:12px;font-size:13px;color:var(--text-200);text-align:center}}
        
        .toast{{position:fixed;bottom:24px;right:24px;background:var(--bg-300);border:1px solid var(--border);padding:14px 20px;border-radius:10px;font-size:14px;display:none;align-items:center;gap:10px;animation:slideUp 0.3s ease;z-index:1100}}
        .toast.active{{display:flex}}
        .toast.success{{border-color:var(--success)}}
        .toast.error{{border-color:var(--error)}}
        @keyframes slideUp{{from{{transform:translateY(20px);opacity:0}}to{{transform:translateY(0);opacity:1}}}}
        
        @media(max-width:768px){{.import-section{{grid-template-columns:1fr}}.page-header{{flex-direction:column;gap:16px}}.nav-tabs{{display:none}}}}
    </style>
</head>
<body>
    <nav class="navbar">
        <div class="nav-left">
            <a href="/" class="logo"><div class="logo-icon">◈</div>ShadowMesh</a>
            <div class="nav-tabs">
                <a href="/dashboard" class="nav-tab active">Overview</a>
                <a href="/dashboard" class="nav-tab">Deployments</a>
                <a href="/metrics" class="nav-tab">Analytics</a>
            </div>
        </div>
        <div class="nav-right">
            <a href="/metrics" class="btn btn-secondary">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 20V10"/><path d="M12 20V4"/><path d="M6 20v-6"/></svg>
                Metrics
            </a>
        </div>
    </nav>
    
    <main class="main">
        <div class="page-header">
            <div>
                <h1 class="page-title">Welcome back</h1>
                <p class="page-subtitle">Deploy your projects to the decentralized web</p>
            </div>
            <button class="btn btn-primary" onclick="showUploadModal()">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
                Add New...
            </button>
        </div>
        
        <section class="import-section">
            <div class="import-card" onclick="showGithubModal()">
                <div class="import-card-icon github">
                    <svg viewBox="0 0 16 16" fill="white"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
                </div>
                <h3>Import from GitHub</h3>
                <p>Deploy directly from a GitHub repository with automatic builds.</p>
            </div>
            <div class="import-card" onclick="showUploadModal()">
                <div class="import-card-icon upload">
                    <svg viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                </div>
                <h3>Upload ZIP</h3>
                <p>Deploy a project by uploading a ZIP file containing your static site.</p>
            </div>
        </section>
        
        <section>
            <div class="section-header"><h2 class="section-title">Recent Deployments</h2></div>
            {deployments_html}
        </section>
    </main>
    
    <div class="modal-overlay" id="uploadModal">
        <div class="modal">
            <div class="modal-header">
                <h2 class="modal-title">Upload Project</h2>
                <button class="modal-close" onclick="closeModal('uploadModal')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg></button>
            </div>
            <div class="modal-body">
                <div class="upload-area" id="uploadArea">
                    <div class="upload-area-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg></div>
                    <h3>Drop your ZIP here</h3>
                    <p>or click to browse</p>
                    <input type="file" id="fileInput" accept=".zip" hidden />
                </div>
                <div class="progress-container" id="progressContainer">
                    <div class="progress-bar"><div class="progress-fill" id="progressFill"></div></div>
                    <div class="progress-text" id="progressText">Deploying...</div>
                </div>
            </div>
        </div>
    </div>
    
    <div class="modal-overlay" id="githubModal">
        <div class="modal">
            <div class="modal-header">
                <h2 class="modal-title">Import from GitHub</h2>
                <button class="modal-close" onclick="closeModal('githubModal')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg></button>
            </div>
            <div class="modal-body">
                <div class="form-group">
                    <label class="form-label">GitHub Repository URL</label>
                    <input type="text" class="form-input" id="githubUrl" placeholder="https://github.com/username/repo">
                </div>
                <div class="form-group">
                    <label class="form-label">Branch</label>
                    <input type="text" class="form-input" id="githubBranch" placeholder="main" value="main">
                </div>
                <div class="progress-container" id="githubProgress">
                    <div class="progress-bar"><div class="progress-fill" id="githubProgressFill"></div></div>
                    <div class="progress-text" id="githubProgressText">Cloning repository...</div>
                </div>
            </div>
            <div class="modal-footer">
                <button class="btn btn-secondary" onclick="closeModal('githubModal')">Cancel</button>
                <button class="btn btn-primary" onclick="deployFromGithub()">Deploy</button>
            </div>
        </div>
    </div>
    
    <div class="toast" id="toast"></div>
    
    <script>
        function showUploadModal(){{document.getElementById('uploadModal').classList.add('active')}}
        function showGithubModal(){{document.getElementById('githubModal').classList.add('active')}}
        function closeModal(id){{document.getElementById(id).classList.remove('active')}}
        document.querySelectorAll('.modal-overlay').forEach(o=>o.addEventListener('click',e=>{{if(e.target===o)o.classList.remove('active')}}));
        
        const uploadArea=document.getElementById('uploadArea'),fileInput=document.getElementById('fileInput');
        uploadArea.addEventListener('dragover',e=>{{e.preventDefault();uploadArea.classList.add('dragover')}});
        uploadArea.addEventListener('dragleave',()=>uploadArea.classList.remove('dragover'));
        uploadArea.addEventListener('drop',e=>{{e.preventDefault();uploadArea.classList.remove('dragover');if(e.dataTransfer.files.length>0)uploadFile(e.dataTransfer.files[0])}});
        uploadArea.addEventListener('click',()=>fileInput.click());
        fileInput.addEventListener('change',e=>{{if(e.target.files.length>0)uploadFile(e.target.files[0])}});
        
        async function uploadFile(file){{
            if(!file.name.endsWith('.zip')){{showToast('Please upload a ZIP file','error');return}}
            const pc=document.getElementById('progressContainer'),pf=document.getElementById('progressFill'),pt=document.getElementById('progressText');
            pc.classList.add('active');pf.style.width='0%';pt.textContent='Uploading...';
            const fd=new FormData();fd.append('file',file);
            let p=0;const iv=setInterval(()=>{{p=Math.min(p+10,90);pf.style.width=p+'%'}},200);
            try{{
                const r=await fetch('/api/deploy',{{method:'POST',body:fd}});
                clearInterval(iv);pf.style.width='100%';
                const res=await r.json();
                if(res.success){{pt.textContent='✓ Deployed!';showToast('Deployment successful!','success');setTimeout(()=>location.reload(),1500)}}
                else{{pt.textContent='Failed: '+res.error;showToast('Failed','error')}}
            }}catch(e){{clearInterval(iv);pt.textContent='Error';showToast('Failed','error')}}
        }}
        
        async function deployFromGithub(){{
            const url=document.getElementById('githubUrl').value.trim(),branch=document.getElementById('githubBranch').value.trim()||'main';
            if(!url){{showToast('Enter a GitHub URL','error');return}}
            const pc=document.getElementById('githubProgress'),pf=document.getElementById('githubProgressFill'),pt=document.getElementById('githubProgressText');
            pc.classList.add('active');pf.style.width='0%';pt.textContent='Cloning...';
            let p=0;const iv=setInterval(()=>{{p=Math.min(p+5,90);pf.style.width=p+'%';if(p<30)pt.textContent='Cloning...';else if(p<60)pt.textContent='Building...';else pt.textContent='Deploying...'}},300);
            try{{
                const r=await fetch('/api/deploy/github',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{url,branch}})}});
                clearInterval(iv);pf.style.width='100%';
                const res=await r.json();
                if(res.success){{pt.textContent='✓ Deployed!';showToast('GitHub deployment successful!','success');setTimeout(()=>location.reload(),1500)}}
                else{{pt.textContent='Failed: '+res.error;showToast('Failed: '+res.error,'error')}}
            }}catch(e){{clearInterval(iv);pt.textContent='Error';showToast('Failed','error')}}
        }}
        
        function copyUrl(cid){{navigator.clipboard.writeText(location.origin+'/ipfs/'+cid+'/index.html').then(()=>showToast('URL copied!','success'))}}
        function showToast(msg,type){{const t=document.getElementById('toast');t.textContent=msg;t.className='toast active '+type;setTimeout(()=>t.classList.remove('active'),4000)}}
    </script>
</body>
</html>"##,
        deployments_html = if deployment_rows.is_empty() {
            r#"<div class="empty-state">
                <div class="empty-state-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/></svg></div>
                <h3>No deployments yet</h3>
                <p>Import a project from GitHub or upload a ZIP to get started</p>
                <button class="btn btn-primary" onclick="showGithubModal()" style="margin-top:8px">Import from GitHub</button>
            </div>"#.to_string()
        } else {
            format!(r#"<div class="deployments-grid">{}</div>"#, deployment_rows)
        }
    );

    Html(html)
}

/// GitHub deploy request
#[derive(Debug, Deserialize)]
pub struct GithubDeployRequest {
    pub url: String,
    pub branch: Option<String>,
}

/// Deploy from GitHub repository
pub async fn deploy_from_github(
    State(state): State<AppState>,
    Json(request): Json<GithubDeployRequest>,
) -> impl IntoResponse {
    let url = request.url.trim();
    let branch = request.branch.as_deref().unwrap_or("main");
    
    // Parse GitHub URL
    let repo_info = match parse_github_url(url) {
        Some(info) => info,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"success":false,"error":"Invalid GitHub URL"}))).into_response(),
    };
    
    // Download repository as ZIP
    let zip_url = format!("https://github.com/{}/{}/archive/refs/heads/{}.zip", repo_info.owner, repo_info.repo, branch);
    
    let client = reqwest::Client::new();
    let response = match client.get(&zip_url).send().await {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"success":false,"error":format!("Failed to download: {}",e)}))).into_response(),
    };
    
    if !response.status().is_success() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"success":false,"error":format!("Repository or branch '{}' not found",branch)}))).into_response();
    }
    
    let zip_bytes = match response.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"success":false,"error":format!("Download failed: {}",e)}))).into_response(),
    };
    
    let Some(storage) = &state.storage else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"success":false,"error":"Storage unavailable"}))).into_response();
    };
    
    let temp_dir = match extract_github_zip(&zip_bytes) {
        Ok(dir) => dir,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"success":false,"error":e}))).into_response(),
    };
    
    let storage = std::sync::Arc::clone(storage);
    let temp_path = temp_dir.path().to_path_buf();
    
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async { storage.store_directory(&temp_path).await })
    }).await;
    
    match result {
        Ok(Ok(upload_result)) => {
            let cid = upload_result.root_cid.clone();
            let port = state.config.server.port;
            
            let deployment = Deployment::new_github(repo_info.repo.clone(), cid.clone(), upload_result.total_size, upload_result.files.len(), branch.to_string());
            state.deployments.write().unwrap().insert(0, deployment);
            
            println!("✅ Deployed {} from GitHub, CID: {}", repo_info.repo, cid);
            
            Json(serde_json::json!({"success":true,"cid":cid,"url":format!("http://localhost:{}/ipfs/{}/index.html",port,cid),"repo":format!("{}/{}",repo_info.owner,repo_info.repo),"branch":branch})).into_response()
        }
        Ok(Err(e)) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"error":format!("Deploy failed: {}",e)}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"success":false,"error":format!("Internal error: {}",e)}))).into_response(),
    }
}

struct GithubRepoInfo { owner: String, repo: String }

fn parse_github_url(url: &str) -> Option<GithubRepoInfo> {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let parts: Vec<&str> = url.split("github.com").last()?.trim_start_matches('/').trim_start_matches(':').split('/').collect();
    if parts.len() >= 2 { Some(GithubRepoInfo { owner: parts[0].to_string(), repo: parts[1].to_string() }) } else { None }
}

fn extract_github_zip(data: &[u8]) -> Result<tempfile::TempDir, String> {
    use std::io::{Cursor, Read, Write};
    use zip::ZipArchive;
    
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).map_err(|e| format!("Invalid ZIP: {}", e))?;
    let temp_dir = tempfile::TempDir::new().map_err(|e| format!("Temp dir failed: {}", e))?;
    
    let mut root_prefix = String::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("ZIP error: {}", e))?;
        let name = file.name().to_string();
        
        if i == 0 && name.ends_with('/') { root_prefix = name.clone(); continue; }
        
        let relative_path = if !root_prefix.is_empty() && name.starts_with(&root_prefix) { &name[root_prefix.len()..] } else { &name };
        if relative_path.is_empty() { continue; }
        
        let out_path = temp_dir.path().join(relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| format!("Dir failed: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() { std::fs::create_dir_all(parent).ok(); }
            let mut outfile = std::fs::File::create(&out_path).map_err(|e| format!("File failed: {}", e))?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents).map_err(|e| format!("Read failed: {}", e))?;
            outfile.write_all(&contents).map_err(|e| format!("Write failed: {}", e))?;
        }
    }
    Ok(temp_dir)
}

#[derive(Debug, Clone, Serialize)]
pub struct Deployment {
    pub name: String,
    pub cid: String,
    pub size: u64,
    pub file_count: usize,
    pub created_at: String,
    pub source: String,
    pub branch: Option<String>,
    pub status: String,
}

impl Deployment {
    pub fn new(name: String, cid: String, size: u64, file_count: usize) -> Self {
        Self { name, cid, size, file_count, created_at: chrono::Utc::now().format("%b %d, %H:%M").to_string(), source: "upload".to_string(), branch: None, status: "Ready".to_string() }
    }
    
    pub fn new_github(name: String, cid: String, size: u64, file_count: usize, branch: String) -> Self {
        Self { name, cid, size, file_count, created_at: chrono::Utc::now().format("%b %d, %H:%M").to_string(), source: "github".to_string(), branch: Some(branch), status: "Ready".to_string() }
    }
}

/// Get all deployments as JSON
pub async fn get_deployments(State(state): State<AppState>) -> impl IntoResponse {
    let deployments = state.deployments.read().unwrap();
    Json(deployments.clone())
}
