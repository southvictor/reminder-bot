mod models;
mod cli;
mod openai_client;
mod handlers;
mod service;
mod runtime;
mod tasks;
mod events;

use std::env;
use std::collections::HashMap;
use std::sync::Arc;
use memory_db::load_db;
use memory_db::DB;
use crate::models::reminder;
use crate::models::todo;

const DEFAULT_RUN_MODE: &str = "cli";

#[tokio::main]
async fn main() {
    let db: DB<reminder::Reminder> = load_db(&reminder::get_db_location()).expect("Unable to load database.");
    let shared_db = Arc::new(tokio::sync::Mutex::new(db));
    let todo_db: DB<todo::TodoItem> =
        load_db(&todo::get_db_location()).unwrap_or_else(|_| HashMap::new());
    let shared_todo_db = Arc::new(tokio::sync::Mutex::new(todo_db));
    let run_mode = env::var("RUN_MODE").unwrap_or(DEFAULT_RUN_MODE.to_string());
    if run_mode == "api" {
        let discord_client_secret = env::var("DISCORD_CLIENT_SECRET")
            .expect("DISCORD_CLIENT_SECRET must be set for bot mode");
        let openai_api_key = env::var("OPENAI_API_KEY")
            .expect("OPENAI_API_KEY environment variable not set");
        runtime::run_api(
            shared_db.clone(),
            shared_todo_db.clone(),
            discord_client_secret,
            openai_api_key,
        ).await;
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
