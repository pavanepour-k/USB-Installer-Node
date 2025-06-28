pub mod ssh;
pub mod vnc;
pub mod web_vnc;

use crate::config::RemoteConfig;
use crate::error::{RemoteError, Result};
use ssh::{SshConfig, SshServer};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use vnc::{VncConfig, VncServer};
use web_vnc::{WebVncConfig, WebVncServer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteManagerState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

pub struct RemoteManager {
    config: Arc<RwLock<RemoteConfig>>,
    state: Arc<RwLock<RemoteManagerState>>,
    vnc_server: Option<Arc<VncServer>>,
    ssh_server: Option<Arc<SshServer>>,
    web_vnc_server: Option<Arc<WebVncServer>>,
}

impl RemoteManager {
    pub fn new(config: Arc<RwLock<RemoteConfig>>) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(RemoteManagerState::Stopped)),
            vnc_server: None,
            ssh_server: None,
            web_vnc_server: None,
        }
    }

    pub async fn start_all(&mut self) -> Result<()> {
        info!("Starting remote access services");
        self.set_state(RemoteManagerState::Starting).await;

        let config = self.config.read().await;
        let mut any_started = false;
        let mut errors = Vec::new();

        if config.vnc.enabled {
            match self.start_vnc(&config.vnc).await {
                Ok(_) => any_started = true,
                Err(e) => {
                    error!("Failed to start VNC: {}", e);
                    errors.push(format!("VNC: {}", e));
                }
            }
        }

        if config.ssh.enabled {
            match self.start_ssh(&config.ssh).await {
                Ok(_) => any_started = true,
                Err(e) => {
                    error!("Failed to start SSH: {}", e);
                    errors.push(format!("SSH: {}", e));
                }
            }
        }

        if config.web_vnc.enabled {
            match self.start_web_vnc(&config.web_vnc).await {
                Ok(_) => any_started = true,
                Err(e) => {
                    error!("Failed to start Web VNC: {}", e);
                    errors.push(format!("Web VNC: {}", e));
                }
            }
        }

        if !any_started && !errors.is_empty() {
            self.set_state(RemoteManagerState::Error(errors.join(", ")))
                .await;
            return Err(RemoteError::StartFailed(
                "No remote services could be started".to_string(),
            ));
        }

        self.set_state(RemoteManagerState::Running).await;
        info!("Remote access services started");
        Ok(())
    }

    pub async fn stop_all(&mut self) -> Result<()> {
        info!("Stopping remote access services");
        self.set_state(RemoteManagerState::Stopping).await;

        let mut errors = Vec::new();

        if let Some(vnc) = &self.vnc_server {
            if let Err(e) = vnc.stop().await {
                error!("Failed to stop VNC: {}", e);
                errors.push(format!("VNC: {}", e));
            }
        }

        if let Some(ssh) = &self.ssh_server {
            if let Err(e) = ssh.stop().await {
                error!("Failed to stop SSH: {}", e);
                errors.push(format!("SSH: {}", e));
            }
        }

        if let Some(web_vnc) = &self.web_vnc_server {
            if let Err(e) = web_vnc.stop().await {
                error!("Failed to stop Web VNC: {}", e);
                errors.push(format!("Web VNC: {}", e));
            }
        }

        self.vnc_server = None;
        self.ssh_server = None;
        self.web_vnc_server = None;

        if !errors.is_empty() {
            self.set_state(RemoteManagerState::Error(errors.join(", ")))
                .await;
            return Err(RemoteError::StopFailed(
                "Some services failed to stop".to_string(),
            ));
        }

        self.set_state(RemoteManagerState::Stopped).await;
        info!("Remote access services stopped");
        Ok(())
    }

    async fn start_vnc(&mut self, config: &crate::config::VncConfig) -> Result<()> {
        let vnc_config = VncConfig {
            display: config.display.clone(),
            port: config.port,
            password: config.password.clone(),
            auth_file: config.auth_file.clone(),
            geometry: config.geometry.clone(),
            depth: config.depth,
            allow_shared: config.allow_shared,
            view_only: config.view_only,
        };

        let server = Arc::new(VncServer::new(vnc_config));
        server.start().await?;
        self.vnc_server = Some(server);
        Ok(())
    }

    async fn start_ssh(&mut self, config: &crate::config::SshConfig) -> Result<()> {
        let ssh_config = SshConfig {
            port: config.port,
            bind_address: config.bind_address.clone(),
            host_key_path: config.host_key_path.clone(),
            authorized_keys_path: config.authorized_keys_path.clone(),
            allow_password_auth: config.allow_password_auth,
            allow_root_login: config.allow_root_login,
            max_sessions: config.max_sessions,
            permit_empty_passwords: config.permit_empty_passwords,
        };

        let server = Arc::new(SshServer::new(ssh_config));
        server.start().await?;
        self.ssh_server = Some(server);
        Ok(())
    }

    async fn start_web_vnc(&mut self, config: &crate::config::WebVncConfig) -> Result<()> {
        let web_vnc_config = WebVncConfig {
            listen_port: config.listen_port,
            vnc_host: config.vnc_host.clone(),
            vnc_port: config.vnc_port,
            cert_path: config.cert_path.clone(),
            key_path: config.key_path.clone(),
            enable_auth: config.enable_auth,
            username: config.username.clone(),
            password: config.password.clone(),
            session_timeout: config.session_timeout,
        };

        let server = Arc::new(WebVncServer::new(web_vnc_config));
        server.start().await?;
        self.web_vnc_server = Some(server);
        Ok(())
    }

    pub async fn get_status(&self) -> HashMap<String, HashMap<String, String>> {
        let mut status = HashMap::new();

        if let Some(vnc) = &self.vnc_server {
            status.insert("vnc".to_string(), vnc.get_status().await);
        }

        if let Some(ssh) = &self.ssh_server {
            status.insert("ssh".to_string(), ssh.get_status().await);
        }

        if let Some(web_vnc) = &self.web_vnc_server {
            status.insert("web_vnc".to_string(), web_vnc.get_status().await);
        }

        status
    }

    pub async fn get_state(&self) -> RemoteManagerState {
        self.state.read().await.clone()
    }

    async fn set_state(&self, state: RemoteManagerState) {
        *self.state.write().await = state;
    }

    pub async fn reload_config(&mut self, config: Arc<RwLock<RemoteConfig>>) -> Result<()> {
        info!("Reloading remote configuration");

        let was_running = self.get_state().await == RemoteManagerState::Running;

        if was_running {
            self.stop_all().await?;
        }

        self.config = config;

        if was_running {
            self.start_all().await?;
        }

        Ok(())
    }

    pub async fn health_check(&self) -> Result<()> {
        let state = self.get_state().await;
        match state {
            RemoteManagerState::Error(e) => Err(RemoteError::HealthCheckFailed(e)),
            RemoteManagerState::Running => {
                let mut unhealthy = Vec::new();

                if let Some(vnc) = &self.vnc_server {
                    if !vnc.is_running().await {
                        unhealthy.push("VNC");
                    }
                }

                if let Some(ssh) = &self.ssh_server {
                    if !ssh.is_running().await {
                        unhealthy.push("SSH");
                    }
                }

                if let Some(web_vnc) = &self.web_vnc_server {
                    if !web_vnc.get_health_status().await {
                        unhealthy.push("Web VNC");
                    }
                }

                if !unhealthy.is_empty() {
                    Err(RemoteError::HealthCheckFailed(format!(
                        "Unhealthy services: {}",
                        unhealthy.join(", ")
                    )))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    pub async fn restart_service(&mut self, service: &str) -> Result<()> {
        match service {
            "vnc" => {
                if let Some(vnc) = &self.vnc_server {
                    vnc.restart().await?;
                }
            }
            "ssh" => {
                if let Some(ssh) = &self.ssh_server {
                    ssh.stop().await?;
                    ssh.start().await?;
                }
            }
            "web_vnc" => {
                if let Some(web_vnc) = &self.web_vnc_server {
                    web_vnc.stop().await?;
                    web_vnc.start().await?;
                }
            }
            _ => return Err(RemoteError::InvalidService(service.to_string())),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_remote_manager_creation() {
        let config = Arc::new(RwLock::new(RemoteConfig::default()));
        let manager = RemoteManager::new(config);
        assert_eq!(manager.get_state().await, RemoteManagerState::Stopped);
    }

    #[tokio::test]
    async fn test_state_transitions() {
        let config = Arc::new(RwLock::new(RemoteConfig::default()));
        let manager = RemoteManager::new(config);

        manager.set_state(RemoteManagerState::Starting).await;
        assert_eq!(manager.get_state().await, RemoteManagerState::Starting);

        manager.set_state(RemoteManagerState::Running).await;
        assert_eq!(manager.get_state().await, RemoteManagerState::Running);

        manager.set_state(RemoteManagerState::Stopping).await;
        assert_eq!(manager.get_state().await, RemoteManagerState::Stopping);
    }

    #[tokio::test]
    async fn test_empty_status() {
        let config = Arc::new(RwLock::new(RemoteConfig::default()));
        let manager = RemoteManager::new(config);
        let status = manager.get_status().await;
        assert!(status.is_empty());
    }
}
