use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub llm: LlmSettings,
    #[serde(default)]
    pub audio: AudioSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmSettings {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AudioSettings {
    pub device_name: Option<String>,
    /// Active ASR model ID: "ctc-zh" or "transducer-en"
    pub asr_model: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            llm: LlmSettings::default(),
            audio: AudioSettings::default(),
        }
    }
}

pub struct SettingsManager {
    path: PathBuf,
    settings: RwLock<AppSettings>,
}

impl SettingsManager {
    /// Load settings from file, or create default if not found.
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join("settings.json");
        let settings = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(_) => AppSettings::default(),
            }
        } else {
            AppSettings::default()
        };
        info!(?path, "Settings loaded");
        Self {
            path,
            settings: RwLock::new(settings),
        }
    }

    pub async fn get(&self) -> AppSettings {
        self.settings.read().await.clone()
    }

    pub async fn update(&self, patch: SettingsPatch) -> AppSettings {
        let mut settings = self.settings.write().await;
        if let Some(llm) = patch.llm {
            if let Some(v) = llm.base_url {
                settings.llm.base_url = Some(v);
            }
            if let Some(v) = llm.api_key {
                settings.llm.api_key = Some(v);
            }
            if let Some(v) = llm.model {
                settings.llm.model = Some(v);
            }
        }
        if let Some(audio) = patch.audio {
            if let Some(v) = audio.device_name {
                settings.audio.device_name = Some(v);
            }
            if let Some(v) = audio.asr_model {
                settings.audio.asr_model = Some(v);
            }
        }
        // Save to disk
        if let Ok(json) = serde_json::to_string_pretty(&*settings) {
            if let Err(e) = std::fs::write(&self.path, json) {
                warn!(error = %e, "Failed to save settings");
            }
        }
        settings.clone()
    }
}

#[derive(Debug, Deserialize)]
pub struct SettingsPatch {
    pub llm: Option<LlmSettingsPatch>,
    pub audio: Option<AudioSettingsPatch>,
}

#[derive(Debug, Deserialize)]
pub struct LlmSettingsPatch {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AudioSettingsPatch {
    pub device_name: Option<String>,
    pub asr_model: Option<String>,
}
