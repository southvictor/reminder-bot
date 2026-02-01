use std::sync::Arc;

use memory_db::DB;
use serenity::model::gateway::GatewayIntents;
use tokio::sync::Mutex;

use crate::handler;
use crate::models::reminder::Reminder;
use crate::tasks::calendar_loop;
use crate::tasks::notification_loop;
use crate::tasks::task_runner::TaskRunner;

pub async fn run_api(
    shared_db: Arc<Mutex<DB<Reminder>>>,
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
    task_runner.add_task(|| {
        tokio::spawn(async move {
            calendar_loop::run_calendar_loop().await;
        });
    });
    task_runner.start_all();

    let token = discord_client_secret;
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES;
    let mut client = serenity::Client::builder(token, intents)
        .event_handler(handler::BotHandler::new(shared_db, openai_api_key_arc))
        .await
        .expect("Error creating Serenity client");

    if let Err(why) = client.start().await {
        eprintln!("Client error: {:?}", why);
    }
}
