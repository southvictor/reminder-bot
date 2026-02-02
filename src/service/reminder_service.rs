use chrono::{DateTime, Utc};
use memory_db::{DB, DBError};
use serenity::builder::{CreateActionRow, CreateButton};

use crate::models::reminder::{self, Reminder};

#[derive(Clone)]
pub struct PendingReminder {
    pub user_id: String,
    pub channel_id: String,
    pub content: String,
    pub time: DateTime<Utc>,
    pub original_text: String,
    pub extra_context: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub message_id: Option<u64>,
}

pub fn render_pending_message(pending: &PendingReminder) -> String {
    let mut body: String = format!(
        "Please confirm your reminder:\nContent: {}\nTime: {}",
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

pub fn pending_buttons(pending_id: &str) -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        CreateButton::new(format!("reminder_confirm:{}", pending_id))
            .label("Confirm date/time")
            .style(serenity::all::ButtonStyle::Success),
        CreateButton::new(format!("reminder_context:{}", pending_id))
            .label("Add context")
            .style(serenity::all::ButtonStyle::Primary),
        CreateButton::new(format!("reminder_cancel:{}", pending_id))
            .label("Cancel")
            .style(serenity::all::ButtonStyle::Danger),
    ])
}

pub struct ReminderService;

impl ReminderService {
    pub async fn create(
        db: &mut DB<Reminder>,
        content: &String,
        notify_users: &String,
        expires_at: &DateTime<Utc>,
        channel: &String,
    ) -> Result<(), DBError> {
        reminder::create_reminder(db, content, notify_users, expires_at, channel).await
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
    async fn create_reminder_populates_db_and_times() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let temp_dir = env::temp_dir().join(format!("reminderbot_test_{}", uuid::Uuid::new_v4()));
        unsafe {
            env::set_var("DB_LOCATION", &temp_dir);
        }

        let mut db: DB<Reminder> = HashMap::new();
        let content = "pay rent".to_string();
        let notify_users = "@user1,@user2".to_string();
        let channel = "123".to_string();
        let expires_at = Utc.with_ymd_and_hms(2026, 2, 10, 12, 0, 0).unwrap();

        ReminderService::create(&mut db, &content, &notify_users, &expires_at, &channel)
            .await
            .expect("create reminder should succeed");

        assert_eq!(db.len(), 1);
        let reminder = db.values().next().unwrap();
        assert_eq!(reminder.content, content);
        assert_eq!(reminder.channel, channel);
        assert_eq!(reminder.notify, vec!["@user1".to_string(), "@user2".to_string()]);

        let expected = vec![
            expires_at - Duration::days(1),
            expires_at - Duration::hours(1),
        ];
        assert_eq!(reminder.notification_times, expected);
    }

    #[test]
    fn render_pending_message_includes_context() {
        let pending = PendingReminder {
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
        assert!(debug.contains("reminder_confirm:abc123"));
        assert!(debug.contains("reminder_context:abc123"));
        assert!(debug.contains("reminder_cancel:abc123"));
    }
}
