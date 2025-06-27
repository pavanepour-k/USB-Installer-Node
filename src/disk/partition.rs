use crate::error::{Result, UsbNodeError};
use log::{debug, error, info, warn};
use std::process::Command;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum PartitionType {
    Primary,
    Extended,
    Logical,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PartitionScheme {
    Mbr,
    Gpt,
}

#[derive(Debug, Clone)]
pub struct PartitionSpec {
    pub size_mb: u64,
    pub partition_type: PartitionType,
    pub filesystem_type: Option<String>,
    pub label: Option<String>,
    pub bootable: bool,
}

#[derive(Debug, Clone)]
pub struct PartitionInfo {
    pub device: String,
    pub number: u32,
    pub start_sector: u64,
    pub end_sector: u64,
    pub size_mb: u64,
    pub filesystem: Option<String>,
    pub label: Option<String>,
    pub bootable: bool,
}

pub struct PartitionManager {
    device: String,
    scheme: PartitionScheme,
}

impl PartitionManager {
    pub fn new(device: String, scheme: PartitionScheme) -> Self {
        Self { device, scheme }
    }

    pub async fn create_partition_table(&self) -> Result<()> {
        info!("Creating {} partition table on {}", 
              match self.scheme { PartitionScheme::Mbr => "MBR", PartitionScheme::Gpt => "GPT" },
              self.device);

        self.validate_device()?;
        self.unmount_all_partitions().await?;

        let label_type = match self.scheme {
            PartitionScheme::Mbr => "msdos",
            PartitionScheme::Gpt => "gpt",
        };

        let output = Command::new("parted")
            .arg("-s")
            .arg(&self.device)
            .arg("mklabel")
            .arg(label_type)
            .output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to execute parted: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsbNodeError::Disk(format!("Failed to create partition table: {}", stderr)));
        }

        info!("Partition table created successfully");
        Ok(())
    }

    pub async fn create_partition(&self, spec: &PartitionSpec) -> Result<u32> {
        info!("Creating partition: {} MB", spec.size_mb);

        let partitions = self.list_partitions().await?;
        let partition_number = partitions.len() as u32 + 1;

        let start = if partitions.is_empty() {
            "1MiB"
        } else {
            return Err(UsbNodeError::Disk("Multiple partition creation not implemented".to_string()));
        };

        let end = format!("{}MiB", spec.size_mb);

        let mut cmd = Command::new("parted");
        cmd.arg("-s")
           .arg(&self.device)
           .arg("mkpart");

        match self.scheme {
            PartitionScheme::Mbr => {
                let part_type = match spec.partition_type {
                    PartitionType::Primary => "primary",
                    PartitionType::Extended => "extended",
                    PartitionType::Logical => "logical",
                };
                cmd.arg(part_type);
            }
            PartitionScheme::Gpt => {
                if let Some(ref label) = spec.label {
                    cmd.arg(label);
                } else {
                    cmd.arg("partition");
                }
            }
        }

        if let Some(ref fs_type) = spec.filesystem_type {
            cmd.arg(fs_type);
        }

        cmd.arg(start).arg(end);

        let output = cmd.output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to execute parted: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsbNodeError::Disk(format!("Failed to create partition: {}", stderr)));
        }

        if spec.bootable {
            self.set_bootable(partition_number).await?;
        }

        info!("Partition {} created successfully", partition_number);
        Ok(partition_number)
    }

    pub async fn delete_partition(&self, partition_number: u32) -> Result<()> {
        info!("Deleting partition {}", partition_number);

        let partition_device = format!("{}{}", self.device, partition_number);
        self.unmount_partition(&partition_device).await?;

        let output = Command::new("parted")
            .arg("-s")
            .arg(&self.device)
            .arg("rm")
            .arg(partition_number.to_string())
            .output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to execute parted: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsbNodeError::Disk(format!("Failed to delete partition: {}", stderr)));
        }

        info!("Partition {} deleted successfully", partition_number);
        Ok(())
    }

    pub async fn list_partitions(&self) -> Result<Vec<PartitionInfo>> {
        let output = Command::new("parted")
            .arg("-s")
            .arg(&self.device)
            .arg("print")
            .output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to execute parted: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsbNodeError::Disk(format!("Failed to list partitions: {}", stderr)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        self.parse_partition_list(&stdout)
    }

    pub async fn resize_partition(&self, partition_number: u32, new_size_mb: u64) -> Result<()> {
        info!("Resizing partition {} to {} MB", partition_number, new_size_mb);

        let partition_device = format!("{}{}", self.device, partition_number);
        self.unmount_partition(&partition_device).await?;

        let output = Command::new("parted")
            .arg("-s")
            .arg(&self.device)
            .arg("resizepart")
            .arg(partition_number.to_string())
            .arg(format!("{}MiB", new_size_mb))
            .output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to execute parted: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsbNodeError::Disk(format!("Failed to resize partition: {}", stderr)));
        }

        info!("Partition {} resized successfully", partition_number);
        Ok(())
    }

    async fn set_bootable(&self, partition_number: u32) -> Result<()> {
        let output = Command::new("parted")
            .arg("-s")
            .arg(&self.device)
            .arg("set")
            .arg(partition_number.to_string())
            .arg("boot")
            .arg("on")
            .output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to execute parted: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(UsbNodeError::Disk(format!("Failed to set bootable flag: {}", stderr)));
        }

        Ok(())
    }

    fn validate_device(&self) -> Result<()> {
        if !Path::new(&self.device).exists() {
            return Err(UsbNodeError::Disk(format!("Device {} does not exist", self.device)));
        }

        let metadata = std::fs::metadata(&self.device)
            .map_err(|e| UsbNodeError::Disk(format!("Failed to get device metadata: {}", e)))?;

        if !metadata.file_type().is_block_device() {
            return Err(UsbNodeError::Disk(format!("{} is not a block device", self.device)));
        }

        Ok(())
    }

    async fn unmount_all_partitions(&self) -> Result<()> {
        let output = Command::new("lsblk")
            .arg("-ln")
            .arg("-o")
            .arg("NAME,MOUNTPOINT")
            .arg(&self.device)
            .output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to list mounted partitions: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && !parts[1].is_empty() {
                let device_name = format!("/dev/{}", parts[0]);
                self.unmount_partition(&device_name).await?;
            }
        }

        Ok(())
    }

    async fn unmount_partition(&self, partition_device: &str) -> Result<()> {
        debug!("Unmounting partition {}", partition_device);

        let output = Command::new("umount")
            .arg(partition_device)
            .output()
            .map_err(|e| UsbNodeError::Disk(format!("Failed to execute umount: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to unmount {}: {}", partition_device, stderr);
        }

        Ok(())
    }

    fn parse_partition_list(&self, output: &str) -> Result<Vec<PartitionInfo>> {
        let mut partitions = Vec::new();
        let mut in_partition_section = false;

        for line in output.lines() {
            if line.starts_with("Number") {
                in_partition_section = true;
                continue;
            }

            if !in_partition_section || line.trim().is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let number = parts[0].parse::<u32>()
                    .map_err(|_| UsbNodeError::Disk("Invalid partition number".to_string()))?;

                let start_str = parts[1].trim_end_matches("B");
                let end_str = parts[2].trim_end_matches("B");
                let size_str = parts[3].trim_end_matches("B");

                let start_sector = self.parse_size_to_sectors(start_str)?;
                let end_sector = self.parse_size_to_sectors(end_str)?;
                let size_mb = self.parse_size_to_mb(size_str)?;

                let filesystem = if parts.len() > 4 && !parts[4].is_empty() {
                    Some(parts[4].to_string())
                } else {
                    None
                };

                let label = if parts.len() > 5 && !parts[5].is_empty() {
                    Some(parts[5].to_string())
                } else {
                    None
                };

                let bootable = parts.len() > 6 && parts[6].contains("boot");

                partitions.push(PartitionInfo {
                    device: format!("{}{}", self.device, number),
                    number,
                    start_sector,
                    end_sector,
                    size_mb,
                    filesystem,
                    label,
                    bootable,
                });
            }
        }

        Ok(partitions)
    }

    fn parse_size_to_sectors(&self, size_str: &str) -> Result<u64> {
        if size_str.ends_with("s") {
            size_str.trim_end_matches("s").parse::<u64>()
                .map_err(|_| UsbNodeError::Disk("Invalid sector count".to_string()))
        } else {
            let size_bytes = self.parse_size_to_bytes(size_str)?;
            Ok(size_bytes / 512) // Assume 512-byte sectors
        }
    }

    fn parse_size_to_mb(&self, size_str: &str) -> Result<u64> {
        let size_bytes = self.parse_size_to_bytes(size_str)?;
        Ok(size_bytes / (1024 * 1024))
    }

    fn parse_size_to_bytes(&self, size_str: &str) -> Result<u64> {
        let size_str = size_str.trim();
        
        if let Some(num_str) = size_str.strip_suffix("TB") {
            let num: f64 = num_str.parse()
                .map_err(|_| UsbNodeError::Disk("Invalid size format".to_string()))?;
            Ok((num * 1024.0 * 1024.0 * 1024.0 * 1024.0) as u64)
        } else if let Some(num_str) = size_str.strip_suffix("GB") {
            let num: f64 = num_str.parse()
                .map_err(|_| UsbNodeError::Disk("Invalid size format".to_string()))?;
            Ok((num * 1024.0 * 1024.0 * 1024.0) as u64)
        } else if let Some(num_str) = size_str.strip_suffix("MB") {
            let num: f64 = num_str.parse()
                .map_err(|_| UsbNodeError::Disk("Invalid size format".to_string()))?;
            Ok((num * 1024.0 * 1024.0) as u64)
        } else if let Some(num_str) = size_str.strip_suffix("KB") {
            let num: f64 = num_str.parse()
                .map_err(|_| UsbNodeError::Disk("Invalid size format".to_string()))?;
            Ok((num * 1024.0) as u64)
        } else if let Some(num_str) = size_str.strip_suffix("kB") {
            let num: f64 = num_str.parse()
                .map_err(|_| UsbNodeError::Disk("Invalid size format".to_string()))?;
            Ok((num * 1000.0) as u64)
        } else {
            size_str.parse::<u64>()
                .map_err(|_| UsbNodeError::Disk("Invalid size format".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_spec_creation() {
        let spec = PartitionSpec {
            size_mb: 1024,
            partition_type: PartitionType::Primary,
            filesystem_type: Some("ext4".to_string()),
            label: Some("root".to_string()),
            bootable: true,
        };

        assert_eq!(spec.size_mb, 1024);
        assert_eq!(spec.partition_type, PartitionType::Primary);
        assert_eq!(spec.filesystem_type, Some("ext4".to_string()));
        assert_eq!(spec.label, Some("root".to_string()));
        assert!(spec.bootable);
    }

    #[test]
    fn test_partition_manager_creation() {
        let manager = PartitionManager::new("/dev/sdb".to_string(), PartitionScheme::Gpt);
        assert_eq!(manager.device, "/dev/sdb");
        assert_eq!(manager.scheme, PartitionScheme::Gpt);
    }

    #[test]
    fn test_parse_size_to_bytes() {
        let manager = PartitionManager::new("/dev/sdb".to_string(), PartitionScheme::Gpt);
        
        assert_eq!(manager.parse_size_to_bytes("1024").unwrap(), 1024);
        assert_eq!(manager.parse_size_to_bytes("1KB").unwrap(), 1024);
        assert_eq!(manager.parse_size_to_bytes("1MB").unwrap(), 1024 * 1024);
        assert_eq!(manager.parse_size_to_bytes("1GB").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_to_mb() {
        let manager = PartitionManager::new("/dev/sdb".to_string(), PartitionScheme::Gpt);
        
        assert_eq!(manager.parse_size_to_mb("1048576").unwrap(), 1);
        assert_eq!(manager.parse_size_to_mb("1MB").unwrap(), 1);
        assert_eq!(manager.parse_size_to_mb("1GB").unwrap(), 1024);
    }

    #[test]
    fn test_parse_size_to_sectors() {
        let manager = PartitionManager::new("/dev/sdb".to_string(), PartitionScheme::Gpt);
        
        assert_eq!(manager.parse_size_to_sectors("512s").unwrap(), 512);
        assert_eq!(manager.parse_size_to_sectors("1MB").unwrap(), 2048); // 1MB / 512 bytes
    }
}