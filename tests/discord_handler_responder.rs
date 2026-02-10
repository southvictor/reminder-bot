use std::collections::HashMap;
use std::sync::Arc;

use reminderBot::handlers::discord::BotHandler;
use reminderBot::handlers::discord_responder::InteractionResponder;
use reminderBot::models::todo::TodoItem;
use reminderBot::service::routing::HeuristicRouter;
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex;

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

#[derive(Default)]
struct MockResponder {
    replies: Mutex<Vec<String>>,
    updates: Mutex<Vec<String>>,
    modals: Mutex<Vec<(String, String)>>,
}

#[serenity::async_trait]
impl InteractionResponder for MockResponder {
    async fn reply_ephemeral(&self, content: &str) {
        let mut replies = self.replies.lock().await;
        replies.push(content.to_string());
    }

    async fn reply_update(&self, content: &str) {
        let mut updates = self.updates.lock().await;
        updates.push(content.to_string());
    }

    async fn show_modal(&self, modal: serenity::builder::CreateModal) {
        let debug = format!("{:?}", modal);
        let mut modals = self.modals.lock().await;
        modals.push((debug, "".to_string()));
    }
}

#[tokio::test]
async fn notify_with_responder_emits_response() {
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, bus, sessions, router);

    let responder = MockResponder::default();
    let decision = handler
        .handle_notify_with(&responder, "call mom tomorrow at 5", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::EmitNotify { .. }
    ));
    let replies = responder.replies.lock().await;
    assert_eq!(replies.last().map(String::as_str), Some("Got it — processing your notification."));
}

#[tokio::test]
async fn notify_with_responder_unknown_message() {
    let _guard = prepare_db_location("notify_with_responder_unknown_message");
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, bus, sessions, router);

    let responder = MockResponder::default();
    let decision = handler
        .handle_notify_with(&responder, "just a thought", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::EmitTodo { .. }
    ));
    let replies = responder.replies.lock().await;
    assert_eq!(
        replies.last().map(String::as_str),
        Some("Added to your todo list.")
    );
}

#[tokio::test]
async fn pending_context_opens_modal() {
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, bus, sessions, router);

    let responder = MockResponder::default();
    handler
        .handle_pending_context_with(&responder, "action123")
        .await;

    let modals = responder.modals.lock().await;
    assert!(modals.last().unwrap().0.contains("action_context_modal:action123"));
}
