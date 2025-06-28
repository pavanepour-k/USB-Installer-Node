pub mod installer;
pub mod mounter;

use crate::config::IsoConfig;
use crate::error::{IsoError, Result};
use installer::{InstallerInfo, InstallerProgress, IsoInstaller};
use mounter::{IsoMounter, MountPoint};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsoManagerState {
    Idle,
    Scanning,
    Mounting,
    Ready,
    Installing,
    Error(String),
}

pub struct IsoManager {
    config: Arc<RwLock<IsoConfig>>,
    state: Arc<RwLock<IsoManagerState>>,
    mounter: Arc<IsoMounter>,
    installer: Arc<IsoInstaller>,
    available_isos: Arc<RwLock<Vec<PathBuf>>>,
    active_iso: Arc<RwLock<Option<PathBuf>>>,
}

impl IsoManager {
    pub fn new(config: Arc<RwLock<IsoConfig>>) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(IsoManagerState::Idle)),
            mounter: Arc::new(IsoMounter::new()),
            installer: Arc::new(IsoInstaller::new()),
            available_isos: Arc::new(RwLock::new(Vec::new())),
            active_iso: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting ISO manager");

        let config = self.config.read().await;
        if !config.enabled {
            info!("ISO management disabled");
            return Ok(());
        }

        if config.auto_scan {
            self.scan_for_isos(&config.iso_paths).await?;
        }

        if config.auto_mount && !self.available_isos.read().await.is_empty() {
            let iso = self.available_isos.read().await[0].clone();
            self.mount_iso(&iso).await?;
        }

        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        info!("Stopping ISO manager");

        if let Err(e) = self.installer.cancel_installer().await {
            warn!("Failed to cancel installer: {}", e);
        }

        let results = self.mounter.unmount_all()?;
        for result in results {
            if let Err(e) = result {
                warn!("Failed to unmount: {}", e);
            }
        }

        self.set_state(IsoManagerState::Idle).await;
        Ok(())
    }

    pub async fn scan_for_isos(&self, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
        info!("Scanning for ISO files");
        self.set_state(IsoManagerState::Scanning).await;

        let mut isos = Vec::new();

        for path in paths {
            if path.is_dir() {
                self.scan_directory(path, &mut isos).await?;
            } else if path.extension().map(|e| e == "iso").unwrap_or(false) {
                isos.push(path.clone());
            }
        }

        info!("Found {} ISO files", isos.len());
        *self.available_isos.write().await = isos.clone();
        self.set_state(IsoManagerState::Idle).await;

        Ok(isos)
    }

    async fn scan_directory(&self, dir: &Path, isos: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| IsoError::IoError(format!("Failed to read directory: {}", e)))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| IsoError::IoError(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            if path.extension().map(|e| e == "iso").unwrap_or(false) {
                isos.push(path);
            }
        }

        Ok(())
    }

    pub async fn mount_iso(&self, iso_path: &Path) -> Result<PathBuf> {
        info!("Mounting ISO: {}", iso_path.display());
        self.set_state(IsoManagerState::Mounting).await;

        let config = self.config.read().await;
        let mount_point = config.mount_point.join(
            iso_path
                .file_stem()
                .ok_or_else(|| IsoError::InvalidIsoFile(iso_path.to_string_lossy().to_string()))?,
        );

        self.mounter
            .mount(iso_path, &mount_point, vec!["ro".to_string()])?;

        *self.active_iso.write().await = Some(iso_path.to_path_buf());
        self.set_state(IsoManagerState::Ready).await;

        Ok(mount_point)
    }

    pub async fn unmount_current(&self) -> Result<()> {
        if let Some(iso) = self.active_iso.write().await.take() {
            self.mounter.unmount(&iso)?;
            self.set_state(IsoManagerState::Idle).await;
        }
        Ok(())
    }

    pub async fn discover_installers(&self) -> Result<Vec<InstallerInfo>> {
        let iso = self
            .active_iso
            .read()
            .await
            .clone()
            .ok_or_else(|| IsoError::NoActiveIso)?;

        let mount_point = self
            .mounter
            .get_mount_point(&iso)?
            .ok_or_else(|| IsoError::NotMounted(iso.to_string_lossy().to_string()))?
            .target;

        self.installer.discover_installer(&mount_point).await
    }

    pub async fn start_installation(
        &self,
        installer: &InstallerInfo,
        auto_mode: bool,
    ) -> Result<mpsc::Receiver<InstallerProgress>> {
        info!("Starting installation with {}", installer.name);
        self.set_state(IsoManagerState::Installing).await;

        let (tx, rx) = mpsc::channel(100);
        let installer_clone = self.installer.clone();
        let installer_info = installer.clone();

        tokio::spawn(async move {
            if let Err(e) = installer_clone
                .start_installer(&installer_info, auto_mode, rx)
                .await
            {
                error!("Installation failed: {}", e);
            }
        });

        Ok(tx)
    }

    pub async fn get_available_isos(&self) -> Vec<PathBuf> {
        self.available_isos.read().await.clone()
    }

    pub async fn get_active_iso(&self) -> Option<PathBuf> {
        self.active_iso.read().await.clone()
    }

    pub async fn get_state(&self) -> IsoManagerState {
        self.state.read().await.clone()
    }

    async fn set_state(&self, state: IsoManagerState) {
        *self.state.write().await = state;
    }

    pub async fn reload_config(&self, config: Arc<RwLock<IsoConfig>>) {
        *self.config.write().await = config.read().await.clone();
    }

    pub async fn health_check(&self) -> Result<()> {
        let state = self.get_state().await;
        match state {
            IsoManagerState::Error(e) => Err(IsoError::HealthCheckFailed(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_iso_manager_creation() {
        let config = Arc::new(RwLock::new(IsoConfig::default()));
        let manager = IsoManager::new(config);
        assert_eq!(manager.get_state().await, IsoManagerState::Idle);
        assert!(manager.get_available_isos().await.is_empty());
        assert!(manager.get_active_iso().await.is_none());
    }

    #[tokio::test]
    async fn test_scan_empty_directory() {
        let config = Arc::new(RwLock::new(IsoConfig::default()));
        let manager = IsoManager::new(config);
        let temp_dir = tempfile::TempDir::new().unwrap();

        let result = manager
            .scan_for_isos(&[temp_dir.path().to_path_buf()])
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_state_transitions() {
        let config = Arc::new(RwLock::new(IsoConfig::default()));
        let manager = IsoManager::new(config);

        manager.set_state(IsoManagerState::Scanning).await;
        assert_eq!(manager.get_state().await, IsoManagerState::Scanning);

        manager.set_state(IsoManagerState::Mounting).await;
        assert_eq!(manager.get_state().await, IsoManagerState::Mounting);

        manager.set_state(IsoManagerState::Ready).await;
        assert_eq!(manager.get_state().await, IsoManagerState::Ready);
    }
}
