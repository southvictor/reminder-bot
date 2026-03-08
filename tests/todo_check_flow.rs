use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use reminderBot::handlers::discord::{BotHandler, TodoCheckResult};
use reminderBot::models::config::UserConfig;
use reminderBot::models::todo::TodoItem;
use reminderBot::service::routing::OpenAIRouter;
use tokio::sync::Mutex;

mod canned_openai;
use canned_openai::CannedOpenAI;

#[tokio::test]
async fn todo_check_marks_complete() {
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_todo.json"));
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db.clone(), config_db, bus, sessions, router, openai);

    let now = Utc::now();
    {
        let mut db = todo_db.lock().await;
        db.insert(
            "t1".to_string(),
            TodoItem {
                id: "t1".to_string(),
                user_id: "@u".to_string(),
                content: "file taxes".to_string(),
                created_at: now - Duration::hours(1),
                completed_at: None,
            },
        );
    }

    let result = handler
        .handle_todo_check_payload("@u", "done", "t1", (now + Duration::hours(1)).timestamp())
        .await;
    assert_eq!(result, TodoCheckResult::MarkedComplete);

    let db = todo_db.lock().await;
    let todo = db.get("t1").unwrap();
    assert!(todo.completed_at.is_some());
}

#[tokio::test]
async fn todo_check_rejects_wrong_user() {
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_todo.json"));
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db.clone(), config_db, bus, sessions, router, openai);

    let now = Utc::now();
    {
        let mut db = todo_db.lock().await;
        db.insert(
            "t2".to_string(),
            TodoItem {
                id: "t2".to_string(),
                user_id: "@u".to_string(),
                content: "file taxes".to_string(),
                created_at: now - Duration::hours(1),
                completed_at: None,
            },
        );
    }

    let result = handler
        .handle_todo_check_payload("@other", "done", "t2", (now + Duration::hours(1)).timestamp())
        .await;
    assert_eq!(result, TodoCheckResult::Forbidden);
}

#[tokio::test]
async fn todo_check_rejects_expired() {
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let openai = Arc::new(CannedOpenAI::from_file("tests/fixtures/openai_canned_todo.json"));
    let router = Arc::new(OpenAIRouter::new(openai.clone()));
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(todo_db.clone(), config_db, bus, sessions, router, openai);

    let now = Utc::now();
    {
        let mut db = todo_db.lock().await;
        db.insert(
            "t3".to_string(),
            TodoItem {
                id: "t3".to_string(),
                user_id: "@u".to_string(),
                content: "file taxes".to_string(),
                created_at: now - Duration::hours(1),
                completed_at: None,
            },
        );
    }

    let result = handler
        .handle_todo_check_payload("@u", "done", "t3", (now - Duration::hours(1)).timestamp())
        .await;
    assert_eq!(result, TodoCheckResult::Expired);
}
