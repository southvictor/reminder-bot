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
