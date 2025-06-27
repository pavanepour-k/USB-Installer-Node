use crate::error::{IsoError, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallerState {
    Idle,
    Discovering,
    Ready,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct InstallerInfo {
    pub name: String,
    pub path: PathBuf,
    pub os_type: String,
    pub version: Option<String>,
    pub auto_installable: bool,
}

#[derive(Debug, Clone)]
pub struct InstallerProgress {
    pub percentage: u8,
    pub message: String,
    pub stage: String,
}

pub struct IsoInstaller {
    state: Arc<RwLock<InstallerState>>,
    current_installer: Arc<RwLock<Option<InstallerInfo>>>,
    process: Arc<RwLock<Option<Child>>>,
    progress_tx: Arc<RwLock<Option<mpsc::Sender<InstallerProgress>>>>,
}

impl IsoInstaller {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(InstallerState::Idle)),
            current_installer: Arc::new(RwLock::new(None)),
            process: Arc::new(RwLock::new(None)),
            progress_tx: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn discover_installer(&self, mount_path: &Path) -> Result<Vec<InstallerInfo>> {
        info!("Discovering installers in {}", mount_path.display());
        self.set_state(InstallerState::Discovering).await;

        let installers = self.scan_for_installers(mount_path).await?;
        
        if installers.is_empty() {
            warn!("No installers found in {}", mount_path.display());
            self.set_state(InstallerState::Failed("No installers found".to_string())).await;
        } else {
            info!("Found {} installers", installers.len());
            self.set_state(InstallerState::Ready).await;
        }

        Ok(installers)
    }

    async fn scan_for_installers(&self, mount_path: &Path) -> Result<Vec<InstallerInfo>> {
        let mut installers = Vec::new();

        let debian_installer = mount_path.join("install.amd");
        if debian_installer.exists() {
            installers.push(InstallerInfo {
                name: "Debian Installer".to_string(),
                path: debian_installer,
                os_type: "debian".to_string(),
                version: self.detect_debian_version(mount_path).await,
                auto_installable: true,
            });
        }

        let ubuntu_installer = mount_path.join("casper");
        if ubuntu_installer.exists() {
            installers.push(InstallerInfo {
                name: "Ubuntu Installer".to_string(),
                path: mount_path.to_path_buf(),
                os_type: "ubuntu".to_string(),
                version: self.detect_ubuntu_version(mount_path).await,
                auto_installable: true,
            });
        }

        let windows_installer = mount_path.join("setup.exe");
        if windows_installer.exists() {
            installers.push(InstallerInfo {
                name: "Windows Installer".to_string(),
                path: windows_installer,
                os_type: "windows".to_string(),
                version: self.detect_windows_version(mount_path).await,
                auto_installable: false,
            });
        }

        let bsd_installer = mount_path.join("bsdinstall");
        if bsd_installer.exists() {
            installers.push(InstallerInfo {
                name: "BSD Installer".to_string(),
                path: bsd_installer,
                os_type: "bsd".to_string(),
                version: None,
                auto_installable: true,
            });
        }

        Ok(installers)
    }

    pub async fn start_installer(
        &self,
        installer: &InstallerInfo,
        auto_mode: bool,
        progress_rx: mpsc::Receiver<InstallerProgress>,
    ) -> Result<()> {
        info!("Starting installer: {}", installer.name);
        
        if self.get_state().await != InstallerState::Ready {
            return Err(IsoError::InvalidState("Installer not ready".to_string()));
        }

        self.set_state(InstallerState::Running).await;
        *self.current_installer.write().await = Some(installer.clone());
        
        let (tx, mut rx) = mpsc::channel(100);
        *self.progress_tx.write().await = Some(tx);

        let result = match installer.os_type.as_str() {
            "debian" => self.run_debian_installer(installer, auto_mode).await,
            "ubuntu" => self.run_ubuntu_installer(installer, auto_mode).await,
            "windows" => self.run_windows_installer(installer).await,
            "bsd" => self.run_bsd_installer(installer, auto_mode).await,
            _ => Err(IsoError::UnsupportedInstaller(installer.os_type.clone())),
        };

        match result {
            Ok(_) => {
                self.set_state(InstallerState::Completed).await;
                info!("Installer completed successfully");
            }
            Err(e) => {
                self.set_state(InstallerState::Failed(e.to_string())).await;
                error!("Installer failed: {}", e);
            }
        }

        result
    }

    async fn run_debian_installer(&self, installer: &InstallerInfo, auto_mode: bool) -> Result<()> {
        let mut cmd = Command::new("debian-installer");
        cmd.current_dir(&installer.path);
        
        if auto_mode {
            cmd.arg("--auto");
            cmd.arg("--priority=critical");
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| IsoError::InstallerFailed(format!("Failed to start Debian installer: {}", e)))?;

        self.monitor_process(&mut child).await?;
        Ok(())
    }

    async fn run_ubuntu_installer(&self, installer: &InstallerInfo, auto_mode: bool) -> Result<()> {
        let mut cmd = Command::new("ubiquity");
        cmd.current_dir(&installer.path);
        
        if auto_mode {
            cmd.arg("--automatic");
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| IsoError::InstallerFailed(format!("Failed to start Ubuntu installer: {}", e)))?;

        self.monitor_process(&mut child).await?;
        Ok(())
    }

    async fn run_windows_installer(&self, installer: &InstallerInfo) -> Result<()> {
        let mut cmd = Command::new(&installer.path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| IsoError::InstallerFailed(format!("Failed to start Windows installer: {}", e)))?;

        self.monitor_process(&mut child).await?;
        Ok(())
    }

    async fn run_bsd_installer(&self, installer: &InstallerInfo, auto_mode: bool) -> Result<()> {
        let mut cmd = Command::new(&installer.path);
        
        if auto_mode {
            cmd.arg("-s");
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| IsoError::InstallerFailed(format!("Failed to start BSD installer: {}", e)))?;

        self.monitor_process(&mut child).await?;
        Ok(())
    }

    async fn monitor_process(&self, child: &mut Child) -> Result<()> {
        *self.process.write().await = Some(child.try_into().map_err(|_| IsoError::ProcessError)?);

        let status = child.wait()
            .map_err(|e| IsoError::InstallerFailed(format!("Process wait failed: {}", e)))?;

        *self.process.write().await = None;

        if !status.success() {
            return Err(IsoError::InstallerFailed(format!("Installer exited with status: {}", status)));
        }

        Ok(())
    }

    pub async fn cancel_installer(&self) -> Result<()> {
        if let Some(mut process) = self.process.write().await.take() {
            process.kill()
                .map_err(|e| IsoError::ProcessError)?;
            
            self.set_state(InstallerState::Cancelled).await;
            info!("Installer cancelled");
        }
        Ok(())
    }

    pub async fn get_state(&self) -> InstallerState {
        self.state.read().await.clone()
    }

    async fn set_state(&self, state: InstallerState) {
        *self.state.write().await = state;
    }

    pub async fn get_current_installer(&self) -> Option<InstallerInfo> {
        self.current_installer.read().await.clone()
    }

    async fn detect_debian_version(&self, mount_path: &Path) -> Option<String> {
        let version_file = mount_path.join(".disk/info");
        if let Ok(content) = tokio::fs::read_to_string(version_file).await {
            content.lines().next().map(|s| s.to_string())
        } else {
            None
        }
    }

    async fn detect_ubuntu_version(&self, mount_path: &Path) -> Option<String> {
        let version_file = mount_path.join(".disk/info");
        if let Ok(content) = tokio::fs::read_to_string(version_file).await {
            content.lines().next().map(|s| s.to_string())
        } else {
            None
        }
    }

    async fn detect_windows_version(&self, mount_path: &Path) -> Option<String> {
        let sources = mount_path.join("sources");
        if sources.exists() {
            Some("Windows Installation Media".to_string())
        } else {
            None
        }
    }

    pub async fn validate_installer(&self, installer: &InstallerInfo) -> Result<bool> {
        if !installer.path.exists() {
            return Ok(false);
        }

        match installer.os_type.as_str() {
            "debian" | "ubuntu" => {
                let required_files = ["dists", "pool"];
                let parent = installer.path.parent().unwrap_or(&installer.path);
                Ok(required_files.iter().all(|f| parent.join(f).exists()))
            }
            "windows" => {
                let parent = installer.path.parent().unwrap_or(&installer.path);
                Ok(parent.join("sources").exists() && parent.join("boot").exists())
            }
            "bsd" => Ok(true),
            _ => Ok(false),
        }
    }
}

impl Default for IsoInstaller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_installer_creation() {
        let installer = IsoInstaller::new();
        assert_eq!(installer.get_state().await, InstallerState::Idle);
        assert!(installer.get_current_installer().await.is_none());
    }

    #[tokio::test]
    async fn test_discover_empty_directory() {
        let installer = IsoInstaller::new();
        let temp_dir = TempDir::new().unwrap();
        
        let result = installer.discover_installer(temp_dir.path()).await.unwrap();
        assert!(result.is_empty());
        assert_eq!(installer.get_state().await, InstallerState::Failed("No installers found".to_string()));
    }

    #[tokio::test]
    async fn test_state_transitions() {
        let installer = IsoInstaller::new();
        
        installer.set_state(InstallerState::Discovering).await;
        assert_eq!(installer.get_state().await, InstallerState::Discovering);
        
        installer.set_state(InstallerState::Ready).await;
        assert_eq!(installer.get_state().await, InstallerState::Ready);
        
        installer.set_state(InstallerState::Running).await;
        assert_eq!(installer.get_state().await, InstallerState::Running);
    }

    #[tokio::test]
    async fn test_installer_info_validation() {
        let installer = IsoInstaller::new();
        let temp_dir = TempDir::new().unwrap();
        
        let info = InstallerInfo {
            name: "Test Installer".to_string(),
            path: temp_dir.path().join("nonexistent"),
            os_type: "debian".to_string(),
            version: None,
            auto_installable: true,
        };
        
        assert!(!installer.validate_installer(&info).await.unwrap());
    }
}