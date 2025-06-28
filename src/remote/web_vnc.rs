use crate::error::{RemoteError, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct WebVncConfig {
    pub listen_port: u16,
    pub vnc_host: String,
    pub vnc_port: u16,
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    pub enable_auth: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub session_timeout: u64,
}

impl Default for WebVncConfig {
    fn default() -> Self {
        Self {
            listen_port: 6080,
            vnc_host: "localhost".to_string(),
            vnc_port: 5900,
            cert_path: None,
            key_path: None,
            enable_auth: false,
            username: None,
            password: None,
            session_timeout: 3600,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebSession {
    pub session_id: String,
    pub client_address: String,
    pub created_at: std::time::SystemTime,
    pub last_activity: std::time::SystemTime,
}

pub struct WebVncServer {
    config: Arc<RwLock<WebVncConfig>>,
    process: Arc<RwLock<Option<Child>>>,
    sessions: Arc<RwLock<HashMap<String, WebSession>>>,
    proxy_health: Arc<RwLock<bool>>,
}

impl WebVncServer {
    pub fn new(config: WebVncConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            process: Arc::new(RwLock::new(None)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            proxy_health: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        if self.is_running().await {
            return Err(RemoteError::AlreadyRunning("Web VNC server".to_string()));
        }

        info!("Starting Web VNC server");

        let config = self.config.read().await;

        if config.enable_auth && (config.username.is_none() || config.password.is_none()) {
            return Err(RemoteError::ConfigError(
                "Authentication enabled but username/password not set".to_string(),
            ));
        }

        let mut cmd = Command::new("websockify");

        cmd.arg("--web").arg("/usr/share/novnc");

        if let (Some(cert), Some(key)) = (&config.cert_path, &config.key_path) {
            cmd.arg("--cert").arg(cert);
            cmd.arg("--key").arg(key);
        } else {
            self.generate_self_signed_cert().await?;
            cmd.arg("--cert").arg("/tmp/novnc.crt");
            cmd.arg("--key").arg("/tmp/novnc.key");
        }

        if config.enable_auth {
            let auth_file = self.create_auth_file().await?;
            cmd.arg("--auth-plugin").arg("BasicHTTPAuth");
            cmd.arg("--auth-source").arg(auth_file);
        }

        cmd.arg(format!("0.0.0.0:{}", config.listen_port));
        cmd.arg(format!("{}:{}", config.vnc_host, config.vnc_port));

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!("Executing websockify command: {:?}", cmd);

        let child = cmd
            .spawn()
            .map_err(|e| RemoteError::StartFailed(format!("Failed to start websockify: {}", e)))?;

        *self.process.write().await = Some(child);

        sleep(Duration::from_secs(1)).await;

        if !self.is_running().await {
            return Err(RemoteError::StartFailed(
                "Web VNC server exited immediately".to_string(),
            ));
        }

        *self.proxy_health.write().await = true;
        self.start_health_monitor();

        info!("Web VNC server started on port {}", config.listen_port);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        if let Some(mut child) = self.process.write().await.take() {
            info!("Stopping Web VNC server");

            child
                .kill()
                .map_err(|e| RemoteError::StopFailed(format!("Failed to kill process: {}", e)))?;

            self.sessions.write().await.clear();
            *self.proxy_health.write().await = false;

            info!("Web VNC server stopped");
        }
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        if let Some(child) = &mut *self.process.write().await {
            match child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    async fn generate_self_signed_cert(&self) -> Result<()> {
        info!("Generating self-signed certificate for HTTPS");

        let output = Command::new("openssl")
            .args(&[
                "req",
                "-x509",
                "-nodes",
                "-days",
                "365",
                "-newkey",
                "rsa:2048",
                "-keyout",
                "/tmp/novnc.key",
                "-out",
                "/tmp/novnc.crt",
                "-subj",
                "/C=US/ST=State/L=City/O=Organization/CN=localhost",
            ])
            .output()
            .map_err(|e| RemoteError::CertGenerationFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(RemoteError::CertGenerationFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(())
    }

    async fn create_auth_file(&self) -> Result<PathBuf> {
        let config = self.config.read().await;
        let auth_file = PathBuf::from("/tmp/novnc_auth");

        if let (Some(user), Some(pass)) = (&config.username, &config.password) {
            let content = format!("{}:{}\n", user, pass);
            tokio::fs::write(&auth_file, content)
                .await
                .map_err(|e| RemoteError::IoError(e.to_string()))?;
        }

        Ok(auth_file)
    }

    fn start_health_monitor(&self) {
        let proxy_health = self.proxy_health.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(10)).await;

                if !*proxy_health.read().await {
                    break;
                }

                let config = config.read().await;
                let url = format!("http://localhost:{}/vnc.html", config.listen_port);

                match reqwest::get(&url).await {
                    Ok(response) => {
                        if !response.status().is_success() {
                            warn!("Web VNC health check failed: {}", response.status());
                        }
                    }
                    Err(e) => {
                        error!("Web VNC health check error: {}", e);
                    }
                }
            }
        });
    }

    pub async fn create_session(&self, client_address: String) -> String {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = WebSession {
            session_id: session_id.clone(),
            client_address,
            created_at: std::time::SystemTime::now(),
            last_activity: std::time::SystemTime::now(),
        };

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);
        session_id
    }

    pub async fn update_session_activity(&self, session_id: &str) {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.last_activity = std::time::SystemTime::now();
        }
    }

    pub async fn cleanup_expired_sessions(&self) {
        let config = self.config.read().await;
        let timeout = Duration::from_secs(config.session_timeout);
        let now = std::time::SystemTime::now();

        self.sessions.write().await.retain(|_, session| {
            if let Ok(elapsed) = now.duration_since(session.last_activity) {
                elapsed < timeout
            } else {
                true
            }
        });
    }

    pub async fn get_health_status(&self) -> bool {
        *self.proxy_health.read().await && self.is_running().await
    }

    pub async fn get_status(&self) -> HashMap<String, String> {
        let mut status = HashMap::new();

        status.insert("running".to_string(), self.is_running().await.to_string());
        status.insert(
            "healthy".to_string(),
            self.get_health_status().await.to_string(),
        );
        status.insert(
            "sessions".to_string(),
            self.sessions.read().await.len().to_string(),
        );

        let config = self.config.read().await;
        status.insert("listen_port".to_string(), config.listen_port.to_string());
        status.insert(
            "vnc_backend".to_string(),
            format!("{}:{}", config.vnc_host, config.vnc_port),
        );
        status.insert("auth_enabled".to_string(), config.enable_auth.to_string());

        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_web_vnc_server_creation() {
        let config = WebVncConfig::default();
        let server = WebVncServer::new(config);
        assert!(!server.is_running().await);
        assert!(server.sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_session_management() {
        let server = WebVncServer::new(WebVncConfig::default());

        let session_id = server.create_session("192.168.1.100".to_string()).await;
        assert_eq!(server.sessions.read().await.len(), 1);

        server.update_session_activity(&session_id).await;

        let session = server.sessions.read().await.get(&session_id).cloned();
        assert!(session.is_some());
        assert_eq!(session.unwrap().client_address, "192.168.1.100");
    }

    #[tokio::test]
    async fn test_config_validation() {
        let mut config = WebVncConfig::default();
        config.enable_auth = true;
        config.username = None;
        config.password = None;

        let server = WebVncServer::new(config);
        let result = server.start().await;

        assert!(result.is_err());
        if let Err(RemoteError::ConfigError(_)) = result {
        } else {
            panic!("Expected ConfigError");
        }
    }
}
