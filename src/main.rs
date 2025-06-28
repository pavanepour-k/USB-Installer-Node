mod config;
mod disk;
mod error;
mod iso;
mod logging;
mod monitoring;
mod network;
mod remote;
mod service;
mod ui;

use crate::config::Config;
use crate::error::Result;
use crate::logging::Logger;
use crate::monitoring::{Monitor, Monitorable};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

struct AppState {
    config: Arc<RwLock<Config>>,
    network_manager: Arc<RwLock<network::NetworkManager>>,
    disk_manager: Arc<disk::DiskManager>,
    iso_manager: Arc<iso::IsoManager>,
    remote_manager: Arc<RwLock<remote::RemoteManager>>,
    ui_manager: Arc<RwLock<ui::UiManager>>,
    monitor: Arc<RwLock<Monitor>>,
    shutdown_tx: broadcast::Sender<()>,
}

struct NetworkMonitorAdapter {
    manager: Arc<RwLock<network::NetworkManager>>,
}

impl Monitorable for NetworkMonitorAdapter {
    fn name(&self) -> &str {
        "network"
    }

    async fn health_check(&self) -> Result<()> {
        self.manager.read().await.health_check().await
    }

    async fn restart(&mut self) -> Result<()> {
        let mut manager = self.manager.write().await;
        manager.stop().await?;
        sleep(Duration::from_millis(500)).await;
        manager.start().await
    }
}

struct RemoteMonitorAdapter {
    manager: Arc<RwLock<remote::RemoteManager>>,
}

impl Monitorable for RemoteMonitorAdapter {
    fn name(&self) -> &str {
        "remote"
    }

    async fn health_check(&self) -> Result<()> {
        self.manager.read().await.health_check().await
    }

    async fn restart(&mut self) -> Result<()> {
        let mut manager = self.manager.write().await;
        manager.stop_all().await?;
        sleep(Duration::from_millis(500)).await;
        manager.start_all().await
    }
}

struct UiMonitorAdapter {
    manager: Arc<RwLock<ui::UiManager>>,
}

impl Monitorable for UiMonitorAdapter {
    fn name(&self) -> &str {
        "ui"
    }

    async fn health_check(&self) -> Result<()> {
        self.manager.read().await.health_check().await
    }

    async fn restart(&mut self) -> Result<()> {
        let mut manager = self.manager.write().await;
        manager.stop().await?;
        sleep(Duration::from_millis(500)).await;
        manager.start().await
    }
}

impl AppState {
    async fn new(config: Config) -> Result<Self> {
        let config = Arc::new(RwLock::new(config));
        let (shutdown_tx, _) = broadcast::channel(16);

        let network_manager = Arc::new(RwLock::new(network::NetworkManager::new(Arc::new(
            RwLock::new(config.read().await.network.clone()),
        ))));

        let disk_manager = Arc::new(disk::DiskManager::new(Arc::new(RwLock::new(
            config.read().await.disk.clone(),
        ))));

        let iso_manager = Arc::new(iso::IsoManager::new(Arc::new(RwLock::new(
            config.read().await.iso.clone(),
        ))));

        let remote_manager = Arc::new(RwLock::new(remote::RemoteManager::new(Arc::new(
            RwLock::new(config.read().await.remote.clone()),
        ))));

        let ui_manager = Arc::new(RwLock::new(ui::UiManager::new(Arc::new(RwLock::new(
            config.read().await.ui.clone(),
        )))));

        let monitor = Arc::new(RwLock::new(Monitor::new(Arc::new(RwLock::new(
            config.read().await.monitoring.clone(),
        )))));

        Ok(Self {
            config,
            network_manager,
            disk_manager,
            iso_manager,
            remote_manager,
            ui_manager,
            monitor,
            shutdown_tx,
        })
    }

    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing USB Installer Node");

        self.check_preconditions().await?;
        self.setup_monitoring().await?;
        self.start_subsystems().await?;

        info!("Initialization complete");
        Ok(())
    }

    async fn check_preconditions(&self) -> Result<()> {
        debug!("Checking system preconditions");

        if !nix::unistd::Uid::effective().is_root() {
            return Err(error::AppError::PermissionDenied(
                "This application must be run as root".to_string(),
            )
            .into());
        }

        let required_commands = ["mount", "umount", "fdisk", "mkfs.ext4", "x11vnc", "sshd"];
        for cmd in &required_commands {
            if std::process::Command::new("which")
                .arg(cmd)
                .output()
                .map(|o| !o.status.success())
                .unwrap_or(true)
            {
                return Err(error::AppError::MissingDependency(cmd.to_string()).into());
            }
        }

        Ok(())
    }

    async fn setup_monitoring(&mut self) -> Result<()> {
        let mut monitor = self.monitor.write().await;

        monitor
            .register_service(Box::new(NetworkMonitorAdapter {
                manager: self.network_manager.clone(),
            }))
            .await;

        monitor
            .register_service(Box::new(RemoteMonitorAdapter {
                manager: self.remote_manager.clone(),
            }))
            .await;

        monitor
            .register_service(Box::new(UiMonitorAdapter {
                manager: self.ui_manager.clone(),
            }))
            .await;

        monitor.start().await?;
        Ok(())
    }

    async fn start_subsystems(&mut self) -> Result<()> {
        info!("Starting subsystems");

        if let Err(e) = self.network_manager.write().await.start().await {
            error!("Failed to start network manager: {}", e);
            return Err(e);
        }

        sleep(Duration::from_millis(500)).await;

        if let Err(e) = self.remote_manager.write().await.start_all().await {
            warn!("Failed to start some remote services: {}", e);
        }

        if let Err(e) = self.iso_manager.start().await {
            warn!("Failed to start ISO manager: {}", e);
        }

        if let Err(e) = self.ui_manager.write().await.start().await {
            warn!("Failed to start UI manager: {}", e);
        }

        Ok(())
    }

    async fn run(&mut self) -> Result<()> {
        info!("USB Installer Node is running");

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;

        loop {
            tokio::select! {
                _ = signal::ctrl_c() => {
                    info!("Received SIGINT");
                    break;
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM");
                    break;
                }
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal");
                    break;
                }
                _ = sleep(Duration::from_secs(60)) => {
                    debug!("Main loop heartbeat");
                    self.perform_maintenance().await;
                }
            }
        }

        Ok(())
    }

    async fn perform_maintenance(&self) {
        debug!("Performing routine maintenance");

        if let Ok(metrics) = self.monitor.read().await.get_prometheus_metrics().await {
            debug!("Current metrics: {} bytes", metrics.len());
        }

        self.monitor.read().await.clear_resolved_alerts().await;
    }

    async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down USB Installer Node");

        let shutdown_start = std::time::Instant::now();

        let _ = self.shutdown_tx.send(());

        if let Err(e) = self.ui_manager.write().await.stop().await {
            warn!("Error stopping UI manager: {}", e);
        }

        if let Err(e) = self.iso_manager.stop().await {
            warn!("Error stopping ISO manager: {}", e);
        }

        if let Err(e) = self.remote_manager.write().await.stop_all().await {
            warn!("Error stopping remote manager: {}", e);
        }

        if let Err(e) = self.network_manager.write().await.stop().await {
            warn!("Error stopping network manager: {}", e);
        }

        if let Err(e) = self.monitor.write().await.stop().await {
            warn!("Error stopping monitor: {}", e);
        }

        let shutdown_duration = shutdown_start.elapsed();
        info!("Shutdown complete in {:?}", shutdown_duration);

        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let result = run_app().await;

    match result {
        Ok(_) => {
            info!("Application exited successfully");
            std::process::exit(0);
        }
        Err(e) => {
            error!("Application failed: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_app() -> Result<()> {
    let config = Config::load("config.toml")?;

    Logger::init(&config.logging)?;

    info!("Starting USB Installer Node v{}", env!("CARGO_PKG_VERSION"));
    debug!("Configuration loaded from config.toml");

    let panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        error!("Panic occurred: {:?}", panic_info);
        panic_hook(panic_info);
    }));

    let mut app = AppState::new(config).await?;

    if let Err(e) = app.initialize().await {
        error!("Initialization failed: {}", e);
        return Err(e);
    }

    let run_result = app.run().await;

    app.shutdown().await?;

    run_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_main_compiles() {
        assert!(true);
    }
}
