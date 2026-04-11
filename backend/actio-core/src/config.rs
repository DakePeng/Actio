#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl LlmConfig {
    pub fn from_env_optional() -> Option<Self> {
        let base_url = std::env::var("LLM_BASE_URL").ok()?;
        let api_key = std::env::var("LLM_API_KEY").ok()?;

        Some(Self {
            base_url,
            api_key,
            model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".into()),
        })
    }
}
