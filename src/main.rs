#![allow(non_snake_case)]

mod models;
mod clients;
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

#[tokio::main]
async fn main() {
    let config_path = env::var("CONFIG_FILE").unwrap_or_else(|_| "./config.properties".to_string());
    let config = AppConfig::from_file(&config_path).unwrap_or_default();

    let get_prop = |key: &str| -> Option<String> {
        config.get(key).or_else(|| env::var(key).ok())
    };

    let db: DB<notification::Notification> = load_db(&notification::get_db_location()).expect("Unable to load database.");
    let shared_db = Arc::new(tokio::sync::Mutex::new(db));
    let todo_db: DB<todo::TodoItem> =
        load_db(&todo::get_db_location()).unwrap_or_else(|_| HashMap::new());
    let shared_todo_db = Arc::new(tokio::sync::Mutex::new(todo_db));
    if let Some(run_mode) = get_prop("RUN_MODE") {
        if run_mode != "api" {
            panic!("Unsupported RUN_MODE {}. Only api mode is supported.", run_mode);
        }
    }

    let discord_client_secret = get_prop("DISCORD_CLIENT_SECRET")
        .expect("DISCORD_CLIENT_SECRET must be set for bot mode");
    let openai_api_key = get_prop("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY environment variable not set");
    runtime::run_api(
        shared_db.clone(),
        shared_todo_db.clone(),
        discord_client_secret,
        openai_api_key,
    )
    .await;
}
