use crate::error::{UsbInstallerError, UsbInstallerResult};
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use std::str;
use std::time::{Duration, Instant};
use tokio::time;

#[derive(Debug, Clone)]
pub struct DhcpLease {
    pub ip: Ipv4Addr,
    pub gateway: Option<Ipv4Addr>,
    pub dns: Vec<Ipv4Addr>,
    pub lease_time: u32,
    pub acquired_at: Instant,
}

#[derive(Debug, Clone)]
pub enum DhcpState {
    Down,
    Requesting,
    Bound,
    Renewing,
    Rebinding,
    Error(String),
}

pub struct DhcpClient {
    interface: String,
    current_lease: Option<DhcpLease>,
    state: DhcpState,
    retry_count: u32,
    max_retries: u32,
}

impl DhcpClient {
    pub fn new(interface: Option<String>) -> UsbInstallerResult<Self> {
        let interface = match interface {
            Some(iface) => iface,
            None => Self::detect_interface()?,
        };

        Ok(Self {
            interface,
            current_lease: None,
            state: DhcpState::Down,
            retry_count: 0,
            max_retries: 5,
        })
    }

    fn detect_interface() -> UsbInstallerResult<String> {
        let output = Command::new("ip")
            .args(&["link", "show"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to list interfaces: {}", e)))?;

        let output_str = str::from_utf8(&output.stdout)
            .map_err(|e| UsbInstallerError::Network(format!("Invalid UTF-8 in interface list: {}", e)))?;

        for line in output_str.lines() {
            if line.contains("state UP") && !line.contains("lo:") {
                if let Some(iface) = line.split(':').nth(1) {
                    let interface = iface.trim().to_string();
                    log::info!("Auto-detected interface: {}", interface);
                    return Ok(interface);
                }
            }
        }

        Err(UsbInstallerError::Network("No active interface found".to_string()))
    }

    pub async fn request_lease(&mut self) -> UsbInstallerResult<DhcpLease> {
        self.state = DhcpState::Requesting;
        self.retry_count = 0;

        while self.retry_count < self.max_retries {
            match self.attempt_dhcp_request().await {
                Ok(lease) => {
                    self.current_lease = Some(lease.clone());
                    self.state = DhcpState::Bound;
                    self.retry_count = 0;
                    log::info!("DHCP lease acquired: IP={}, Gateway={:?}, DNS={:?}", 
                               lease.ip, lease.gateway, lease.dns);
                    return Ok(lease);
                }
                Err(e) => {
                    self.retry_count += 1;
                    let backoff = Duration::from_secs(2_u64.pow(self.retry_count));
                    
                    log::warn!("DHCP request failed (attempt {}): {}", self.retry_count, e);
                    
                    if self.retry_count >= self.max_retries {
                        self.state = DhcpState::Error(format!("Max retries exceeded: {}", e));
                        return Err(e);
                    }

                    log::info!("Retrying DHCP request in {} seconds", backoff.as_secs());
                    time::sleep(backoff).await;
                }
            }
        }

        let error = UsbInstallerError::Network("DHCP request failed after max retries".to_string());
        self.state = DhcpState::Error(error.to_string());
        Err(error)
    }

    async fn attempt_dhcp_request(&self) -> UsbInstallerResult<DhcpLease> {
        let output = Command::new("dhclient")
            .args(&["-v", &self.interface])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to run dhclient: {}", e)))?;

        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("Unknown error");
            return Err(UsbInstallerError::Network(format!("dhclient failed: {}", stderr)));
        }

        self.parse_lease_info().await
    }

    async fn parse_lease_info(&self) -> UsbInstallerResult<DhcpLease> {
        let output = Command::new("ip")
            .args(&["addr", "show", &self.interface])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to get interface info: {}", e)))?;

        let output_str = str::from_utf8(&output.stdout)
            .map_err(|e| UsbInstallerError::Network(format!("Invalid UTF-8 in interface info: {}", e)))?;

        let ip = self.extract_ip_address(output_str)?;
        let gateway = self.get_gateway().await?;
        let dns = self.get_dns_servers().await?;

        Ok(DhcpLease {
            ip,
            gateway,
            dns,
            lease_time: 3600,
            acquired_at: Instant::now(),
        })
    }

    fn extract_ip_address(&self, output: &str) -> UsbInstallerResult<Ipv4Addr> {
        for line in output.lines() {
            if line.contains("inet ") && !line.contains("127.0.0.1") {
                if let Some(ip_part) = line.split_whitespace().nth(1) {
                    if let Some(ip_str) = ip_part.split('/').next() {
                        return ip_str.parse::<Ipv4Addr>()
                            .map_err(|e| UsbInstallerError::Network(format!("Invalid IP address: {}", e)));
                    }
                }
            }
        }
        Err(UsbInstallerError::Network("No IP address found".to_string()))
    }

    async fn get_gateway(&self) -> UsbInstallerResult<Option<Ipv4Addr>> {
        let output = Command::new("ip")
            .args(&["route", "show", "default"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to get gateway: {}", e)))?;

        let output_str = str::from_utf8(&output.stdout).unwrap_or("");
        
        for line in output_str.lines() {
            if line.contains("default via") {
                if let Some(gateway_str) = line.split_whitespace().nth(2) {
                    if let Ok(gateway) = gateway_str.parse::<Ipv4Addr>() {
                        return Ok(Some(gateway));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn get_dns_servers(&self) -> UsbInstallerResult<Vec<Ipv4Addr>> {
        let mut dns_servers = Vec::new();
        
        if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
            for line in content.lines() {
                if line.starts_with("nameserver ") {
                    if let Some(dns_str) = line.split_whitespace().nth(1) {
                        if let Ok(dns) = dns_str.parse::<Ipv4Addr>() {
                            dns_servers.push(dns);
                        }
                    }
                }
            }
        }
        
        Ok(dns_servers)
    }

    pub async fn renew_lease(&mut self) -> UsbInstallerResult<()> {
        if self.current_lease.is_none() {
            return Err(UsbInstallerError::Network("No active lease to renew".to_string()));
        }

        self.state = DhcpState::Renewing;
        log::info!("Renewing DHCP lease for interface {}", self.interface);

        match self.request_lease().await {
            Ok(_) => {
                log::info!("DHCP lease renewed successfully");
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to renew DHCP lease: {}", e);
                self.state = DhcpState::Error(e.to_string());
                Err(e)
            }
        }
    }

    pub fn release_lease(&mut self) -> UsbInstallerResult<()> {
        if self.current_lease.is_none() {
            return Ok(());
        }

        log::info!("Releasing DHCP lease for interface {}", self.interface);

        let output = Command::new("dhclient")
            .args(&["-r", &self.interface])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to release lease: {}", e)))?;

        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("Unknown error");
            log::warn!("dhclient release warning: {}", stderr);
        }

        self.current_lease = None;
        self.state = DhcpState::Down;
        log::info!("DHCP lease released");
        Ok(())
    }

    pub fn get_lease(&self) -> Option<&DhcpLease> {
        self.current_lease.as_ref()
    }

    pub fn get_state(&self) -> &DhcpState {
        &self.state
    }

    pub fn get_interface(&self) -> &str {
        &self.interface
    }

    pub fn is_lease_expired(&self) -> bool {
        if let Some(lease) = &self.current_lease {
            let elapsed = lease.acquired_at.elapsed().as_secs() as u32;
            elapsed > lease.lease_time
        } else {
            true
        }
    }

    pub async fn monitor_link_status(&self) -> UsbInstallerResult<bool> {
        let output = Command::new("ip")
            .args(&["link", "show", &self.interface])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to check link status: {}", e)))?;

        let output_str = str::from_utf8(&output.stdout).unwrap_or("");
        Ok(output_str.contains("state UP"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dhcp_client_creation() {
        let client = DhcpClient::new(Some("eth0".to_string()));
        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.get_interface(), "eth0");
        assert!(matches!(client.get_state(), DhcpState::Down));
    }

    #[test]
    fn test_ip_address_extraction() {
        let client = DhcpClient::new(Some("eth0".to_string())).unwrap();
        let output = "2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc pfifo_fast state UP group default qlen 1000\n    inet 192.168.1.100/24 brd 192.168.1.255 scope global dynamic eth0";
        let ip = client.extract_ip_address(output).unwrap();
        assert_eq!(ip, Ipv4Addr::new(192, 168, 1, 100));
    }

    #[test]
    fn test_lease_expiration() {
        let mut client = DhcpClient::new(Some("eth0".to_string())).unwrap();
        assert!(client.is_lease_expired());
        
        client.current_lease = Some(DhcpLease {
            ip: Ipv4Addr::new(192, 168, 1, 100),
            gateway: None,
            dns: vec![],
            lease_time: 3600,
            acquired_at: Instant::now(),
        });
        
        assert!(!client.is_lease_expired());
    }
}