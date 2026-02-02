use chrono::{DateTime, Utc};

use super::openai_service::OpenAIClient;
use crate::models::reminder::Reminder;

pub struct NotificationService;

impl NotificationService {
    pub async fn build_message<C: OpenAIClient + ?Sized>(
        reminder: &Reminder,
        openai: &C,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::openai_service::OpenAIClient;
    use chrono::TimeZone;
    use serenity::async_trait;

    struct FakeOpenAI {
        response: Result<String, String>,
    }

    #[async_trait]
    impl OpenAIClient for FakeOpenAI {
        async fn generate_prompt(
            &self,
            _prompt: &str,
            _prompt_type: &str,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            match &self.response {
                Ok(body) => Ok(body.clone()),
                Err(err) => Err(err.clone().into()),
            }
        }
    }

    #[tokio::test]
    async fn build_message_uses_ai_response() {
        let reminder = Reminder {
            id: "id1".to_string(),
            content: "call mom".to_string(),
            notify: vec!["@u".to_string()],
            notification_times: vec![
                Utc.with_ymd_and_hms(2026, 2, 10, 11, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 2, 10, 12, 0, 0).unwrap(),
            ],
            channel: "123".to_string(),
        };
        let fake = FakeOpenAI {
            response: Ok("Remember to call mom at noon.".to_string()),
        };

        let msg = NotificationService::build_message(&reminder, &fake).await;
        assert!(msg.contains("<@u>"));
        assert!(msg.contains("Remember to call mom at noon."));
    }

    #[tokio::test]
    async fn build_message_falls_back_on_error() {
        let reminder = Reminder {
            id: "id2".to_string(),
            content: "file taxes".to_string(),
            notify: vec!["@u".to_string()],
            notification_times: vec![
                Utc.with_ymd_and_hms(2026, 2, 10, 11, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 2, 10, 12, 0, 0).unwrap(),
            ],
            channel: "123".to_string(),
        };
        let fake = FakeOpenAI {
            response: Err("boom".into()),
        };

        let msg = NotificationService::build_message(&reminder, &fake).await;
        assert!(msg.contains("Hours remaining"));
        assert!(msg.contains("file taxes"));
    }
}
