use crate::clients::openai_client;
use serenity::async_trait;
use tokio::time::{timeout, Duration};

#[async_trait]
pub trait OpenAIClient: Send + Sync {
    async fn generate_prompt(
        &self,
        prompt: &str,
        prompt_type: &str,
        timezone: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

pub struct OpenAIService {
    api_key: String,
}

impl OpenAIService {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    async fn generate_prompt_internal(
        &self,
        prompt: &str,
        prompt_type: &str,
        timezone: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        openai_client::generate_openai_prompt(prompt, prompt_type, timezone, &self.api_key).await
    }
}

#[async_trait]
impl OpenAIClient for OpenAIService {
    async fn generate_prompt(
        &self,
        prompt: &str,
        prompt_type: &str,
        timezone: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let result = timeout(Duration::from_secs(6), self.generate_prompt_internal(prompt, prompt_type, timezone)).await;
        match result {
            Ok(inner) => inner,
            Err(_) => Err("OpenAI request timed out".into()),
        }
    }
}
