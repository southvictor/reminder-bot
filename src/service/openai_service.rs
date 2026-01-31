use crate::openai_client;

pub struct OpenAIService {
    api_key: String,
}

impl OpenAIService {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn generate_prompt(
        &self,
        prompt: &str,
        prompt_type: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        openai_client::generate_openai_prompt(prompt, prompt_type, &self.api_key).await
    }
}
