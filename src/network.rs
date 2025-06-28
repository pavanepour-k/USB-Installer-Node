use crate::config::NetworkConfig;
use crate::error::{Result, UsbNodeError};
use crate::network::dhcp::DhcpManager;
use crate::network::hostname::HostnameManager;
use crate::network::tunnel::TunnelManager;
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod dhcp;
pub mod hostname;
pub mod tunnel;

#[derive(Debug, Clone, PartialEq)]
pub enum NetworkState {
    Down,
    Configuring,
    Up,
    Error,
    Recovering,
}

#[derive(Debug)]
pub struct NetworkStatus {
    pub state: NetworkState,
    pub interface: Option<String>,
    pub ip_address: Option<String>,
    pub hostname: Option<String>,
    pub tunnel_connected: bool,
    pub error_message: Option<String>,
}

pub struct NetworkManager {
    config: NetworkConfig,
    dhcp_manager: DhcpManager,
    hostname_manager: HostnameManager,
    tunnel_manager: TunnelManager,
    state: Arc<RwLock<NetworkState>>,
    status: Arc<RwLock<NetworkStatus>>,
}

impl NetworkManager {
    pub fn new(config: NetworkConfig) -> Self {
        let dhcp_manager = DhcpManager::new(config.dhcp.clone());
        let hostname_manager = HostnameManager::new(config.hostname.clone());
        let tunnel_manager = TunnelManager::new(config.tunnel.clone());

        Self {
            config,
            dhcp_manager,
            hostname_manager,
            tunnel_manager,
            state: Arc::new(RwLock::new(NetworkState::Down)),
            status: Arc::new(RwLock::new(NetworkStatus {
                state: NetworkState::Down,
                interface: None,
                ip_address: None,
                hostname: None,
                tunnel_connected: false,
                error_message: None,
            })),
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting network manager");
        self.set_state(NetworkState::Configuring).await;

        match self.configure_network().await {
            Ok(_) => {
                self.set_state(NetworkState::Up).await;
                info!("Network manager started successfully");
                Ok(())
            }
            Err(e) => {
                self.set_state(NetworkState::Error).await;
                self.set_error_message(Some(e.to_string())).await;
                error!("Failed to start network manager: {}", e);
                Err(e)
            }
        }
    }

    pub async fn stop(&self) -> Result<()> {
        info!("Stopping network manager");

        if let Err(e) = self.tunnel_manager.stop().await {
            warn!("Error stopping tunnel manager: {}", e);
        }

        if let Err(e) = self.dhcp_manager.stop().await {
            warn!("Error stopping DHCP manager: {}", e);
        }

        self.set_state(NetworkState::Down).await;
        self.clear_status().await;
        info!("Network manager stopped");
        Ok(())
    }

    pub async fn get_status(&self) -> NetworkStatus {
        let mut status = self.status.read().await.clone();
        status.state = *self.state.read().await;
        status.tunnel_connected = self.tunnel_manager.health_check().await.unwrap_or(false);
        status
    }

    pub async fn health_check(&self) -> Result<bool> {
        let state = *self.state.read().await;

        match state {
            NetworkState::Up => {
                let dhcp_healthy = self.dhcp_manager.health_check().await?;
                let tunnel_healthy = if self.config.tunnel.enabled {
                    self.tunnel_manager.health_check().await.unwrap_or(false)
                } else {
                    true
                };

                Ok(dhcp_healthy && tunnel_healthy)
            }
            _ => Ok(false),
        }
    }

    pub async fn restart(&self) -> Result<()> {
        info!("Restarting network manager");
        self.set_state(NetworkState::Recovering).await;

        self.stop().await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        self.start().await
    }

    async fn configure_network(&self) -> Result<()> {
        debug!("Configuring DHCP");
        self.dhcp_manager.start().await?;

        let dhcp_status = self.dhcp_manager.get_status().await;
        self.update_dhcp_status(&dhcp_status).await;

        debug!("Configuring hostname");
        self.hostname_manager.start().await?;

        let hostname_status = self.hostname_manager.get_status().await;
        self.update_hostname_status(&hostname_status).await;

        if self.config.tunnel.enabled {
            debug!("Configuring tunnel");
            self.tunnel_manager.start().await?;
        }

        Ok(())
    }

    async fn update_dhcp_status(&self, dhcp_status: &crate::network::dhcp::DhcpStatus) {
        let mut status = self.status.write().await;
        status.interface = dhcp_status.interface.clone();
        status.ip_address = dhcp_status.ip_address.clone();
    }

    async fn update_hostname_status(
        &self,
        hostname_status: &crate::network::hostname::HostnameStatus,
    ) {
        let mut status = self.status.write().await;
        status.hostname = Some(hostname_status.hostname.clone());
    }

    async fn set_state(&self, state: NetworkState) {
        *self.state.write().await = state;
    }

    async fn set_error_message(&self, message: Option<String>) {
        self.status.write().await.error_message = message;
    }

    async fn clear_status(&self) {
        let mut status = self.status.write().await;
        status.interface = None;
        status.ip_address = None;
        status.hostname = None;
        status.tunnel_connected = false;
        status.error_message = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DhcpConfig, HostnameConfig, TunnelConfig};

    fn create_test_config() -> NetworkConfig {
        NetworkConfig {
            dhcp: DhcpConfig {
                interface: Some("eth0".to_string()),
                timeout: 30,
                retry_count: 3,
                retry_interval: 5,
            },
            hostname: HostnameConfig {
                prefix: "usb-node".to_string(),
                enable_mdns: true,
                mdns_domain: "local".to_string(),
            },
            tunnel: TunnelConfig {
                enabled: false,
                tunnel_type: "tailscale".to_string(),
                auth_key: None,
                config_path: None,
                remote_host: None,
                private_key_path: None,
                hostname: None,
                local_port: None,
                remote_port: None,
            },
        }
    }

    #[tokio::test]
    async fn test_network_manager_creation() {
        let config = create_test_config();
        let manager = NetworkManager::new(config);

        let status = manager.get_status().await;
        assert_eq!(status.state, NetworkState::Down);
        assert!(status.interface.is_none());
        assert!(status.ip_address.is_none());
        assert!(!status.tunnel_connected);
    }

    #[tokio::test]
    async fn test_network_state_transitions() {
        let config = create_test_config();
        let manager = NetworkManager::new(config);

        let initial_state = *manager.state.read().await;
        assert_eq!(initial_state, NetworkState::Down);

        manager.set_state(NetworkState::Configuring).await;
        let configuring_state = *manager.state.read().await;
        assert_eq!(configuring_state, NetworkState::Configuring);

        manager.set_state(NetworkState::Up).await;
        let up_state = *manager.state.read().await;
        assert_eq!(up_state, NetworkState::Up);
    }

    #[tokio::test]
    async fn test_error_message_handling() {
        let config = create_test_config();
        let manager = NetworkManager::new(config);

        let error_msg = "Test error".to_string();
        manager.set_error_message(Some(error_msg.clone())).await;

        let status = manager.get_status().await;
        assert_eq!(status.error_message, Some(error_msg));
    }

    #[tokio::test]
    async fn test_health_check_down_state() {
        let config = create_test_config();
        let manager = NetworkManager::new(config);

        let health = manager.health_check().await.unwrap();
        assert!(!health);
    }
}
