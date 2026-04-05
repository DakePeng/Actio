use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Child;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};

pub struct WorkerManager {
    host: String,
    port: u16,
    process: Arc<Mutex<Option<Child>>>,
    restart_count: Arc<Mutex<u32>>,
}

impl WorkerManager {
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            process: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        info!(port = self.port, "Starting Python Worker");
        let child = tokio::process::Command::new("python3")
            .arg("python-worker/main.py")
            .env("WORKER_HOST", &self.host)
            .env("WORKER_PORT", self.port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        *self.process.lock().await = Some(child);
        sleep(Duration::from_secs(2)).await;
        info!("Python Worker started");

        self.spawn_monitor().await;
        Ok(())
    }

    async fn spawn_monitor(&self) {
        let process = self.process.clone();
        let restart_count = self.restart_count.clone();
        let host = self.host.clone();
        let port = self.port;

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(3)).await;
                let mut proc = process.lock().await;
                if let Some(ref mut child) = *proc {
                    match child.try_wait() {
                        Ok(None) => {
                            // Still running
                        }
                        Ok(Some(status)) => {
                            warn!(exit_code = ?status.code(), "Python Worker exited");
                            *proc = None;
                            let mut count = restart_count.lock().await;
                            if *count < 3 {
                                *count += 1;
                                warn!(attempt = *count, "Restarting Python Worker");
                                drop(proc);
                                drop(count);
                                if let Ok(new_child) = tokio::process::Command::new("python3")
                                    .arg("python-worker/main.py")
                                    .env("WORKER_HOST", &host)
                                    .env("WORKER_PORT", port.to_string())
                                    .stdout(Stdio::piped())
                                    .stderr(Stdio::piped())
                                    .spawn()
                                {
                                    *process.lock().await = Some(new_child);
                                }
                            } else {
                                error!("Max restarts reached; worker in failed state");
                            }
                        }
                        Err(e) => {
                            error!("Failed to check worker status: {}", e);
                        }
                    }
                }
            }
        });
    }

    #[allow(dead_code)]
    pub async fn stop(&self) {
        let mut proc = self.process.lock().await;
        if let Some(mut child) = proc.take() {
            let _ = child.kill().await;
            info!("Python Worker stopped");
        }
    }
}
