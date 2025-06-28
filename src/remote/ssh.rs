use crate::error::{RemoteError, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct VncConfig {
    pub display: String,
    pub port: u16,
    pub password: Option<String>,
    pub auth_file: Option<PathBuf>,
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

#[derive(Debug, Clone)]
pub struct VncClient {
    pub address: String,
    pub connected_at: std::time::SystemTime,
}

pub struct VncServer {
    config: Arc<RwLock<VncConfig>>,
    process: Arc<RwLock<Option<Child>>>,
    clients: Arc<RwLock<HashMap<String, VncClient>>>,
    restart_count: Arc<RwLock<u32>>,
}

impl VncServer {
    pub fn new(config: VncConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            process: Arc::new(RwLock::new(None)),
            clients: Arc::new(RwLock::new(HashMap::new())),
            restart_count: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        if self.is_running().await {
            return Err(RemoteError::AlreadyRunning("VNC server".to_string()));
        }

        info!("Starting VNC server");

        let config = self.config.read().await;
        let mut cmd = Command::new("x11vnc");

        cmd.arg("-display").arg(&config.display);
        cmd.arg("-rfbport").arg(config.port.to_string());

        if let Some(password) = &config.password {
            cmd.arg("-passwd").arg(password);
        } else if let Some(auth_file) = &config.auth_file {
            cmd.arg("-rfbauth").arg(auth_file);
        } else {
            cmd.arg("-nopw");
        }

        if let Some(geometry) = &config.geometry {
            cmd.arg("-geometry").arg(geometry);
        }

        if let Some(depth) = config.depth {
            cmd.arg("-depth").arg(depth.to_string());
        }

        if config.allow_shared {
            cmd.arg("-shared");
        }

        if config.view_only {
            cmd.arg("-viewonly");
        }

        cmd.arg("-forever");
        cmd.arg("-bg");
        cmd.arg("-noxdamage");

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!("Executing VNC command: {:?}", cmd);

        let child = cmd
            .spawn()
            .map_err(|e| RemoteError::StartFailed(format!("Failed to start x11vnc: {}", e)))?;

        *self.process.write().await = Some(child);

        sleep(Duration::from_millis(500)).await;

        if !self.is_running().await {
            return Err(RemoteError::StartFailed(
                "VNC server exited immediately".to_string(),
            ));
        }

        info!("VNC server started on port {}", config.port);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        if let Some(mut child) = self.process.write().await.take() {
            info!("Stopping VNC server");

            child
                .kill()
                .map_err(|e| RemoteError::StopFailed(format!("Failed to kill process: {}", e)))?;

            self.clients.write().await.clear();
            info!("VNC server stopped");
        }
        Ok(())
    }

    pub async fn restart(&self) -> Result<()> {
        info!("Restarting VNC server");
        self.stop().await?;
        sleep(Duration::from_millis(100)).await;
        self.start().await?;

        let mut count = self.restart_count.write().await;
        *count += 1;

        Ok(())
    }

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

    pub async fn get_clients(&self) -> HashMap<String, VncClient> {
        self.clients.read().await.clone()
    }

    pub async fn add_client(&self, address: String) {
        let client = VncClient {
            address: address.clone(),
            connected_at: std::time::SystemTime::now(),
        };
        self.clients.write().await.insert(address, client);
    }

    pub async fn remove_client(&self, address: &str) {
        self.clients.write().await.remove(address);
    }

    pub async fn get_status(&self) -> HashMap<String, String> {
        let mut status = HashMap::new();

        status.insert("running".to_string(), self.is_running().await.to_string());
        status.insert(
            "clients".to_string(),
            self.clients.read().await.len().to_string(),
        );
        status.insert(
            "restart_count".to_string(),
            self.restart_count.read().await.to_string(),
        );

        let config = self.config.read().await;
        status.insert("port".to_string(), config.port.to_string());
        status.insert("display".to_string(), config.display.clone());

        status
    }

    pub async fn update_config(&self, new_config: VncConfig) -> Result<()> {
        let was_running = self.is_running().await;

        if was_running {
            self.stop().await?;
        }

        *self.config.write().await = new_config;

        if was_running {
            self.start().await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vnc_server_creation() {
        let config = VncConfig::default();
        let server = VncServer::new(config);
        assert!(!server.is_running().await);
        assert!(server.get_clients().await.is_empty());
    }

    #[tokio::test]
    async fn test_client_management() {
        let server = VncServer::new(VncConfig::default());

        server.add_client("192.168.1.100".to_string()).await;
        assert_eq!(server.get_clients().await.len(), 1);

        server.add_client("192.168.1.101".to_string()).await;
        assert_eq!(server.get_clients().await.len(), 2);

        server.remove_client("192.168.1.100").await;
        assert_eq!(server.get_clients().await.len(), 1);
    }

    #[tokio::test]
    async fn test_status() {
        let server = VncServer::new(VncConfig::default());
        let status = server.get_status().await;

        assert_eq!(status.get("running").unwrap(), "false");
        assert_eq!(status.get("clients").unwrap(), "0");
        assert_eq!(status.get("restart_count").unwrap(), "0");
        assert_eq!(status.get("port").unwrap(), "5900");
        assert_eq!(status.get("display").unwrap(), ":0");
    }
}
