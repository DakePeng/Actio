use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DownloadSource {
    #[default]
    HuggingFace,
    HfMirror,
    ModelScope,
}

impl DownloadSource {
    /// Resolve the download URL for a given model from its source-specific URLs.
    pub fn resolve_url<'a>(&self, sources: &'a GgufSources) -> &'a str {
        match self {
            DownloadSource::HuggingFace => &sources.huggingface,
            DownloadSource::HfMirror => &sources.hf_mirror,
            DownloadSource::ModelScope => &sources.model_scope,
        }
    }
}

/// Per-source download URLs for a GGUF model file.
#[derive(Debug, Clone, Serialize)]
pub struct GgufSources {
    pub huggingface: String,
    pub hf_mirror: String,
    pub model_scope: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalLlmInfo {
    pub id: String,
    pub name: String,
    pub gguf_filename: String,
    pub sources: GgufSources,
    pub size_mb: u32,
    pub ram_mb: u32,
    pub recommended_ram_gb: u32,
    pub context_window: u32,
    pub description: String,
    pub runtime_supported: bool,
}

pub fn available_local_llms() -> Vec<LocalLlmInfo> {
    vec![
        LocalLlmInfo {
            id: "qwen3.5-0.8b".into(),
            name: "Qwen3.5 0.8B".into(),
            gguf_filename: "Qwen3.5-0.8B-Q4_K_M.gguf".into(),
            sources: GgufSources {
                huggingface: "https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_K_M.gguf".into(),
                hf_mirror: "https://hf-mirror.com/unsloth/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_K_M.gguf".into(),
                model_scope: "https://modelscope.cn/models/unsloth/Qwen3.5-0.8B-GGUF/resolve/master/Qwen3.5-0.8B-Q4_K_M.gguf".into(),
            },
            size_mb: 510,  // 535,171,328 bytes
            ram_mb: 700,
            recommended_ram_gb: 8,
            context_window: 262144,
            description: "Smallest, fastest. Pre-quantized Q4_K_M GGUF.".into(),
            runtime_supported: true,
        },
        LocalLlmInfo {
            id: "qwen3.5-2b".into(),
            name: "Qwen3.5 2B".into(),
            gguf_filename: "Qwen3.5-2B-Q4_K_M.gguf".into(),
            sources: GgufSources {
                huggingface: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf".into(),
                hf_mirror: "https://hf-mirror.com/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf".into(),
                model_scope: "https://modelscope.cn/models/unsloth/Qwen3.5-2B-GGUF/resolve/master/Qwen3.5-2B-Q4_K_M.gguf".into(),
            },
            size_mb: 1221,  // 1,280,835,840 bytes
            ram_mb: 1700,
            recommended_ram_gb: 16,
            context_window: 262144,
            description: "Better quality. Pre-quantized Q4_K_M GGUF.".into(),
            runtime_supported: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_two_entries() {
        let catalog = available_local_llms();
        assert_eq!(catalog.len(), 2);
    }

    #[test]
    fn catalog_ids_are_unique() {
        let catalog = available_local_llms();
        let mut ids: Vec<&str> = catalog.iter().map(|m| m.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), catalog.len(), "duplicate ids in catalog");
    }

    #[test]
    fn catalog_default_is_first_entry() {
        let catalog = available_local_llms();
        assert_eq!(catalog[0].id, "qwen3.5-0.8b");
    }

    #[test]
    fn all_v1_entries_are_runtime_supported() {
        for m in available_local_llms() {
            assert!(
                m.runtime_supported,
                "{} is in the catalog but not runtime_supported",
                m.id
            );
        }
    }

    #[test]
    fn all_entries_have_gguf_filename() {
        for m in available_local_llms() {
            assert!(
                m.gguf_filename.ends_with(".gguf"),
                "{} has non-gguf filename: {}",
                m.id,
                m.gguf_filename
            );
        }
    }

    #[test]
    fn resolve_url_selects_correct_source() {
        let sources = GgufSources {
            huggingface: "https://hf.example.com/model.gguf".into(),
            hf_mirror: "https://mirror.example.com/model.gguf".into(),
            model_scope: "https://ms.example.com/model.gguf".into(),
        };
        assert!(DownloadSource::HuggingFace.resolve_url(&sources).contains("hf.example.com"));
        assert!(DownloadSource::HfMirror.resolve_url(&sources).contains("mirror.example.com"));
        assert!(DownloadSource::ModelScope.resolve_url(&sources).contains("ms.example.com"));
    }
}
