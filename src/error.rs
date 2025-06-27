use std::fmt;
use std::io;

/// Main error type for the USB installer node application
#[derive(Debug)]
pub enum Error {
    /// Configuration errors
    Config(ConfigError),
    /// Network subsystem errors
    Network(NetworkError),
    /// Disk operations errors
    Disk(DiskError),
    /// ISO handling errors
    Iso(IsoError),
    /// Remote access errors
    Remote(RemoteError),
    /// Service management errors
    Service(ServiceError),
    /// UI errors
    Ui(UiError),
    /// Monitoring errors
    Monitoring(MonitoringError),
    /// I/O errors
    Io(io::Error),
    /// General errors
    General(String),
}

#[derive(Debug)]
pub enum ConfigError {
    /// Failed to read configuration file
    ReadFailed(io::Error),
    /// Failed to parse configuration
    ParseFailed(String),
    /// Invalid configuration value
    ValidationFailed(String),
    /// Missing required field
    MissingField(String),
    /// Environment variable error
    EnvVarError(String),
}

#[derive(Debug)]
pub enum NetworkError {
    /// DHCP configuration failed
    DhcpFailed(String),
    /// Hostname configuration failed
    HostnameFailed(String),
    /// Tunnel setup failed
    TunnelFailed(String),
    /// Interface not found
    InterfaceNotFound(String),
    /// Network state transition error
    StateTransitionError(String),
    /// Link down
    LinkDown(String),
}

#[derive(Debug)]
pub enum DiskError {
    /// Partition operation failed
    PartitionFailed(String),
    /// Format operation failed
    FormatFailed(String),
    /// Disk not found
    DiskNotFound(String),
    /// Invalid partition layout
    InvalidLayout(String),
    /// Insufficient space
    InsufficientSpace(u64, u64),
    /// Operation not atomic
    NonAtomicOperation(String),
}

#[derive(Debug)]
pub enum IsoError {
    /// ISO not found
    NotFound(String),
    /// Mount failed
    MountFailed(String),
    /// Unmount failed
    UnmountFailed(String),
    /// Invalid ISO format
    InvalidFormat(String),
    /// Installer not found in ISO
    InstallerNotFound(String),
    /// Installer execution failed
    InstallerFailed(String),
}

#[derive(Debug)]
pub enum RemoteError {
    /// VNC server error
    VncError(String),
    /// SSH server error
    SshError(String),
    /// Web VNC error
    WebVncError(String),
    /// Authentication failed
    AuthFailed(String),
    /// Process spawn failed
    ProcessFailed(String),
    /// Key generation failed
    KeyGenerationFailed(String),
    /// Certificate error
    CertificateError(String),
}

#[derive(Debug)]
pub enum ServiceError {
    /// Service installation failed
    InstallFailed(String),
    /// Service removal failed
    RemoveFailed(String),
    /// Service start failed
    StartFailed(String),
    /// Invalid service configuration
    InvalidConfig(String),
    /// Platform not supported
    PlatformNotSupported(String),
}

#[derive(Debug)]
pub enum UiError {
    /// GUI initialization failed
    InitFailed(String),
    /// Render error
    RenderError(String),
    /// Input handling error
    InputError(String),
    /// State sync error
    StateSyncError(String),
    /// GUI crash
    GuiCrash(String),
}

#[derive(Debug)]
pub enum MonitoringError {
    /// Watchdog error
    WatchdogError(String),
    /// Metrics collection failed
    MetricsError(String),
    /// Alert dispatch failed
    AlertError(String),
    /// Recovery failed
    RecoveryFailed(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Config(e) => write!(f, "Configuration error: {}", e),
            Error::Network(e) => write!(f, "Network error: {}", e),
            Error::Disk(e) => write!(f, "Disk error: {}", e),
            Error::Iso(e) => write!(f, "ISO error: {}", e),
            Error::Remote(e) => write!(f, "Remote access error: {}", e),
            Error::Service(e) => write!(f, "Service error: {}", e),
            Error::Ui(e) => write!(f, "UI error: {}", e),
            Error::Monitoring(e) => write!(f, "Monitoring error: {}", e),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::General(msg) => write!(f, "General error: {}", msg),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::ReadFailed(e) => write!(f, "Failed to read config: {}", e),
            ConfigError::ParseFailed(msg) => write!(f, "Failed to parse config: {}", msg),
            ConfigError::ValidationFailed(msg) => write!(f, "Config validation failed: {}", msg),
            ConfigError::MissingField(field) => write!(f, "Missing required field: {}", field),
            ConfigError::EnvVarError(msg) => write!(f, "Environment variable error: {}", msg),
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::DhcpFailed(msg) => write!(f, "DHCP failed: {}", msg),
            NetworkError::HostnameFailed(msg) => write!(f, "Hostname setup failed: {}", msg),
            NetworkError::TunnelFailed(msg) => write!(f, "Tunnel setup failed: {}", msg),
            NetworkError::InterfaceNotFound(iface) => write!(f, "Interface not found: {}", iface),
            NetworkError::StateTransitionError(msg) => write!(f, "State transition error: {}", msg),
            NetworkError::LinkDown(iface) => write!(f, "Link down: {}", iface),
        }
    }
}

impl fmt::Display for DiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiskError::PartitionFailed(msg) => write!(f, "Partition failed: {}", msg),
            DiskError::FormatFailed(msg) => write!(f, "Format failed: {}", msg),
            DiskError::DiskNotFound(disk) => write!(f, "Disk not found: {}", disk),
            DiskError::InvalidLayout(msg) => write!(f, "Invalid layout: {}", msg),
            DiskError::InsufficientSpace(need, have) => {
                write!(f, "Insufficient space: need {} bytes, have {} bytes", need, have)
            }
            DiskError::NonAtomicOperation(msg) => write!(f, "Non-atomic operation: {}", msg),
        }
    }
}

impl fmt::Display for IsoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IsoError::NotFound(path) => write!(f, "ISO not found: {}", path),
            IsoError::MountFailed(msg) => write!(f, "Mount failed: {}", msg),
            IsoError::UnmountFailed(msg) => write!(f, "Unmount failed: {}", msg),
            IsoError::InvalidFormat(msg) => write!(f, "Invalid ISO format: {}", msg),
            IsoError::InstallerNotFound(msg) => write!(f, "Installer not found: {}", msg),
            IsoError::InstallerFailed(msg) => write!(f, "Installer failed: {}", msg),
        }
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteError::VncError(msg) => write!(f, "VNC error: {}", msg),
            RemoteError::SshError(msg) => write!(f, "SSH error: {}", msg),
            RemoteError::WebVncError(msg) => write!(f, "Web VNC error: {}", msg),
            RemoteError::AuthFailed(msg) => write!(f, "Authentication failed: {}", msg),
            RemoteError::ProcessFailed(msg) => write!(f, "Process failed: {}", msg),
            RemoteError::KeyGenerationFailed(msg) => write!(f, "Key generation failed: {}", msg),
            RemoteError::CertificateError(msg) => write!(f, "Certificate error: {}", msg),
        }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceError::InstallFailed(msg) => write!(f, "Service install failed: {}", msg),
            ServiceError::RemoveFailed(msg) => write!(f, "Service remove failed: {}", msg),
            ServiceError::StartFailed(msg) => write!(f, "Service start failed: {}", msg),
            ServiceError::InvalidConfig(msg) => write!(f, "Invalid service config: {}", msg),
            ServiceError::PlatformNotSupported(platform) => {
                write!(f, "Platform not supported: {}", platform)
            }
        }
    }
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UiError::InitFailed(msg) => write!(f, "UI init failed: {}", msg),
            UiError::RenderError(msg) => write!(f, "Render error: {}", msg),
            UiError::InputError(msg) => write!(f, "Input error: {}", msg),
            UiError::StateSyncError(msg) => write!(f, "State sync error: {}", msg),
            UiError::GuiCrash(msg) => write!(f, "GUI crash: {}", msg),
        }
    }
}

impl fmt::Display for MonitoringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MonitoringError::WatchdogError(msg) => write!(f, "Watchdog error: {}", msg),
            MonitoringError::MetricsError(msg) => write!(f, "Metrics error: {}", msg),
            MonitoringError::AlertError(msg) => write!(f, "Alert error: {}", msg),
            MonitoringError::RecoveryFailed(msg) => write!(f, "Recovery failed: {}", msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl std::error::Error for ConfigError {}
impl std::error::Error for NetworkError {}
impl std::error::Error for DiskError {}
impl std::error::Error for IsoError {}
impl std::error::Error for RemoteError {}
impl std::error::Error for ServiceError {}
impl std::error::Error for UiError {}
impl std::error::Error for MonitoringError {}

// From implementations
impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<ConfigError> for Error {
    fn from(err: ConfigError) -> Self {
        Error::Config(err)
    }
}

impl From<NetworkError> for Error {
    fn from(err: NetworkError) -> Self {
        Error::Network(err)
    }
}

impl From<DiskError> for Error {
    fn from(err: DiskError) -> Self {
        Error::Disk(err)
    }
}

impl From<IsoError> for Error {
    fn from(err: IsoError) -> Self {
        Error::Iso(err)
    }
}

impl From<RemoteError> for Error {
    fn from(err: RemoteError) -> Self {
        Error::Remote(err)
    }
}

impl From<ServiceError> for Error {
    fn from(err: ServiceError) -> Self {
        Error::Service(err)
    }
}

impl From<UiError> for Error {
    fn from(err: UiError) -> Self {
        Error::Ui(err)
    }
}

impl From<MonitoringError> for Error {
    fn from(err: MonitoringError) -> Self {
        Error::Monitoring(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;