use std::collections::HashMap;
use std::sync::Arc;

use reminderBot::handlers::discord::BotHandler;
use reminderBot::models::config::UserConfig;
use reminderBot::models::todo::TodoItem;
use reminderBot::service::routing::OpenAIRouter;
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex;

mod canned_openai;
use canned_openai::CannedOpenAI;

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

#[tokio::test]
async fn canned_openai_todo_flow_creates_todo() {
    let _guard = prepare_db_location("canned_openai_todo_flow_creates_todo");
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_todo.json"));
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db.clone(), config_db, bus, sessions, router, openai);

    let decision = handler
        .handle_notify_internal("file taxes", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::EmitTodo { .. }
    ));

    let db = todo_db.lock().await;
    assert_eq!(db.len(), 1);
    let todo = db.values().next().expect("todo should exist");
    assert_eq!(todo.content, "file taxes");
}
