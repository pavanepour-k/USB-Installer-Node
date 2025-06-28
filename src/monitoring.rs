use crate::config::MonitoringConfig;
use crate::error::{MonitoringError, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub struct Alert {
    pub id: String,
    pub severity: AlertSeverity,
    pub module: String,
    pub message: String,
    pub timestamp: SystemTime,
    pub resolved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub timestamp: SystemTime,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ServiceHealth {
    pub name: String,
    pub healthy: bool,
    pub uptime: Duration,
    pub last_check: Instant,
    pub error_count: u32,
    pub restart_count: u32,
}

pub trait Monitorable: Send + Sync {
    fn name(&self) -> &str;
    fn health_check(&self) -> impl std::future::Future<Output = Result<()>> + Send;
    fn restart(&mut self) -> impl std::future::Future<Output = Result<()>> + Send;
}

pub struct Monitor {
    config: Arc<RwLock<MonitoringConfig>>,
    services: Arc<RwLock<HashMap<String, Box<dyn Monitorable>>>>,
    health_status: Arc<RwLock<HashMap<String, ServiceHealth>>>,
    alerts: Arc<RwLock<Vec<Alert>>>,
    metrics: Arc<RwLock<Vec<Metric>>>,
    alert_tx: mpsc::Sender<Alert>,
    alert_rx: Arc<RwLock<mpsc::Receiver<Alert>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl Monitor {
    pub fn new(config: Arc<RwLock<MonitoringConfig>>) -> Self {
        let (alert_tx, alert_rx) = mpsc::channel(1000);

        Self {
            config,
            services: Arc::new(RwLock::new(HashMap::new())),
            health_status: Arc::new(RwLock::new(HashMap::new())),
            alerts: Arc::new(RwLock::new(Vec::new())),
            metrics: Arc::new(RwLock::new(Vec::new())),
            alert_tx,
            alert_rx: Arc::new(RwLock::new(alert_rx)),
            shutdown_tx: None,
        }
    }

    pub async fn register_service(&self, service: Box<dyn Monitorable>) {
        let name = service.name().to_string();
        let health = ServiceHealth {
            name: name.clone(),
            healthy: true,
            uptime: Duration::from_secs(0),
            last_check: Instant::now(),
            error_count: 0,
            restart_count: 0,
        };

        self.services.write().await.insert(name.clone(), service);
        self.health_status.write().await.insert(name, health);

        info!("Registered service for monitoring: {}", name);
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting monitoring service");

        let config = self.config.read().await;
        if !config.enabled {
            info!("Monitoring disabled");
            return Ok(());
        }

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let check_interval = Duration::from_secs(config.check_interval);
        let mut interval_timer = interval(check_interval);

        let services = self.services.clone();
        let health_status = self.health_status.clone();
        let alert_tx = self.alert_tx.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        Self::check_all_services(&services, &health_status, &alert_tx, &config).await;
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Monitoring shutdown received");
                        break;
                    }
                }
            }
        });

        self.start_alert_processor().await;
        self.start_metrics_collector().await;

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping monitoring service");

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        Ok(())
    }

    async fn check_all_services(
        services: &Arc<RwLock<HashMap<String, Box<dyn Monitorable>>>>,
        health_status: &Arc<RwLock<HashMap<String, ServiceHealth>>>,
        alert_tx: &mpsc::Sender<Alert>,
        config: &Arc<RwLock<MonitoringConfig>>,
    ) {
        let services = services.read().await;
        let mut status = health_status.write().await;
        let config = config.read().await;

        for (name, service) in services.iter() {
            let start = Instant::now();
            let result = service.health_check().await;

            if let Some(health) = status.get_mut(name) {
                health.last_check = start;

                match result {
                    Ok(_) => {
                        if !health.healthy {
                            health.healthy = true;
                            health.error_count = 0;

                            let alert = Alert {
                                id: uuid::Uuid::new_v4().to_string(),
                                severity: AlertSeverity::Info,
                                module: name.clone(),
                                message: format!("Service {} recovered", name),
                                timestamp: SystemTime::now(),
                                resolved: true,
                            };

                            let _ = alert_tx.send(alert).await;
                        }
                        health.uptime = health.uptime.saturating_add(config.check_interval);
                    }
                    Err(e) => {
                        health.healthy = false;
                        health.error_count += 1;

                        let severity = if health.error_count >= config.max_failures {
                            AlertSeverity::Critical
                        } else {
                            AlertSeverity::Warning
                        };

                        let alert = Alert {
                            id: uuid::Uuid::new_v4().to_string(),
                            severity,
                            module: name.clone(),
                            message: format!("Health check failed: {}", e),
                            timestamp: SystemTime::now(),
                            resolved: false,
                        };

                        let _ = alert_tx.send(alert).await;

                        if health.error_count >= config.max_failures && config.auto_restart {
                            warn!(
                                "Service {} exceeded failure threshold, attempting restart",
                                name
                            );
                            health.error_count = 0;
                            health.restart_count += 1;

                            drop(status);
                            drop(services);

                            if let Some(mut service) = services.write().await.get_mut(name) {
                                if let Err(e) = service.restart().await {
                                    error!("Failed to restart service {}: {}", name, e);
                                }
                            }

                            return;
                        }
                    }
                }
            }
        }
    }

    async fn start_alert_processor(&self) {
        let alerts = self.alerts.clone();
        let mut alert_rx = self.alert_rx.write().await;

        tokio::spawn(async move {
            while let Some(alert) = alert_rx.recv().await {
                match alert.severity {
                    AlertSeverity::Info => info!("[ALERT] {}: {}", alert.module, alert.message),
                    AlertSeverity::Warning => warn!("[ALERT] {}: {}", alert.module, alert.message),
                    AlertSeverity::Error => error!("[ALERT] {}: {}", alert.module, alert.message),
                    AlertSeverity::Critical => {
                        error!("[CRITICAL] {}: {}", alert.module, alert.message)
                    }
                }

                alerts.write().await.push(alert);
            }
        });
    }

    async fn start_metrics_collector(&self) {
        let metrics = self.metrics.clone();
        let health_status = self.health_status.clone();
        let mut interval_timer = interval(Duration::from_secs(60));

        tokio::spawn(async move {
            loop {
                interval_timer.tick().await;

                let status = health_status.read().await;
                let mut current_metrics = Vec::new();

                for (name, health) in status.iter() {
                    current_metrics.push(Metric {
                        name: "service_healthy".to_string(),
                        value: if health.healthy { 1.0 } else { 0.0 },
                        unit: "boolean".to_string(),
                        timestamp: SystemTime::now(),
                        labels: [("service".to_string(), name.clone())].into(),
                    });

                    current_metrics.push(Metric {
                        name: "service_uptime".to_string(),
                        value: health.uptime.as_secs_f64(),
                        unit: "seconds".to_string(),
                        timestamp: SystemTime::now(),
                        labels: [("service".to_string(), name.clone())].into(),
                    });

                    current_metrics.push(Metric {
                        name: "service_error_count".to_string(),
                        value: health.error_count as f64,
                        unit: "count".to_string(),
                        timestamp: SystemTime::now(),
                        labels: [("service".to_string(), name.clone())].into(),
                    });

                    current_metrics.push(Metric {
                        name: "service_restart_count".to_string(),
                        value: health.restart_count as f64,
                        unit: "count".to_string(),
                        timestamp: SystemTime::now(),
                        labels: [("service".to_string(), name.clone())].into(),
                    });
                }

                metrics.write().await.extend(current_metrics);
            }
        });
    }

    pub async fn get_alerts(&self, resolved: Option<bool>) -> Vec<Alert> {
        let alerts = self.alerts.read().await;

        match resolved {
            Some(r) => alerts.iter().filter(|a| a.resolved == r).cloned().collect(),
            None => alerts.clone(),
        }
    }

    pub async fn get_health_status(&self) -> HashMap<String, ServiceHealth> {
        self.health_status.read().await.clone()
    }

    pub async fn get_metrics(&self) -> Vec<Metric> {
        self.metrics.read().await.clone()
    }

    pub async fn get_prometheus_metrics(&self) -> String {
        let metrics = self.metrics.read().await;
        let mut output = String::new();

        for metric in metrics.iter() {
            output.push_str(&format!("# TYPE {} gauge\n", metric.name));

            let labels = metric
                .labels
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v))
                .collect::<Vec<_>>()
                .join(",");

            if labels.is_empty() {
                output.push_str(&format!("{} {}\n", metric.name, metric.value));
            } else {
                output.push_str(&format!("{}{{{}}} {}\n", metric.name, labels, metric.value));
            }
        }

        output
    }

    pub async fn clear_resolved_alerts(&self) {
        self.alerts.write().await.retain(|a| !a.resolved);
    }

    pub async fn reload_config(&self, config: Arc<RwLock<MonitoringConfig>>) {
        *self.config.write().await = config.read().await.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockService {
        name: String,
        healthy: bool,
    }

    impl Monitorable for MockService {
        fn name(&self) -> &str {
            &self.name
        }

        async fn health_check(&self) -> Result<()> {
            if self.healthy {
                Ok(())
            } else {
                Err(MonitoringError::HealthCheckFailed(
                    "Mock failure".to_string(),
                ))
            }
        }

        async fn restart(&mut self) -> Result<()> {
            self.healthy = true;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_monitor_creation() {
        let config = Arc::new(RwLock::new(MonitoringConfig::default()));
        let monitor = Monitor::new(config);

        assert!(monitor.get_health_status().await.is_empty());
        assert!(monitor.get_alerts(None).await.is_empty());
    }

    #[tokio::test]
    async fn test_service_registration() {
        let config = Arc::new(RwLock::new(MonitoringConfig::default()));
        let monitor = Monitor::new(config);

        let service = Box::new(MockService {
            name: "test_service".to_string(),
            healthy: true,
        });

        monitor.register_service(service).await;

        let status = monitor.get_health_status().await;
        assert_eq!(status.len(), 1);
        assert!(status.contains_key("test_service"));
    }

    #[tokio::test]
    async fn test_alert_creation() {
        let alert = Alert {
            id: "test-id".to_string(),
            severity: AlertSeverity::Warning,
            module: "test".to_string(),
            message: "Test alert".to_string(),
            timestamp: SystemTime::now(),
            resolved: false,
        };

        assert_eq!(alert.severity, AlertSeverity::Warning);
        assert!(!alert.resolved);
    }
}
