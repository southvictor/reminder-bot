use serde::{Deserialize, Serialize};
use chrono::DateTime;
use chrono::Utc;
use chrono::Duration;
use memory_db::DB;
use memory_db::save_db;
use memory_db::DBError;
use uuid::Uuid;
use std::env;

// Returns the directory where DB + backups live.
// Defaults to a relative ".reminderbot" directory.
pub fn get_db_location() -> String {
    env::var("DB_LOCATION").unwrap_or("./data".to_string())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Reminder {
    pub id: String,
    pub content: String,
    pub notify: Vec<String>,
    pub notification_times: Vec<DateTime<Utc>>,
    pub channel: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AIReminder {
    pub content: String,
    pub time: DateTime<Utc>
}

pub async fn create_reminder(
    db: &mut DB<Reminder>,
    content: &String,
    notify_users: &String,
    expires_at: &DateTime<Utc>,
    channel: &String,
) -> Result<(), DBError> {
    let users: Vec<String> = notify_users.split(",").map(|user| {user.to_string()}).collect();
    let id = Uuid::new_v4().to_string();
    let mut notification_times: Vec<DateTime<Utc>> = Vec::new();
    if let Some(one_hour_before) = expires_at.checked_sub_signed(Duration::hours(1)) {
        notification_times.push(one_hour_before);
    }
    if let Some(one_day_before) = expires_at.checked_sub_signed(Duration::days(1)) {
        notification_times.push(one_day_before);
    }
    notification_times.sort();
    db.insert(
        id.clone(),
        Reminder {
            id: id.clone(),
            content: content.to_string(),
            notify: users,
            notification_times: notification_times,
            channel: channel.to_string(),
        },
    );
    save_db(&get_db_location(), db)
}
