# Otter Network — Phase 2 Testing Guide: Static Site Deployment

Hey everyone! Thanks for helping test Otter. I'm helping coordinate this round of testing. You all did great work in Phase 1 getting nodes running and connected — now we're moving to the fun part: actually deploying sites to the network.

This guide walks you through everything step by step. If something breaks or behaves unexpectedly, that's exactly what we want to find — please report it.

---

## What You'll Need

- A GitHub account
- A browser (Chrome or Firefox recommended)
- A static site to deploy (your own repo, or use one of our test repos below)
- Optional: Rust toolchain if you want to test the CLI

## Test Repos You Can Use

If you don't have a static site ready, fork one of these:

- **Plain HTML**: Any repo with an `index.html`
- **Vite/React**: `npm create vite@latest my-app -- --template react` then push to GitHub
- **Next.js**: `npx create-next-app@latest` (make sure it's configured for static export)

---

## Test 1: Deploy from GitHub (Dashboard)

This is the main flow most users will use.

1. Open the dashboard: **http://62.171.189.140:8081/**
2. You should see the **Overview** page with stats cards and a getting-started checklist
3. Click **"Connect GitHub"** and authorize the app
4. Click **"Import"** in the sidebar (or the quick action button)
5. Select the **"Import from GitHub"** tab
6. Choose your repository from the dropdown
7. Select the branch (defaults to `main`)
8. If your project is in a subdirectory, use the **folder browser** to select it
9. **Optional**: Expand **"Build Settings"** to:
   - Add environment variables (e.g., `VITE_API_URL=https://example.com`)
   - Override the build command
   - Override the output directory
10. Click **Deploy**
11. Watch the **streaming build logs** — you should see install, build, and upload steps in real time
12. On success, you'll get a **CID** and a link to your deployment

### What to report:
- Did framework detection work? (check the build logs — it should say "Detected: Vite" or similar)
- Did the build logs stream smoothly or freeze?
- Did the deploy succeed or fail? If failed, copy the error.
- Can you access your site at the CID URL?

---

## Test 2: Visit Your Deployed Site

After deploying, click **"Visit"** on the deployment detail page, or go to:

```
http://62.171.189.140:8081/<your-CID>
```

### What to check:
- Does the page load?
- Do styles (CSS) work?
- Do scripts (JS) work?
- Do images load?
- If it's a single-page app (React/Vue), does client-side routing work? (click around, then refresh the page)
- Check the browser console (F12) for any errors

---

## Test 3: Assign a .shadow Domain

1. From your deployment detail page, click **"Assign Domain"**
2. Enter a name (e.g., `my-cool-site`) — it will become `my-cool-site.shadow`
3. Click **Assign**
4. You should see a success toast

Alternatively, go to **Domains** in the sidebar:
- Register a new domain
- Use the **Resolve** tool to look up any `.shadow` name
- Try reassigning a domain to a different deployment
- Try deleting a domain

### What to report:
- Did domain assignment work?
- Can you resolve the domain name?
- Did reassignment work?
- Any errors or confusing behavior?

---

## Test 4: Redeploy

1. Go to your deployment detail page
2. Click **"Redeploy"**
3. Confirm in the dialog
4. Watch the streaming build logs again
5. The CID will change (new content = new hash)

### What to report:
- Did redeploy work?
- Did the old CID stop working? (it shouldn't — old content stays on the network)
- Did the domain auto-update to the new CID?

---

## Test 5: Deploy via ZIP Upload

1. Build your site locally (e.g., `npm run build`)
2. Zip the output directory (e.g., `cd dist && zip -r site.zip .`)
3. Go to **Import** → **Upload ZIP** tab
4. Drag and drop your zip file
5. Wait for upload and processing
6. Visit the CID URL

### What to report:
- Did the upload work?
- Does the site load from the CID?
- Any size limit issues?

---

## Test 6: Deploy via CLI (Optional — for technical testers)

If you have Rust installed:

```bash
# Clone the repo and build the CLI
git clone https://github.com/Antismart/shadowmesh.git
cd shadowmesh
cargo build --release -p shadowmesh-cli

# Set up an alias
alias smesh="./target/release/shadowmesh-cli"

# In your static site project directory:
smesh init
# Should print: "Detected framework: Vite" (or whatever you're using)

# Deploy to the network
smesh deploy . --gateway-url http://62.171.189.140:8081

# List your deployments
smesh deployments --gateway-url http://62.171.189.140:8081
```

### What to report:
- Did `smesh init` detect your framework correctly?
- Did `smesh deploy` work? Did you see build output?
- Did `smesh deployments` show your deploy?

---

## Test 7: Analytics

1. Go to **Usage** in the sidebar
2. You should see live metrics updating every 5 seconds:
   - Request counts
   - Bandwidth chart
   - Cache hit/miss ratio
   - IPFS health status
   - Uptime

3. Go to a specific deployment's detail page — you should see per-deployment request and bandwidth stats.

### What to report:
- Are metrics updating?
- Do the numbers look reasonable?
- Any UI issues?

---

## Test 8: Command Palette

Press **Cmd+K** (Mac) or **Ctrl+K** (Windows/Linux) from any page.

- Search for a deployment by name
- Search for a domain
- Navigate to a page (Settings, Analytics, etc.)
- Use arrow keys and Enter to select

### What to report:
- Does it open?
- Does search work?
- Does keyboard navigation work?

---

## How to Report Issues

For each issue, please include:
1. **What you were doing** (which test, which step)
2. **What you expected** to happen
3. **What actually happened**
4. **Browser console errors** (F12 → Console tab → screenshot or copy)
5. **Your browser** (Chrome/Firefox/Safari + version)
6. **The URL** you were on when it happened

Report issues in our group chat or open a GitHub issue at: https://github.com/Antismart/shadowmesh/issues

---

## Known Limitations

- The gateway is running on a single VPS. If many testers deploy simultaneously, builds may queue up.
- `.shadow` domains resolve within the network only — they won't work in a regular browser address bar (you need to use the dashboard or SDK).
- The CID-served dashboard (`/QmXXX/...`) works but may have minor routing quirks on page refresh.
- Build caching is enabled but you'll only notice speed improvements on your second deploy of the same project.

---

## Quick Reference

| What | URL |
|------|-----|
| Dashboard | http://62.171.189.140:8081/ |
| Health check | http://62.171.189.140:8081/health |
| API docs | http://62.171.189.140:8081/api/deploy/info |
| GitHub repo | https://github.com/Antismart/shadowmesh |

Thanks for testing! Every bug you find makes Otter stronger.
