#![allow(non_snake_case)]

mod models;
mod cli;
mod openai_client;
mod action;
mod handlers;
mod service;
mod runtime;
mod tasks;
mod events;
mod config;

use std::env;
use std::collections::HashMap;
use std::sync::Arc;
use memory_db::load_db;
use memory_db::DB;
use crate::models::notification;
use crate::models::todo;
use crate::config::AppConfig;

const DEFAULT_RUN_MODE: &str = "cli";

#[tokio::main]
async fn main() {
    let config = match env::var("CONFIG_FILE") {
        Ok(path) => AppConfig::from_file(&path).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    };

    let get_prop = |key: &str| -> Option<String> {
        config.get(key).or_else(|| env::var(key).ok())
    };

    let db: DB<notification::Notification> = load_db(&notification::get_db_location()).expect("Unable to load database.");
    let shared_db = Arc::new(tokio::sync::Mutex::new(db));
    let todo_db: DB<todo::TodoItem> =
        load_db(&todo::get_db_location()).unwrap_or_else(|_| HashMap::new());
    let shared_todo_db = Arc::new(tokio::sync::Mutex::new(todo_db));
    let run_mode = get_prop("RUN_MODE").unwrap_or(DEFAULT_RUN_MODE.to_string());
    if run_mode == "api" {
        let discord_client_secret = get_prop("DISCORD_CLIENT_SECRET")
            .expect("DISCORD_CLIENT_SECRET must be set for bot mode");
        let openai_api_key = get_prop("OPENAI_API_KEY")
            .expect("OPENAI_API_KEY environment variable not set");
        runtime::run_api(
            shared_db.clone(),
            shared_todo_db.clone(),
            discord_client_secret,
            openai_api_key,
        ).await;
    } else if run_mode == "cli" {
        let default_channel = get_prop("DISCORD_CHANNEL_ID")
            .expect("DISCORD_CHANNEL_ID environment variable not set");
        let default_user = get_prop("DISCORD_USER_ID")
            .expect("DISCORD_USER_ID environment variable not set");
        let openai_api_key = get_prop("OPENAI_API_KEY")
            .expect("OPENAI_API_KEY environment variable not set");
        cli::cli(shared_db.clone(), default_user, default_channel, openai_api_key).await;
    } else {
        println!("Invalid run mode {}", run_mode);
    }
}
