//! Command execution and output formatting.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

use crate::client::NodeClient;
use crate::gateway_client::GatewayClient;

// ── Helpers ─────────────────────────────────────────────────────────

fn print_json(val: &Value) {
    match serde_json::to_string_pretty(val) {
        Ok(s) => println!("{s}"),
        Err(_) => println!("{val}"),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max - 3])
    } else {
        s.to_string()
    }
}

// ── Status ──────────────────────────────────────────────────────────

pub async fn status(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_status().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("ShadowMesh Node Status");
    println!("  Peer ID:    {}", truncate(val["peer_id"].as_str().unwrap_or("-"), 24));
    println!("  Name:       {}", val["name"].as_str().unwrap_or("-"));
    println!("  Version:    {}", val["version"].as_str().unwrap_or("-"));
    println!("  Status:     {}", val["status"].as_str().unwrap_or("-"));
    println!("  Uptime:     {}", val["uptime_formatted"].as_str().unwrap_or("-"));
    println!("  Peers:      {}", val["connected_peers"].as_u64().unwrap_or(0));
    println!(
        "  Storage:    {} / {}",
        format_bytes(val["storage_used"].as_u64().unwrap_or(0)),
        format_bytes(val["storage_capacity"].as_u64().unwrap_or(0)),
    );
    println!("  Bandwidth:  {} served", val["bandwidth_served"].as_str().unwrap_or("0 B"));

    Ok(())
}

// ── Health ──────────────────────────────────────────────────────────

pub async fn health(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_health().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    let healthy = val["healthy"].as_bool().unwrap_or(false);
    let icon = if healthy { "OK" } else { "DEGRADED" };

    println!("Health: {icon}");
    if let Some(checks) = val["checks"].as_object() {
        for (name, ok) in checks {
            let mark = if ok.as_bool().unwrap_or(false) { "+" } else { "-" };
            println!("  [{mark}] {name}");
        }
    }

    Ok(())
}

// ── Peers ───────────────────────────────────────────────────────────

pub async fn peers(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_peers().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    let peers = val.as_array().map(|a| a.as_slice()).unwrap_or(&[]);

    println!(
        "{:<28} {:<36} {:>7}",
        "PEER ID", "ADDRESS", "LATENCY"
    );

    for peer in peers {
        let id = truncate(peer["peer_id"].as_str().unwrap_or("-"), 25);
        let addrs = peer["addresses"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.as_str())
            .unwrap_or("-");
        let lat = match peer["latency_ms"].as_u64() {
            Some(0) => "0ms".to_string(),
            Some(ms) => format!("{ms}ms"),
            None => "-".to_string(),
        };
        println!("{:<28} {:<36} {:>7}", id, truncate(addrs, 33), lat);
    }

    println!("\n{} peer(s)", peers.len());

    Ok(())
}

// ── Metrics ─────────────────────────────────────────────────────────

pub async fn metrics(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_metrics().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Node Metrics");
    println!("  Uptime:           {}", val["uptime_formatted"].as_str().unwrap_or("-"));

    if let Some(req) = val.get("requests") {
        println!("\n  Requests");
        println!("    Total:          {}", req["total"].as_u64().unwrap_or(0));
        println!("    Successful:     {}", req["successful"].as_u64().unwrap_or(0));
        println!("    Failed:         {}", req["failed"].as_u64().unwrap_or(0));
        println!("    Success Rate:   {:.1}%", req["success_rate"].as_f64().unwrap_or(0.0));
        println!("    Cache Hits:     {}", req["cache_hits"].as_u64().unwrap_or(0));
        println!("    Cache Rate:     {:.1}%", req["cache_hit_rate"].as_f64().unwrap_or(0.0));
    }

    if let Some(bw) = val.get("bandwidth") {
        println!("\n  Bandwidth");
        println!("    Served:         {}", format_bytes(bw["total_served"].as_u64().unwrap_or(0)));
        println!("    Received:       {}", format_bytes(bw["total_received"].as_u64().unwrap_or(0)));
        println!("    Uploaded:       {}", format_bytes(bw["total_uploaded"].as_u64().unwrap_or(0)));
    }

    if let Some(net) = val.get("network") {
        println!("\n  Network");
        println!("    Connected:      {}", net["connected_peers"].as_u64().unwrap_or(0));
        println!("    Discovered:     {}", net["discovered_peers"].as_u64().unwrap_or(0));
        println!("    DHT Records:    {}", net["dht_records"].as_u64().unwrap_or(0));
    }

    Ok(())
}

// ── Upload ──────────────────────────────────────────────────────────

pub async fn upload(client: &NodeClient, file_path: &Path, json: bool) -> Result<()> {
    let val = client.upload(file_path).await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Uploaded successfully!");
    println!("  CID:   {}", val["cid"].as_str().unwrap_or("-"));
    println!("  Name:  {}", val["name"].as_str().unwrap_or("-"));
    println!("  Size:  {}", format_bytes(val["size"].as_u64().unwrap_or(0)));
    println!("  MIME:  {}", val["mime_type"].as_str().unwrap_or("-"));

    Ok(())
}

// ── Storage Stats ───────────────────────────────────────────────────

pub async fn storage(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_storage_stats().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Storage");
    println!("  Used:       {}", format_bytes(val["total_bytes"].as_u64().unwrap_or(0)));
    println!("  Capacity:   {}", format_bytes(val["capacity_bytes"].as_u64().unwrap_or(0)));
    println!("  Fragments:  {}", val["fragment_count"].as_u64().unwrap_or(0));
    println!("  Content:    {}", val["content_count"].as_u64().unwrap_or(0));
    println!("  Pinned:     {}", val["pinned_count"].as_u64().unwrap_or(0));

    if let Some(gc) = val["last_gc"].as_u64() {
        println!("  Last GC:    {gc}");
    }

    Ok(())
}

// ── List Content ────────────────────────────────────────────────────

pub async fn list(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.list_content().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    let items = val.as_array().map(|a| a.as_slice()).unwrap_or(&[]);

    if items.is_empty() {
        println!("No content stored.");
        return Ok(());
    }

    println!(
        "{:<20} {:<20} {:>10} {:>6} {}",
        "CID", "NAME", "SIZE", "PINNED", "MIME"
    );

    for item in items {
        let cid = truncate(item["cid"].as_str().unwrap_or("-"), 17);
        let name = truncate(item["name"].as_str().unwrap_or("-"), 17);
        let size = format_bytes(item["total_size"].as_u64().unwrap_or(0));
        let pinned = if item["pinned"].as_bool().unwrap_or(false) {
            "yes"
        } else {
            "no"
        };
        let mime = item["mime_type"].as_str().unwrap_or("-");
        println!("{:<20} {:<20} {:>10} {:>6} {}", cid, name, size, pinned, mime);
    }

    println!("\n{} item(s)", items.len());

    Ok(())
}

// ── Get Content ─────────────────────────────────────────────────────

pub async fn get(client: &NodeClient, cid: &str, json: bool) -> Result<()> {
    let val = client.get_content(cid).await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Content Details");
    println!("  CID:        {}", val["cid"].as_str().unwrap_or("-"));
    println!("  Name:       {}", val["name"].as_str().unwrap_or("-"));
    println!("  Size:       {}", format_bytes(val["total_size"].as_u64().unwrap_or(0)));
    println!("  Fragments:  {}", val["fragment_count"].as_u64().unwrap_or(0));
    println!("  Pinned:     {}", val["pinned"].as_bool().unwrap_or(false));
    println!("  MIME:       {}", val["mime_type"].as_str().unwrap_or("-"));
    println!("  Stored At:  {}", val["stored_at"].as_u64().unwrap_or(0));

    Ok(())
}

// ── Pin / Unpin ─────────────────────────────────────────────────────

pub async fn pin(client: &NodeClient, cid: &str) -> Result<()> {
    client.pin(cid).await?;
    println!("Pinned: {cid}");
    Ok(())
}

pub async fn unpin(client: &NodeClient, cid: &str) -> Result<()> {
    client.unpin(cid).await?;
    println!("Unpinned: {cid}");
    Ok(())
}

// ── Delete ──────────────────────────────────────────────────────────

pub async fn delete(client: &NodeClient, cid: &str) -> Result<()> {
    client.delete_content(cid).await?;
    println!("Deleted: {cid}");
    Ok(())
}

// ── Fetch ───────────────────────────────────────────────────────────

pub async fn fetch(client: &NodeClient, cid: &str, json: bool) -> Result<()> {
    let val = client.fetch_remote(cid).await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Fetched from P2P network!");
    println!("  CID:   {}", val["cid"].as_str().unwrap_or("-"));
    println!("  Name:  {}", val["name"].as_str().unwrap_or("-"));
    println!("  Size:  {}", format_bytes(val["size"].as_u64().unwrap_or(0)));
    println!("  MIME:  {}", val["mime_type"].as_str().unwrap_or("-"));

    Ok(())
}

// ── Garbage Collection ──────────────────────────────────────────────

pub async fn gc(client: &NodeClient, target_gb: f64, json: bool) -> Result<()> {
    let val = client.run_gc(target_gb).await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Garbage collection complete");
    println!("  Freed:     {}", format_bytes(val["freed_bytes"].as_u64().unwrap_or(0)));
    println!("  Fragments: {} removed", val["removed_fragments"].as_u64().unwrap_or(0));
    println!("  Content:   {} removed", val["removed_content"].as_u64().unwrap_or(0));

    Ok(())
}

// ── Config ──────────────────────────────────────────────────────────

pub async fn config(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_config().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Node Configuration");

    if let Some(id) = val.get("identity") {
        println!("\n  Identity");
        println!("    Name:       {}", id["name"].as_str().unwrap_or("-"));
    }

    if let Some(s) = val.get("storage") {
        println!("\n  Storage");
        println!("    Data Dir:   {}", s["data_dir"].as_str().unwrap_or("-"));
        println!("    Capacity:   {}", format_bytes(s["max_storage_bytes"].as_u64().unwrap_or(0)));
    }

    if let Some(n) = val.get("network") {
        println!("\n  Network");
        println!("    Max Peers:  {}", n["max_peers"].as_u64().unwrap_or(0));
        println!("    DHT:        {}", n["enable_dht"].as_bool().unwrap_or(false));
    }

    if let Some(d) = val.get("dashboard") {
        println!("\n  Dashboard");
        println!("    Host:       {}", d["host"].as_str().unwrap_or("-"));
        println!("    Port:       {}", d["port"].as_u64().unwrap_or(0));
    }

    Ok(())
}

pub async fn config_set(
    client: &NodeClient,
    name: Option<String>,
    max_peers: Option<usize>,
    storage_gb: Option<f64>,
    bandwidth_mbps: Option<u64>,
    json: bool,
) -> Result<()> {
    let mut body = serde_json::Map::new();

    if let Some(n) = name {
        body.insert("node_name".to_string(), Value::String(n));
    }
    if let Some(p) = max_peers {
        body.insert("max_peers".to_string(), Value::Number(p.into()));
    }
    if let Some(s) = storage_gb {
        let num = serde_json::Number::from_f64(s)
            .ok_or_else(|| anyhow::anyhow!("Invalid storage value: {s} (must be a finite number)"))?;
        body.insert("max_storage_gb".to_string(), Value::Number(num));
    }
    if let Some(b) = bandwidth_mbps {
        body.insert("max_bandwidth_mbps".to_string(), Value::Number(b.into()));
    }

    if body.is_empty() {
        println!("No configuration changes specified.");
        return Ok(());
    }

    let val = client.update_config(Value::Object(body)).await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Configuration updated successfully.");

    Ok(())
}

// ── Bandwidth ───────────────────────────────────────────────────────

pub async fn bandwidth(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_bandwidth().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Bandwidth");
    println!("  Inbound:      {} ({}/s)", val["total_inbound"].as_str().unwrap_or("0 B"), format_bytes(val["inbound_bps"].as_u64().unwrap_or(0)));
    println!("  Outbound:     {} ({}/s)", val["total_outbound"].as_str().unwrap_or("0 B"), format_bytes(val["outbound_bps"].as_u64().unwrap_or(0)));

    if let Some(lim) = val["limit_inbound"].as_u64() {
        println!("  Limit In:     {}/s", format_bytes(lim));
    }
    if let Some(lim) = val["limit_outbound"].as_u64() {
        println!("  Limit Out:    {}/s", format_bytes(lim));
    }

    Ok(())
}

// ── Replication ─────────────────────────────────────────────────────

pub async fn replication(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_replication_health().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("Replication Health");

    if let Some(h) = val.get("health") {
        println!("\n  Content");
        println!("    Total:            {}", h["total_content"].as_u64().unwrap_or(0));
        println!("    Healthy:          {}", h["healthy_content"].as_u64().unwrap_or(0));
        println!("    Under-replicated: {}", h["under_replicated_content"].as_u64().unwrap_or(0));
        println!("    Critical:         {}", h["critical_content"].as_u64().unwrap_or(0));
        println!("    Factor:           {}", h["replication_factor"].as_u64().unwrap_or(0));
    }

    if let Some(s) = val.get("stats") {
        println!("\n  Stats");
        println!("    Replicated:       {}", s["total_replicated"].as_u64().unwrap_or(0));
        println!("    Bytes:            {}", format_bytes(s["total_bytes_replicated"].as_u64().unwrap_or(0)));
        println!("    Failures:         {}", s["total_failures"].as_u64().unwrap_or(0));
        println!("    Scanning:         {}", s["scan_in_progress"].as_bool().unwrap_or(false));
    }

    Ok(())
}

// ── Download ────────────────────────────────────────────────────

pub async fn download(
    client: &NodeClient,
    cid: &str,
    output: Option<&Path>,
) -> Result<()> {
    let (bytes, _content_type, _content_name) = client.download(cid).await?;

    let out_path = match output {
        Some(p) => p.to_path_buf(),
        None => std::path::PathBuf::from(format!("{}.bin", truncate(cid, 32))),
    };

    tokio::fs::write(&out_path, &bytes).await?;
    println!(
        "Downloaded {} to {}",
        format_bytes(bytes.len() as u64),
        out_path.display()
    );

    Ok(())
}

// ── Ready ───────────────────────────────────────────────────────

pub async fn ready(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.get_ready().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    let healthy = val["healthy"].as_bool().unwrap_or(false);
    let icon = if healthy { "OK" } else { "NOT READY" };

    println!("Ready: {icon}");
    if let Some(checks) = val["checks"].as_object() {
        for (name, ok) in checks {
            let mark = if ok.as_bool().unwrap_or(false) { "+" } else { "-" };
            println!("  [{mark}] {name}");
        }
    }

    Ok(())
}

// ── Shutdown ────────────────────────────────────────────────────────

pub async fn shutdown(client: &NodeClient, json: bool) -> Result<()> {
    let val = client.shutdown().await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    println!("{}", val["message"].as_str().unwrap_or("Shutdown initiated"));

    Ok(())
}

// ── Deploy (zip + upload) ───────────────────────────────────────────

/// Directories to skip when creating the deploy ZIP.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".shadowmesh",
    "target",
    "dist",
    "build",
    ".next",
];

/// Create an in-memory ZIP of `dir`, excluding common build artifacts.
fn create_deploy_zip(dir: &Path) -> Result<Vec<u8>> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("Directory not found: {}", dir.display()))?;

    let buf = std::io::Cursor::new(Vec::new());
    let mut zip_writer = zip::ZipWriter::new(buf);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut file_count: usize = 0;

    for entry in WalkDir::new(&dir).into_iter().filter_entry(|e| {
        // Skip excluded directories
        if e.file_type().is_dir() {
            if let Some(name) = e.file_name().to_str() {
                return !SKIP_DIRS.contains(&name);
            }
        }
        true
    }) {
        let entry = entry?;
        let abs_path = entry.path();
        let rel_path = abs_path
            .strip_prefix(&dir)
            .unwrap_or(abs_path)
            .to_string_lossy()
            .to_string();

        // Skip the root directory itself
        if rel_path.is_empty() {
            continue;
        }

        if entry.file_type().is_dir() {
            zip_writer.add_directory(&format!("{}/", rel_path), options)?;
        } else {
            zip_writer.start_file(&rel_path, options)?;
            let data = std::fs::read(abs_path)
                .with_context(|| format!("Failed to read file: {}", abs_path.display()))?;
            zip_writer.write_all(&data)?;
            file_count += 1;
        }
    }

    let cursor = zip_writer.finish()?;
    let zip_bytes = cursor.into_inner();

    if file_count == 0 {
        bail!("No files found in directory: {}", dir.display());
    }

    println!(
        "Zipped {} file(s) ({})",
        file_count,
        format_bytes(zip_bytes.len() as u64)
    );

    Ok(zip_bytes)
}

pub async fn deploy(
    gateway: &GatewayClient,
    dir: &Path,
    name: Option<&str>,
    json: bool,
) -> Result<()> {
    let dir_name = name
        .map(|s| s.to_string())
        .or_else(|| {
            dir.canonicalize()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        })
        .unwrap_or_else(|| "deploy".to_string());

    println!("Deploying \"{}\" ...", dir_name);

    let zip_data = create_deploy_zip(dir)?;

    println!("Uploading to gateway...");
    let val = gateway.deploy_zip(zip_data).await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    let success = val["success"].as_bool().unwrap_or(false);
    if !success {
        let err = val["error"].as_str().unwrap_or("Unknown error");
        bail!("Deploy failed: {}", err);
    }

    println!("\nDeployment successful!");
    println!("  CID:        {}", val["cid"].as_str().unwrap_or("-"));
    println!("  URL:        {}", val["url"].as_str().unwrap_or("-"));
    println!("  IPFS:       {}", val["ipfs_url"].as_str().unwrap_or("-"));
    println!("  Shadow:     {}", val["shadow_url"].as_str().unwrap_or("-"));
    println!(
        "  Files:      {}",
        val["file_count"].as_u64().unwrap_or(0)
    );
    println!(
        "  Size:       {}",
        format_bytes(val["total_size"].as_u64().unwrap_or(0))
    );

    Ok(())
}

// ── Deploy from GitHub ──────────────────────────────────────────────

pub async fn deploy_github(
    gateway: &GatewayClient,
    url: &str,
    branch: &str,
    root_dir: Option<&str>,
    json: bool,
) -> Result<()> {
    println!("Deploying from GitHub: {} (branch: {})", url, branch);
    if let Some(rd) = root_dir {
        println!("  Root directory: {}", rd);
    }

    let val = gateway.deploy_github(url, branch, root_dir).await?;

    if json {
        print_json(&val);
        return Ok(());
    }

    let deploy_id = val["deploy_id"].as_str().unwrap_or("");
    let stream_url = val["stream_url"].as_str().unwrap_or("");

    if deploy_id.is_empty() || stream_url.is_empty() {
        // Synchronous response — show result directly
        let success = val["success"].as_bool().unwrap_or(false);
        if !success {
            let err = val["error"].as_str().unwrap_or("Unknown error");
            bail!("Deploy failed: {}", err);
        }
        println!("\nDeployment successful!");
        println!("  CID: {}", val["cid"].as_str().unwrap_or("-"));
        println!("  URL: {}", val["url"].as_str().unwrap_or("-"));
        return Ok(());
    }

    println!("  Deploy ID: {}", deploy_id);
    println!("\nBuild logs:");

    gateway.stream_build_logs(stream_url).await?;

    Ok(())
}

// ── List Deployments ────────────────────────────────────────────────

pub async fn deployments(gateway: &GatewayClient, json: bool) -> Result<()> {
    let items = gateway.list_deployments().await?;

    if json {
        print_json(&Value::Array(items));
        return Ok(());
    }

    if items.is_empty() {
        println!("No deployments found.");
        return Ok(());
    }

    println!(
        "{:<24} {:<20} {:>10} {:>6} {:<12} {}",
        "CID", "NAME", "SIZE", "FILES", "STATUS", "CREATED"
    );

    for item in &items {
        let cid = truncate(item["cid"].as_str().unwrap_or("-"), 21);
        let name = truncate(item["name"].as_str().unwrap_or("-"), 17);
        let size = format_bytes(item["size"].as_u64().unwrap_or(0));
        let files = item["file_count"].as_u64().unwrap_or(0);
        let status = item["status"].as_str().unwrap_or("-");
        let created = item["created_at"].as_str().unwrap_or("-");
        println!(
            "{:<24} {:<20} {:>10} {:>6} {:<12} {}",
            cid, name, size, files, status, created
        );
    }

    println!("\n{} deployment(s)", items.len());

    Ok(())
}

// ── Init (framework detection) ──────────────────────────────────────

/// Framework detection rules: (glob patterns to check, framework name, build command, output dir).
const FRAMEWORKS: &[(&[&str], &str, &str, &str)] = &[
    (
        &["next.config.js", "next.config.ts", "next.config.mjs"],
        "Next.js",
        "npm run build",
        ".next",
    ),
    (
        &["vite.config.ts", "vite.config.js", "vite.config.mjs"],
        "Vite",
        "npm run build",
        "dist",
    ),
    (
        &["nuxt.config.ts", "nuxt.config.js"],
        "Nuxt",
        "npm run build",
        ".output",
    ),
    (
        &["svelte.config.js", "svelte.config.ts"],
        "SvelteKit",
        "npm run build",
        "build",
    ),
    (
        &["astro.config.mjs", "astro.config.ts"],
        "Astro",
        "npm run build",
        "dist",
    ),
    (&["angular.json"], "Angular", "npm run build", "dist"),
    (
        &["gatsby-config.js", "gatsby-config.ts"],
        "Gatsby",
        "npm run build",
        "public",
    ),
    (
        &["package.json"],
        "Node.js",
        "npm run build",
        "dist",
    ),
    (
        &["index.html"],
        "Static HTML",
        "(none)",
        ".",
    ),
];

pub async fn init() -> Result<()> {
    let cwd = std::env::current_dir().context("Cannot determine current directory")?;

    println!("ShadowMesh Project Init");
    println!("  Directory: {}", cwd.display());

    let mut detected: Option<(&str, &str, &str)> = None;

    for (files, name, build_cmd, output_dir) in FRAMEWORKS {
        for file in *files {
            if cwd.join(file).exists() {
                detected = Some((name, build_cmd, output_dir));
                break;
            }
        }
        if detected.is_some() {
            break;
        }
    }

    match detected {
        Some((framework, build_cmd, output_dir)) => {
            println!("\n  Detected framework: {}", framework);
            println!("\n  Deploy configuration:");
            println!("    Build command:  {}", build_cmd);
            println!("    Output dir:     {}", output_dir);
            println!("\n  To deploy, run:");
            if output_dir == "." {
                println!("    shadowmesh-cli deploy");
            } else {
                println!("    shadowmesh-cli deploy {}", output_dir);
            }
            println!("\n  Or deploy via GitHub:");
            println!("    shadowmesh-cli deploy-github <repo-url> --branch main");
        }
        None => {
            println!("\n  No known framework detected.");
            println!("  You can still deploy any directory:");
            println!("    shadowmesh-cli deploy .");
        }
    }

    Ok(())
}
