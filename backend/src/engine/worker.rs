use std::path::{Path, PathBuf};
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
        let child = tokio::process::Command::new(worker_python_command())
            .arg(worker_script_path())
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
                                if let Ok(new_child) = tokio::process::Command::new(worker_python_command())
                                    .arg(worker_script_path())
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

fn worker_script_path() -> &'static str {
    "python-worker/main.py"
}

fn worker_python_command() -> PathBuf {
    let override_bin = std::env::var("PYTHON_WORKER_BIN").ok();
    resolve_worker_python(Path::new("."), override_bin.as_deref())
}

fn resolve_worker_python(root: &Path, override_bin: Option<&str>) -> PathBuf {
    if let Some(bin) = override_bin.filter(|bin| !bin.trim().is_empty()) {
        return PathBuf::from(bin);
    }

    let venv_python = if cfg!(windows) {
        root.join("python-worker").join(".venv").join("Scripts").join("python.exe")
    } else {
        root.join("python-worker").join(".venv").join("bin").join("python")
    };

    if venv_python.exists() {
        return venv_python;
    }

    if cfg!(windows) {
        PathBuf::from("python")
    } else {
        PathBuf::from("python3")
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_worker_python;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn expected_venv_path(root: &Path) -> PathBuf {
        if cfg!(windows) {
            root.join("python-worker").join(".venv").join("Scripts").join("python.exe")
        } else {
            root.join("python-worker").join(".venv").join("bin").join("python")
        }
    }

    #[test]
    fn prefers_local_venv_python_when_present() {
        let temp_dir = tempfile::tempdir().unwrap();
        let venv_python = expected_venv_path(temp_dir.path());
        fs::create_dir_all(venv_python.parent().unwrap()).unwrap();
        fs::write(&venv_python, b"").unwrap();

        let resolved = resolve_worker_python(temp_dir.path(), None);

        assert_eq!(resolved, venv_python);
    }

    #[test]
    fn falls_back_to_platform_python_when_local_venv_is_missing() {
        let temp_dir = tempfile::tempdir().unwrap();
        let resolved = resolve_worker_python(temp_dir.path(), None);
        let expected = if cfg!(windows) { "python" } else { "python3" };

        assert_eq!(resolved, PathBuf::from(expected));
    }

    #[test]
    fn prefers_explicit_override_over_local_venv() {
        let temp_dir = tempfile::tempdir().unwrap();
        let venv_python = expected_venv_path(temp_dir.path());
        fs::create_dir_all(venv_python.parent().unwrap()).unwrap();
        fs::write(&venv_python, b"").unwrap();

        let resolved = resolve_worker_python(temp_dir.path(), Some("custom-python"));

        assert_eq!(resolved, PathBuf::from("custom-python"));
    }
}
