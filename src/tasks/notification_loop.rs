use chrono::{DateTime, Utc};
use tokio::time::sleep;
use std::time::Duration;
use std::sync::Arc;

use memory_db::{DB, save_db};
use crate::models::reminder::{Reminder, get_db_location};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use tokio::sync::Mutex;
use crate::service::notification_service::NotificationService;
use crate::service::openai_service::{OpenAIClient, OpenAIService};
use serenity::async_trait;

#[async_trait]
pub trait MessageSender: Send + Sync {
    async fn send_message(&self, channel_id: &str, content: &str) -> Result<(), String>;
}

pub struct DiscordSender {
    token: String,
}

impl DiscordSender {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl MessageSender for DiscordSender {
    async fn send_message(&self, channel_id: &str, content: &str) -> Result<(), String> {
        let channel = channel_id
            .parse::<u64>()
            .map(ChannelId::new)
            .map_err(|_| "Failed to parse channel id".to_string())?;
        let http: Http = Http::new(&self.token);
        channel
            .say(&http, content)
            .await
            .map_err(|e| format!("Error sending message: {:?}", e))?;
        Ok(())
    }
}

pub async fn run_notification_loop(
    db: Arc<Mutex<DB<Reminder>>>,
    client_secret: Arc<String>,
    openai_api_key: Arc<String>,
) {
    let sender = DiscordSender::new(client_secret.to_string());
    let openai = OpenAIService::new(openai_api_key.to_string());
    loop {
        sleep(Duration::from_secs(5)).await;
        let mut db = db.lock().await;
        let _ = notification_tick(&mut db, &sender, &openai, Utc::now()).await;
    }
}

pub async fn notification_tick<C: OpenAIClient + ?Sized, S: MessageSender + ?Sized>(
    db: &mut DB<Reminder>,
    sender: &S,
    openai: &C,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let mut reminders_expired: Vec<String> = Vec::new();
    for reminder in db.values_mut() {
        if reminder.notification_times.is_empty() {
            reminders_expired.push(reminder.id.clone());
            continue;
        }
        let notification_time_result = reminder.notification_times.first();
        if let Some(notification_time) = notification_time_result {
            if *notification_time < now {
                let message_body = NotificationService::build_message(reminder, openai).await;
                sender
                    .send_message(&reminder.channel, &message_body)
                    .await?;
                reminder.notification_times.remove(0);
                if reminder.notification_times.is_empty() {
                    reminders_expired.push(reminder.id.clone());
                }
            }
        }
    }
    for reminder_id in reminders_expired {
        println!("No more notifications for {}. expiring", reminder_id);
        db.remove(reminder_id.as_str());
    }
    save_db(&get_db_location(), db).map_err(|e| e.to_string())?;
    Ok(())
}
