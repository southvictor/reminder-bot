use std::collections::HashMap;
use std::sync::Arc;

use reminderBot::handlers::action::{Action, ActionEngine, ActionEvent, ActionStore};
use reminderBot::events::queue::EventBus;
use reminderBot::events::worker::run_event_worker;
use reminderBot::handlers::discord::BotHandler;
use reminderBot::models::notification::Notification;
use reminderBot::models::todo::TodoItem;
use reminderBot::service::approval_prompt::ApprovalPromptService;
use reminderBot::service::openai_service::OpenAIClient;
use reminderBot::service::routing::HeuristicRouter;
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Duration};

static ENV_LOCK: StdMutex<()> = StdMutex::new(());

fn prepare_db_location(test_name: &str) -> std::sync::MutexGuard<'static, ()> {
    let guard = ENV_LOCK.lock().unwrap();
    let base = format!("./data/test_{}", test_name);
    std::fs::create_dir_all(&base).unwrap();
    unsafe {
        std::env::set_var("DB_LOCATION", &base);
    }
    guard
}

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

struct CapturingApprovalPrompt {
    prompts: Mutex<Vec<String>>,
}

impl CapturingApprovalPrompt {
    fn new() -> Self {
        Self {
            prompts: Mutex::new(Vec::new()),
        }
    }

    async fn latest_action_id(&self) -> Option<String> {
        let prompts = self.prompts.lock().await;
        prompts.last().cloned()
    }
}

#[serenity::async_trait]
impl ApprovalPromptService for CapturingApprovalPrompt {
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
async fn end_to_end_notify_confirm_flow() {
    let (bus, rx) = EventBus::new(16);
    let store = Arc::new(Mutex::new(ActionStore::new()));
    let openai = Arc::new(FakeOpenAI {
        response: Ok(
            "{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string(),
        ),
    });
    let approval = Arc::new(CapturingApprovalPrompt::new());
    let notification_db = Arc::new(Mutex::new(HashMap::<String, Notification>::new()));

    let engine = ActionEngine::new(
        store.clone(),
        openai,
        approval.clone(),
        notification_db.clone(),
    );

    let worker = tokio::spawn(run_event_worker(rx, engine));

    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, bus.clone(), sessions, router);

    let decision = handler
        .handle_notify_internal("call mom tomorrow at 5", "@u", "123")
        .await;
    assert!(matches!(decision, reminderBot::service::notify_flow::NotifyDecision::EmitNotify { .. }));

    let action_id = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(id) = approval.latest_action_id().await {
                break id;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("prompt not received");

    bus.emit(ActionEvent::ApprovalConfirmed {
        action_id,
        user_id: "@u".to_string(),
    })
    .await;

    drop(handler);
    drop(bus);
    let _ = worker.await;

    let db = notification_db.lock().await;
    assert_eq!(db.len(), 1);
    let notification = db.values().next().unwrap();
    assert_eq!(notification.content, "call mom");
    assert_eq!(notification.channel, "123");
}

#[tokio::test]
async fn end_to_end_notify_rejection_flow() {
    let (bus, rx) = EventBus::new(16);
    let store = Arc::new(Mutex::new(ActionStore::new()));
    let openai = Arc::new(FakeOpenAI {
        response: Ok(
            "{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string(),
        ),
    });
    let approval = Arc::new(CapturingApprovalPrompt::new());
    let notification_db = Arc::new(Mutex::new(HashMap::<String, Notification>::new()));

    let engine = ActionEngine::new(
        store.clone(),
        openai,
        approval.clone(),
        notification_db.clone(),
    );

    let worker = tokio::spawn(run_event_worker(rx, engine));

    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, bus.clone(), sessions, router);

    let decision = handler
        .handle_notify_internal("call mom tomorrow at 5", "@u", "123")
        .await;
    assert!(matches!(decision, reminderBot::service::notify_flow::NotifyDecision::EmitNotify { .. }));

    let action_id = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(id) = approval.latest_action_id().await {
                break id;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("prompt not received");

    bus.emit(ActionEvent::ApprovalCanceled {
        action_id,
        user_id: "@u".to_string(),
    })
    .await;

    drop(handler);
    drop(bus);
    let _ = worker.await;

    let db = notification_db.lock().await;
    assert_eq!(db.len(), 0);
}

#[tokio::test]
async fn end_to_end_notify_context_correction_flow() {
    let (bus, rx) = EventBus::new(16);
    let store = Arc::new(Mutex::new(ActionStore::new()));
    let openai = Arc::new(FakeOpenAI {
        response: Ok(
            "{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string(),
        ),
    });
    let approval = Arc::new(CapturingApprovalPrompt::new());
    let notification_db = Arc::new(Mutex::new(HashMap::<String, Notification>::new()));

    let engine = ActionEngine::new(
        store.clone(),
        openai,
        approval.clone(),
        notification_db.clone(),
    );

    let worker = tokio::spawn(run_event_worker(rx, engine));

    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, bus.clone(), sessions, router);

    let decision = handler
        .handle_notify_internal("call mom tomorrow at 5", "@u", "123")
        .await;
    assert!(matches!(decision, reminderBot::service::notify_flow::NotifyDecision::EmitNotify { .. }));

    let action_id = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(id) = approval.latest_action_id().await {
                break id;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("prompt not received");

    bus.emit(ActionEvent::ContextSubmitted {
        action_id: action_id.clone(),
        user_id: "@u".to_string(),
        context: "actually next day".to_string(),
    })
    .await;

    let _ = timeout(Duration::from_secs(2), async {
        loop {
            let store_guard = store.lock().await;
            if let Some(action) = store_guard.get(&action_id) {
                if let Some(draft) = action.notification_draft() {
                    if draft.extra_context.as_deref() == Some("actually next day") {
                        break;
                    }
                }
            }
            drop(store_guard);
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("context update not applied");

    bus.emit(ActionEvent::ApprovalConfirmed {
        action_id,
        user_id: "@u".to_string(),
    })
    .await;

    drop(handler);
    drop(bus);
    let _ = worker.await;

    let db = notification_db.lock().await;
    assert_eq!(db.len(), 1);
    let notification = db.values().next().unwrap();
    assert_eq!(notification.content, "call mom");
    assert_eq!(notification.channel, "123");
}

#[tokio::test]
async fn end_to_end_unknown_message_flow() {
    let _guard = prepare_db_location("end_to_end_unknown_message_flow");
    let (bus, _rx) = EventBus::new(16);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, bus, sessions, router);

    let decision = handler
        .handle_notify_internal("just a phrase", "@u", "123")
        .await;
    let response = BotHandler::notify_response(&decision);

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::EmitTodo { .. }
    ));
    assert_eq!(response, "Added to your todo list.");
}
