use crate::error::{ServiceError, Result};
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub service_name: String,
    pub executable_path: PathBuf,
    pub description: String,
    pub working_directory: PathBuf,
    pub user: Option<String>,
    pub group: Option<String>,
    pub restart_policy: RestartPolicy,
    pub environment: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    Never,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            service_name: "usb-installer-node".to_string(),
            executable_path: PathBuf::from("/usr/local/bin/usb-installer-node"),
            description: "USB Installer Node Service".to_string(),
            working_directory: PathBuf::from("/var/lib/usb-installer-node"),
            user: Some("root".to_string()),
            group: Some("root".to_string()),
            restart_policy: RestartPolicy::Always,
            environment: vec![],
        }
    }
}

pub struct ServiceInit;

impl ServiceInit {
    pub fn new() -> Self {
        Self
    }

    pub fn enable_autorun(&self, config: &ServiceConfig) -> Result<()> {
        info!("Enabling autorun for service: {}", config.service_name);

        #[cfg(target_os = "linux")]
        {
            if self.has_systemd()? {
                self.install_systemd_service(config)?;
            } else if self.has_sysvinit()? {
                self.install_sysvinit_service(config)?;
            } else {
                return Err(ServiceError::UnsupportedInitSystem);
            }
        }

        #[cfg(target_os = "freebsd")]
        {
            self.install_rc_script(config)?;
        }

        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            return Err(ServiceError::UnsupportedPlatform);
        }

        Ok(())
    }

    pub fn disable_autorun(&self, service_name: &str) -> Result<()> {
        info!("Disabling autorun for service: {}", service_name);

        #[cfg(target_os = "linux")]
        {
            if self.has_systemd()? {
                self.uninstall_systemd_service(service_name)?;
            } else if self.has_sysvinit()? {
                self.uninstall_sysvinit_service(service_name)?;
            } else {
                return Err(ServiceError::UnsupportedInitSystem);
            }
        }

        #[cfg(target_os = "freebsd")]
        {
            self.uninstall_rc_script(service_name)?;
        }

        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            return Err(ServiceError::UnsupportedPlatform);
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn has_systemd(&self) -> Result<bool> {
        Ok(Path::new("/run/systemd/system").exists())
    }

    #[cfg(target_os = "linux")]
    fn has_sysvinit(&self) -> Result<bool> {
        Ok(Path::new("/etc/init.d").exists())
    }

    #[cfg(target_os = "linux")]
    fn install_systemd_service(&self, config: &ServiceConfig) -> Result<()> {
        let service_content = self.generate_systemd_unit(config);
        let service_path = format!("/etc/systemd/system/{}.service", config.service_name);

        std::fs::write(&service_path, service_content)
            .map_err(|e| ServiceError::IoError(format!("Failed to write service file: {}", e)))?;

        std::fs::set_permissions(&service_path, std::os::unix::fs::PermissionsExt::from_mode(0o644))
            .map_err(|e| ServiceError::IoError(format!("Failed to set permissions: {}", e)))?;

        std::process::Command::new("systemctl")
            .args(&["daemon-reload"])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("systemctl daemon-reload failed: {}", e)))?;

        std::process::Command::new("systemctl")
            .args(&["enable", &config.service_name])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("systemctl enable failed: {}", e)))?;

        info!("Systemd service installed and enabled");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn generate_systemd_unit(&self, config: &ServiceConfig) -> String {
        let mut unit = String::new();
        
        unit.push_str("[Unit]\n");
        unit.push_str(&format!("Description={}\n", config.description));
        unit.push_str("After=network.target\n");
        unit.push_str("\n");

        unit.push_str("[Service]\n");
        unit.push_str(&format!("Type=simple\n"));
        unit.push_str(&format!("ExecStart={}\n", config.executable_path.display()));
        unit.push_str(&format!("WorkingDirectory={}\n", config.working_directory.display()));
        
        if let Some(user) = &config.user {
            unit.push_str(&format!("User={}\n", user));
        }
        
        if let Some(group) = &config.group {
            unit.push_str(&format!("Group={}\n", group));
        }

        match config.restart_policy {
            RestartPolicy::Always => unit.push_str("Restart=always\n"),
            RestartPolicy::OnFailure => unit.push_str("Restart=on-failure\n"),
            RestartPolicy::Never => unit.push_str("Restart=no\n"),
        }

        unit.push_str("RestartSec=10\n");

        for (key, value) in &config.environment {
            unit.push_str(&format!("Environment=\"{}={}\"\n", key, value));
        }

        unit.push_str("\n");
        unit.push_str("[Install]\n");
        unit.push_str("WantedBy=multi-user.target\n");

        unit
    }

    #[cfg(target_os = "linux")]
    fn uninstall_systemd_service(&self, service_name: &str) -> Result<()> {
        std::process::Command::new("systemctl")
            .args(&["stop", service_name])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("systemctl stop failed: {}", e)))?;

        std::process::Command::new("systemctl")
            .args(&["disable", service_name])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("systemctl disable failed: {}", e)))?;

        let service_path = format!("/etc/systemd/system/{}.service", service_name);
        if Path::new(&service_path).exists() {
            std::fs::remove_file(&service_path)
                .map_err(|e| ServiceError::IoError(format!("Failed to remove service file: {}", e)))?;
        }

        std::process::Command::new("systemctl")
            .args(&["daemon-reload"])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("systemctl daemon-reload failed: {}", e)))?;

        info!("Systemd service removed");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn install_sysvinit_service(&self, config: &ServiceConfig) -> Result<()> {
        let script_content = self.generate_sysvinit_script(config);
        let script_path = format!("/etc/init.d/{}", config.service_name);

        std::fs::write(&script_path, script_content)
            .map_err(|e| ServiceError::IoError(format!("Failed to write init script: {}", e)))?;

        std::fs::set_permissions(&script_path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .map_err(|e| ServiceError::IoError(format!("Failed to set permissions: {}", e)))?;

        std::process::Command::new("update-rc.d")
            .args(&[&config.service_name, "defaults"])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("update-rc.d failed: {}", e)))?;

        info!("SysVinit service installed");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn generate_sysvinit_script(&self, config: &ServiceConfig) -> String {
        format!(
            r#"#!/bin/sh
### BEGIN INIT INFO
# Provides:          {name}
# Required-Start:    $network $local_fs
# Required-Stop:     $network $local_fs
# Default-Start:     2 3 4 5
# Default-Stop:      0 1 6
# Short-Description: {desc}
### END INIT INFO

DAEMON={exec}
NAME={name}
PIDFILE=/var/run/$NAME.pid

case "$1" in
  start)
    echo "Starting $NAME..."
    start-stop-daemon --start --background --make-pidfile --pidfile $PIDFILE --exec $DAEMON
    ;;
  stop)
    echo "Stopping $NAME..."
    start-stop-daemon --stop --pidfile $PIDFILE
    rm -f $PIDFILE
    ;;
  restart)
    $0 stop
    $0 start
    ;;
  status)
    if [ -f $PIDFILE ]; then
      echo "$NAME is running"
    else
      echo "$NAME is not running"
    fi
    ;;
  *)
    echo "Usage: $0 {{start|stop|restart|status}}"
    exit 1
    ;;
esac

exit 0
"#,
            name = config.service_name,
            desc = config.description,
            exec = config.executable_path.display()
        )
    }

    #[cfg(target_os = "linux")]
    fn uninstall_sysvinit_service(&self, service_name: &str) -> Result<()> {
        std::process::Command::new("service")
            .args(&[service_name, "stop"])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("service stop failed: {}", e)))?;

        std::process::Command::new("update-rc.d")
            .args(&["-f", service_name, "remove"])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("update-rc.d remove failed: {}", e)))?;

        let script_path = format!("/etc/init.d/{}", service_name);
        if Path::new(&script_path).exists() {
            std::fs::remove_file(&script_path)
                .map_err(|e| ServiceError::IoError(format!("Failed to remove init script: {}", e)))?;
        }

        info!("SysVinit service removed");
        Ok(())
    }

    #[cfg(target_os = "freebsd")]
    fn install_rc_script(&self, config: &ServiceConfig) -> Result<()> {
        let script_content = self.generate_rc_script(config);
        let script_path = format!("/usr/local/etc/rc.d/{}", config.service_name);

        std::fs::write(&script_path, script_content)
            .map_err(|e| ServiceError::IoError(format!("Failed to write rc script: {}", e)))?;

        std::fs::set_permissions(&script_path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .map_err(|e| ServiceError::IoError(format!("Failed to set permissions: {}", e)))?;

        let rc_conf_line = format!("{}_enable=\"YES\"\n", config.service_name);
        std::fs::OpenOptions::new()
            .append(true)
            .open("/etc/rc.conf")
            .and_then(|mut file| std::io::Write::write_all(&mut file, rc_conf_line.as_bytes()))
            .map_err(|e| ServiceError::IoError(format!("Failed to update rc.conf: {}", e)))?;

        info!("FreeBSD rc script installed");
        Ok(())
    }

    #[cfg(target_os = "freebsd")]
    fn generate_rc_script(&self, config: &ServiceConfig) -> String {
        format!(
            r#"#!/bin/sh

# PROVIDE: {name}
# REQUIRE: NETWORKING
# KEYWORD: shutdown

. /etc/rc.subr

name="{name}"
rcvar="{name}_enable"
command="{exec}"
pidfile="/var/run/{name}.pid"

load_rc_config $name

: ${{{{name}}_enable:="NO"}}

run_rc_command "$1"
"#,
            name = config.service_name,
            exec = config.executable_path.display()
        )
    }

    #[cfg(target_os = "freebsd")]
    fn uninstall_rc_script(&self, service_name: &str) -> Result<()> {
        std::process::Command::new("service")
            .args(&[service_name, "stop"])
            .output()
            .map_err(|e| ServiceError::CommandFailed(format!("service stop failed: {}", e)))?;

        let script_path = format!("/usr/local/etc/rc.d/{}", service_name);
        if Path::new(&script_path).exists() {
            std::fs::remove_file(&script_path)
                .map_err(|e| ServiceError::IoError(format!("Failed to remove rc script: {}", e)))?;
        }

        info!("FreeBSD rc script removed");
        Ok(())
    }
}

impl Default for ServiceInit {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_config_default() {
        let config = ServiceConfig::default();
        assert_eq!(config.service_name, "usb-installer-node");
        assert_eq!(config.restart_policy, RestartPolicy::Always);
    }

    #[test]
    fn test_service_init_creation() {
        let init = ServiceInit::new();
        // Just ensure it creates without panic
        assert!(true);
    }
}