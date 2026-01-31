use chrono::{DateTime, Utc};
use memory_db::{DB, DBError};

use crate::models::reminder::{self, Reminder};

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
