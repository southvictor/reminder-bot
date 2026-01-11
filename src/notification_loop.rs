use chrono::DateTime;
use chrono::Utc;
use tokio::time::sleep;
use std::time::Duration;
use std::sync::Arc;

use memory_db::{DB, save_db};
use crate::reminder::{Reminder, get_db_location};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use tokio::sync::Mutex;
use crate::openai_client;

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

    let hours_remaining = match (reminder.notification_times.first(), reminder.notification_times.last()) {
        (Some(first), Some(last)) => (*last - *first).num_hours(),
        (_,_) => 0,
    };
    let notifications: Vec<String> = reminder
        .notify
        .iter()
        .map(|user| format!("<{}>", user))
        .collect();
    let event_time: &DateTime<Utc> = reminder.notification_times.last().unwrap();

    let structured_input = format!(
        "users: {users}\ncontent: {content}\nevent_time: {event_time}\nhours_remaining: {hours}",
        users = notifications.join(", "),
        content = reminder.content,
        event_time = event_time,
        hours = hours_remaining,
    );

    let text_result = openai_client::generate_openai_prompt(
        &structured_input,
        "notification_message",
        openai_api_key,
    )
    .await;

    let message_body = match text_result {
        Ok(body) => body,
        Err(err) => {
            eprintln!(
                "Failed to generate natural language notification, falling back. Error: {}",
                err
            );
            format!(
                "{}\n You have an upcoming event at {}\n {}\n Hours remaining: {}",
                notifications.join(","),
                event_time,
                reminder.content,
                hours_remaining
            )
        }
    };
    let http: Http = Http::new(client_secret);
    if let Err(why) = channel_id.say(&http, message_body).await {
        eprintln!("Error sending message: {:?}", why);
    }
}
