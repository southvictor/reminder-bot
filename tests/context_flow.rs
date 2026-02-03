use std::collections::HashMap;
use std::sync::Arc;

use chrono::TimeZone;
use reminderBot::events::queue::{Event, EventBus};
use reminderBot::events::worker::run_event_worker;
use reminderBot::service::openai_service::OpenAIClient;
use reminderBot::service::reminder_service::PendingReminder;
use tokio::sync::Mutex;

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

#[tokio::test]
async fn context_submission_updates_pending() {
    let (bus, rx) = EventBus::new(4);
    let pending: Arc<Mutex<HashMap<String, PendingReminder>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let pending_id = "p1".to_string();
    let user_id = "@u".to_string();
    let mut pending_map = pending.lock().await;
    pending_map.insert(
        pending_id.clone(),
        PendingReminder {
            user_id: user_id.clone(),
            channel_id: "123".to_string(),
            content: "call mom".to_string(),
            time: chrono::Utc.with_ymd_and_hms(2026, 2, 2, 12, 0, 0).unwrap(),
            original_text: "call mom tomorrow".to_string(),
            extra_context: None,
            expires_at: chrono::Utc.with_ymd_and_hms(2026, 2, 2, 12, 5, 0).unwrap(),
            message_id: None,
        },
    );
    drop(pending_map);

    let fake_openai = Arc::new(FakeOpenAI {
        response: Ok(
            "{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string(),
        ),
    });
    let token = Arc::new("fake".to_string());
    let worker = tokio::spawn(run_event_worker(rx, fake_openai, token, pending.clone()));

    bus.emit(Event::ContextSubmitted {
        pending_id: pending_id.clone(),
        user_id: user_id.clone(),
        context: "actually next day".to_string(),
    })
    .await;
    drop(bus);
    let _ = worker.await;

    let pending_map = pending.lock().await;
    let updated = pending_map.get(&pending_id).expect("pending should exist");
    assert_eq!(updated.content, "call mom");
    assert_eq!(
        updated.time,
        chrono::Utc.with_ymd_and_hms(2026, 2, 3, 12, 0, 0).unwrap()
    );
    assert_eq!(updated.extra_context.as_deref(), Some("actually next day"));
}
