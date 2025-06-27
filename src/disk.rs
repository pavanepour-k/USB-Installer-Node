pub mod partition;
pub mod format;

use crate::error::{DiskError, Result};
use crate::config::DiskConfig;
use partition::{DiskPartitioner, PartitionParams};
use format::{DiskFormatter, FormatParams};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskManagerState {
    Idle,
    Partitioning,
    Formatting,
    Busy,
    Error(String),
}

pub struct DiskManager {
    config: Arc<RwLock<DiskConfig>>,
    state: Arc<RwLock<DiskManagerState>>,
    partitioner: DiskPartitioner,
    formatter: DiskFormatter,
}

impl DiskManager {
    pub fn new(config: Arc<RwLock<DiskConfig>>) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(DiskManagerState::Idle)),
            partitioner: DiskPartitioner::new(),
            formatter: DiskFormatter::new(),
        }
    }

    pub async fn prepare_disk(&self, device: &str) -> Result<()> {
        info!("Starting disk preparation for {}", device);
        
        let config = self.config.read().await;
        if !config.enabled {
            info!("Disk management disabled");
            return Ok(());
        }

        self.set_state(DiskManagerState::Busy).await;

        let result = self.prepare_disk_internal(device, &config).await;
        
        match &result {
            Ok(_) => {
                info!("Disk preparation completed successfully");
                self.set_state(DiskManagerState::Idle).await;
            }
            Err(e) => {
                error!("Disk preparation failed: {}", e);
                self.set_state(DiskManagerState::Error(e.to_string())).await;
            }
        }

        result
    }

    async fn prepare_disk_internal(&self, device: &str, config: &DiskConfig) -> Result<()> {
        if config.auto_partition {
            self.set_state(DiskManagerState::Partitioning).await;
            self.auto_partition_disk(device, config)?;
        }

        if config.auto_format {
            self.set_state(DiskManagerState::Formatting).await;
            self.auto_format_partitions(device, config)?;
        }

        Ok(())
    }

    fn auto_partition_disk(&self, device: &str, config: &DiskConfig) -> Result<()> {
        info!("Auto-partitioning disk {}", device);

        let layout = config.partition_layout.as_ref()
            .ok_or_else(|| DiskError::InvalidConfiguration("No partition layout defined".to_string()))?;

        let table_type = partition::PartitionTableType::from_str(&layout.table_type)?;
        
        self.partitioner.create_partition_table(device, table_type)?;

        let mut start_sector = 2048;
        
        for (i, partition_config) in layout.partitions.iter().enumerate() {
            let size_sectors = self.calculate_size_sectors(&partition_config.size, device)?;
            
            let params = PartitionParams::new(
                device.to_string(),
                i as u32 + 1,
                start_sector,
                size_sectors,
            )
            .with_type_guid(partition_config.type_guid.clone())
            .with_name(partition_config.name.clone())
            .with_flags(partition_config.flags.clone());

            self.partitioner.create_partition(&params)?;
            
            start_sector += size_sectors;
        }

        Ok(())
    }

    fn auto_format_partitions(&self, device: &str, config: &DiskConfig) -> Result<()> {
        info!("Auto-formatting partitions on {}", device);

        let layout = config.partition_layout.as_ref()
            .ok_or_else(|| DiskError::InvalidConfiguration("No partition layout defined".to_string()))?;

        for (i, partition_config) in layout.partitions.iter().enumerate() {
            if let Some(fs_type_str) = &partition_config.filesystem {
                let partition_device = format!("{}{}",
                    device,
                    if device.ends_with(char::is_numeric) { "p" } else { "" },
                    i + 1
                );

                let fs_type = format::FileSystemType::from_str(fs_type_str)?;
                
                let mut params = FormatParams::new(partition_device.clone(), fs_type);
                
                if let Some(label) = &partition_config.label {
                    params = params.with_label(label.clone());
                }
                
                if config.force_format {
                    params = params.force();
                }

                self.formatter.format(&params)?;
            }
        }

        Ok(())
    }

    fn calculate_size_sectors(&self, size_str: &str, device: &str) -> Result<u64> {
        if size_str == "100%" {
            let disk_info = self.partitioner.get_disk_info(device)?;
            Ok(disk_info.total_sectors - 2048)
        } else if size_str.ends_with('%') {
            let percentage = size_str.trim_end_matches('%').parse::<f64>()
                .map_err(|_| DiskError::InvalidParameter(format!("Invalid percentage: {}", size_str)))?;
            let disk_info = self.partitioner.get_disk_info(device)?;
            Ok(((disk_info.total_sectors as f64 * percentage / 100.0) as u64).max(1))
        } else {
            self.parse_size_to_sectors(size_str)
        }
    }

    fn parse_size_to_sectors(&self, size_str: &str) -> Result<u64> {
        let size_str = size_str.to_uppercase();
        let (value, unit) = if let Some(pos) = size_str.find(|c: char| c.is_alphabetic()) {
            let (num, unit) = size_str.split_at(pos);
            (num.parse::<f64>().map_err(|_| DiskError::InvalidParameter(format!("Invalid size: {}", size_str)))?, unit)
        } else {
            (size_str.parse::<f64>().map_err(|_| DiskError::InvalidParameter(format!("Invalid size: {}", size_str)))?, "B")
        };

        let bytes = match unit {
            "B" => value,
            "KB" | "K" => value * 1024.0,
            "MB" | "M" => value * 1024.0 * 1024.0,
            "GB" | "G" => value * 1024.0 * 1024.0 * 1024.0,
            "TB" | "T" => value * 1024.0 * 1024.0 * 1024.0 * 1024.0,
            _ => return Err(DiskError::InvalidParameter(format!("Unknown size unit: {}", unit))),
        };

        Ok((bytes / 512.0).ceil() as u64)
    }

    pub async fn partition_disk(&self, params: &PartitionParams) -> Result<()> {
        self.set_state(DiskManagerState::Partitioning).await;
        let result = self.partitioner.create_partition(params);
        self.set_state(DiskManagerState::Idle).await;
        result
    }

    pub async fn format_partition(&self, params: &FormatParams) -> Result<()> {
        self.set_state(DiskManagerState::Formatting).await;
        let result = self.formatter.format(params);
        self.set_state(DiskManagerState::Idle).await;
        result
    }

    pub async fn list_disks(&self) -> Result<Vec<String>> {
        self.partitioner.list_disks()
    }

    pub async fn get_disk_info(&self, device: &str) -> Result<partition::DiskInfo> {
        self.partitioner.get_disk_info(device)
    }

    pub async fn get_partition_info(&self, device: &str) -> Result<Vec<partition::PartitionInfo>> {
        self.partitioner.list_partitions(device)
    }

    pub async fn get_state(&self) -> DiskManagerState {
        self.state.read().await.clone()
    }

    async fn set_state(&self, state: DiskManagerState) {
        *self.state.write().await = state;
    }

    pub async fn reload_config(&self, config: Arc<RwLock<DiskConfig>>) {
        *self.config.write().await = config.read().await.clone();
    }

    pub async fn health_check(&self) -> Result<()> {
        let state = self.get_state().await;
        match state {
            DiskManagerState::Error(e) => Err(DiskError::HealthCheckFailed(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_disk_manager_creation() {
        let config = Arc::new(RwLock::new(DiskConfig::default()));
        let manager = DiskManager::new(config);
        assert_eq!(manager.get_state().await, DiskManagerState::Idle);
    }

    #[test]
    fn test_parse_size_to_sectors() {
        let config = Arc::new(RwLock::new(DiskConfig::default()));
        let manager = DiskManager::new(config);

        assert_eq!(manager.parse_size_to_sectors("1024B").unwrap(), 2);
        assert_eq!(manager.parse_size_to_sectors("1KB").unwrap(), 2);
        assert_eq!(manager.parse_size_to_sectors("1MB").unwrap(), 2048);
        assert_eq!(manager.parse_size_to_sectors("1GB").unwrap(), 2097152);
        assert_eq!(manager.parse_size_to_sectors("512").unwrap(), 1);
        
        assert!(manager.parse_size_to_sectors("invalid").is_err());
        assert!(manager.parse_size_to_sectors("1XB").is_err());
    }

    #[test]
    fn test_calculate_size_sectors() {
        let config = Arc::new(RwLock::new(DiskConfig::default()));
        let manager = DiskManager::new(config);

        assert_eq!(manager.parse_size_to_sectors("10GB").unwrap(), 20971520);
        assert!(manager.calculate_size_sectors("50%", "/dev/sda").is_err());
    }
}