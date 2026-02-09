use std::collections::HashMap;
use std::sync::Arc;

use chrono::TimeZone;
use reminderBot::action::{Action, ActionEngine, ActionEvent, ActionPayload, ActionStatus, ActionStore, ActionType, NotificationDraft};
use reminderBot::service::approval_prompt::ApprovalPromptService;
use reminderBot::service::openai_service::OpenAIClient;
use reminderBot::models::notification::Notification;
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
struct FakeApprovalPrompt;

#[serenity::async_trait]
impl ApprovalPromptService for FakeApprovalPrompt {
    async fn prompt(&self, _action: &mut Action) -> Result<(), String> {
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
async fn approval_confirmed_creates_notification() {
    let store = Arc::new(Mutex::new(ActionStore::new()));
    let openai = Arc::new(FakeOpenAI {
        response: Ok("{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string()),
    });
    let approval = Arc::new(FakeApprovalPrompt::default());
    let db = Arc::new(Mutex::new(HashMap::<String, Notification>::new()));
    let engine = ActionEngine::new(store.clone(), openai, approval, db.clone());

    engine
        .handle_event(ActionEvent::NotifyRequested {
            text: "call mom tomorrow".to_string(),
            user_id: "@u".to_string(),
            channel_id: "123".to_string(),
        })
        .await;

    let action_id = {
        let guard = store.lock().await;
        guard.ids().into_iter().next().expect("action exists")
    };

    engine
        .handle_event(ActionEvent::ApprovalConfirmed {
            action_id: action_id.clone(),
            user_id: "@u".to_string(),
        })
        .await;

    let db_guard = db.lock().await;
    assert_eq!(db_guard.len(), 1);
    let notification = db_guard.values().next().unwrap();
    assert_eq!(notification.content, "call mom");
    assert_eq!(notification.channel, "123");
}

#[tokio::test]
async fn approval_canceled_marks_rejected() {
    let store = Arc::new(Mutex::new(ActionStore::new()));
    let openai = Arc::new(FakeOpenAI {
        response: Ok("{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string()),
    });
    let approval = Arc::new(FakeApprovalPrompt::default());
    let db = Arc::new(Mutex::new(HashMap::<String, Notification>::new()));
    let engine = ActionEngine::new(store.clone(), openai, approval, db);

    let draft = NotificationDraft {
        user_id: "@u".to_string(),
        channel_id: "123".to_string(),
        content: "call mom".to_string(),
        time: chrono::Utc.with_ymd_and_hms(2026, 2, 3, 12, 0, 0).unwrap(),
        original_text: "call mom".to_string(),
        extra_context: None,
        expires_at: chrono::Utc.with_ymd_and_hms(2026, 2, 3, 12, 5, 0).unwrap(),
        message_id: None,
    };

    let action_id = "a1".to_string();
    let action = Action {
        id: action_id.clone(),
        action_type: ActionType::CreateNotification,
        status: ActionStatus::AwaitingApproval,
        user_id: "@u".to_string(),
        channel_id: "123".to_string(),
        payload: Some(ActionPayload::NotificationDraft(draft)),
        created_at: chrono::Utc.with_ymd_and_hms(2026, 2, 3, 12, 0, 0).unwrap(),
        updated_at: chrono::Utc.with_ymd_and_hms(2026, 2, 3, 12, 0, 0).unwrap(),
    };

    {
        let mut guard = store.lock().await;
        guard.insert(action);
    }

    engine
        .handle_event(ActionEvent::ApprovalCanceled {
            action_id: action_id.clone(),
            user_id: "@u".to_string(),
        })
        .await;

    let guard = store.lock().await;
    let updated = guard.get(&action_id).expect("action exists");
    assert_eq!(updated.status, ActionStatus::Rejected);
}
