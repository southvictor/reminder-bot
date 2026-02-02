use std::collections::HashMap;
use std::env;
use std::sync::{Mutex, OnceLock};

use chrono::TimeZone;
use reminderBot::models::reminder::Reminder;
use reminderBot::tasks::notification_loop::{notification_tick, MessageSender};
use reminderBot::service::openai_service::OpenAIClient;
use tokio::sync::Mutex as TokioMutex;

struct FakeOpenAI {
    response: Result<String, String>,
}

#[serenity::async_trait]
impl OpenAIClient for FakeOpenAI {
    async fn generate_prompt(
        &self,
        _prompt: &str,
        _prompt_type: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        match &self.response {
            Ok(body) => Ok(body.clone()),
            Err(err) => Err(err.clone().into()),
        }
    }
}

struct MockSender {
    sent: TokioMutex<Vec<(String, String)>>,
}

#[serenity::async_trait]
impl MessageSender for MockSender {
    async fn send_message(&self, channel_id: &str, content: &str) -> Result<(), String> {
        let mut sent = self.sent.lock().await;
        sent.push((channel_id.to_string(), content.to_string()));
        Ok(())
    }
}

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[tokio::test]
async fn notification_tick_sends_and_expires_reminder() {
    let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp_dir = env::temp_dir().join(format!("reminderbot_it_{}", uuid::Uuid::new_v4()));
    unsafe {
        env::set_var("DB_LOCATION", &temp_dir);
    }

    let now = chrono::Utc.with_ymd_and_hms(2026, 2, 2, 12, 0, 0).unwrap();
    let mut db: HashMap<String, Reminder> = HashMap::new();
    db.insert(
        "r1".to_string(),
        Reminder {
            id: "r1".to_string(),
            content: "call mom".to_string(),
            notify: vec!["@u".to_string()],
            notification_times: vec![now - chrono::Duration::minutes(1)],
            channel: "123".to_string(),
        },
    );

    let openai = FakeOpenAI {
        response: Ok("Remember to call mom at noon.".to_string()),
    };
    let sender = MockSender {
        sent: TokioMutex::new(Vec::new()),
    };

    notification_tick(&mut db, &sender, &openai, now)
        .await
        .expect("tick should succeed");

    assert!(db.is_empty());
    let sent = sender.sent.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "123");
    assert!(sent[0].1.contains("Remember to call mom at noon."));
}
