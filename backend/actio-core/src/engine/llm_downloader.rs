use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use crate::engine::llm_catalog::{available_local_llms, LocalLlmInfo};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LlmDownloadStatus {
    Idle,
    Downloading {
        llm_id: String,
        progress: f32,
        bytes_downloaded: u64,
        bytes_total: u64,
    },
    Error {
        llm_id: String,
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LlmDownloadError {
    #[error("another download is already in progress")]
    AlreadyInProgress,
    #[error("network error: {0}")]
    Network(String),
    #[error("hash mismatch — file corrupt, deleted")]
    HashMismatch,
    #[error("disk full or write failed: {0}")]
    DiskWrite(String),
    #[error("unknown model id {0}")]
    UnknownModel(String),
}

pub struct LlmDownloader {
    model_dir: PathBuf,
    status: Arc<RwLock<LlmDownloadStatus>>,
    download_lock: Arc<Mutex<()>>,
}

impl LlmDownloader {
    pub fn new(model_dir: PathBuf, download_lock: Arc<Mutex<()>>) -> Self {
        Self {
            model_dir,
            status: Arc::new(RwLock::new(LlmDownloadStatus::Idle)),
            download_lock,
        }
    }

    pub fn llm_root(&self) -> PathBuf {
        self.model_dir.join("llms")
    }

    pub fn gguf_path(&self, info: &LocalLlmInfo) -> PathBuf {
        self.llm_root().join(&info.id).join(&info.gguf_filename)
    }

    pub fn is_downloaded(&self, info: &LocalLlmInfo) -> bool {
        let p = self.gguf_path(info);
        std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false)
    }

    pub fn catalog_with_status(&self) -> Vec<LocalLlmInfo> {
        let mut catalog = available_local_llms();
        for entry in catalog.iter_mut() {
            entry.downloaded = self.is_downloaded(entry);
        }
        catalog
    }

    pub async fn current_status(&self) -> LlmDownloadStatus {
        self.status.read().await.clone()
    }

    pub async fn start_download(self: Arc<Self>, llm_id: String) -> Result<(), LlmDownloadError> {
        let info = available_local_llms()
            .into_iter()
            .find(|m| m.id == llm_id)
            .ok_or_else(|| LlmDownloadError::UnknownModel(llm_id.clone()))?;

        let lock_guard = self
            .download_lock
            .clone()
            .try_lock_owned()
            .map_err(|_| LlmDownloadError::AlreadyInProgress)?;

        let this = Arc::clone(&self);
        tokio::spawn(async move {
            let _guard = lock_guard;
            if let Err(e) = this.run_download(&info).await {
                warn!(llm_id = %info.id, error = %e, "LLM download failed");
                let mut status = this.status.write().await;
                *status = LlmDownloadStatus::Error {
                    llm_id: info.id.clone(),
                    message: e.to_string(),
                };
            } else {
                let mut status = this.status.write().await;
                *status = LlmDownloadStatus::Idle;
            }
        });

        Ok(())
    }

    async fn run_download(&self, info: &LocalLlmInfo) -> Result<(), LlmDownloadError> {
        let dir = self.llm_root().join(&info.id);
        std::fs::create_dir_all(&dir).map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;

        let final_path = dir.join(&info.gguf_filename);
        let partial_path = dir.join(format!("{}.partial", info.gguf_filename));

        if partial_path.exists() {
            let _ = std::fs::remove_file(&partial_path);
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            info.hf_repo, info.gguf_filename
        );
        info!(llm_id = %info.id, %url, "Starting LLM download");

        let resp = reqwest::Client::new()
            .get(&url)
            .send()
            .await
            .map_err(|e| LlmDownloadError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(LlmDownloadError::Network(format!(
                "HTTP {}: {}",
                resp.status(),
                url
            )));
        }

        let bytes_total = resp.content_length().unwrap_or(0);
        let mut bytes_downloaded: u64 = 0;
        {
            let mut status = self.status.write().await;
            *status = LlmDownloadStatus::Downloading {
                llm_id: info.id.clone(),
                progress: 0.0,
                bytes_downloaded: 0,
                bytes_total,
            };
        }

        let mut file = tokio::fs::File::create(&partial_path)
            .await
            .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
        let mut hasher = Sha256::new();

        use futures::StreamExt;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| LlmDownloadError::Network(e.to_string()))?;
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
            bytes_downloaded += chunk.len() as u64;
            let mut status = self.status.write().await;
            *status = LlmDownloadStatus::Downloading {
                llm_id: info.id.clone(),
                progress: if bytes_total > 0 {
                    bytes_downloaded as f32 / bytes_total as f32
                } else {
                    0.0
                },
                bytes_downloaded,
                bytes_total,
            };
        }
        file.flush()
            .await
            .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
        file.sync_all()
            .await
            .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
        drop(file);

        let actual_hash = format!("{:x}", hasher.finalize());
        if actual_hash != info.sha256 {
            warn!(
                llm_id = %info.id,
                expected = %info.sha256,
                actual = %actual_hash,
                "GGUF hash mismatch — deleting"
            );
            let _ = std::fs::remove_file(&partial_path);
            return Err(LlmDownloadError::HashMismatch);
        }

        std::fs::rename(&partial_path, &final_path)
            .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
        info!(llm_id = %info.id, "LLM download complete and verified");
        Ok(())
    }

    pub async fn delete(&self, llm_id: &str) -> Result<(), LlmDownloadError> {
        let info = available_local_llms()
            .into_iter()
            .find(|m| m.id == llm_id)
            .ok_or_else(|| LlmDownloadError::UnknownModel(llm_id.into()))?;
        let dir = self.llm_root().join(&info.id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_downloader(tmp: &TempDir) -> Arc<LlmDownloader> {
        Arc::new(LlmDownloader::new(
            tmp.path().to_path_buf(),
            Arc::new(Mutex::new(())),
        ))
    }

    #[test]
    fn llm_root_is_under_model_dir() {
        let tmp = TempDir::new().unwrap();
        let dl = make_downloader(&tmp);
        assert_eq!(dl.llm_root(), tmp.path().join("llms"));
    }

    #[test]
    fn is_downloaded_false_for_missing_file() {
        let tmp = TempDir::new().unwrap();
        let dl = make_downloader(&tmp);
        let catalog = available_local_llms();
        assert!(!dl.is_downloaded(&catalog[0]));
    }

    #[test]
    fn is_downloaded_true_for_present_file() {
        let tmp = TempDir::new().unwrap();
        let dl = make_downloader(&tmp);
        let catalog = available_local_llms();
        let info = &catalog[0];
        let dir = dl.llm_root().join(&info.id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(&info.gguf_filename), b"x").unwrap();
        assert!(dl.is_downloaded(info));
    }

    #[tokio::test]
    async fn current_status_starts_idle() {
        let tmp = TempDir::new().unwrap();
        let dl = make_downloader(&tmp);
        let s = dl.current_status().await;
        assert!(matches!(s, LlmDownloadStatus::Idle));
    }

    #[tokio::test]
    async fn second_download_while_first_in_progress_fails_fast() {
        let tmp = TempDir::new().unwrap();
        let lock = Arc::new(Mutex::new(()));
        let dl = Arc::new(LlmDownloader::new(tmp.path().to_path_buf(), lock.clone()));
        let _held = lock.lock().await;
        let result = dl.start_download("qwen3.5-0.8b-q4km".into()).await;
        assert!(matches!(result, Err(LlmDownloadError::AlreadyInProgress)));
    }

    #[tokio::test]
    async fn delete_removes_directory() {
        let tmp = TempDir::new().unwrap();
        let dl = make_downloader(&tmp);
        let catalog = available_local_llms();
        let info = &catalog[0];
        let dir = dl.llm_root().join(&info.id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(&info.gguf_filename), b"x").unwrap();
        dl.delete(&info.id).await.unwrap();
        assert!(!dir.exists());
    }
}
