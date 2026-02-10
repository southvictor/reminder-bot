use std::sync::Arc;

use memory_db::DB;
use serenity::model::gateway::GatewayIntents;
use tokio::sync::Mutex;

use crate::handlers::action::{ActionEngine, ActionStore};
use crate::handlers::discord;
use std::collections::HashMap;
use crate::models::notification::Notification;
use crate::models::todo::TodoItem;
use crate::tasks::calendar_loop;
use crate::tasks::notification_loop;
use crate::tasks::todo_loop;
use crate::tasks::task_runner::TaskRunner;
use crate::events::queue::EventBus;
use crate::events::worker::run_event_worker;
use crate::service::approval_prompt::DiscordApprovalPromptService;
use crate::service::openai_service::OpenAIClient;
use crate::service::openai_service::OpenAIService;
use crate::service::notify_flow::{PendingSession, SessionKey};
use crate::service::routing::OpenAIRouter;

pub async fn run_api(
    shared_db: Arc<Mutex<DB<Notification>>>,
    shared_todo_db: Arc<Mutex<DB<TodoItem>>>,
    discord_client_secret: String,
    openai_api_key: String,
) {
    let discord_client_secret_arc = Arc::new(discord_client_secret.clone());
    let openai_api_key_arc = Arc::new(openai_api_key);

    let mut task_runner = TaskRunner::new();
    task_runner.add_task({
        let db = shared_db.clone();
        let secret = discord_client_secret_arc.clone();
        let openai = openai_api_key_arc.clone();
        move || {
            tokio::spawn(async move {
                notification_loop::run_notification_loop(db, secret, openai).await;
            });
        }
    });
    task_runner.add_task({
        let todo_db = shared_todo_db.clone();
        let secret = discord_client_secret_arc.clone();
        move || {
            tokio::spawn(async move {
                todo_loop::run_todo_loop(todo_db, secret).await;
            });
        }
    });
    task_runner.add_task(|| {
        tokio::spawn(async move {
            calendar_loop::run_calendar_loop().await;
        });
    });
    task_runner.start_all();

    let action_store = Arc::new(Mutex::new(ActionStore::new()));
    let sessions: Arc<Mutex<HashMap<SessionKey, PendingSession>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let (event_bus, event_rx) = EventBus::new(256);
    let worker_openai: Arc<dyn OpenAIClient> =
        Arc::new(OpenAIService::new(openai_api_key_arc.as_ref().to_string()));
    let router: Arc<dyn crate::service::routing::IntentRouter> =
        Arc::new(OpenAIRouter::new(worker_openai.clone()));
    let worker_secret = discord_client_secret_arc.clone();
    let approval_service: Arc<dyn crate::service::approval_prompt::ApprovalPromptService> =
        Arc::new(DiscordApprovalPromptService::new(worker_secret.clone()));
    let engine = ActionEngine::new(
        action_store.clone(),
        worker_openai,
        approval_service,
        shared_db.clone(),
    );
    tokio::spawn(async move {
        run_event_worker(event_rx, engine).await;
    });

    let token = discord_client_secret;
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES;
    let mut client = serenity::Client::builder(token, intents)
        .event_handler(discord::BotHandler::new(
            shared_todo_db,
            event_bus,
            sessions,
            router,
        ))
        .await
        .expect("Error creating Serenity client");

    if let Err(why) = client.start().await {
        eprintln!("Client error: {:?}", why);
    }
}
