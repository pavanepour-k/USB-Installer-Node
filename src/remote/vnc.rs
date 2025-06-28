use crate::error::{RemoteError, Result};
use std::{
    collections::HashMap,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    sync::RwLock,
    task,
    time::{sleep, Duration},
};
use tracing::{debug, error, info, warn};

/// Maximum number of times to attempt restarting the VNC server on crash.
const MAX_RESTARTS: u32 = 5;

/// Configuration for the x11vnc server.
#[derive(Debug, Clone)]
pub struct VncConfig {
    pub display: String,
    pub port: u16,
    pub password: Option<String>,
    pub auth_file: Option<std::path::PathBuf>,
    pub geometry: Option<String>,
    pub depth: Option<u8>,
    pub allow_shared: bool,
    pub view_only: bool,
}

impl Default for VncConfig {
    fn default() -> Self {
        Self {
            display: ":0".to_string(),
            port: 5900,
            password: None,
            auth_file: None,
            geometry: None,
            depth: None,
            allow_shared: true,
            view_only: false,
        }
    }
}

/// Manages an x11vnc child process, restarts on crash, tracks clients.
#[derive(Clone)]
pub struct VncServer {
    config: Arc<RwLock<VncConfig>>,
    process: Arc<RwLock<Option<Child>>>,
    client_count: Arc<AtomicUsize>,
    restart_count: Arc<AtomicUsize>,
    monitor_handle: Arc<RwLock<Option<task::JoinHandle<()>>>>,
}

impl VncServer {
    /// Create a new VNC server manager.
    pub fn new(config: VncConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            process: Arc::new(RwLock::new(None)),
            client_count: Arc::new(AtomicUsize::new(0)),
            restart_count: Arc::new(AtomicUsize::new(0)),
            monitor_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the x11vnc process and monitoring task.
    pub async fn start(&self) -> Result<()> {
        if self.is_running().await {
            return Err(RemoteError::AlreadyRunning("VNC server".into()));
        }
        info!("Starting VNC server");
        self.spawn_process().await?;
        // allow time for process to initialize
        sleep(Duration::from_millis(500)).await;
        if !self.is_running().await {
            return Err(RemoteError::StartFailed(
                "VNC server exited immediately".into(),
            ));
        }
        // spawn monitor task
        let me = self.clone();
        let handle = task::spawn(async move {
            me.monitor().await;
        });
        *self.monitor_handle.write().await = Some(handle);
        info!(
            "VNC server started on port {}",
            self.config.read().await.port
        );
        Ok(())
    }

    /// Internal helper to spawn the x11vnc child process.
    async fn spawn_process(&self) -> Result<()> {
        let cfg = self.config.read().await;
        let mut cmd = Command::new("x11vnc");
        cmd.arg("-display").arg(&cfg.display);
        cmd.arg("-rfbport").arg(cfg.port.to_string());
        if let Some(pass) = &cfg.password {
            cmd.arg("-passwd").arg(pass);
        } else if let Some(auth) = &cfg.auth_file {
            cmd.arg("-rfbauth").arg(auth);
        } else {
            cmd.arg("-nopw");
        }
        if let Some(geom) = &cfg.geometry {
            cmd.arg("-geometry").arg(geom);
        }
        if let Some(d) = cfg.depth {
            cmd.arg("-depth").arg(d.to_string());
        }
        if cfg.allow_shared {
            cmd.arg("-shared");
        }
        if cfg.view_only {
            cmd.arg("-viewonly");
        }
        cmd.arg("-forever").arg("-bg").arg("-noxdamage");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        debug!("Executing x11vnc command: {:?}", cmd);
        let child = cmd
            .spawn()
            .map_err(|e| RemoteError::StartFailed(format!("Failed to start x11vnc: {}", e)))?;
        *self.process.write().await = Some(child);
        Ok(())
    }

    /// Monitor the child process, restart on crash up to MAX_RESTARTS.
    async fn monitor(&self) {
        loop {
            // take ownership of the running child
            let child_opt = { self.process.write().await.take() };
            let mut child = match child_opt {
                Some(c) => c,
                None => break,
            };
            // wait for exit without blocking async executor
            match task::spawn_blocking(move || child.wait()).await {
                Ok(Ok(status)) => warn!("VNC server exited: {:?}", status),
                Ok(Err(e)) => error!("Error waiting for VNC server: {}", e),
                Err(e) => error!("Monitor task failed: {}", e),
            }
            // check bounded restart
            let prev = self.restart_count.fetch_add(1, Ordering::SeqCst);
            if prev < MAX_RESTARTS {
                let now = prev + 1;
                info!("Restarting VNC server ({}/{})", now, MAX_RESTARTS);
                if let Err(e) = self.spawn_process().await {
                    error!("Failed to restart VNC server: {}", e);
                    break;
                }
            } else {
                error!("Exceeded max restarts ({}), giving up", MAX_RESTARTS);
                break;
            }
        }
    }

    /// Stop the VNC server and monitoring task.
    pub async fn stop(&self) -> Result<()> {
        if let Some(mut child) = self.process.write().await.take() {
            info!("Stopping VNC server");
            child
                .kill()
                .map_err(|e| RemoteError::StopFailed(format!("Failed to kill x11vnc: {}", e)))?;
        }
        if let Some(handle) = self.monitor_handle.write().await.take() {
            handle.abort();
        }
        Ok(())
    }

    /// Returns true if the x11vnc process is alive.
    pub async fn is_running(&self) -> bool {
        if let Some(child) = &mut *self.process.write().await {
            match child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Increment the count of connected clients.
    pub fn add_client(&self) {
        self.client_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Decrement the count of connected clients.
    pub fn remove_client(&self) {
        self.client_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get the current number of connected clients.
    pub fn client_count(&self) -> usize {
        self.client_count.load(Ordering::SeqCst)
    }

    /// Retrieve a simple status map for monitoring.
    pub async fn get_status(&self) -> HashMap<String, String> {
        let mut status = HashMap::new();
        status.insert("running".to_string(), self.is_running().await.to_string());
        status.insert("clients".to_string(), self.client_count().to_string());
        status.insert(
            "restart_count".to_string(),
            self.restart_count.load(Ordering::SeqCst).to_string(),
        );
        let cfg = self.config.read().await;
        status.insert("port".to_string(), cfg.port.to_string());
        status.insert("display".to_string(), cfg.display.clone());
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vnc_server_creation() {
        let server = VncServer::new(VncConfig::default());
        assert!(!server.is_running().await);
        assert_eq!(server.client_count(), 0);
        assert_eq!(server.restart_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_client_management() {
        let server = VncServer::new(VncConfig::default());
        server.add_client();
        assert_eq!(server.client_count(), 1);
        server.add_client();
        assert_eq!(server.client_count(), 2);
        server.remove_client();
        assert_eq!(server.client_count(), 1);
    }

    #[tokio::test]
    async fn test_get_status() {
        let server = VncServer::new(VncConfig::default());
        let status = server.get_status().await;
        assert_eq!(status.get("running").unwrap(), "false");
        assert_eq!(status.get("clients").unwrap(), "0");
        assert_eq!(status.get("restart_count").unwrap(), "0");
        assert_eq!(status.get("port").unwrap(), "5900");
        assert_eq!(status.get("display").unwrap(), ":0");
    }
}
