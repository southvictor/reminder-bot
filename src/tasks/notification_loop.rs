use chrono::Utc;
use tokio::time::sleep;
use std::time::Duration;
use std::sync::Arc;

use memory_db::{DB, save_db};
use crate::models::reminder::{Reminder, get_db_location};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use tokio::sync::Mutex;
use crate::service::notification_service::NotificationService;
use crate::service::openai_service::OpenAIService;

pub async fn run_notification_loop(
    db: Arc<Mutex<DB<Reminder>>>,
    client_secret: Arc<String>,
    openai_api_key: Arc<String>,
) {
    loop {
        sleep(Duration::from_secs(5)).await;
        let mut db = db.lock().await;
        let mut reminders_expired: Vec<String> = Vec::new();
        for reminder in db.values_mut() {
            if reminder.notification_times.len() == 0 {
                reminders_expired.push(reminder.id.clone());
                continue;
            }
            let notification_time_result = reminder.notification_times.first();
            if let Some(notification_time) = notification_time_result {
                if *notification_time < Utc::now() {
                    send_message(reminder, &client_secret, &openai_api_key).await;
                    reminder.notification_times.remove(0);
                }
            }
        }
        for reminder_id in reminders_expired {
            println!("No more notifications for {}. expiring", reminder_id);
            db.remove(reminder_id.as_str());
        }
        match save_db(&&get_db_location(), &db) {
            Ok(_) => println!("Finished running notification loop."),
            Err(_) => println!("Failed to save state after running notification loop."),
        }
    }
}

async fn send_message(
    reminder: &Reminder,
    client_secret: &str,
    openai_api_key: &str,
) {
    let channel_id = match reminder.channel.parse::<u64>() {
        Ok(channel_id_bytes) => ChannelId::new(channel_id_bytes),
        Err(_) => {
            println!("Failed to parse channel id, skipping notification.");
            return
        },
    };

    let openai = OpenAIService::new(openai_api_key.to_string());
    let message_body = NotificationService::build_message(reminder, &openai).await;
    let http: Http = Http::new(client_secret);
    if let Err(why) = channel_id.say(&http, message_body).await {
        eprintln!("Error sending message: {:?}", why);
    }
}
