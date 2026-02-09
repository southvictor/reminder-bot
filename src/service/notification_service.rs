use chrono::{DateTime, Utc};
use memory_db::{DB, DBError};
use serenity::builder::{CreateActionRow, CreateButton};

use crate::action::NotificationDraft;
use crate::models::notification::{self, Notification};

pub fn render_pending_message(pending: &NotificationDraft) -> String {
    let mut body: String = format!(
        "Please confirm your notification:\nContent: {}\nTime: {}",
        pending.content,
        pending.time
    );
    if let Some(ctx) = &pending.extra_context {
        if !ctx.trim().is_empty() {
            body.push_str(&format!("\nAdditional context: {}", ctx.trim()));
        }
    }
    body
}

pub fn pending_buttons(action_id: &str) -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        CreateButton::new(format!("action_confirm:{}", action_id))
            .label("Confirm date/time")
            .style(serenity::all::ButtonStyle::Success),
        CreateButton::new(format!("action_context:{}", action_id))
            .label("Add context")
            .style(serenity::all::ButtonStyle::Primary),
        CreateButton::new(format!("action_cancel:{}", action_id))
            .label("Cancel")
            .style(serenity::all::ButtonStyle::Danger),
    ])
}

pub struct NotificationService;

impl NotificationService {
    pub async fn create(
        db: &mut DB<Notification>,
        content: &String,
        notify_users: &String,
        expires_at: &DateTime<Utc>,
        channel: &String,
    ) -> Result<(), DBError> {
        notification::create_notification(db, content, notify_users, expires_at, channel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use std::collections::HashMap;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[tokio::test]
    async fn create_notification_populates_db_and_times() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let temp_dir = env::temp_dir().join(format!("notificationbot_test_{}", uuid::Uuid::new_v4()));
        unsafe {
            env::set_var("DB_LOCATION", &temp_dir);
        }

        let mut db: DB<Notification> = HashMap::new();
        let content = "pay rent".to_string();
        let notify_users = "@user1,@user2".to_string();
        let channel = "123".to_string();
        let expires_at = Utc.with_ymd_and_hms(2026, 2, 10, 12, 0, 0).unwrap();

        NotificationService::create(&mut db, &content, &notify_users, &expires_at, &channel)
            .await
            .expect("create notification should succeed");

        assert_eq!(db.len(), 1);
        let notification = db.values().next().unwrap();
        assert_eq!(notification.content, content);
        assert_eq!(notification.channel, channel);
        assert_eq!(notification.notify, vec!["@user1".to_string(), "@user2".to_string()]);

        let expected = vec![
            expires_at - Duration::days(1),
            expires_at - Duration::hours(1),
        ];
        assert_eq!(notification.notification_times, expected);
    }

    #[test]
    fn render_pending_message_includes_context() {
        let pending = NotificationDraft {
            user_id: "@u".to_string(),
            channel_id: "123".to_string(),
            content: "buy milk".to_string(),
            time: Utc.with_ymd_and_hms(2026, 2, 10, 12, 0, 0).unwrap(),
            original_text: "buy milk tomorrow".to_string(),
            extra_context: Some("add eggs".to_string()),
            expires_at: Utc.with_ymd_and_hms(2026, 2, 10, 12, 5, 0).unwrap(),
            message_id: None,
        };

        let body = render_pending_message(&pending);
        assert!(body.contains("buy milk"));
        assert!(body.contains("Additional context: add eggs"));
    }

    #[test]
    fn pending_buttons_include_namespaced_ids() {
        let buttons = pending_buttons("abc123");
        let debug = format!("{:?}", buttons);
        assert!(debug.contains("action_confirm:abc123"));
        assert!(debug.contains("action_context:abc123"));
        assert!(debug.contains("action_cancel:abc123"));
    }
}
