use chrono::{DateTime, Utc};

use super::openai_service::OpenAIService;
use crate::models::reminder::Reminder;

pub struct NotificationService;

impl NotificationService {
    pub async fn build_message(
        reminder: &Reminder,
        openai: &OpenAIService,
    ) -> String {
        let hours_remaining = match (
            reminder.notification_times.first(),
            reminder.notification_times.last(),
        ) {
            (Some(first), Some(last)) => (*last - *first).num_hours(),
            _ => 0,
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

        let text_result = openai
            .generate_prompt(&structured_input, "notification_message")
            .await;

        match text_result {
            Ok(body) => format!(
                "{}\n{}",
                notifications.join(", "),
                body
            ),
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
        }
    }
}
