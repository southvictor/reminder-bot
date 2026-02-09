use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::models::notification::Notification;
use crate::service::openai_service::OpenAIClient;

#[derive(Serialize)]
struct MessageContext<'a> {
    content: &'a str,
    event_time: DateTime<Utc>,
    next_notification_time: Option<DateTime<Utc>>,
    hours_remaining: Option<i64>,
}

pub struct NotificationMessageService;

impl NotificationMessageService {
    pub async fn build_message<C: OpenAIClient + ?Sized>(
        notification: &Notification,
        openai: &C,
    ) -> String {
        let event_time = match notification.notification_times.last() {
            Some(t) => *t,
            None => {
                return format!("Notification: {}", notification.content);
            }
        };
        let next_time = notification.notification_times.first().copied();
        let hours_remaining = next_time.map(|t| (t - Utc::now()).num_hours());
        let context = MessageContext {
            content: notification.content.as_str(),
            event_time,
            next_notification_time: next_time,
            hours_remaining,
        };
        let structured = match serde_json::to_string(&context) {
            Ok(v) => v,
            Err(_) => return format!("Notification: {} at {}", notification.content, event_time),
        };

        match openai.generate_prompt(&structured, "notification_message").await {
            Ok(body) if !body.trim().is_empty() => body,
            _ => format!("Notification: {} at {}", notification.content, event_time),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    struct FakeOpenAI {
        response: Result<String, String>,
    }

    #[serenity::async_trait]
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
        let notification = Notification {
            id: "n1".to_string(),
            content: "pay rent".to_string(),
            notify: vec!["@u".to_string()],
            notification_times: vec![Utc.with_ymd_and_hms(2026, 2, 10, 12, 0, 0).unwrap()],
            channel: "123".to_string(),
        };
        let fake = FakeOpenAI {
            response: Ok("Pay rent at noon.".to_string()),
        };

        let msg = NotificationMessageService::build_message(&notification, &fake).await;
        assert_eq!(msg, "Pay rent at noon.");
    }

    #[tokio::test]
    async fn build_message_falls_back_on_error() {
        let event_time = Utc.with_ymd_and_hms(2026, 2, 10, 12, 0, 0).unwrap();
        let notification = Notification {
            id: "n1".to_string(),
            content: "pay rent".to_string(),
            notify: vec!["@u".to_string()],
            notification_times: vec![event_time],
            channel: "123".to_string(),
        };
        let fake = FakeOpenAI {
            response: Err("boom".to_string()),
        };

        let msg = NotificationMessageService::build_message(&notification, &fake).await;
        assert!(msg.contains("Notification: pay rent"));
        assert!(msg.contains("2026-02-10"));
    }
}
