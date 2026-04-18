use std::path::PathBuf;

use serde::Serialize;
use tracing::info;

use crate::engine::llm_catalog::{available_local_llms, DownloadSource, LocalLlmInfo};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LlmDownloadStatus {
    Idle,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmDownloadError {
    #[error("disk full or write failed: {0}")]
    DiskWrite(String),
    #[error("unknown model id {0}")]
    UnknownModel(String),
    #[error("download failed: {0}")]
    DownloadFailed(String),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
}

pub struct LlmDownloader {
    model_dir: PathBuf,
}

impl LlmDownloader {
    pub fn new(model_dir: PathBuf) -> Self {
        Self { model_dir }
    }

    pub fn llm_root(&self) -> PathBuf {
        self.model_dir.join("llms")
    }

    /// Path where a model's GGUF file is stored.
    pub fn gguf_path(&self, info: &LocalLlmInfo) -> PathBuf {
        self.model_dir
            .join("llms")
            .join(&info.id)
            .join(&info.gguf_filename)
    }

    /// Check if a model's GGUF file is already downloaded.
    pub fn has_gguf(&self, info: &LocalLlmInfo) -> bool {
        self.gguf_path(info).exists()
    }

    pub fn catalog(&self) -> Vec<LocalLlmInfo> {
        available_local_llms()
    }

    pub async fn current_status(&self) -> LlmDownloadStatus {
        LlmDownloadStatus::Idle
    }

    /// Download a GGUF model file via HTTP with progress reporting.
    ///
    /// Progress is sent as a float in [0.0, 1.0] via the `progress_tx` channel.
    /// The download is async (reqwest streaming) and cancellable via task abort.
    pub async fn download_gguf(
        &self,
        info: &LocalLlmInfo,
        source: DownloadSource,
        progress_tx: tokio::sync::watch::Sender<f32>,
    ) -> Result<PathBuf, LlmDownloadError> {
        let url = source.resolve_url(&info.sources);
        let dest = self.gguf_path(info);

        info!(
            llm_id = %info.id,
            url = %url,
            dest = %dest.display(),
            "Starting GGUF download"
        );

        // Ensure parent directory exists
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
        }

        let client = reqwest::Client::builder()
            .user_agent("Actio/0.1")
            .build()
            .map_err(|e| LlmDownloadError::DownloadFailed(e.to_string()))?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| LlmDownloadError::DownloadFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmDownloadError::DownloadFailed(format!(
                "HTTP {}: {}",
                response.status(),
                url
            )));
        }

        let total_size = response.content_length().unwrap_or(0);
        let expected_size = info.size_mb as u64 * 1_000_000;
        let size_for_progress = if total_size > 0 {
            total_size
        } else {
            expected_size
        };

        // Stream to a temporary file, then rename on success
        let tmp_dest = dest.with_extension("gguf.part");

        let mut file = tokio::fs::File::create(&tmp_dest)
            .await
            .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;

        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;

        let mut stream = response.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| LlmDownloadError::DownloadFailed(e.to_string()))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;

            if size_for_progress > 0 {
                let progress = (downloaded as f32 / size_for_progress as f32).min(0.99);
                let _ = progress_tx.send(progress);
            }
        }

        file.flush()
            .await
            .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
        drop(file);

        // Rename temp file to final destination
        tokio::fs::rename(&tmp_dest, &dest)
            .await
            .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;

        let _ = progress_tx.send(1.0);

        info!(
            llm_id = %info.id,
            downloaded_mb = downloaded / 1_000_000,
            "GGUF download complete"
        );

        Ok(dest)
    }

    /// Delete a model's GGUF file and any partial downloads.
    pub async fn delete(&self, llm_id: &str) -> Result<(), LlmDownloadError> {
        let info = available_local_llms()
            .into_iter()
            .find(|m| m.id == llm_id)
            .ok_or_else(|| LlmDownloadError::UnknownModel(llm_id.into()))?;

        let gguf = self.gguf_path(&info);
        let partial = gguf.with_extension("gguf.part");

        // Remove the GGUF file
        if gguf.exists() {
            tokio::fs::remove_file(&gguf)
                .await
                .map_err(|e| LlmDownloadError::DiskWrite(e.to_string()))?;
            info!(llm_id = %llm_id, path = %gguf.display(), "Deleted GGUF file");
        }

        // Remove any partial download
        if partial.exists() {
            let _ = tokio::fs::remove_file(&partial).await;
        }

        // Try to remove the model directory if now empty
        let model_dir = self.model_dir.join("llms").join(llm_id);
        if model_dir.exists() {
            let _ = tokio::fs::remove_dir(&model_dir).await; // only succeeds if empty
        }

        Ok(())
    }

    /// Clean up any partial download for a model (e.g., after cancelled download).
    pub async fn cleanup_partial(&self, info: &LocalLlmInfo) {
        let partial = self.gguf_path(info).with_extension("gguf.part");
        if partial.exists() {
            let _ = tokio::fs::remove_file(&partial).await;
            info!(llm_id = %info.id, "Cleaned up partial download");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_downloader() -> LlmDownloader {
        let tmp = std::env::temp_dir().join("actio-test-llm");
        LlmDownloader::new(tmp)
    }

    #[test]
    fn llm_root_is_under_model_dir() {
        let dl = make_downloader();
        assert!(dl.llm_root().ends_with("llms"));
    }

    #[tokio::test]
    async fn current_status_is_idle() {
        let dl = make_downloader();
        let s = dl.current_status().await;
        assert!(matches!(s, LlmDownloadStatus::Idle));
    }

    #[tokio::test]
    async fn delete_unknown_model_fails() {
        let dl = make_downloader();
        let result = dl.delete("does-not-exist").await;
        assert!(matches!(result, Err(LlmDownloadError::UnknownModel(_))));
    }

    #[test]
    fn has_gguf_returns_false_for_missing_file() {
        let dl = make_downloader();
        let info = available_local_llms().into_iter().next().unwrap();
        assert!(!dl.has_gguf(&info));
    }

    #[test]
    fn gguf_path_is_correct() {
        let dl = make_downloader();
        let info = available_local_llms().into_iter().next().unwrap();
        let path = dl.gguf_path(&info);
        assert!(path.ends_with("qwen3.5-0.8b/Qwen3.5-0.8B-Q4_K_M.gguf"));
    }
}
