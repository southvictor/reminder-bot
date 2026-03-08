use std::collections::HashMap;
use std::sync::Arc;

use reminderBot::handlers::discord::BotHandler;
use reminderBot::handlers::discord_responder::InteractionResponder;
use reminderBot::models::config::UserConfig;
use reminderBot::models::todo::TodoItem;
use reminderBot::service::routing::OpenAIRouter;
mod canned_openai;
use canned_openai::CannedOpenAI;
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
    let _guard = prepare_db_location("notify_with_responder_emits_response");
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_calendar_event.json"));
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, config_db, bus, sessions, router, openai);

    let decision = handler
        .handle_notify_internal("call mom tomorrow at 5", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::EmitCalendarEvent { .. }
    ));
    let response = BotHandler::notify_response(&decision);
    assert_eq!(response, "Got it — processing your calendar event.");
}

#[tokio::test]
async fn notify_with_responder_unknown_message() {
    let _guard = prepare_db_location("notify_with_responder_unknown_message");
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_todo.json"));
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, config_db, bus, sessions, router, openai);

    let decision = handler
        .handle_notify_internal("just a thought", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::EmitTodo { .. }
    ));
    let response = BotHandler::notify_response(&decision);
    assert_eq!(response, "Added to your todo list.");
}

#[tokio::test]
async fn notify_with_responder_timezone_message() {
    let _guard = prepare_db_location("notify_with_responder_timezone_message");
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_config.json"));
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, config_db.clone(), bus, sessions, router, openai);

    let decision = handler
        .handle_notify_internal("set my timezone to eastern time", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::ConfirmConfig { .. }
    ));

    let decision = handler
        .handle_notify_internal("confirm", "@u", "123")
        .await;
    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::ApplyConfig { .. }
    ));

    let db = config_db.lock().await;
    let tz = reminderBot::models::config::get_user_timezone(&db, "@u");
    assert_eq!(tz.as_deref(), Some("America/New_York"));
}

#[tokio::test]
async fn pending_context_opens_modal() {
    let _guard = prepare_db_location("pending_context_opens_modal");
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_unknown.json"));
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db, config_db, bus, sessions, router, openai);

    let responder = MockResponder::default();
    handler
        .handle_pending_context_with(&responder, "action123")
        .await;

    let modals = responder.modals.lock().await;
    assert!(modals.last().unwrap().0.contains("action_context_modal:action123"));
}
