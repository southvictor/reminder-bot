use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use memory_db::{DB, save_db};
use serenity::async_trait;
use serenity::all::ButtonStyle;
use serenity::builder::{CreateActionRow, CreateButton};
use serenity::http::Http;
use serenity::model::id::UserId;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::models::config::{self, UserConfig};
use crate::models::todo::{get_db_location, TodoItem};

const TODO_PROMPT_INTERVAL_HOURS: i64 = 23;
const MAX_TODOS_PER_PROMPT: usize = 5;

#[async_trait]
pub trait DmSender: Send + Sync {
    async fn send_dm_with_components(
        &self,
        user_id: &str,
        content: &str,
        components: Vec<CreateActionRow>,
    ) -> Result<(), String>;
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
    async fn send_dm_with_components(
        &self,
        user_id: &str,
        content: &str,
        components: Vec<CreateActionRow>,
    ) -> Result<(), String> {
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
            .send_message(
                &http,
                serenity::builder::CreateMessage::new()
                    .content(content)
                    .components(components),
            )
            .await
            .map_err(|e| format!("Failed to send DM: {:?}", e))?;
        Ok(())
    }
}

pub async fn run_todo_loop(
    todo_db: Arc<Mutex<DB<TodoItem>>>,
    config_db: Arc<Mutex<DB<UserConfig>>>,
    discord_token: Arc<String>,
) {
    let sender = DiscordDmSender::new(discord_token.to_string());
    loop {
        let now = Utc::now();
        let mut todo_db_guard = todo_db.lock().await;
        let mut config_db_guard = config_db.lock().await;
        let _ = daily_checklist_tick(&mut todo_db_guard, &mut config_db_guard, &sender, now).await;
        sleep(std::time::Duration::from_secs(60)).await;
    }
}

pub async fn daily_checklist_tick(
    todo_db: &mut DB<TodoItem>,
    config_db: &mut DB<UserConfig>,
    sender: &dyn DmSender,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let mut todos_by_user: HashMap<String, Vec<TodoItem>> = HashMap::new();
    for todo in todo_db.values() {
        if todo.completed_at.is_some() {
            continue;
        }
        todos_by_user
            .entry(todo.user_id.clone())
            .or_default()
            .push(todo.clone());
    }

    for (user_id, mut todos) in todos_by_user {
        todos.sort_by_key(|todo| todo.created_at);
        let tz = resolve_timezone(config_db, &user_id);
        let last_prompt = config_db
            .get(&user_id)
            .and_then(|cfg| cfg.last_todo_prompt_at);
        if !should_prompt(now, last_prompt) {
            continue;
        }

        let local_now = now.with_timezone(&tz);
        let content = build_checklist_message(&todos, local_now);
        let expires_at = now + Duration::hours(TODO_PROMPT_INTERVAL_HOURS);
        let components = build_checklist_components(&todos, expires_at);
        sender
            .send_dm_with_components(&user_id.trim_start_matches('@'), &content, components)
            .await?;
        config::set_last_todo_prompt_at(config_db, &user_id, now)
            .map_err(|e| format!("Failed to update todo prompt timestamp: {}", e))?;
    }

    save_db(&get_db_location(), todo_db)
        .map_err(|e| format!("Failed to save todo db: {}", e))?;
    Ok(())
}

fn resolve_timezone(config_db: &DB<UserConfig>, user_id: &str) -> Tz {
    let tz_value = config::get_user_timezone(config_db, user_id)
        .unwrap_or_else(|| "America/New_York".to_string());
    Tz::from_str(&tz_value).unwrap_or(chrono_tz::America::New_York)
}

fn should_prompt(now: DateTime<Utc>, last_prompt: Option<DateTime<Utc>>) -> bool {
    match last_prompt {
        None => true,
        Some(previous) => now - previous >= Duration::hours(TODO_PROMPT_INTERVAL_HOURS),
    }
}

fn build_checklist_message(todos: &[TodoItem], local_now: DateTime<Tz>) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Daily todo check-in for {}:",
        local_now.format("%Y-%m-%d")
    ));
    for (index, todo) in todos.iter().take(MAX_TODOS_PER_PROMPT).enumerate() {
        lines.push(format!("{}. {}", index + 1, todo.content));
    }
    if todos.len() > MAX_TODOS_PER_PROMPT {
        lines.push(format!(
            "And {} more...",
            todos.len() - MAX_TODOS_PER_PROMPT
        ));
    }
    lines.join("
")
}

fn build_checklist_components(
    todos: &[TodoItem],
    expires_at: DateTime<Utc>,
) -> Vec<CreateActionRow> {
    let mut rows = Vec::new();
    #[allow(unused_variables)]
    for (_index, todo) in todos.iter().take(MAX_TODOS_PER_PROMPT).enumerate() {
        let done_button = CreateButton::new(format!(
            "todo_check:done:{}:{}",
            todo.id,
            expires_at.timestamp()
        ))
        .label("Yes")
        .style(ButtonStyle::Success);
        let skip_button = CreateButton::new(format!(
            "todo_check:skip:{}:{}",
            todo.id,
            expires_at.timestamp()
        ))
        .label("No")
        .style(ButtonStyle::Secondary);
        rows.push(CreateActionRow::Buttons(vec![done_button, skip_button]));
    }
    rows
}
