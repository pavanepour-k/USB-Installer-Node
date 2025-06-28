pub mod installer_gui;

use crate::config::UiConfig;
use crate::error::{Result, UiError};
use installer_gui::{GuiConfig, GuiEvent, GuiState, InstallProgress, InstallerGui};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiManagerState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct UiMessage {
    pub msg_type: UiMessageType,
    pub content: String,
    pub data: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiMessageType {
    Progress,
    Error,
    Warning,
    Info,
    Success,
    Input,
}

pub struct UiManager {
    config: Arc<RwLock<UiConfig>>,
    state: Arc<RwLock<UiManagerState>>,
    gui: Arc<InstallerGui>,
    message_tx: mpsc::Sender<UiMessage>,
    message_rx: Arc<RwLock<mpsc::Receiver<UiMessage>>>,
    backend_tx: Option<mpsc::Sender<HashMap<String, String>>>,
}

impl UiManager {
    pub fn new(config: Arc<RwLock<UiConfig>>) -> Self {
        let (message_tx, message_rx) = mpsc::channel(1000);

        let gui_config = GuiConfig {
            window_title: "USB Installer Node".to_string(),
            width: 800,
            height: 600,
            fullscreen: false,
            theme: "dark".to_string(),
            language: "en".to_string(),
            auto_restart: true,
        };

        Self {
            config,
            state: Arc::new(RwLock::new(UiManagerState::Stopped)),
            gui: Arc::new(InstallerGui::new(gui_config)),
            message_tx,
            message_rx: Arc::new(RwLock::new(message_rx)),
            backend_tx: None,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting UI manager");
        self.set_state(UiManagerState::Starting).await;

        let config = self.config.read().await;
        if !config.enabled {
            info!("UI disabled");
            self.set_state(UiManagerState::Stopped).await;
            return Ok(());
        }

        self.gui.start().await?;
        self.start_message_processor().await;

        self.set_state(UiManagerState::Running).await;
        info!("UI manager started");

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping UI manager");
        self.set_state(UiManagerState::Stopping).await;

        self.gui.stop().await?;

        self.set_state(UiManagerState::Stopped).await;
        info!("UI manager stopped");

        Ok(())
    }

    async fn start_message_processor(&self) {
        let gui = self.gui.clone();
        let mut rx = self.message_rx.write().await;

        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                match message.msg_type {
                    UiMessageType::Progress => {
                        if let (Some(step), Some(percentage)) =
                            (message.data.get("step"), message.data.get("percentage"))
                        {
                            let progress = InstallProgress {
                                current_step: step.clone(),
                                total_steps: message
                                    .data
                                    .get("total_steps")
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0),
                                completed_steps: message
                                    .data
                                    .get("completed_steps")
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0),
                                percentage: percentage.parse().unwrap_or(0),
                                message: message.content,
                                timestamp: std::time::SystemTime::now(),
                            };

                            if let Err(e) = gui.display_progress(progress).await {
                                error!("Failed to display progress: {}", e);
                            }
                        }
                    }
                    UiMessageType::Error => {
                        gui.show_error("Error", &message.content).await;
                    }
                    UiMessageType::Success => {
                        gui.show_success(&message.content).await;
                    }
                    UiMessageType::Info | UiMessageType::Warning => {
                        gui.add_log(message.content).await;
                    }
                    UiMessageType::Input => {
                        debug!("Received input message: {:?}", message.data);
                    }
                }
            }
        });
    }

    pub async fn send_message(&self, message: UiMessage) -> Result<()> {
        self.message_tx
            .send(message)
            .await
            .map_err(|_| UiError::MessageChannelClosed)?;
        Ok(())
    }

    pub async fn update_progress(
        &self,
        step: &str,
        percentage: u8,
        message: &str,
        total_steps: u32,
        completed_steps: u32,
    ) -> Result<()> {
        let mut data = HashMap::new();
        data.insert("step".to_string(), step.to_string());
        data.insert("percentage".to_string(), percentage.to_string());
        data.insert("total_steps".to_string(), total_steps.to_string());
        data.insert("completed_steps".to_string(), completed_steps.to_string());

        self.send_message(UiMessage {
            msg_type: UiMessageType::Progress,
            content: message.to_string(),
            data,
        })
        .await
    }

    pub async fn show_error(&self, error: &str) -> Result<()> {
        self.send_message(UiMessage {
            msg_type: UiMessageType::Error,
            content: error.to_string(),
            data: HashMap::new(),
        })
        .await
    }

    pub async fn show_success(&self, message: &str) -> Result<()> {
        self.send_message(UiMessage {
            msg_type: UiMessageType::Success,
            content: message.to_string(),
            data: HashMap::new(),
        })
        .await
    }

    pub async fn show_info(&self, info: &str) -> Result<()> {
        self.send_message(UiMessage {
            msg_type: UiMessageType::Info,
            content: info.to_string(),
            data: HashMap::new(),
        })
        .await
    }

    pub async fn show_warning(&self, warning: &str) -> Result<()> {
        self.send_message(UiMessage {
            msg_type: UiMessageType::Warning,
            content: warning.to_string(),
            data: HashMap::new(),
        })
        .await
    }

    pub async fn handle_remote_event(
        &self,
        event_type: &str,
        data: HashMap<String, String>,
    ) -> Result<()> {
        self.gui.handle_remote_input(event_type, data).await
    }

    pub async fn get_gui_state(&self) -> GuiState {
        self.gui.get_state().await
    }

    pub async fn get_logs(&self, limit: Option<usize>) -> Vec<String> {
        self.gui.get_logs(limit).await
    }

    pub async fn set_backend_channel(&mut self, tx: mpsc::Sender<HashMap<String, String>>) {
        self.backend_tx = Some(tx);
    }

    pub async fn process_gui_events(&self) -> Result<()> {
        let events = self.gui.process_events().await?;

        if let Some(tx) = &self.backend_tx {
            for event in events {
                if let Err(e) = tx.send(event.data).await {
                    warn!("Failed to send event to backend: {}", e);
                }
            }
        }

        Ok(())
    }

    pub async fn get_state(&self) -> UiManagerState {
        self.state.read().await.clone()
    }

    async fn set_state(&self, state: UiManagerState) {
        *self.state.write().await = state;
    }

    pub async fn reload_config(&mut self, config: Arc<RwLock<UiConfig>>) -> Result<()> {
        info!("Reloading UI configuration");

        let was_running = self.get_state().await == UiManagerState::Running;

        if was_running {
            self.stop().await?;
        }

        self.config = config;

        if was_running {
            self.start().await?;
        }

        Ok(())
    }

    pub async fn health_check(&self) -> Result<()> {
        let state = self.get_state().await;
        match state {
            UiManagerState::Error(e) => Err(UiError::HealthCheckFailed(e)),
            UiManagerState::Running => {
                let gui_state = self.gui.get_state().await;
                match gui_state {
                    GuiState::Crashed => Err(UiError::GuiCrashed),
                    GuiState::Failed(e) => Err(UiError::HealthCheckFailed(e)),
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    pub async fn get_localized_string(&self, key: &str) -> String {
        let config = self.config.read().await;
        let lang = &config.language;

        match (lang.as_str(), key) {
            ("en", "welcome") => "Welcome to USB Installer".to_string(),
            ("en", "select_os") => "Select Operating System".to_string(),
            ("en", "install") => "Install".to_string(),
            ("en", "cancel") => "Cancel".to_string(),
            ("en", "partitioning") => "Partitioning disk...".to_string(),
            ("en", "installing") => "Installing OS...".to_string(),
            ("en", "complete") => "Installation complete!".to_string(),
            ("en", "error") => "An error occurred".to_string(),
            _ => key.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ui_manager_creation() {
        let config = Arc::new(RwLock::new(UiConfig::default()));
        let manager = UiManager::new(config);
        assert_eq!(manager.get_state().await, UiManagerState::Stopped);
    }

    #[tokio::test]
    async fn test_message_sending() {
        let config = Arc::new(RwLock::new(UiConfig::default()));
        let manager = UiManager::new(config);

        let message = UiMessage {
            msg_type: UiMessageType::Info,
            content: "Test message".to_string(),
            data: HashMap::new(),
        };

        assert!(manager.send_message(message).await.is_ok());
    }

    #[tokio::test]
    async fn test_progress_update() {
        let config = Arc::new(RwLock::new(UiConfig::default()));
        let manager = UiManager::new(config);

        let result = manager
            .update_progress("Installing", 50, "Installing packages...", 10, 5)
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_localization() {
        let config = Arc::new(RwLock::new(UiConfig::default()));
        let manager = UiManager::new(config);

        let welcome = manager.get_localized_string("welcome").await;
        assert_eq!(welcome, "Welcome to USB Installer");

        let unknown = manager.get_localized_string("unknown_key").await;
        assert_eq!(unknown, "unknown_key");
    }
}
