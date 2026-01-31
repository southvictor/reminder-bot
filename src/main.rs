mod models;
mod cli;
mod notification_loop;
mod openai_client;
mod handler;
mod calendar_loop;
mod service;

use std::env;
use std::sync::Arc;
use memory_db::load_db;
use memory_db::DB;
use serenity::model::gateway::GatewayIntents;
use crate::models::reminder;

const DEFAULT_RUN_MODE: &str = "cli";

#[tokio::main]
async fn main() {
    let db: DB<reminder::Reminder> = load_db(&reminder::get_db_location()).expect("Unable to load database.");
    let shared_db = Arc::new(tokio::sync::Mutex::new(db));
    let run_mode = env::var("RUN_MODE").unwrap_or(DEFAULT_RUN_MODE.to_string());
    if run_mode == "api" {
        let discord_client_secret = env::var("DISCORD_CLIENT_SECRET")
            .expect("DISCORD_CLIENT_SECRET must be set for bot mode");
        let discord_client_secret_arc = Arc::new(discord_client_secret.clone());
        let openai_api_key = env::var("OPENAI_API_KEY")
            .expect("OPENAI_API_KEY environment variable not set");
        let openai_api_key_arc = Arc::new(openai_api_key);
        let token = discord_client_secret;
        let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES;
        let mut client = serenity::Client::builder(token, intents)
            .event_handler(handler::BotHandler::new(
                shared_db.clone(),
                discord_client_secret_arc.clone(),
                openai_api_key_arc.clone(),
            ))
            .await
            .expect("Error creating Serenity client");

        if let Err(why) = client.start().await {
            eprintln!("Client error: {:?}", why);
        }
    } else if run_mode == "cli" {
        let default_channel = env::var("DISCORD_CHANNEL_ID")
            .expect("DISCORD_CHANNEL_ID environment variable not set");
        let default_user = env::var("DISCORD_USER_ID")
            .expect("DISCORD_USER_ID environment variable not set");
        let openai_api_key = env::var("OPENAI_API_KEY")
            .expect("OPENAI_API_KEY environment variable not set");
        cli::cli(shared_db.clone(), default_user, default_channel, openai_api_key).await;
    } else {
        println!("Invalid run mode {}", run_mode);
    }
}
