use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::engine::llm_catalog::DownloadSource;
use crate::engine::llm_router::LlmSelection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub llm: LlmSettings,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub keyboard: KeyboardSettings,
}

/// The legacy flat shape had `base_url`, `api_key`, `model` directly on
/// `LlmSettings`. The new shape nests them under `remote` and adds
/// `selection` + `local_endpoint_port`. A custom `Deserialize` impl
/// handles both shapes transparently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    #[serde(default)]
    pub selection: LlmSelection,

    #[serde(default)]
    pub remote: RemoteLlmSettings,

    #[serde(default = "default_local_endpoint_port")]
    pub local_endpoint_port: u16,

    #[serde(default)]
    pub download_source: DownloadSource,

    #[serde(default)]
    pub load_on_startup: bool,

    // Legacy flat fields — accepted during deserialization so old
    // settings.json files parse without error. migrate_legacy_selection()
    // promotes these into `remote` and sets `selection: Remote` when both
    // base_url and api_key are present.
    #[serde(default, skip_serializing)]
    base_url: Option<String>,
    #[serde(default, skip_serializing)]
    api_key: Option<String>,
    #[serde(default, skip_serializing)]
    model: Option<String>,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            selection: LlmSelection::Disabled,
            remote: RemoteLlmSettings::default(),
            local_endpoint_port: default_local_endpoint_port(),
            download_source: DownloadSource::default(),
            load_on_startup: false,
            base_url: None,
            api_key: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteLlmSettings {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

fn default_local_endpoint_port() -> u16 {
    3001
}

/// Post-deserialization migration for legacy flat LlmSettings. If the
/// deserialized selection is Disabled but legacy flat fields or
/// remote.base_url + remote.api_key are populated, promote to Remote.
pub fn migrate_legacy_selection(llm: &mut LlmSettings) {
    // First, move any legacy flat fields into remote
    if llm.base_url.is_some() && llm.remote.base_url.is_none() {
        llm.remote.base_url = llm.base_url.take();
    }
    if llm.api_key.is_some() && llm.remote.api_key.is_none() {
        llm.remote.api_key = llm.api_key.take();
    }
    if llm.model.is_some() && llm.remote.model.is_none() {
        llm.remote.model = llm.model.take();
    }

    // Promote Disabled → Remote when credentials are present
    if llm.selection == LlmSelection::Disabled {
        if llm.remote.base_url.is_some() && llm.remote.api_key.is_some() {
            llm.selection = LlmSelection::Remote;
            info!("Migrated legacy LLM settings: promoted Disabled → Remote");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub device_name: Option<String>,
    pub asr_model: Option<String>,
    #[serde(default)]
    pub download_source: DownloadSource,
    /// Number of days to keep retained voiceprint-candidate clips on disk
    /// before the background cleanup task deletes them.
    #[serde(default = "default_clip_retention_days")]
    pub clip_retention_days: u32,
}

fn default_clip_retention_days() -> u32 {
    3
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            device_name: None,
            asr_model: None,
            download_source: DownloadSource::default(),
            clip_retention_days: default_clip_retention_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardSettings {
    #[serde(default = "default_shortcuts")]
    pub shortcuts: HashMap<String, String>,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            shortcuts: default_shortcuts(),
        }
    }
}

fn default_shortcuts() -> HashMap<String, String> {
    let mut m = HashMap::new();
    // Global
    m.insert("toggle_board_tray".into(), "Ctrl+\\".into());
    m.insert("start_dictation".into(), "Ctrl+Shift+Space".into());
    m.insert("new_todo".into(), "Ctrl+N".into());
    // Tab navigation
    m.insert("tab_board".into(), "Ctrl+1".into());
    m.insert("tab_people".into(), "Ctrl+2".into());
    m.insert("tab_recording".into(), "Ctrl+3".into());
    m.insert("tab_archive".into(), "Ctrl+4".into());
    m.insert("tab_settings".into(), "Ctrl+5".into());
    // Card navigation
    m.insert("card_up".into(), "ArrowUp".into());
    m.insert("card_down".into(), "ArrowDown".into());
    m.insert("card_expand".into(), "Enter".into());
    m.insert("card_archive".into(), "Delete".into());
    m
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            llm: LlmSettings::default(),
            audio: AudioSettings::default(),
            keyboard: KeyboardSettings::default(),
        }
    }
}

pub struct SettingsManager {
    path: PathBuf,
    settings: RwLock<AppSettings>,
}

impl SettingsManager {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join("settings.json");
        let mut settings: AppSettings = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(_) => AppSettings::default(),
            }
        } else {
            AppSettings::default()
        };

        // Post-deser migration: promote Disabled → Remote for legacy users
        migrate_legacy_selection(&mut settings.llm);

        // Env var bootstrap: seed Remote when LLM_BASE_URL + LLM_API_KEY are
        // set and settings.json had no LLM section at all.
        if settings.llm.selection == LlmSelection::Disabled
            && settings.llm.remote.base_url.is_none()
        {
            if let Some(cfg) = crate::config::LlmConfig::from_env_optional() {
                settings.llm.remote.base_url = Some(cfg.base_url);
                settings.llm.remote.api_key = Some(cfg.api_key);
                settings.llm.remote.model = Some(cfg.model);
                settings.llm.selection = LlmSelection::Remote;
                info!("Seeded LLM settings from env vars (LLM_BASE_URL + LLM_API_KEY)");
            }
        }

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
            if let Some(sel) = llm.selection {
                settings.llm.selection = sel;
            }
            if let Some(remote_patch) = llm.remote {
                if let Some(v) = remote_patch.base_url {
                    settings.llm.remote.base_url = Some(v);
                }
                if let Some(v) = remote_patch.api_key {
                    settings.llm.remote.api_key = Some(v);
                }
                if let Some(v) = remote_patch.model {
                    settings.llm.remote.model = Some(v);
                }
            }
            if let Some(p) = llm.local_endpoint_port {
                settings.llm.local_endpoint_port = p;
            }
            if let Some(src) = llm.download_source {
                settings.llm.download_source = src;
            }
            if let Some(v) = llm.load_on_startup {
                settings.llm.load_on_startup = v;
            }
            // Legacy flat-shape patches
            if let Some(v) = llm.base_url {
                settings.llm.remote.base_url = Some(v);
            }
            if let Some(v) = llm.api_key {
                settings.llm.remote.api_key = Some(v);
            }
            if let Some(v) = llm.model {
                settings.llm.remote.model = Some(v);
            }
        }
        if let Some(audio) = patch.audio {
            if let Some(v) = audio.device_name {
                settings.audio.device_name = Some(v);
            }
            if let Some(v) = audio.asr_model {
                settings.audio.asr_model = Some(v);
            }
            if let Some(v) = audio.download_source {
                settings.audio.download_source = v;
            }
        }
        if let Some(keyboard) = patch.keyboard {
            if let Some(shortcuts) = keyboard.shortcuts {
                for (k, v) in shortcuts {
                    settings.keyboard.shortcuts.insert(k, v);
                }
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsPatch {
    pub llm: Option<LlmSettingsPatch>,
    pub audio: Option<AudioSettingsPatch>,
    pub keyboard: Option<KeyboardSettingsPatch>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct KeyboardSettingsPatch {
    pub shortcuts: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LlmSettingsPatch {
    pub selection: Option<LlmSelection>,
    pub remote: Option<RemoteLlmSettingsPatch>,
    pub local_endpoint_port: Option<u16>,
    pub download_source: Option<DownloadSource>,
    pub load_on_startup: Option<bool>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct RemoteLlmSettingsPatch {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioSettingsPatch {
    pub device_name: Option<String>,
    pub asr_model: Option<String>,
    pub download_source: Option<DownloadSource>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_legacy_flat_llm_shape() {
        let json = r#"{
            "llm": {
                "base_url": "https://api.openai.com/v1",
                "api_key": "sk-legacy",
                "model": "gpt-4o-mini"
            },
            "audio": {}
        }"#;
        let mut settings: AppSettings = serde_json::from_str(json).unwrap();
        migrate_legacy_selection(&mut settings.llm);
        assert_eq!(
            settings.llm.remote.base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(settings.llm.remote.api_key.as_deref(), Some("sk-legacy"));
        assert_eq!(settings.llm.remote.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(settings.llm.selection, LlmSelection::Remote);
        assert_eq!(settings.llm.local_endpoint_port, 3001);
    }

    #[test]
    fn legacy_flat_shape_without_api_key_stays_disabled() {
        let json = r#"{
            "llm": {
                "base_url": "https://api.openai.com/v1"
            },
            "audio": {}
        }"#;
        let mut settings: AppSettings = serde_json::from_str(json).unwrap();
        migrate_legacy_selection(&mut settings.llm);
        assert_eq!(settings.llm.selection, LlmSelection::Disabled);
    }

    #[test]
    fn deserializes_new_nested_llm_shape() {
        let json = r#"{
            "llm": {
                "selection": {"kind": "local", "id": "qwen3.5-0.8b"},
                "remote": {"base_url": "https://example.com/v1", "api_key": null, "model": null},
                "local_endpoint_port": 11434
            },
            "audio": {}
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(matches!(
            settings.llm.selection,
            LlmSelection::Local { ref id } if id == "qwen3.5-0.8b"
        ));
        assert_eq!(settings.llm.local_endpoint_port, 11434);
        assert_eq!(
            settings.llm.remote.base_url.as_deref(),
            Some("https://example.com/v1")
        );
    }

    #[test]
    fn missing_llm_section_uses_defaults() {
        let json = r#"{"audio": {}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.llm.selection, LlmSelection::Disabled);
        assert_eq!(settings.llm.local_endpoint_port, 3001);
        assert!(settings.llm.remote.base_url.is_none());
    }
}
