use crate::config::TunnelConfig;
use crate::error::{Result, UsbNodeError};
use log::{debug, error, info, warn};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

#[derive(Debug, Clone, PartialEq)]
pub enum TunnelState {
    Disconnected,
    Connecting,
    Connected,
    Error,
    Reconnecting,
}

#[derive(Debug)]
pub struct TunnelStatus {
    pub state: TunnelState,
    pub endpoint: Option<String>,
    pub latency: Option<Duration>,
    pub last_connected: Option<Instant>,
    pub error_count: u32,
}

pub struct TunnelManager {
    config: TunnelConfig,
    process: Arc<RwLock<Option<Child>>>,
    status: Arc<RwLock<TunnelStatus>>,
    shutdown: Arc<RwLock<bool>>,
}

impl TunnelManager {
    pub fn new(config: TunnelConfig) -> Self {
        Self {
            config,
            process: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(TunnelStatus {
                state: TunnelState::Disconnected,
                endpoint: None,
                latency: None,
                last_connected: None,
                error_count: 0,
            })),
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            info!("Tunnel disabled in configuration");
            return Ok(());
        }

        self.validate_config()?;
        self.set_state(TunnelState::Connecting).await;

        match self.spawn_tunnel_process().await {
            Ok(child) => {
                *self.process.write().await = Some(child);
                self.set_state(TunnelState::Connected).await;
                self.update_connected_time().await;
                info!("Tunnel started successfully");
                
                tokio::spawn(self.clone().monitor_process());
                Ok(())
            }
            Err(e) => {
                self.increment_error_count().await;
                self.set_state(TunnelState::Error).await;
                error!("Failed to start tunnel: {}", e);
                Err(e)
            }
        }
    }

    pub async fn stop(&self) -> Result<()> {
        *self.shutdown.write().await = true;
        
        if let Some(mut child) = self.process.write().await.take() {
            match child.kill() {
                Ok(_) => {
                    info!("Tunnel process terminated");
                    let _ = child.wait();
                }
                Err(e) => {
                    warn!("Failed to kill tunnel process: {}", e);
                }
            }
        }

        self.set_state(TunnelState::Disconnected).await;
        Ok(())
    }

    pub async fn get_status(&self) -> TunnelStatus {
        self.status.read().await.clone()
    }

    pub async fn health_check(&self) -> Result<bool> {
        let status = self.get_status().await;
        
        match status.state {
            TunnelState::Connected => {
                if let Some(endpoint) = &status.endpoint {
                    self.ping_endpoint(endpoint).await
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }

    async fn spawn_tunnel_process(&self) -> Result<Child> {
        let mut cmd = match self.config.tunnel_type.as_str() {
            "tailscale" => self.build_tailscale_command()?,
            "wireguard" => self.build_wireguard_command()?,
            "ssh" => self.build_ssh_command()?,
            _ => {
                return Err(UsbNodeError::Network(format!(
                    "Unsupported tunnel type: {}",
                    self.config.tunnel_type
                )));
            }
        };

        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped())
           .stdin(Stdio::null());

        cmd.spawn().map_err(|e| {
            UsbNodeError::Network(format!("Failed to spawn tunnel process: {}", e))
        })
    }

    fn build_tailscale_command(&self) -> Result<Command> {
        let mut cmd = Command::new("tailscale");
        cmd.arg("up");
        
        if let Some(auth_key) = &self.config.auth_key {
            cmd.arg("--authkey").arg(auth_key);
        }
        
        if let Some(hostname) = &self.config.hostname {
            cmd.arg("--hostname").arg(hostname);
        }
        
        cmd.arg("--accept-routes");
        Ok(cmd)
    }

    fn build_wireguard_command(&self) -> Result<Command> {
        let config_path = self.config.config_path.as_ref()
            .ok_or_else(|| UsbNodeError::Network("WireGuard config path required".to_string()))?;
        
        let mut cmd = Command::new("wg-quick");
        cmd.arg("up").arg(config_path);
        Ok(cmd)
    }

    fn build_ssh_command(&self) -> Result<Command> {
        let remote_host = self.config.remote_host.as_ref()
            .ok_or_else(|| UsbNodeError::Network("SSH remote host required".to_string()))?;
        
        let mut cmd = Command::new("ssh");
        cmd.arg("-N") // No command execution
           .arg("-f") // Background
           .arg("-o").arg("StrictHostKeyChecking=no")
           .arg("-o").arg("ServerAliveInterval=30")
           .arg("-o").arg("ServerAliveCountMax=3");
        
        if let Some(key_path) = &self.config.private_key_path {
            cmd.arg("-i").arg(key_path);
        }
        
        if let Some(local_port) = self.config.local_port {
            if let Some(remote_port) = self.config.remote_port {
                cmd.arg("-L").arg(format!("{}:localhost:{}", local_port, remote_port));
            }
        }
        
        cmd.arg(remote_host);
        Ok(cmd)
    }

    fn validate_config(&self) -> Result<()> {
        match self.config.tunnel_type.as_str() {
            "tailscale" => {
                if self.config.auth_key.is_none() {
                    return Err(UsbNodeError::Config(
                        "Tailscale auth key required".to_string()
                    ));
                }
            }
            "wireguard" => {
                if self.config.config_path.is_none() {
                    return Err(UsbNodeError::Config(
                        "WireGuard config path required".to_string()
                    ));
                }
            }
            "ssh" => {
                if self.config.remote_host.is_none() {
                    return Err(UsbNodeError::Config(
                        "SSH remote host required".to_string()
                    ));
                }
            }
            _ => {
                return Err(UsbNodeError::Config(format!(
                    "Unsupported tunnel type: {}",
                    self.config.tunnel_type
                )));
            }
        }
        Ok(())
    }

    async fn ping_endpoint(&self, endpoint: &str) -> Result<bool> {
        let output = Command::new("ping")
            .arg("-c").arg("1")
            .arg("-W").arg("3")
            .arg(endpoint)
            .output()
            .await
            .map_err(|e| UsbNodeError::Network(format!("Ping failed: {}", e)))?;

        Ok(output.status.success())
    }

    async fn set_state(&self, state: TunnelState) {
        self.status.write().await.state = state;
    }

    async fn update_connected_time(&self) {
        self.status.write().await.last_connected = Some(Instant::now());
    }

    async fn increment_error_count(&self) {
        self.status.write().await.error_count += 1;
    }

    async fn monitor_process(self) {
        let mut backoff = Duration::from_secs(1);
        const MAX_BACKOFF: Duration = Duration::from_secs(60);
        const BACKOFF_MULTIPLIER: u32 = 2;

        loop {
            if *self.shutdown.read().await {
                break;
            }

            sleep(Duration::from_secs(5)).await;

            let process_alive = {
                let mut process_guard = self.process.write().await;
                if let Some(ref mut child) = process_guard.as_mut() {
                    match child.try_wait() {
                        Ok(Some(_)) => false, // Process exited
                        Ok(None) => true,     // Process still running
                        Err(_) => false,      // Error checking process
                    }
                } else {
                    false
                }
            };

            if !process_alive {
                warn!("Tunnel process died, attempting restart");
                self.set_state(TunnelState::Reconnecting).await;
                self.increment_error_count().await;

                sleep(backoff).await;

                match self.spawn_tunnel_process().await {
                    Ok(child) => {
                        *self.process.write().await = Some(child);
                        self.set_state(TunnelState::Connected).await;
                        self.update_connected_time().await;
                        info!("Tunnel process restarted successfully");
                        backoff = Duration::from_secs(1); // Reset backoff
                    }
                    Err(e) => {
                        error!("Failed to restart tunnel process: {}", e);
                        self.set_state(TunnelState::Error).await;
                        backoff = std::cmp::min(
                            backoff * BACKOFF_MULTIPLIER,
                            MAX_BACKOFF
                        );
                    }
                }
            }
        }

        debug!("Tunnel monitor stopped");
    }
}

impl Clone for TunnelManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            process: Arc::clone(&self.process),
            status: Arc::clone(&self.status),
            shutdown: Arc::clone(&self.shutdown),
        }
    }
}