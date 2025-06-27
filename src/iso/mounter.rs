use crate::error::{IsoError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct MountPoint {
    pub source: PathBuf,
    pub target: PathBuf,
    pub fs_type: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountState {
    Unmounted,
    Mounting,
    Mounted,
    Unmounting,
    Error(String),
}

pub struct IsoMounter {
    mount_points: Arc<Mutex<HashMap<PathBuf, MountPoint>>>,
    state: Arc<Mutex<HashMap<PathBuf, MountState>>>,
}

impl IsoMounter {
    pub fn new() -> Self {
        Self {
            mount_points: Arc::new(Mutex::new(HashMap::new())),
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn mount(&self, source: &Path, target: &Path, options: Vec<String>) -> Result<()> {
        info!("Mounting {} to {}", source.display(), target.display());

        if !source.exists() {
            return Err(IsoError::FileNotFound(source.to_string_lossy().to_string()));
        }

        if !self.is_iso_file(source)? {
            return Err(IsoError::InvalidIsoFile(source.to_string_lossy().to_string()));
        }

        self.create_mount_point(target)?;

        self.set_state(source, MountState::Mounting)?;

        let mut cmd = Command::new("mount");
        cmd.arg("-o").arg(format!("loop,ro,{}", options.join(",")));
        cmd.arg(source);
        cmd.arg(target);

        debug!("Executing mount command: {:?}", cmd);

        let output = cmd.output().map_err(|e| {
            self.set_state(source, MountState::Error(e.to_string())).ok();
            IsoError::MountFailed(source.to_string_lossy().to_string(), e.to_string())
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.set_state(source, MountState::Error(stderr.to_string())).ok();
            return Err(IsoError::MountFailed(
                source.to_string_lossy().to_string(),
                stderr.to_string(),
            ));
        }

        let mount_point = MountPoint {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
            fs_type: "iso9660".to_string(),
            options,
        };

        self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .insert(source.to_path_buf(), mount_point);

        self.set_state(source, MountState::Mounted)?;

        info!("Successfully mounted {} to {}", source.display(), target.display());
        Ok(())
    }

    pub fn unmount(&self, source: &Path) -> Result<()> {
        info!("Unmounting {}", source.display());

        let mount_point = self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .get(source)
            .cloned()
            .ok_or_else(|| IsoError::NotMounted(source.to_string_lossy().to_string()))?;

        self.set_state(source, MountState::Unmounting)?;

        let mut cmd = Command::new("umount");
        cmd.arg(&mount_point.target);

        let output = cmd.output().map_err(|e| {
            self.set_state(source, MountState::Error(e.to_string())).ok();
            IsoError::UnmountFailed(source.to_string_lossy().to_string(), e.to_string())
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.set_state(source, MountState::Error(stderr.to_string())).ok();
            return Err(IsoError::UnmountFailed(
                source.to_string_lossy().to_string(),
                stderr.to_string(),
            ));
        }

        self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .remove(source);

        self.set_state(source, MountState::Unmounted)?;

        info!("Successfully unmounted {}", source.display());
        Ok(())
    }

    pub fn remount(&self, source: &Path, options: Vec<String>) -> Result<()> {
        let mount_point = self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .get(source)
            .cloned()
            .ok_or_else(|| IsoError::NotMounted(source.to_string_lossy().to_string()))?;

        self.unmount(source)?;
        self.mount(source, &mount_point.target, options)?;
        Ok(())
    }

    pub fn unmount_all(&self) -> Result<Vec<Result<()>>> {
        let sources: Vec<PathBuf> = self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .keys()
            .cloned()
            .collect();

        let mut results = Vec::new();
        for source in sources {
            results.push(self.unmount(&source));
        }
        Ok(results)
    }

    pub fn get_mount_point(&self, source: &Path) -> Result<Option<MountPoint>> {
        Ok(self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .get(source)
            .cloned())
    }

    pub fn list_mounted(&self) -> Result<Vec<MountPoint>> {
        Ok(self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .values()
            .cloned()
            .collect())
    }

    pub fn is_mounted(&self, source: &Path) -> Result<bool> {
        Ok(self.mount_points.lock()
            .map_err(|_| IsoError::LockError)?
            .contains_key(source))
    }

    pub fn get_state(&self, source: &Path) -> Result<MountState> {
        Ok(self.state.lock()
            .map_err(|_| IsoError::LockError)?
            .get(source)
            .cloned()
            .unwrap_or(MountState::Unmounted))
    }

    fn set_state(&self, source: &Path, state: MountState) -> Result<()> {
        self.state.lock()
            .map_err(|_| IsoError::LockError)?
            .insert(source.to_path_buf(), state);
        Ok(())
    }

    fn is_iso_file(&self, path: &Path) -> Result<bool> {
        if let Some(ext) = path.extension() {
            if ext.to_ascii_lowercase() == "iso" {
                return Ok(true);
            }
        }

        let output = Command::new("file")
            .arg("-b")
            .arg(path)
            .output()
            .map_err(|e| IsoError::CommandFailed(format!("file command failed: {}", e)))?;

        let file_type = String::from_utf8_lossy(&output.stdout);
        Ok(file_type.contains("ISO 9660"))
    }

    fn create_mount_point(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            std::fs::create_dir_all(path)
                .map_err(|e| IsoError::IoError(format!("Failed to create mount point: {}", e)))?;
        }
        Ok(())
    }

    pub fn verify_mount(&self, source: &Path) -> Result<bool> {
        let mount_point = self.get_mount_point(source)?
            .ok_or_else(|| IsoError::NotMounted(source.to_string_lossy().to_string()))?;

        let output = Command::new("findmnt")
            .args(&["-n", "-o", "SOURCE,TARGET"])
            .arg(&mount_point.target)
            .output()
            .map_err(|e| IsoError::CommandFailed(format!("findmnt failed: {}", e)))?;

        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            Ok(output_str.contains(&source.to_string_lossy().to_string()))
        } else {
            Ok(false)
        }
    }
}

impl Default for IsoMounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_mounter_creation() {
        let mounter = IsoMounter::new();
        assert!(mounter.list_mounted().unwrap().is_empty());
    }

    #[test]
    fn test_mount_state_tracking() {
        let mounter = IsoMounter::new();
        let source = Path::new("/tmp/test.iso");
        
        assert_eq!(mounter.get_state(source).unwrap(), MountState::Unmounted);
        
        mounter.set_state(source, MountState::Mounting).unwrap();
        assert_eq!(mounter.get_state(source).unwrap(), MountState::Mounting);
        
        mounter.set_state(source, MountState::Mounted).unwrap();
        assert_eq!(mounter.get_state(source).unwrap(), MountState::Mounted);
    }

    #[test]
    fn test_is_mounted() {
        let mounter = IsoMounter::new();
        let source = PathBuf::from("/tmp/test.iso");
        let target = PathBuf::from("/mnt/iso");
        
        assert!(!mounter.is_mounted(&source).unwrap());
        
        let mount_point = MountPoint {
            source: source.clone(),
            target,
            fs_type: "iso9660".to_string(),
            options: vec!["ro".to_string()],
        };
        
        mounter.mount_points.lock().unwrap().insert(source.clone(), mount_point);
        assert!(mounter.is_mounted(&source).unwrap());
    }

    #[test]
    fn test_mount_nonexistent_file() {
        let mounter = IsoMounter::new();
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("nonexistent.iso");
        let target = temp_dir.path().join("mount");
        
        let result = mounter.mount(&source, &target, vec![]);
        assert!(result.is_err());
        if let Err(IsoError::FileNotFound(_)) = result {
        } else {
            panic!("Expected FileNotFound error");
        }
    }
}