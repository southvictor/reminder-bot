use std::collections::HashMap;
use std::sync::Arc;

use reminderBot::handlers::discord::BotHandler;
use reminderBot::handlers::discord_responder::InteractionResponder;
use reminderBot::models::config::UserConfig;
use reminderBot::models::todo::TodoItem;
use reminderBot::service::openai_service::OpenAIClient;
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


    async fn reply_ephemeral_with_components(
        &self,
        content: &str,
        _components: Vec<serenity::builder::CreateActionRow>,
    ) {
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

struct FakeOpenAI {
    response: Result<String, String>,
}

#[serenity::async_trait]
impl OpenAIClient for FakeOpenAI {
    async fn generate_prompt(
        &self,
        _prompt: &str,
        prompt_type: &str,
        _timezone: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
                        match prompt_type {
            "intent_router" => Ok("{\"intent\":\"calendar_event\"}".to_string()),
            "config_parser" => Ok("{\"kind\":\"timezone\",\"value\":\"America/New_York\"}".to_string()),
            "calendar_event_parser" => Ok("{\"content\":\"call mom\",\"time\":\"2026-02-03T12:00:00Z\"}".to_string()),
            "todo_parser" => Ok("{\"content\":\"buy milk\"}".to_string()),
            _ => match &self.response {
                Ok(body) => Ok(body.clone()),
                Err(err) => Err(err.clone().into()),
            },
        }


    }
}

#[tokio::test]
async fn notify_with_responder_emits_response() {
    let _guard = prepare_db_location("notify_with_responder_emits_response");
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let openai = Arc::new(FakeOpenAI { response: Ok("".to_string()) });
    let handler = BotHandler::new(todo_db, config_db, bus, sessions, router, openai);

    let responder = MockResponder::default();
    let decision = handler
        .handle_notify_with(&responder, "call mom tomorrow at 5", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::EmitCalendarEvent { .. }
    ));
    let replies = responder.replies.lock().await;
    assert_eq!(replies.last().map(String::as_str), Some("Got it — processing your calendar event."));
}

#[tokio::test]
async fn notify_with_responder_unknown_message() {
    let _guard = prepare_db_location("notify_with_responder_unknown_message");
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let openai = Arc::new(FakeOpenAI { response: Ok("".to_string()) });
    let handler = BotHandler::new(todo_db, config_db, bus, sessions, router, openai);

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
async fn notify_with_responder_timezone_message() {
    let _guard = prepare_db_location("notify_with_responder_timezone_message");
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let openai = Arc::new(FakeOpenAI { response: Ok("".to_string()) });
    let handler = BotHandler::new(todo_db, config_db.clone(), bus, sessions, router, openai);

    let responder = MockResponder::default();
    let decision = handler
        .handle_notify_with(&responder, "set my timezone to eastern time", "@u", "123")
        .await;

    assert!(matches!(
        decision,
        reminderBot::service::notify_flow::NotifyDecision::ConfirmConfig { .. }
    ));
    {
        let replies = responder.replies.lock().await;
        assert!(
            replies.last().map(String::as_str)
                .unwrap_or("")
                .contains("buttons to confirm")
        );
    }

    let decision = handler
        .handle_notify_with(&responder, "confirm", "@u", "123")
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
    let (bus, _rx) = reminderBot::events::queue::EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let config_db = Arc::new(Mutex::new(HashMap::<String, UserConfig>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let openai = Arc::new(FakeOpenAI { response: Ok("".to_string()) });
    let handler = BotHandler::new(todo_db, config_db, bus, sessions, router, openai);

    let responder = MockResponder::default();
    handler
        .handle_pending_context_with(&responder, "action123")
        .await;

    let modals = responder.modals.lock().await;
    assert!(modals.last().unwrap().0.contains("action_context_modal:action123"));
}
