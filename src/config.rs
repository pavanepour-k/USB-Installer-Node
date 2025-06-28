use crate::error::{ConfigError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::{env, fs};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub network: NetworkConfig,
    pub remote: RemoteConfig,
    pub iso: IsoConfig,
    pub ui: UiConfig,
    pub logging: LoggingConfig,
    pub disk: DiskConfig,
    pub service: ServiceConfig,
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub interface: Option<String>,
    pub dhcp_timeout: u64,
    pub hostname_prefix: String,
    pub mdns_enabled: bool,
    pub tunnel: TunnelConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub enabled: bool,
    pub provider: TunnelProvider,
    pub config_path: Option<PathBuf>,
    pub reconnect_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelProvider {
    Tailscale,
    Wireguard,
    Ssh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub vnc: VncConfig,
    pub ssh: SshConfig,
    pub web_vnc: WebVncConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VncConfig {
    pub enabled: bool,
    pub port: u16,
    pub display: String,
    pub password: Option<String>,
    pub view_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub enabled: bool,
    pub port: u16,
    pub key_path: PathBuf,
    pub authorized_keys_path: PathBuf,
    pub password_auth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebVncConfig {
    pub enabled: bool,
    pub port: u16,
    pub https: bool,
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    pub auth_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsoConfig {
    pub search_paths: Vec<PathBuf>,
    pub patterns: Vec<String>,
    pub mount_point: PathBuf,
    pub auto_mount: bool,
    pub auto_launch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub language: String,
    pub fullscreen: bool,
    pub show_logs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: LogLevel,
    pub file_path: Option<PathBuf>,
    pub console: bool,
    pub max_file_size: u64,
    pub max_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskConfig {
    pub auto_partition: bool,
    pub partition_scheme: PartitionScheme,
    pub default_filesystem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartitionScheme {
    Mbr,
    Gpt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub autorun: bool,
    pub service_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub watchdog_interval: u64,
    pub max_restart_attempts: u32,
    pub restart_delay: u64,
    pub metrics_port: Option<u16>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path).map_err(ConfigError::ReadFailed)?;
        let mut config: Config =
            toml::from_str(&content).map_err(|e| ConfigError::ParseFailed(e.to_string()))?;

        config.apply_env_overrides()?;
        config.validate()?;

        Ok(config)
    }

    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        match Self::load(path) {
            Ok(config) => Ok(config),
            Err(_) => {
                let config = Self::default();
                config.validate()?;
                Ok(config)
            }
        }
    }

    fn apply_env_overrides(&mut self) -> Result<()> {
        if let Ok(val) = env::var("USB_NODE_LOG_LEVEL") {
            self.logging.level = match val.to_lowercase().as_str() {
                "trace" => LogLevel::Trace,
                "debug" => LogLevel::Debug,
                "info" => LogLevel::Info,
                "warn" => LogLevel::Warn,
                "error" => LogLevel::Error,
                _ => {
                    return Err(
                        ConfigError::EnvVarError(format!("Invalid log level: {}", val)).into(),
                    )
                }
            };
        }

        if let Ok(val) = env::var("USB_NODE_INTERFACE") {
            self.network.interface = Some(val);
        }

        if let Ok(val) = env::var("USB_NODE_VNC_PORT") {
            self.remote.vnc.port = val
                .parse()
                .map_err(|_| ConfigError::EnvVarError("Invalid VNC port".to_string()))?;
        }

        if let Ok(val) = env::var("USB_NODE_SSH_PORT") {
            self.remote.ssh.port = val
                .parse()
                .map_err(|_| ConfigError::EnvVarError("Invalid SSH port".to_string()))?;
        }

        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.network.dhcp_timeout == 0 {
            return Err(
                ConfigError::ValidationFailed("DHCP timeout must be > 0".to_string()).into(),
            );
        }

        if self.network.hostname_prefix.is_empty() {
            return Err(ConfigError::ValidationFailed(
                "Hostname prefix cannot be empty".to_string(),
            )
            .into());
        }

        if self.remote.vnc.port == 0 || self.remote.vnc.port > 65535 {
            return Err(ConfigError::ValidationFailed("Invalid VNC port".to_string()).into());
        }

        if self.remote.ssh.port == 0 || self.remote.ssh.port > 65535 {
            return Err(ConfigError::ValidationFailed("Invalid SSH port".to_string()).into());
        }

        if self.remote.web_vnc.port == 0 || self.remote.web_vnc.port > 65535 {
            return Err(ConfigError::ValidationFailed("Invalid Web VNC port".to_string()).into());
        }

        if self.remote.web_vnc.https
            && (self.remote.web_vnc.cert_path.is_none() || self.remote.web_vnc.key_path.is_none())
        {
            return Err(ConfigError::ValidationFailed(
                "HTTPS requires cert and key paths".to_string(),
            )
            .into());
        }

        if self.iso.search_paths.is_empty() {
            return Err(ConfigError::ValidationFailed(
                "ISO search paths cannot be empty".to_string(),
            )
            .into());
        }

        if self.monitoring.watchdog_interval == 0 {
            return Err(
                ConfigError::ValidationFailed("Watchdog interval must be > 0".to_string()).into(),
            );
        }

        if self.monitoring.max_restart_attempts == 0 {
            return Err(ConfigError::ValidationFailed(
                "Max restart attempts must be > 0".to_string(),
            )
            .into());
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            remote: RemoteConfig::default(),
            iso: IsoConfig::default(),
            ui: UiConfig::default(),
            logging: LoggingConfig::default(),
            disk: DiskConfig::default(),
            service: ServiceConfig::default(),
            monitoring: MonitoringConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            interface: None,
            dhcp_timeout: 30,
            hostname_prefix: "usb-node".to_string(),
            mdns_enabled: true,
            tunnel: TunnelConfig::default(),
        }
    }
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: TunnelProvider::Tailscale,
            config_path: None,
            reconnect_interval: 60,
        }
    }
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            vnc: VncConfig::default(),
            ssh: SshConfig::default(),
            web_vnc: WebVncConfig::default(),
        }
    }
}

impl Default for VncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 5900,
            display: ":0".to_string(),
            password: None,
            view_only: false,
        }
    }
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 22,
            key_path: PathBuf::from("/etc/ssh/ssh_host_rsa_key"),
            authorized_keys_path: PathBuf::from("/root/.ssh/authorized_keys"),
            password_auth: false,
        }
    }
}

impl Default for WebVncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 6080,
            https: false,
            cert_path: None,
            key_path: None,
            auth_required: true,
        }
    }
}

impl Default for IsoConfig {
    fn default() -> Self {
        Self {
            search_paths: vec![PathBuf::from("/installers")],
            patterns: vec!["*.iso".to_string()],
            mount_point: PathBuf::from("/mnt/iso"),
            auto_mount: true,
            auto_launch: false,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
            language: "en".to_string(),
            fullscreen: false,
            show_logs: true,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            file_path: Some(PathBuf::from("/var/log/usb-installer.log")),
            console: true,
            max_file_size: 10 * 1024 * 1024,
            max_files: 5,
        }
    }
}

impl Default for DiskConfig {
    fn default() -> Self {
        Self {
            auto_partition: false,
            partition_scheme: PartitionScheme::Gpt,
            default_filesystem: "ext4".to_string(),
        }
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            autorun: true,
            service_name: "usb-installer-node".to_string(),
            description: "USB Installer Node Service".to_string(),
        }
    }
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            watchdog_interval: 30,
            max_restart_attempts: 3,
            restart_delay: 5,
            metrics_port: Some(9090),
        }
    }
}

pub struct ConfigManager {
    config: Arc<RwLock<Config>>,
}

impl ConfigManager {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
        }
    }

    pub fn get(&self) -> Arc<RwLock<Config>> {
        Arc::clone(&self.config)
    }

    pub fn reload<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let new_config = Config::load(path)?;
        let mut config = self.config.write().map_err(|_| {
            ConfigError::ValidationFailed("Failed to acquire write lock".to_string())
        })?;
        *config = new_config;
        Ok(())
    }
}
