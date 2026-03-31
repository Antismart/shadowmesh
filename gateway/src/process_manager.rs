use dashmap::DashMap;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::process::Command;

const HEALTH_HEALTHY: u8 = 0;
const HEALTH_STARTING: u8 = 1;
const HEALTH_UNHEALTHY: u8 = 2;
const HEALTH_STOPPED: u8 = 3;

pub struct ProcessManagerConfig {
    pub port_range_start: u16,
    pub port_range_end: u16,
    pub max_processes: usize,
    pub memory_limit_mb: u64,
    pub health_check_interval: Duration,
    pub startup_timeout: Duration,
    pub max_restart_count: u32,
    pub node_binary: String,
}

impl From<crate::config::DynamicDeployConfig> for ProcessManagerConfig {
    fn from(cfg: crate::config::DynamicDeployConfig) -> Self {
        Self {
            port_range_start: cfg.port_range_start,
            port_range_end: cfg.port_range_end,
            max_processes: cfg.max_processes,
            memory_limit_mb: cfg.memory_limit_mb,
            health_check_interval: Duration::from_secs(cfg.health_check_interval_seconds),
            startup_timeout: Duration::from_secs(cfg.startup_timeout_seconds),
            max_restart_count: cfg.max_restart_count,
            node_binary: cfg.node_binary,
        }
    }
}

#[derive(Serialize)]
pub struct ProcessStatus {
    pub cid: String,
    pub port: u16,
    pub pid: u32,
    pub health: String,
    pub uptime_seconds: u64,
    pub restart_count: u32,
    pub memory_mb: Option<u64>,
}

struct ManagedProcess {
    cid: String,
    port: u16,
    child: tokio::process::Child,
    started_at: Instant,
    restart_count: u32,
    health: AtomicU8,
    working_dir: PathBuf,
    start_command: Vec<String>,
    env: HashMap<String, String>,
    consecutive_failures: u32,
}

impl ManagedProcess {
    fn health_str(&self) -> &'static str {
        match self.health.load(Ordering::Relaxed) {
            HEALTH_HEALTHY => "healthy",
            HEALTH_STARTING => "starting",
            HEALTH_UNHEALTHY => "unhealthy",
            HEALTH_STOPPED => "stopped",
            _ => "unknown",
        }
    }

    fn pid(&self) -> u32 {
        self.child.id().unwrap_or(0)
    }
}

struct PortAllocator {
    base: u16,
    count: u16,
    allocated: Vec<bool>,
}

impl PortAllocator {
    fn new(start: u16, end: u16) -> Self {
        let count = end.saturating_sub(start).saturating_add(1);
        Self {
            base: start,
            count,
            allocated: vec![false; count as usize],
        }
    }

    fn allocate(&mut self) -> Option<u16> {
        for (i, slot) in self.allocated.iter_mut().enumerate() {
            if !*slot {
                *slot = true;
                return Some(self.base + i as u16);
            }
        }
        None
    }

    fn release(&mut self, port: u16) {
        if port >= self.base {
            let idx = (port - self.base) as usize;
            if idx < self.allocated.len() {
                self.allocated[idx] = false;
            }
        }
    }
}

const ALLOWED_PROCESS_BINARIES: &[&str] = &[
    "node", "npm", "npx", "yarn", "pnpm", "bun", "next", "nuxt", "remix-serve",
];

pub struct ProcessManager {
    processes: Arc<DashMap<String, Mutex<ManagedProcess>>>,
    port_allocator: Arc<Mutex<PortAllocator>>,
    config: ProcessManagerConfig,
}

impl ProcessManager {
    pub fn new(config: ProcessManagerConfig) -> Self {
        let allocator = PortAllocator::new(config.port_range_start, config.port_range_end);
        Self {
            processes: Arc::new(DashMap::new()),
            port_allocator: Arc::new(Mutex::new(allocator)),
            config,
        }
    }

    pub async fn spawn_process(
        &self,
        cid: &str,
        working_dir: &Path,
        start_cmd: &[String],
        env: HashMap<String, String>,
    ) -> Result<u16, String> {
        if self.processes.len() >= self.config.max_processes {
            return Err("Maximum number of dynamic processes reached".into());
        }

        let port = {
            let mut alloc = self.port_allocator.lock().map_err(|e| e.to_string())?;
            alloc.allocate().ok_or("No available ports")?
        };

        let (program, args) = if start_cmd.is_empty() {
            return Err("Empty start command".into());
        } else {
            (&start_cmd[0], &start_cmd[1..])
        };

        // SEC-1: Validate binary against allowlist to prevent command injection
        let binary_name = std::path::Path::new(program.as_str())
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(program);
        if !ALLOWED_PROCESS_BINARIES.contains(&binary_name) {
            return Err(format!(
                "Binary '{}' not allowed. Allowed: {}",
                program,
                ALLOWED_PROCESS_BINARIES.join(", ")
            ));
        }

        // SEC-7: Validate working_dir has no path traversal
        let canonical = working_dir.canonicalize().map_err(|e| format!("Invalid working dir: {}", e))?;
        if canonical.to_string_lossy().contains("..") {
            return Err("Working directory contains path traversal".into());
        }

        let mut cmd_env = env.clone();
        let port_str = port.to_string();
        cmd_env.insert("PORT".into(), port_str.clone());
        cmd_env.insert("HOST".into(), "127.0.0.1".into());
        cmd_env.insert("NODE_ENV".into(), "production".into());

        // P4: Expand {PORT} placeholders in env overrides (e.g. Nuxt NITRO_PORT)
        for val in cmd_env.values_mut() {
            if val.contains("{PORT}") {
                *val = val.replace("{PORT}", &port_str);
            }
        }

        // Inherit PATH from the gateway process
        if let Ok(path) = std::env::var("PATH") {
            cmd_env.entry("PATH".into()).or_insert(path);
        }
        if let Ok(home) = std::env::var("HOME") {
            cmd_env.entry("HOME".into()).or_insert(home);
        }

        tracing::info!(
            cid = %cid, port = port, cmd = ?start_cmd,
            "spawning dynamic process"
        );

        let child = Command::new(program)
            .args(args)
            .current_dir(working_dir)
            .envs(&cmd_env)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn {}: {}", program, e))?;

        let managed = ManagedProcess {
            cid: cid.to_string(),
            port,
            child,
            started_at: Instant::now(),
            restart_count: 0,
            health: AtomicU8::new(HEALTH_STARTING),
            working_dir: working_dir.to_path_buf(),
            start_command: start_cmd.to_vec(),
            env,
            consecutive_failures: 0,
        };

        self.processes.insert(cid.to_string(), Mutex::new(managed));

        // Wait for the process to become healthy (TCP connect to port)
        let deadline = Instant::now() + self.config.startup_timeout;
        loop {
            if Instant::now() > deadline {
                tracing::warn!(cid = %cid, port = port, "process startup timed out");
                let _ = self.stop_process(cid).await;
                return Err("Process failed to start within timeout".into());
            }

            match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
                Ok(_) => {
                    if let Some(entry) = self.processes.get(cid) {
                        if let Ok(proc) = entry.lock() {
                            proc.health.store(HEALTH_HEALTHY, Ordering::Relaxed);
                        }
                    }
                    tracing::info!(cid = %cid, port = port, "process is healthy");
                    return Ok(port);
                }
                Err(_) => {
                    // Check if the process has exited
                    let exited = if let Some(entry) = self.processes.get(cid) {
                        if let Ok(mut proc) = entry.lock() {
                            matches!(proc.child.try_wait(), Ok(Some(_)))
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if exited {
                        if let Ok(mut alloc) = self.port_allocator.lock() {
                            alloc.release(port);
                        }
                        self.processes.remove(cid);
                        return Err("Process exited during startup".into());
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    pub async fn stop_process(&self, cid: &str) -> Result<(), String> {
        let entry = self.processes.remove(cid);
        let Some((_, proc_mutex)) = entry else {
            return Err("Process not found".into());
        };

        let mut proc = proc_mutex.into_inner().map_err(|e| e.to_string())?;
        proc.health.store(HEALTH_STOPPED, Ordering::Relaxed);
        let port = proc.port;
        let pid = proc.pid();

        tracing::info!(cid = %cid, pid = pid, port = port, "stopping process");

        // Try graceful kill first
        #[cfg(unix)]
        {
            if pid > 0 {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = proc.child.start_kill();
        }

        // Wait up to 10 seconds for graceful shutdown
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match proc.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() > deadline => {
                    tracing::warn!(cid = %cid, pid = pid, "force-killing process");
                    let _ = proc.child.kill().await;
                    break;
                }
                Ok(None) => tokio::time::sleep(Duration::from_millis(250)).await,
                Err(_) => break,
            }
        }

        // Release port
        if let Ok(mut alloc) = self.port_allocator.lock() {
            alloc.release(port);
        }

        Ok(())
    }

    pub fn get_port(&self, cid: &str) -> Option<u16> {
        let entry = self.processes.get(cid)?;
        let proc = entry.lock().ok()?;
        if proc.health.load(Ordering::Relaxed) == HEALTH_HEALTHY {
            Some(proc.port)
        } else {
            None
        }
    }

    pub fn get_status(&self, cid: &str) -> Option<ProcessStatus> {
        let entry = self.processes.get(cid)?;
        let proc = entry.lock().ok()?;
        Some(ProcessStatus {
            cid: proc.cid.clone(),
            port: proc.port,
            pid: proc.pid(),
            health: proc.health_str().to_string(),
            uptime_seconds: proc.started_at.elapsed().as_secs(),
            restart_count: proc.restart_count,
            memory_mb: read_process_memory(proc.pid()),
        })
    }

    pub async fn restart_process(&self, cid: &str) -> Result<u16, String> {
        let (working_dir, start_command, env, restart_count) = {
            let entry = self.processes.get(cid).ok_or("Process not found")?;
            let proc = entry.lock().map_err(|e| e.to_string())?;
            (
                proc.working_dir.clone(),
                proc.start_command.clone(),
                proc.env.clone(),
                proc.restart_count,
            )
        };

        self.stop_process(cid).await?;
        let port = self.spawn_process(cid, &working_dir, &start_command, env).await?;

        // Update restart count
        if let Some(entry) = self.processes.get(cid) {
            if let Ok(mut proc) = entry.lock() {
                proc.restart_count = restart_count + 1;
            }
        }

        Ok(port)
    }

    pub async fn run_health_loop(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let interval = self.config.health_check_interval;
        let max_restarts = self.config.max_restart_count;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = shutdown.changed() => {
                    tracing::info!("process health loop shutting down");
                    return;
                }
            }

            let cids: Vec<String> = self.processes.iter().map(|e| e.key().clone()).collect();

            for cid in cids {
                // P1: Use try_lock to prevent deadlock in health loop
                let (port, was_healthy) = {
                    let Some(entry) = self.processes.get(&cid) else { continue };
                    let Ok(proc) = entry.try_lock() else { continue };
                    let h = proc.health.load(Ordering::Relaxed);
                    if h == HEALTH_STOPPED || h == HEALTH_STARTING {
                        continue;
                    }
                    (proc.port, h == HEALTH_HEALTHY)
                };

                let healthy = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
                    .await
                    .is_ok();

                // P3: Check memory limits
                if healthy && self.config.memory_limit_mb > 0 {
                    let pid = self.processes.get(&cid)
                        .and_then(|e| e.try_lock().ok().map(|p| p.pid()));
                    if let Some(pid) = pid {
                        if let Some(mem_mb) = read_process_memory(pid) {
                            if mem_mb > self.config.memory_limit_mb {
                                tracing::warn!(
                                    cid = %cid, pid = pid, mem_mb = mem_mb,
                                    limit_mb = self.config.memory_limit_mb,
                                    "process exceeded memory limit, killing"
                                );
                                let _ = self.stop_process(&cid).await;
                                continue;
                            }
                        }
                    }
                }

                if healthy {
                    if let Some(entry) = self.processes.get(&cid) {
                        if let Ok(mut proc) = entry.try_lock() {
                            proc.health.store(HEALTH_HEALTHY, Ordering::Relaxed);
                            proc.consecutive_failures = 0;
                        }
                    }
                } else {
                    let should_restart = {
                        let Some(entry) = self.processes.get(&cid) else { continue };
                        let Ok(mut proc) = entry.try_lock() else { continue };
                        proc.consecutive_failures += 1;

                        if proc.consecutive_failures >= 3 {
                            proc.health.store(HEALTH_UNHEALTHY, Ordering::Relaxed);
                        }

                        if was_healthy {
                            tracing::warn!(
                                cid = %cid, port = port,
                                failures = proc.consecutive_failures,
                                "process health check failed"
                            );
                        }

                        proc.consecutive_failures >= 5
                            && proc.restart_count < max_restarts
                    };

                    if should_restart {
                        tracing::warn!(cid = %cid, "auto-restarting unhealthy process");
                        if let Err(e) = self.restart_process(&cid).await {
                            tracing::error!(cid = %cid, error = %e, "auto-restart failed");
                        }
                    }
                }
            }
        }
    }

    pub async fn shutdown_all(&self) {
        let cids: Vec<String> = self.processes.iter().map(|e| e.key().clone()).collect();
        for cid in cids {
            if let Err(e) = self.stop_process(&cid).await {
                tracing::warn!(cid = %cid, error = %e, "failed to stop process during shutdown");
            }
        }
    }
}

fn read_process_memory(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string(format!("/proc/{}/statm", pid)).ok()?;
        let rss_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        let page_size = 4096u64;
        Some(rss_pages * page_size / (1024 * 1024))
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let rss_kb: u64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()?;
        Some(rss_kb / 1024)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_allocator_basic() {
        let mut alloc = PortAllocator::new(9000, 9002);
        assert_eq!(alloc.allocate(), Some(9000));
        assert_eq!(alloc.allocate(), Some(9001));
        assert_eq!(alloc.allocate(), Some(9002));
        assert_eq!(alloc.allocate(), None);
    }

    #[test]
    fn port_allocator_release_reuse() {
        let mut alloc = PortAllocator::new(9000, 9001);
        assert_eq!(alloc.allocate(), Some(9000));
        assert_eq!(alloc.allocate(), Some(9001));
        assert_eq!(alloc.allocate(), None);
        alloc.release(9000);
        assert_eq!(alloc.allocate(), Some(9000));
    }

    #[test]
    fn port_allocator_release_out_of_range() {
        let mut alloc = PortAllocator::new(9000, 9002);
        alloc.release(8000); // Should not panic
        alloc.release(10000); // Should not panic
    }

    #[test]
    fn process_manager_config_from_dynamic_config() {
        let cfg = crate::config::DynamicDeployConfig::default();
        let pmc: ProcessManagerConfig = cfg.into();
        assert_eq!(pmc.port_range_start, 9000);
        assert_eq!(pmc.port_range_end, 9999);
        assert_eq!(pmc.max_processes, 50);
    }
}
