pub mod init;

use crate::config::ServiceConfig as AppServiceConfig;
use crate::error::Result;
use init::{RestartPolicy, ServiceConfig, ServiceInit};

/// High level wrapper around [`ServiceInit`] that converts the
/// application configuration into platform specific service files.
#[derive(Debug, Clone)]
pub struct ServiceManager {
    config: AppServiceConfig,
    init: ServiceInit,
}

impl ServiceManager {
    /// Create a new manager from the application [`ServiceConfig`].
    pub fn new(config: AppServiceConfig) -> Self {
        Self {
            config,
            init: ServiceInit::new(),
        }
    }

    /// Install and enable the service for autorun if `autorun` is set.
    pub fn install(&self) -> Result<()> {
        if self.config.autorun {
            let cfg = self.to_init_config();
            self.init.enable_autorun(&cfg)?;
        }
        Ok(())
    }

    /// Disable and remove the service.
    pub fn uninstall(&self) -> Result<()> {
        self.init.disable_autorun(&self.config.service_name)
    }

    fn to_init_config(&self) -> ServiceConfig {
        ServiceConfig {
            service_name: self.config.service_name.clone(),
            executable_path: std::env::current_exe()
                .unwrap_or_else(|_| "./usb-installer-node".into()),
            description: self.config.description.clone(),
            working_directory: std::env::current_dir().unwrap_or_else(|_| "/".into()),
            user: Some("root".to_string()),
            group: Some("root".to_string()),
            restart_policy: RestartPolicy::Always,
            environment: Vec::new(),
        }
    }
}
