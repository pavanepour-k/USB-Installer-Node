use crate::error::{Result, UiError};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiState {
    Initializing,
    Ready,
    Installing,
    Completed,
    Failed(String),
    Crashed,
}

#[derive(Debug, Clone)]
pub struct InstallProgress {
    pub current_step: String,
    pub total_steps: u32,
    pub completed_steps: u32,
    pub percentage: u8,
    pub message: String,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone)]
pub struct GuiEvent {
    pub event_type: GuiEventType,
    pub data: HashMap<String, String>,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiEventType {
    Click,
    KeyPress,
    SelectionChange,
    InputChange,
    WindowResize,
    RemoteInput,
}

#[derive(Debug, Clone)]
pub struct GuiConfig {
    pub window_title: String,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub theme: String,
    pub language: String,
    pub auto_restart: bool,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            window_title: "USB Installer Node".to_string(),
            width: 800,
            height: 600,
            fullscreen: false,
            theme: "dark".to_string(),
            language: "en".to_string(),
            auto_restart: true,
        }
    }
}

pub struct InstallerGui {
    config: Arc<RwLock<GuiConfig>>,
    state: Arc<RwLock<GuiState>>,
    progress: Arc<RwLock<InstallProgress>>,
    event_tx: mpsc::Sender<GuiEvent>,
    event_rx: Arc<RwLock<mpsc::Receiver<GuiEvent>>>,
    logs: Arc<RwLock<Vec<String>>>,
    restart_count: Arc<RwLock<u32>>,
}

impl InstallerGui {
    pub fn new(config: GuiConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);

        Self {
            config: Arc::new(RwLock::new(config)),
            state: Arc::new(RwLock::new(GuiState::Initializing)),
            progress: Arc::new(RwLock::new(InstallProgress {
                current_step: "Initializing".to_string(),
                total_steps: 0,
                completed_steps: 0,
                percentage: 0,
                message: String::new(),
                timestamp: SystemTime::now(),
            })),
            event_tx,
            event_rx: Arc::new(RwLock::new(event_rx)),
            logs: Arc::new(RwLock::new(Vec::new())),
            restart_count: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting installer GUI");
        self.set_state(GuiState::Initializing).await;

        self.simulate_gui_thread().await?;

        self.set_state(GuiState::Ready).await;
        info!("Installer GUI ready");

        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        info!("Stopping installer GUI");
        self.set_state(GuiState::Ready).await;
        Ok(())
    }

    async fn simulate_gui_thread(&self) -> Result<()> {
        let state = self.state.clone();
        let config = self.config.clone();
        let logs = self.logs.clone();
        let restart_count = self.restart_count.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                let current_state = state.read().await.clone();
                if current_state == GuiState::Crashed {
                    let config = config.read().await;
                    if config.auto_restart {
                        warn!("GUI crashed, attempting restart");
                        let mut count = restart_count.write().await;
                        *count += 1;

                        *state.write().await = GuiState::Initializing;
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        *state.write().await = GuiState::Ready;

                        info!("GUI restarted successfully (attempt #{})", *count);
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn display_progress(&self, progress: InstallProgress) -> Result<()> {
        *self.progress.write().await = progress.clone();
        self.add_log(format!(
            "[{}] {}: {} ({}%)",
            chrono::DateTime::<chrono::Utc>::from(progress.timestamp).format("%H:%M:%S"),
            progress.current_step,
            progress.message,
            progress.percentage
        ))
        .await;
        Ok(())
    }

    pub async fn handle_remote_input(
        &self,
        input_type: &str,
        data: HashMap<String, String>,
    ) -> Result<()> {
        let event = GuiEvent {
            event_type: match input_type {
                "click" => GuiEventType::Click,
                "key" => GuiEventType::KeyPress,
                _ => GuiEventType::RemoteInput,
            },
            data,
            timestamp: SystemTime::now(),
        };

        self.event_tx
            .send(event)
            .await
            .map_err(|_| UiError::EventChannelClosed)?;

        Ok(())
    }

    pub async fn get_state(&self) -> GuiState {
        self.state.read().await.clone()
    }

    async fn set_state(&self, state: GuiState) {
        *self.state.write().await = state;
    }

    pub async fn get_progress(&self) -> InstallProgress {
        self.progress.read().await.clone()
    }

    pub async fn add_log(&self, message: String) {
        let mut logs = self.logs.write().await;
        logs.push(format!(
            "[{}] {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
            message
        ));

        if logs.len() > 1000 {
            logs.drain(0..100);
        }
    }

    pub async fn get_logs(&self, limit: Option<usize>) -> Vec<String> {
        let logs = self.logs.read().await;
        match limit {
            Some(n) => logs.iter().rev().take(n).rev().cloned().collect(),
            None => logs.clone(),
        }
    }

    pub async fn simulate_crash(&self) {
        warn!("Simulating GUI crash");
        self.set_state(GuiState::Crashed).await;
    }

    pub async fn restart(&self) -> Result<()> {
        info!("Manually restarting GUI");
        self.stop().await?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        self.start().await?;

        let mut count = self.restart_count.write().await;
        *count += 1;

        Ok(())
    }

    pub async fn update_config(&self, new_config: GuiConfig) -> Result<()> {
        *self.config.write().await = new_config;
        Ok(())
    }

    pub async fn process_events(&self) -> Result<Vec<GuiEvent>> {
        let mut events = Vec::new();
        let mut rx = self.event_rx.write().await;

        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        Ok(events)
    }

    pub async fn show_error(&self, title: &str, message: &str) {
        self.add_log(format!("ERROR: {} - {}", title, message))
            .await;
        self.set_state(GuiState::Failed(message.to_string())).await;
    }

    pub async fn show_success(&self, message: &str) {
        self.add_log(format!("SUCCESS: {}", message)).await;
        self.set_state(GuiState::Completed).await;
    }

    pub async fn get_restart_count(&self) -> u32 {
        *self.restart_count.read().await
    }

    pub async fn clear_logs(&self) {
        self.logs.write().await.clear();
    }
}

impl Default for InstallerGui {
    fn default() -> Self {
        Self::new(GuiConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gui_creation() {
        let gui = InstallerGui::default();
        assert_eq!(gui.get_state().await, GuiState::Initializing);
        assert_eq!(gui.get_restart_count().await, 0);
    }

    #[tokio::test]
    async fn test_progress_tracking() {
        let gui = InstallerGui::default();

        let progress = InstallProgress {
            current_step: "Partitioning".to_string(),
            total_steps: 5,
            completed_steps: 2,
            percentage: 40,
            message: "Creating partitions".to_string(),
            timestamp: SystemTime::now(),
        };

        gui.display_progress(progress.clone()).await.unwrap();

        let retrieved = gui.get_progress().await;
        assert_eq!(retrieved.current_step, "Partitioning");
        assert_eq!(retrieved.percentage, 40);
    }

    #[tokio::test]
    async fn test_log_management() {
        let gui = InstallerGui::default();

        gui.add_log("Test log 1".to_string()).await;
        gui.add_log("Test log 2".to_string()).await;
        gui.add_log("Test log 3".to_string()).await;

        let logs = gui.get_logs(Some(2)).await;
        assert_eq!(logs.len(), 2);

        gui.clear_logs().await;
        let logs = gui.get_logs(None).await;
        assert!(logs.is_empty());
    }

    #[tokio::test]
    async fn test_event_handling() {
        let gui = InstallerGui::default();

        let mut data = HashMap::new();
        data.insert("x".to_string(), "100".to_string());
        data.insert("y".to_string(), "200".to_string());

        gui.handle_remote_input("click", data).await.unwrap();

        let events = gui.process_events().await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, GuiEventType::Click);
    }
}
