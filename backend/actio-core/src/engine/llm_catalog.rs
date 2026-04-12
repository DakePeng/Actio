use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LocalLlmInfo {
    pub id: String,
    pub name: String,
    pub hf_repo: String,
    pub gguf_filename: String,
    pub sha256: String,
    pub size_mb: u32,
    pub ram_mb: u32,
    pub recommended_ram_gb: u32,
    pub context_window: u32,
    pub description: String,
    pub downloaded: bool,
    pub runtime_supported: bool,
}

pub fn available_local_llms() -> Vec<LocalLlmInfo> {
    vec![
        LocalLlmInfo {
            id: "qwen3.5-0.8b-q4km".into(),
            name: "Qwen3.5 0.8B (Q4_K_M)".into(),
            hf_repo: "unsloth/Qwen3.5-0.8B-GGUF".into(),
            gguf_filename: "Qwen3.5-0.8B-Q4_K_M.gguf".into(),
            sha256: "e5926ccfef0c54aebf5d8bda01b2fb6c12ceff4f02a490bc108c5fabf60b334e".into(),
            size_mb: 510,
            ram_mb: 700,
            recommended_ram_gb: 8,
            context_window: 262144,
            description: "Smallest, fastest. Recommended for most laptops.".into(),
            downloaded: false,
            runtime_supported: true,
        },
        LocalLlmInfo {
            id: "qwen3.5-2b-q4km".into(),
            name: "Qwen3.5 2B (Q4_K_M)".into(),
            hf_repo: "unsloth/Qwen3.5-2B-GGUF".into(),
            gguf_filename: "Qwen3.5-2B-Q4_K_M.gguf".into(),
            sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223".into(),
            size_mb: 1221,
            ram_mb: 1700,
            recommended_ram_gb: 16,
            context_window: 262144,
            description: "Better quality. Recommended for 16+ GB RAM.".into(),
            downloaded: false,
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
        assert_eq!(catalog[0].id, "qwen3.5-0.8b-q4km");
    }

    #[test]
    fn catalog_sha256s_are_hex() {
        for m in available_local_llms() {
            assert_eq!(m.sha256.len(), 64, "sha256 for {} is not 64 hex chars", m.id);
            assert!(
                m.sha256.chars().all(|c| c.is_ascii_hexdigit() && (c.is_ascii_digit() || c.is_ascii_lowercase())),
                "sha256 for {} is not lowercase hex", m.id
            );
        }
    }
}
