use std::collections::HashMap;
use std::fs;

use reminderBot::service::openai_service::OpenAIClient;

pub struct CannedOpenAI {
    responses: HashMap<String, String>,
}

impl CannedOpenAI {
    pub fn from_file(path: &str) -> Self {
        let contents = fs::read_to_string(path).expect("fixture should load");
        let responses: HashMap<String, String> =
            serde_json::from_str(&contents).expect("fixture should be valid JSON");
        Self { responses }
    }
}

#[serenity::async_trait]
impl OpenAIClient for CannedOpenAI {
    async fn generate_prompt(
        &self,
        _prompt: &str,
        prompt_type: &str,
        _timezone: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.responses
            .get(prompt_type)
            .cloned()
            .ok_or_else(|| format!("missing canned response for {}", prompt_type).into())
    }
}
