use crate::config::LoggingConfig;
use crate::error::{UsbInstallerError, UsbInstallerResult};
use log::{Level, LevelFilter};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

pub struct Logger {
    config: Arc<RwLock<LoggingConfig>>,
    file_writer: Option<Arc<Mutex<File>>>,
    console_enabled: bool,
}

impl Logger {
    pub fn new(config: LoggingConfig) -> UsbInstallerResult<Self> {
        let file_writer = if let Some(ref path) = config.file_path {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| {
                    UsbInstallerError::Logging(format!("Failed to open log file: {}", e))
                })?;
            Some(Arc::new(Mutex::new(file)))
        } else {
            None
        };

        let logger = Self {
            config: Arc::new(RwLock::new(config)),
            file_writer,
            console_enabled: true,
        };

        logger.init_log_backend()?;
        Ok(logger)
    }

    pub fn init_log_backend(&self) -> UsbInstallerResult<()> {
        let config = self
            .config
            .read()
            .map_err(|_| UsbInstallerError::Logging("Failed to read config".to_string()))?;

        let level_filter = match config.level.as_str() {
            "error" => LevelFilter::Error,
            "warn" => LevelFilter::Warn,
            "info" => LevelFilter::Info,
            "debug" => LevelFilter::Debug,
            "trace" => LevelFilter::Trace,
            _ => LevelFilter::Info,
        };

        env_logger::Builder::from_default_env()
            .filter_level(level_filter)
            .format(|buf, record| {
                let timestamp = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                writeln!(
                    buf,
                    "{} [{}] {}: {}",
                    timestamp,
                    record.level(),
                    record.target(),
                    record.args()
                )
            })
            .init();

        Ok(())
    }

    pub fn set_level(&self, level: &str) -> UsbInstallerResult<()> {
        let mut config = self
            .config
            .write()
            .map_err(|_| UsbInstallerError::Logging("Failed to write config".to_string()))?;
        config.level = level.to_string();
        self.init_log_backend()
    }

    pub fn reload_config(&self, new_config: LoggingConfig) -> UsbInstallerResult<()> {
        let mut config = self
            .config
            .write()
            .map_err(|_| UsbInstallerError::Logging("Failed to write config".to_string()))?;
        *config = new_config;
        self.init_log_backend()
    }

    pub fn log_with_context(
        &self,
        level: Level,
        subsystem: &str,
        request_id: Option<&str>,
        message: &str,
    ) {
        let context = match request_id {
            Some(id) => format!("[{}:{}]", subsystem, id),
            None => format!("[{}]", subsystem),
        };

        match level {
            Level::Error => log::error!("{} {}", context, message),
            Level::Warn => log::warn!("{} {}", context, message),
            Level::Info => log::info!("{} {}", context, message),
            Level::Debug => log::debug!("{} {}", context, message),
            Level::Trace => log::trace!("{} {}", context, message),
        }

        if let Some(ref writer) = self.file_writer {
            if let Ok(mut file) = writer.lock() {
                let timestamp = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let _ = writeln!(file, "{} [{}] {} {}", timestamp, level, context, message);
                let _ = file.flush();
            }
        }
    }

    pub fn rotate_log(&self) -> UsbInstallerResult<()> {
        if let Some(ref writer) = self.file_writer {
            let config = self
                .config
                .read()
                .map_err(|_| UsbInstallerError::Logging("Failed to read config".to_string()))?;

            if let Some(ref path) = config.file_path {
                let backup_path = format!(
                    "{}.{}",
                    path,
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );

                std::fs::rename(path, backup_path).map_err(|e| {
                    UsbInstallerError::Logging(format!("Failed to rotate log: {}", e))
                })?;

                let new_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| {
                        UsbInstallerError::Logging(format!("Failed to create new log file: {}", e))
                    })?;

                if let Ok(mut file_guard) = writer.lock() {
                    *file_guard = new_file;
                }
            }
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! log_context {
    ($level:expr, $subsystem:expr, $msg:expr) => {
        log::log!($level, "[{}] {}", $subsystem, $msg)
    };
    ($level:expr, $subsystem:expr, $request_id:expr, $msg:expr) => {
        log::log!($level, "[{}:{}] {}", $subsystem, $request_id, $msg)
    };
}

#[macro_export]
macro_rules! error_context {
    ($subsystem:expr, $msg:expr) => {
        $crate::log_context!(log::Level::Error, $subsystem, $msg)
    };
    ($subsystem:expr, $request_id:expr, $msg:expr) => {
        $crate::log_context!(log::Level::Error, $subsystem, $request_id, $msg)
    };
}

#[macro_export]
macro_rules! warn_context {
    ($subsystem:expr, $msg:expr) => {
        $crate::log_context!(log::Level::Warn, $subsystem, $msg)
    };
    ($subsystem:expr, $request_id:expr, $msg:expr) => {
        $crate::log_context!(log::Level::Warn, $subsystem, $request_id, $msg)
    };
}

#[macro_export]
macro_rules! info_context {
    ($subsystem:expr, $msg:expr) => {
        $crate::log_context!(log::Level::Info, $subsystem, $msg)
    };
    ($subsystem:expr, $request_id:expr, $msg:expr) => {
        $crate::log_context!(log::Level::Info, $subsystem, $request_id, $msg)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_logger_creation() {
        let config = LoggingConfig {
            level: "info".to_string(),
            file_path: None,
            console: true,
            rotation_size: 10485760,
        };

        let logger = Logger::new(config);
        assert!(logger.is_ok());
    }

    #[test]
    fn test_file_logging() {
        let temp_dir = tempdir().unwrap();
        let log_path = temp_dir.path().join("test.log");

        let config = LoggingConfig {
            level: "info".to_string(),
            file_path: Some(log_path.to_string_lossy().to_string()),
            console: true,
            rotation_size: 10485760,
        };

        let logger = Logger::new(config).unwrap();
        logger.log_with_context(Level::Info, "test", None, "Test message");

        let content = fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("Test message"));
    }

    #[test]
    fn test_level_change() {
        let config = LoggingConfig {
            level: "info".to_string(),
            file_path: None,
            console: true,
            rotation_size: 10485760,
        };

        let logger = Logger::new(config).unwrap();
        assert!(logger.set_level("debug").is_ok());
    }
}
