use crate::error::{DiskError, Result};
use std::collections::HashMap;
use std::process::Command;
use tracing::{debug, error, info, warn};

/// Supported file system types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSystemType {
    Ext4,
    Ext3,
    Ext2,
    Xfs,
    Btrfs,
    Vfat,
    Ntfs,
    F2fs,
}

impl FileSystemType {
    /// Get the mkfs command for this file system type
    fn mkfs_command(&self) -> &'static str {
        match self {
            Self::Ext4 => "mkfs.ext4",
            Self::Ext3 => "mkfs.ext3",
            Self::Ext2 => "mkfs.ext2",
            Self::Xfs => "mkfs.xfs",
            Self::Btrfs => "mkfs.btrfs",
            Self::Vfat => "mkfs.vfat",
            Self::Ntfs => "mkfs.ntfs",
            Self::F2fs => "mkfs.f2fs",
        }
    }

    /// Parse file system type from string
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "ext4" => Ok(Self::Ext4),
            "ext3" => Ok(Self::Ext3),
            "ext2" => Ok(Self::Ext2),
            "xfs" => Ok(Self::Xfs),
            "btrfs" => Ok(Self::Btrfs),
            "vfat" | "fat32" => Ok(Self::Vfat),
            "ntfs" => Ok(Self::Ntfs),
            "f2fs" => Ok(Self::F2fs),
            _ => Err(DiskError::InvalidFileSystem(s.to_string())),
        }
    }
}

/// Format parameters for a partition
#[derive(Debug, Clone)]
pub struct FormatParams {
    /// Target device path (e.g., /dev/sda1)
    pub device: String,
    /// File system type
    pub fs_type: FileSystemType,
    /// Volume label (optional)
    pub label: Option<String>,
    /// UUID (optional, auto-generated if not specified)
    pub uuid: Option<String>,
    /// Additional mkfs options
    pub extra_options: Vec<String>,
    /// Force format without confirmation
    pub force: bool,
}

impl FormatParams {
    /// Create new format parameters with defaults
    pub fn new(device: String, fs_type: FileSystemType) -> Self {
        Self {
            device,
            fs_type,
            label: None,
            uuid: None,
            extra_options: Vec::new(),
            force: false,
        }
    }

    /// Set volume label
    pub fn with_label(mut self, label: String) -> Self {
        self.label = Some(label);
        self
    }

    /// Set UUID
    pub fn with_uuid(mut self, uuid: String) -> Self {
        self.uuid = Some(uuid);
        self
    }

    /// Add extra mkfs option
    pub fn add_option(mut self, option: String) -> Self {
        self.extra_options.push(option);
        self
    }

    /// Enable force mode
    pub fn force(mut self) -> Self {
        self.force = true;
        self
    }
}

/// Disk formatter implementation
pub struct DiskFormatter;

impl DiskFormatter {
    /// Create a new disk formatter
    pub fn new() -> Self {
        Self
    }

    /// Format a partition with the specified parameters
    pub fn format(&self, params: &FormatParams) -> Result<()> {
        info!(
            "Formatting {} as {:?}",
            params.device, params.fs_type
        );

        // Validate device exists
        self.validate_device(&params.device)?;

        // Check if device is mounted
        if self.is_mounted(&params.device)? {
            return Err(DiskError::DeviceMounted(params.device.clone()));
        }

        // Build mkfs command
        let mut cmd = Command::new(params.fs_type.mkfs_command());

        // Add file system specific options
        self.add_fs_options(&mut cmd, params)?;

        // Add common options
        if params.force {
            match params.fs_type {
                FileSystemType::Ext4 | FileSystemType::Ext3 | FileSystemType::Ext2 => {
                    cmd.arg("-F");
                }
                FileSystemType::Xfs => {
                    cmd.arg("-f");
                }
                FileSystemType::Btrfs => {
                    cmd.arg("-f");
                }
                FileSystemType::Ntfs => {
                    cmd.arg("-F");
                }
                _ => {}
            }
        }

        // Add label if specified
        if let Some(label) = &params.label {
            self.add_label_option(&mut cmd, params.fs_type, label);
        }

        // Add UUID if specified
        if let Some(uuid) = &params.uuid {
            self.add_uuid_option(&mut cmd, params.fs_type, uuid)?;
        }

        // Add extra options
        for option in &params.extra_options {
            cmd.arg(option);
        }

        // Add device path
        cmd.arg(&params.device);

        debug!("Executing format command: {:?}", cmd);

        // Execute format command
        let output = cmd.output().map_err(|e| {
            error!("Failed to execute mkfs command: {}", e);
            DiskError::CommandFailed(format!("mkfs execution failed: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("Format failed: {}", stderr);
            return Err(DiskError::FormatFailed(params.device.clone(), stderr.to_string()));
        }

        info!("Successfully formatted {}", params.device);

        // Verify format
        self.verify_format(&params.device, params.fs_type)?;

        Ok(())
    }

    /// Format multiple partitions
    pub fn format_batch(&self, partitions: &[FormatParams]) -> Result<Vec<Result<()>>> {
        let mut results = Vec::new();

        for params in partitions {
            info!("Batch formatting: {}", params.device);
            let result = self.format(params);
            
            if let Err(ref e) = result {
                warn!("Failed to format {}: {}", params.device, e);
            }
            
            results.push(result);
        }

        Ok(results)
    }

    /// Validate device exists and is a block device
    fn validate_device(&self, device: &str) -> Result<()> {
        use std::path::Path;

        let path = Path::new(device);
        if !path.exists() {
            return Err(DiskError::DeviceNotFound(device.to_string()));
        }

        let metadata = std::fs::metadata(device).map_err(|e| {
            DiskError::IoError(format!("Failed to get device metadata: {}", e))
        })?;

        if !metadata.file_type().is_block_device() {
            return Err(DiskError::InvalidDevice(device.to_string()));
        }

        Ok(())
    }

    /// Check if device is mounted
    fn is_mounted(&self, device: &str) -> Result<bool> {
        let output = Command::new("findmnt")
            .args(&["-n", "-o", "SOURCE", device])
            .output()
            .map_err(|e| DiskError::CommandFailed(format!("findmnt failed: {}", e)))?;

        Ok(output.status.success() && !output.stdout.is_empty())
    }

    /// Add file system specific options
    fn add_fs_options(&self, cmd: &mut Command, params: &FormatParams) -> Result<()> {
        match params.fs_type {
            FileSystemType::Ext4 => {
                cmd.args(&["-t", "ext4"]);
            }
            FileSystemType::Xfs => {
                cmd.arg("-q");
            }
            FileSystemType::Btrfs => {
                cmd.arg("-q");
            }
            FileSystemType::Ntfs => {
                cmd.arg("-Q");
            }
            _ => {}
        }
        Ok(())
    }

    /// Add label option based on file system type
    fn add_label_option(&self, cmd: &mut Command, fs_type: FileSystemType, label: &str) {
        match fs_type {
            FileSystemType::Ext4 | FileSystemType::Ext3 | FileSystemType::Ext2 => {
                cmd.args(&["-L", label]);
            }
            FileSystemType::Xfs => {
                cmd.args(&["-L", label]);
            }
            FileSystemType::Btrfs => {
                cmd.args(&["-L", label]);
            }
            FileSystemType::Vfat => {
                cmd.args(&["-n", label]);
            }
            FileSystemType::Ntfs => {
                cmd.args(&["-L", label]);
            }
            FileSystemType::F2fs => {
                cmd.args(&["-l", label]);
            }
        }
    }

    /// Add UUID option based on file system type
    fn add_uuid_option(&self, cmd: &mut Command, fs_type: FileSystemType, uuid: &str) -> Result<()> {
        self.validate_uuid(uuid)?;

        match fs_type {
            FileSystemType::Ext4 | FileSystemType::Ext3 | FileSystemType::Ext2 => {
                cmd.args(&["-U", uuid]);
            }
            FileSystemType::Xfs => {
                cmd.args(&["-m", &format!("uuid={}", uuid)]);
            }
            FileSystemType::Btrfs => {
                cmd.args(&["-U", uuid]);
            }
            FileSystemType::Vfat => {
                // FAT32 uses volume ID, not UUID
                warn!("FAT32 does not support UUID, ignoring");
            }
            FileSystemType::Ntfs => {
                warn!("NTFS UUID setting not supported by mkfs.ntfs");
            }
            FileSystemType::F2fs => {
                cmd.args(&["-U", uuid]);
            }
        }
        Ok(())
    }

    /// Validate UUID format
    fn validate_uuid(&self, uuid: &str) -> Result<()> {
        let uuid_regex = regex::Regex::new(
            r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"
        ).map_err(|e| DiskError::InvalidParameter(format!("Invalid regex: {}", e)))?;

        if !uuid_regex.is_match(uuid) {
            return Err(DiskError::InvalidParameter(format!("Invalid UUID format: {}", uuid)));
        }

        Ok(())
    }

    /// Verify the format was successful
    fn verify_format(&self, device: &str, expected_fs: FileSystemType) -> Result<()> {
        let output = Command::new("blkid")
            .args(&["-p", "-o", "value", "-s", "TYPE", device])
            .output()
            .map_err(|e| DiskError::CommandFailed(format!("blkid failed: {}", e)))?;

        if !output.status.success() {
            return Err(DiskError::VerificationFailed(
                device.to_string(),
                "Failed to read file system type".to_string()
            ));
        }

        let detected_fs = String::from_utf8_lossy(&output.stdout).trim().to_string();
        
        let expected_fs_str = match expected_fs {
            FileSystemType::Ext4 => "ext4",
            FileSystemType::Ext3 => "ext3",
            FileSystemType::Ext2 => "ext2",
            FileSystemType::Xfs => "xfs",
            FileSystemType::Btrfs => "btrfs",
            FileSystemType::Vfat => "vfat",
            FileSystemType::Ntfs => "ntfs",
            FileSystemType::F2fs => "f2fs",
        };

        if detected_fs != expected_fs_str {
            return Err(DiskError::VerificationFailed(
                device.to_string(),
                format!("Expected {}, but found {}", expected_fs_str, detected_fs)
            ));
        }

        debug!("Format verification successful for {}", device);
        Ok(())
    }

    /// Get file system information
    pub fn get_fs_info(&self, device: &str) -> Result<HashMap<String, String>> {
        let output = Command::new("blkid")
            .args(&["-p", "-o", "export", device])
            .output()
            .map_err(|e| DiskError::CommandFailed(format!("blkid failed: {}", e)))?;

        if !output.status.success() {
            return Err(DiskError::CommandFailed("Failed to get file system info".to_string()));
        }

        let mut info = HashMap::new();
        let output_str = String::from_utf8_lossy(&output.stdout);

        for line in output_str.lines() {
            if let Some((key, value)) = line.split_once('=') {
                info.insert(key.to_string(), value.to_string());
            }
        }

        Ok(info)
    }
}

impl Default for DiskFormatter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filesystem_type_from_str() {
        assert_eq!(FileSystemType::from_str("ext4").unwrap(), FileSystemType::Ext4);
        assert_eq!(FileSystemType::from_str("EXT4").unwrap(), FileSystemType::Ext4);
        assert_eq!(FileSystemType::from_str("fat32").unwrap(), FileSystemType::Vfat);
        assert_eq!(FileSystemType::from_str("vfat").unwrap(), FileSystemType::Vfat);
        assert!(FileSystemType::from_str("invalid").is_err());
    }

    #[test]
    fn test_format_params_builder() {
        let params = FormatParams::new("/dev/sda1".to_string(), FileSystemType::Ext4)
            .with_label("test-label".to_string())
            .with_uuid("12345678-1234-1234-1234-123456789012".to_string())
            .add_option("-E".to_string())
            .add_option("lazy_itable_init=0".to_string())
            .force();

        assert_eq!(params.device, "/dev/sda1");
        assert_eq!(params.fs_type, FileSystemType::Ext4);
        assert_eq!(params.label, Some("test-label".to_string()));
        assert_eq!(params.uuid, Some("12345678-1234-1234-1234-123456789012".to_string()));
        assert_eq!(params.extra_options.len(), 2);
        assert!(params.force);
    }

    #[test]
    fn test_uuid_validation() {
        let formatter = DiskFormatter::new();
        
        assert!(formatter.validate_uuid("12345678-1234-1234-1234-123456789012").is_ok());
        assert!(formatter.validate_uuid("invalid-uuid").is_err());
        assert!(formatter.validate_uuid("12345678123412341234123456789012").is_err());
        assert!(formatter.validate_uuid("").is_err());
    }
}