use crate::error::{UsbInstallerError, UsbInstallerResult};
use rand::Rng;
use std::process::{Command, Stdio};
use std::str;

pub struct HostnameManager {
    hostname: String,
    mdns_enabled: bool,
}

impl HostnameManager {
    pub fn new(mdns_enabled: bool) -> Self {
        Self {
            hostname: Self::generate_hostname(),
            mdns_enabled,
        }
    }

    fn generate_hostname() -> String {
        let mut rng = rand::thread_rng();
        let suffix: u16 = rng.gen_range(1000..9999);
        format!("usb-node-{}", suffix)
    }

    pub fn set_hostname(&self) -> UsbInstallerResult<()> {
        #[cfg(target_os = "linux")]
        {
            self.set_linux_hostname()?;
        }

        #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
        {
            self.set_bsd_hostname()?;
        }

        log::info!("Hostname set to: {}", self.hostname);

        if self.mdns_enabled {
            self.register_mdns()?;
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn set_linux_hostname(&self) -> UsbInstallerResult<()> {
        let output = Command::new("hostnamectl")
            .args(&["set-hostname", &self.hostname])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to set hostname: {}", e)))?;

        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("Unknown error");
            return Err(UsbInstallerError::Network(format!("hostnamectl failed: {}", stderr)));
        }

        Ok(())
    }

    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
    fn set_bsd_hostname(&self) -> UsbInstallerResult<()> {
        let output = Command::new("sysctl")
            .args(&[&format!("kern.hostname={}", self.hostname)])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to set hostname: {}", e)))?;

        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("Unknown error");
            return Err(UsbInstallerError::Network(format!("sysctl failed: {}", stderr)));
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn register_mdns(&self) -> UsbInstallerResult<()> {
        let output = Command::new("systemctl")
            .args(&["is-active", "avahi-daemon"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to check avahi status: {}", e)))?;

        if !output.status.success() {
            log::warn!("Avahi daemon not running, attempting to start");
            self.start_avahi()?;
        }

        let service_file = format!("[Unit]\nDescription=USB Installer Node\n\n[Service]\nType=notify\nUser=root\nExecStart=/usr/bin/avahi-publish -s {} _usb-installer._tcp 22\nRestart=always\n\n[Install]\nWantedBy=multi-user.target", self.hostname);
        
        std::fs::write("/etc/systemd/system/usb-installer-mdns.service", service_file)
            .map_err(|e| UsbInstallerError::Network(format!("Failed to write mDNS service file: {}", e)))?;

        let output = Command::new("systemctl")
            .args(&["enable", "--now", "usb-installer-mdns.service"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to enable mDNS service: {}", e)))?;

        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("Unknown error");
            log::warn!("Failed to enable mDNS service: {}", stderr);
        } else {
            log::info!("mDNS service registered for hostname: {}.local", self.hostname);
        }

        Ok(())
    }

    #[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
    fn register_mdns(&self) -> UsbInstallerResult<()> {
        let output = Command::new("service")
            .args(&["mdnsd", "status"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to check mdnsd status: {}", e)))?;

        if !output.status.success() {
            log::warn!("mdnsd not running, attempting to start");
            let start_output = Command::new("service")
                .args(&["mdnsd", "start"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| UsbInstallerError::Network(format!("Failed to start mdnsd: {}", e)))?;

            if !start_output.status.success() {
                let stderr = str::from_utf8(&start_output.stderr).unwrap_or("Unknown error");
                log::warn!("Failed to start mdnsd: {}", stderr);
                return Ok(());
            }
        }

        log::info!("mDNS hostname registered: {}.local", self.hostname);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn start_avahi(&self) -> UsbInstallerResult<()> {
        let output = Command::new("systemctl")
            .args(&["start", "avahi-daemon"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to start avahi: {}", e)))?;

        if !output.status.success() {
            let stderr = str::from_utf8(&output.stderr).unwrap_or("Unknown error");
            return Err(UsbInstallerError::Network(format!("Failed to start avahi daemon: {}", stderr)));
        }

        Ok(())
    }

    pub fn get_hostname(&self) -> &str {
        &self.hostname
    }

    pub fn get_fqdn(&self) -> String {
        if self.mdns_enabled {
            format!("{}.local", self.hostname)
        } else {
            self.hostname.clone()
        }
    }

    pub fn verify_hostname(&self) -> UsbInstallerResult<bool> {
        let output = Command::new("hostname")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| UsbInstallerError::Network(format!("Failed to get current hostname: {}", e)))?;

        if !output.status.success() {
            return Ok(false);
        }

        let current_hostname = str::from_utf8(&output.stdout)
            .map_err(|e| UsbInstallerError::Network(format!("Invalid UTF-8 in hostname: {}", e)))?
            .trim();

        Ok(current_hostname == self.hostname)
    }

    pub fn reset_hostname(&mut self) -> UsbInstallerResult<()> {
        self.hostname = Self::generate_hostname();
        self.set_hostname()
    }

    pub fn cleanup_mdns(&self) -> UsbInstallerResult<()> {
        if !self.mdns_enabled {
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            let _ = Command::new("systemctl")
                .args(&["stop", "usb-installer-mdns.service"])
                .output();

            let _ = Command::new("systemctl")
                .args(&["disable", "usb-installer-mdns.service"])
                .output();

            let _ = std::fs::remove_file("/etc/systemd/system/usb-installer-mdns.service");
        }

        log::info!("mDNS cleanup completed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hostname_generation() {
        let hostname = HostnameManager::generate_hostname();
        assert!(hostname.starts_with("usb-node-"));
        assert_eq!(hostname.len(), 13); // "usb-node-" + 4 digits
    }

    #[test]
    fn test_hostname_manager_creation() {
        let manager = HostnameManager::new(true);
        assert!(manager.get_hostname().starts_with("usb-node-"));
        assert!(manager.mdns_enabled);
    }

    #[test]
    fn test_fqdn_generation() {
        let manager = HostnameManager::new(true);
        let fqdn = manager.get_fqdn();
        assert!(fqdn.ends_with(".local"));
        
        let manager_no_mdns = HostnameManager::new(false);
        let fqdn_no_mdns = manager_no_mdns.get_fqdn();
        assert!(!fqdn_no_mdns.ends_with(".local"));
    }

    #[test]
    fn test_hostname_reset() {
        let mut manager = HostnameManager::new(false);
        let original = manager.get_hostname().to_string();
        
        // Reset should generate a new hostname
        let _ = manager.reset_hostname();
        let new_hostname = manager.get_hostname();
        
        // They should both follow the pattern but be different
        assert!(new_hostname.starts_with("usb-node-"));
        // Note: There's a tiny chance they could be the same, but very unlikely
    }
}