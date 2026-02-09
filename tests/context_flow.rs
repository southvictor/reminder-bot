use std::collections::HashMap;
use std::sync::Arc;

use chrono::TimeZone;
use reminderBot::action::{Action, ActionEngine, ActionEvent, ActionPayload, ActionStatus, ActionStore, ActionType, NotificationDraft};
use reminderBot::events::queue::EventBus;
use reminderBot::events::worker::run_event_worker;
use reminderBot::models::notification::Notification;
use reminderBot::service::approval_prompt::ApprovalPromptService;
use reminderBot::service::openai_service::OpenAIClient;
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

#[derive(Default)]
struct FakeApprovalPrompt {
    prompts: Mutex<Vec<String>>,
}

#[serenity::async_trait]
impl ApprovalPromptService for FakeApprovalPrompt {
    async fn prompt(&self, action: &mut Action) -> Result<(), String> {
        let mut prompts = self.prompts.lock().await;
        prompts.push(action.id.clone());
        Ok(())
    }

    async fn update_status(&self, _action: &Action, _message: &str) -> Result<(), String> {
        Ok(())
    }

    async fn update_status_message(
        &self,
        _channel_id: &str,
        _user_id: &str,
        _message: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn context_submission_updates_pending() {
    let (bus, rx) = EventBus::new(4);
    let store = Arc::new(Mutex::new(ActionStore::new()));

    let pending_id = "p1".to_string();
    let user_id = "@u".to_string();

    let draft = NotificationDraft {
        user_id: user_id.clone(),
        channel_id: "123".to_string(),
        content: "call mom".to_string(),
        time: chrono::Utc.with_ymd_and_hms(2026, 2, 2, 12, 0, 0).unwrap(),
        original_text: "call mom tomorrow".to_string(),
        extra_context: None,
        expires_at: chrono::Utc.with_ymd_and_hms(2026, 2, 2, 12, 5, 0).unwrap(),
        message_id: None,
    };

    let action = Action {
        id: pending_id.clone(),
        action_type: ActionType::CreateNotification,
        status: ActionStatus::AwaitingApproval,
        user_id: user_id.clone(),
        channel_id: "123".to_string(),
        payload: Some(ActionPayload::NotificationDraft(draft)),
        created_at: chrono::Utc.with_ymd_and_hms(2026, 2, 2, 12, 0, 0).unwrap(),
        updated_at: chrono::Utc.with_ymd_and_hms(2026, 2, 2, 12, 0, 0).unwrap(),
    };

    {
        let mut store_guard = store.lock().await;
        store_guard.insert(action);
    }

    let fake_openai = Arc::new(FakeOpenAI {
        response: Ok(
            "{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string(),
        ),
    });
    let approval = Arc::new(FakeApprovalPrompt::default());
    let db = Arc::new(Mutex::new(HashMap::<String, Notification>::new()));

    let engine = ActionEngine::new(store.clone(), fake_openai, approval, db);
    let worker = tokio::spawn(run_event_worker(rx, engine));

    bus.emit(ActionEvent::ContextSubmitted {
        action_id: pending_id.clone(),
        user_id: user_id.clone(),
        context: "actually next day".to_string(),
    })
    .await;
    drop(bus);
    let _ = worker.await;

    let store_guard = store.lock().await;
    let updated = store_guard.get(&pending_id).expect("action should exist");
    let draft = updated.notification_draft().expect("draft should exist");
    assert_eq!(draft.content, "call mom");
    assert_eq!(
        draft.time,
        chrono::Utc.with_ymd_and_hms(2026, 2, 3, 12, 0, 0).unwrap()
    );
    assert_eq!(draft.extra_context.as_deref(), Some("actually next day"));
}
