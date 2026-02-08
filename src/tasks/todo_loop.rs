use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::America::New_York;
use memory_db::{DB, save_db};
use serenity::async_trait;
use serenity::http::Http;
use serenity::model::id::UserId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::models::todo::{get_db_location, TodoItem};

#[async_trait]
pub trait DmSender: Send + Sync {
    async fn send_dm(&self, user_id: &str, content: &str) -> Result<(), String>;
}

pub struct DiscordDmSender {
    token: String,
}

impl DiscordDmSender {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl DmSender for DiscordDmSender {
    async fn send_dm(&self, user_id: &str, content: &str) -> Result<(), String> {
        let id = user_id
            .parse::<u64>()
            .map(UserId::new)
            .map_err(|_| "Failed to parse user id".to_string())?;
        let http = Http::new(&self.token);
        let channel = id
            .create_dm_channel(&http)
            .await
            .map_err(|e| format!("Failed to create DM channel: {:?}", e))?;
        channel
            .say(&http, content)
            .await
            .map_err(|e| format!("Failed to send DM: {:?}", e))?;
        Ok(())
    }
}

pub async fn run_todo_loop(db: Arc<Mutex<DB<TodoItem>>>, discord_token: Arc<String>) {
    let sender = DiscordDmSender::new(discord_token.to_string());
    loop {
        let next_run = next_daily_run(Utc::now());
        let sleep_for = (next_run - Utc::now())
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(60));
        sleep(sleep_for).await;
        let mut db = db.lock().await;
        let _ = daily_summary_tick(&mut db, &sender).await;
    }
}

fn next_daily_run(now: DateTime<Utc>) -> DateTime<Utc> {
    let now_local = now.with_timezone(&New_York);
    let today = now_local.date_naive();
    let target_local = New_York
        .from_local_datetime(&today.and_hms_opt(7, 0, 0).unwrap())
        .single()
        .unwrap_or_else(|| New_York.from_utc_datetime(&today.and_hms_opt(7, 0, 0).unwrap()));

    if now_local < target_local {
        target_local.with_timezone(&Utc)
    } else {
        (target_local + Duration::days(1)).with_timezone(&Utc)
    }
}

async fn daily_summary_tick<S: DmSender + ?Sized>(
    db: &mut DB<TodoItem>,
    sender: &S,
) -> Result<(), String> {
    let mut by_user: HashMap<String, Vec<TodoItem>> = HashMap::new();
    for item in db.values() {
        if item.completed_at.is_none() {
            by_user
                .entry(item.user_id.clone())
                .or_default()
                .push(item.clone());
        }
    }

    for (user_id, mut items) in by_user {
        items.sort_by_key(|item| item.created_at);
        let mut body = String::from("Good morning! Here is your current todo list:\n");
        for (idx, item) in items.iter().enumerate() {
            body.push_str(&format!("{}) {}\n", idx + 1, item.content));
        }
        sender.send_dm(&user_id, body.trim_end()).await?;
    }

    save_db(&get_db_location(), db).map_err(|e| e.to_string())?;
    Ok(())
}
