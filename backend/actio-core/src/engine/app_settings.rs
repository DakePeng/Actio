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
    /// User-selected UI language. `None` means "not yet chosen" — the
    /// frontend falls back to the OS locale on first launch. Expected
    /// values are BCP-47 codes like `"en"` or `"zh-CN"`; the backend
    /// stores the string as-is and only uses it to drive the default
    /// ASR / embedding recommender when the language transitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
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

/// Post-deser migration: copy a stale `tab_recording` shortcut binding to
/// `tab_live` (renamed in the always-on listening feature) and drop the
/// old key. No-op if `tab_live` is already set or `tab_recording` is
/// absent. Runs once per process via `SettingsManager::new`.
pub fn migrate_tab_recording_shortcut(keyboard: &mut KeyboardSettings) {
    let Some(value) = keyboard.shortcuts.remove("tab_recording") else {
        return;
    };
    keyboard.shortcuts.entry("tab_live".to_string()).or_insert(value);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub device_name: Option<String>,
    pub asr_model: Option<String>,
    /// Selected speaker-embedding model id from the Common Models catalog.
    /// `None` means no embedding backend chosen yet — the app will reject
    /// voiceprint enrollment with an actionable error.
    #[serde(default)]
    pub speaker_embedding_model: Option<String>,
    #[serde(default)]
    pub download_source: DownloadSource,
    /// Number of days to keep retained voiceprint-candidate clips on disk
    /// before the background cleanup task deletes them.
    #[serde(default = "default_clip_retention_days")]
    pub clip_retention_days: u32,
    /// Cosine similarity at or above which a match is called "confirmed"
    /// (rendered with full opacity + colour). Typical 0.50–0.65.
    #[serde(default = "default_speaker_confirm_threshold")]
    pub speaker_confirm_threshold: f32,
    /// Cosine similarity at or above which a match is called "tentative"
    /// (rendered dimmed with a `?` badge). Typical 0.35–0.45.
    #[serde(default = "default_speaker_tentative_threshold")]
    pub speaker_tentative_threshold: f32,
    /// VAD segments shorter than this many milliseconds are skipped by the
    /// identifier entirely — short clips yield unstable embeddings and tend
    /// to produce false negatives regardless of the actual speaker.
    #[serde(default = "default_speaker_min_duration_ms")]
    pub speaker_min_duration_ms: u32,
    /// Milliseconds of time-decay window for the continuity state machine.
    /// When a Confirmed match is received, subsequent Unknown / weak
    /// segments within this window inherit that speaker. 0 disables
    /// carry-over entirely. Clamped to [0, 60000] on patch.
    #[serde(default = "default_speaker_continuity_window_ms")]
    pub speaker_continuity_window_ms: u32,
    /// Whether the inference pipeline stays running whenever the process is
    /// up. When false (legacy behaviour), pipeline_supervisor hibernates
    /// the pipeline any time no WebSocket subscriber is attached. The
    /// windowed extractor only produces cards for sessions that are still
    /// recording, so the feature only meaningfully works with this toggle
    /// on — which is the default.
    #[serde(default = "default_always_listening")]
    pub always_listening: bool,
    /// Window length in milliseconds for the background action extractor.
    /// Defaults to 5 minutes.
    #[serde(default = "default_window_length_ms")]
    pub window_length_ms: u32,
    /// Step size between consecutive windows. Must be <= window_length_ms;
    /// overlap is `window_length_ms - window_step_ms`.
    #[serde(default = "default_window_step_ms")]
    pub window_step_ms: u32,
    /// How often the scheduler wakes to check for new windows to process.
    #[serde(default = "default_extraction_tick_secs")]
    pub extraction_tick_secs: u32,

    /// Per-mode ASR model selection. `live_asr_model` drives dictation/
    /// translation; `archive_asr_model` drives the batch processor. Both
    /// fall back to the legacy `asr_model` if unset (read-time migration).
    #[serde(default)]
    pub live_asr_model: Option<String>,
    #[serde(default)]
    pub archive_asr_model: Option<String>,

    /// Target clip duration in seconds before the boundary watcher starts
    /// looking for a silence to close on. Default 300 (5 min).
    #[serde(default = "default_clip_target_secs")]
    pub clip_target_secs: u32,
    /// Hard cap — clip force-closes at this duration even mid-utterance.
    #[serde(default = "default_clip_max_secs")]
    pub clip_max_secs: u32,
    /// Minimum VAD silence duration to count as a clip boundary, once past
    /// `clip_target_secs`. Default 1500 ms.
    #[serde(default = "default_clip_close_silence_ms")]
    pub clip_close_silence_ms: u32,

    /// AHC cosine threshold inside `cluster::ahc`. Smaller = more clusters.
    #[serde(default = "default_cluster_cosine_threshold")]
    pub cluster_cosine_threshold: f32,

    /// Per-clip WAV files older than this many days are swept by the
    /// background cleanup task. Replaces the per-failed-segment retention
    /// path that used `clip_retention_days`.
    #[serde(default = "default_audio_retention_days")]
    pub audio_retention_days: u32,
    /// Provisional speakers (kind='provisional') with no match in this many
    /// days are GC'd (DELETE cascades their attached segments' speaker_id).
    #[serde(default = "default_provisional_voiceprint_gc_days")]
    pub provisional_voiceprint_gc_days: u32,

    /// Opt-in for the new batch-clip-processing pipeline. When true,
    /// start_server boots the always-on CaptureDaemon + ClipWriter +
    /// BatchProcessor instead of the legacy InferencePipeline supervisor.
    /// The two paths are mutually exclusive — both would try to grab the
    /// microphone. Default false so existing installs are unchanged.
    #[serde(default)]
    pub use_batch_pipeline: bool,
}

fn default_clip_retention_days() -> u32 {
    3
}

fn default_speaker_confirm_threshold() -> f32 {
    0.55
}

fn default_speaker_tentative_threshold() -> f32 {
    0.40
}

fn default_speaker_min_duration_ms() -> u32 {
    1500
}

fn default_speaker_continuity_window_ms() -> u32 {
    15_000
}

fn default_always_listening() -> bool {
    true
}

fn default_window_length_ms() -> u32 {
    5 * 60 * 1000
}

fn default_window_step_ms() -> u32 {
    4 * 60 * 1000
}

fn default_extraction_tick_secs() -> u32 {
    60
}

fn default_clip_target_secs() -> u32 {
    300
}

fn default_clip_max_secs() -> u32 {
    360
}

fn default_clip_close_silence_ms() -> u32 {
    1500
}

fn default_cluster_cosine_threshold() -> f32 {
    0.4
}

fn default_audio_retention_days() -> u32 {
    14
}

fn default_provisional_voiceprint_gc_days() -> u32 {
    30
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            device_name: None,
            asr_model: None,
            speaker_embedding_model: None,
            download_source: DownloadSource::default(),
            clip_retention_days: default_clip_retention_days(),
            speaker_confirm_threshold: default_speaker_confirm_threshold(),
            speaker_tentative_threshold: default_speaker_tentative_threshold(),
            speaker_min_duration_ms: default_speaker_min_duration_ms(),
            speaker_continuity_window_ms: default_speaker_continuity_window_ms(),
            always_listening: default_always_listening(),
            window_length_ms: default_window_length_ms(),
            window_step_ms: default_window_step_ms(),
            extraction_tick_secs: default_extraction_tick_secs(),
            live_asr_model: None,
            archive_asr_model: None,
            clip_target_secs: default_clip_target_secs(),
            clip_max_secs: default_clip_max_secs(),
            clip_close_silence_ms: default_clip_close_silence_ms(),
            cluster_cosine_threshold: default_cluster_cosine_threshold(),
            audio_retention_days: default_audio_retention_days(),
            provisional_voiceprint_gc_days: default_provisional_voiceprint_gc_days(),
            use_batch_pipeline: false,
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
    m.insert("tab_live".into(), "Ctrl+3".into());
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
            language: None,
        }
    }
}

/// ASR model ids that cover Chinese audio at reasonable latency/quality.
/// Ordered from preferred to fallback. Anything outside this list is
/// considered "English-only" for the purpose of the recommender.
const CHINESE_CAPABLE_ASR: &[&str] = &[
    "sense_voice_multi",
    "zipformer_ctc_zh_small",
    "paraformer_zh_small",
    "zhen_zipformer_bilingual",
    "zh_zipformer_14m",
    "zh_conformer",
    "zh_lstm",
    "funasr_nano",
];

const CHINESE_CAPABLE_EMBEDDING: &[&str] = &[
    "campplus_zh_en",
    "campplus_zh",
    "eres2net_base",
    "eres2netv2",
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageRecommendations {
    pub asr_model: Option<String>,
    pub speaker_embedding_model: Option<String>,
}

/// Recommend ASR + embedding defaults when language transitions. Returns a
/// suggested id per slot **only when the current selection lacks coverage
/// for the target language** — so users who have already picked a
/// compatible model are never silently overridden.
pub fn recommend_models_for_language(
    lang: &str,
    current: &AudioSettings,
) -> LanguageRecommendations {
    let wants_zh = lang.to_lowercase().starts_with("zh");
    let mut recs = LanguageRecommendations::default();

    if wants_zh {
        let asr_ok = current
            .asr_model
            .as_deref()
            .map(|id| CHINESE_CAPABLE_ASR.contains(&id))
            .unwrap_or(false);
        if !asr_ok {
            recs.asr_model = Some("sense_voice_multi".to_string());
        }
        let emb_ok = current
            .speaker_embedding_model
            .as_deref()
            .map(|id| CHINESE_CAPABLE_EMBEDDING.contains(&id))
            .unwrap_or(false);
        if !emb_ok {
            recs.speaker_embedding_model = Some("campplus_zh_en".to_string());
        }
    }
    // For English we leave the current selection alone; multilingual models
    // still handle English fine, and single-Chinese models simply don't
    // transcribe English but we don't force-swap — that's a user choice.

    recs
}

pub struct ResolvedAsrModels {
    pub live: Option<String>,
    pub archive: Option<String>,
}

impl AudioSettings {
    /// Resolve the live and archive ASR model selections, falling back to
    /// the legacy `asr_model` when either is unset. Single source of truth
    /// for callers that need to know which model to load.
    pub fn resolved_asr_models(&self) -> ResolvedAsrModels {
        ResolvedAsrModels {
            live: self.live_asr_model.clone().or_else(|| self.asr_model.clone()),
            archive: self.archive_asr_model.clone().or_else(|| self.asr_model.clone()),
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
        migrate_tab_recording_shortcut(&mut settings.keyboard);

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
            if let Some(v) = audio.speaker_embedding_model {
                settings.audio.speaker_embedding_model = Some(v);
            }
            if let Some(v) = audio.download_source {
                settings.audio.download_source = v;
            }
            if let Some(v) = audio.speaker_confirm_threshold {
                settings.audio.speaker_confirm_threshold = v.clamp(0.0, 1.0);
            }
            if let Some(v) = audio.speaker_tentative_threshold {
                settings.audio.speaker_tentative_threshold = v.clamp(0.0, 1.0);
            }
            if let Some(v) = audio.speaker_min_duration_ms {
                settings.audio.speaker_min_duration_ms = v;
            }
            if let Some(v) = audio.speaker_continuity_window_ms {
                settings.audio.speaker_continuity_window_ms = v.min(60_000);
            }
            if let Some(v) = audio.always_listening {
                settings.audio.always_listening = v;
            }
            if let Some(v) = audio.window_length_ms {
                // 1–15 min; below 1 min is useless LLM fodder, above 15 min
                // blows out token budgets and extraction latency.
                settings.audio.window_length_ms = v.clamp(60_000, 15 * 60 * 1000);
            }
            if let Some(v) = audio.window_step_ms {
                // step must be <= length so consecutive windows don't skip
                // audio. Clamp to [30s, length].
                let max_step = settings.audio.window_length_ms;
                settings.audio.window_step_ms = v.clamp(30_000, max_step);
            }
            if let Some(v) = audio.extraction_tick_secs {
                // 10s – 5min.
                settings.audio.extraction_tick_secs = v.clamp(10, 300);
            }
            if let Some(v) = audio.live_asr_model {
                settings.audio.live_asr_model = v;
            }
            if let Some(v) = audio.archive_asr_model {
                settings.audio.archive_asr_model = v;
            }
            if let Some(v) = audio.clip_target_secs {
                settings.audio.clip_target_secs = v;
            }
            if let Some(v) = audio.clip_max_secs {
                settings.audio.clip_max_secs = v;
            }
            if let Some(v) = audio.clip_close_silence_ms {
                settings.audio.clip_close_silence_ms = v;
            }
            if let Some(v) = audio.cluster_cosine_threshold {
                settings.audio.cluster_cosine_threshold = v.clamp(0.0, 1.0);
            }
            if let Some(v) = audio.audio_retention_days {
                settings.audio.audio_retention_days = v;
            }
            if let Some(v) = audio.provisional_voiceprint_gc_days {
                settings.audio.provisional_voiceprint_gc_days = v;
            }
            if let Some(v) = audio.use_batch_pipeline {
                settings.audio.use_batch_pipeline = v;
            }
        }
        if let Some(keyboard) = patch.keyboard {
            if let Some(shortcuts) = keyboard.shortcuts {
                for (k, v) in shortcuts {
                    settings.keyboard.shortcuts.insert(k, v);
                }
            }
        }
        if let Some(new_lang) = patch.language {
            let changed = settings.language.as_deref() != Some(new_lang.as_str());
            settings.language = Some(new_lang.clone());
            if changed {
                // Auto-pick sensible ASR / embedding defaults only when the
                // current selection doesn't cover the target language.
                let recs = recommend_models_for_language(&new_lang, &settings.audio);
                if let Some(asr) = recs.asr_model {
                    info!(%asr, lang = %new_lang, "auto-selecting ASR for new language");
                    settings.audio.asr_model = Some(asr);
                }
                if let Some(emb) = recs.speaker_embedding_model {
                    info!(%emb, lang = %new_lang, "auto-selecting speaker embedding for new language");
                    settings.audio.speaker_embedding_model = Some(emb);
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

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SettingsPatch {
    pub llm: Option<LlmSettingsPatch>,
    pub audio: Option<AudioSettingsPatch>,
    pub keyboard: Option<KeyboardSettingsPatch>,
    #[serde(default)]
    pub language: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AudioSettingsPatch {
    pub device_name: Option<String>,
    pub asr_model: Option<String>,
    pub speaker_embedding_model: Option<String>,
    pub download_source: Option<DownloadSource>,
    pub speaker_confirm_threshold: Option<f32>,
    pub speaker_tentative_threshold: Option<f32>,
    pub speaker_min_duration_ms: Option<u32>,
    pub speaker_continuity_window_ms: Option<u32>,
    pub always_listening: Option<bool>,
    pub window_length_ms: Option<u32>,
    pub window_step_ms: Option<u32>,
    pub extraction_tick_secs: Option<u32>,
    pub live_asr_model: Option<Option<String>>,
    pub archive_asr_model: Option<Option<String>>,
    pub clip_target_secs: Option<u32>,
    pub clip_max_secs: Option<u32>,
    pub clip_close_silence_ms: Option<u32>,
    pub cluster_cosine_threshold: Option<f32>,
    pub audio_retention_days: Option<u32>,
    pub provisional_voiceprint_gc_days: Option<u32>,
    pub use_batch_pipeline: Option<bool>,
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

    #[test]
    fn zh_recommends_defaults_when_current_is_english_only() {
        let mut audio = AudioSettings::default();
        audio.asr_model = Some("whisper_base".to_string());
        audio.speaker_embedding_model = Some("speaker_something_en".to_string());
        let recs = recommend_models_for_language("zh-CN", &audio);
        assert_eq!(recs.asr_model.as_deref(), Some("sense_voice_multi"));
        assert_eq!(
            recs.speaker_embedding_model.as_deref(),
            Some("campplus_zh_en")
        );
    }

    #[test]
    fn zh_keeps_existing_when_already_chinese_capable() {
        let mut audio = AudioSettings::default();
        audio.asr_model = Some("zhen_zipformer_bilingual".to_string());
        audio.speaker_embedding_model = Some("campplus_zh".to_string());
        let recs = recommend_models_for_language("zh-CN", &audio);
        assert!(recs.asr_model.is_none());
        assert!(recs.speaker_embedding_model.is_none());
    }

    #[test]
    fn en_does_not_force_any_swap() {
        let mut audio = AudioSettings::default();
        audio.asr_model = Some("zh_conformer".to_string());
        let recs = recommend_models_for_language("en", &audio);
        assert!(recs.asr_model.is_none());
        assert!(recs.speaker_embedding_model.is_none());
    }

    #[test]
    fn migrates_tab_recording_shortcut_to_tab_live() {
        let mut shortcuts = std::collections::HashMap::new();
        shortcuts.insert("tab_recording".to_string(), "Ctrl+9".to_string());
        let mut keyboard = KeyboardSettings { shortcuts };

        super::migrate_tab_recording_shortcut(&mut keyboard);

        assert!(!keyboard.shortcuts.contains_key("tab_recording"));
        assert_eq!(keyboard.shortcuts.get("tab_live"), Some(&"Ctrl+9".to_string()));
    }

    #[test]
    fn migrate_no_op_when_tab_live_already_set() {
        let mut shortcuts = std::collections::HashMap::new();
        shortcuts.insert("tab_recording".to_string(), "Ctrl+9".to_string());
        shortcuts.insert("tab_live".to_string(), "Ctrl+3".to_string());
        let mut keyboard = KeyboardSettings { shortcuts };

        super::migrate_tab_recording_shortcut(&mut keyboard);

        // tab_live keeps its existing value; tab_recording is removed
        assert_eq!(keyboard.shortcuts.get("tab_live"), Some(&"Ctrl+3".to_string()));
        assert!(!keyboard.shortcuts.contains_key("tab_recording"));
    }

    #[test]
    fn audio_settings_defaults_have_clip_processing_fields() {
        let s = AudioSettings::default();
        assert_eq!(s.clip_target_secs, 300);
        assert_eq!(s.clip_max_secs, 360);
        assert_eq!(s.clip_close_silence_ms, 1500);
        assert!((s.cluster_cosine_threshold - 0.4).abs() < 1e-6);
        assert_eq!(s.audio_retention_days, 14);
        assert_eq!(s.provisional_voiceprint_gc_days, 30);
        assert_eq!(s.live_asr_model, None);
        assert_eq!(s.archive_asr_model, None);
    }

    #[test]
    fn live_and_archive_asr_default_to_legacy_asr_model_when_unset() {
        use crate::engine::app_settings::AudioSettings;
        let mut s = AudioSettings::default();
        s.asr_model = Some("zipformer-en".to_string());
        s.live_asr_model = None;
        s.archive_asr_model = None;
        let resolved = s.resolved_asr_models();
        assert_eq!(resolved.live.as_deref(), Some("zipformer-en"));
        assert_eq!(resolved.archive.as_deref(), Some("zipformer-en"));
    }
}
