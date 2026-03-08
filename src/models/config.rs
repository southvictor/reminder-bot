use memory_db::{DB, DBError, save_db};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::env;

// Returns the directory where config DB + backups live.
// Defaults to a relative "./data/config" directory.
pub fn get_db_location() -> String {
    let base = env::var("DB_LOCATION").unwrap_or("./data".to_string());
    format!("{}/config", base)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserConfig {
    pub user_id: String,
    pub timezone: String,
    #[serde(default)]
    pub last_todo_prompt_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn set_user_timezone(
    db: &mut DB<UserConfig>,
    user_id: &str,
    timezone: &str,
) -> Result<(), DBError> {
    let existing = db.get(user_id).cloned();
    db.insert(
        user_id.to_string(),
        UserConfig {
            user_id: user_id.to_string(),
            timezone: timezone.to_string(),
            last_todo_prompt_at: existing.and_then(|c| c.last_todo_prompt_at),
        },
    );
    save_db(&get_db_location(), db)
}

pub fn get_user_timezone(db: &DB<UserConfig>, user_id: &str) -> Option<String> {
    db.get(user_id).map(|cfg| cfg.timezone.clone())
}


pub fn set_last_todo_prompt_at(
    db: &mut DB<UserConfig>,
    user_id: &str,
    timestamp: DateTime<Utc>,
) -> Result<(), DBError> {
    let timezone = db
        .get(user_id)
        .map(|cfg| cfg.timezone.clone())
        .unwrap_or_else(|| "America/New_York".to_string());
    db.insert(
        user_id.to_string(),
        UserConfig {
            user_id: user_id.to_string(),
            timezone,
            last_todo_prompt_at: Some(timestamp),
        },
    );
    save_db(&get_db_location(), db)
}
