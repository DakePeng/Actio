#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub worker_host: String,
    pub worker_port: u16,
    pub http_port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL"),
            worker_host: std::env::var("WORKER_HOST").unwrap_or("127.0.0.1".into()),
            worker_port: std::env::var("WORKER_PORT").unwrap_or("50051".into()).parse().unwrap(),
            http_port: std::env::var("HTTP_PORT").unwrap_or("3000".into()).parse().unwrap(),
        }
    }
}

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
