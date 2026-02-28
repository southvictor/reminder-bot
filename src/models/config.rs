use memory_db::{DB, DBError, save_db};
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
}

pub fn set_user_timezone(
    db: &mut DB<UserConfig>,
    user_id: &str,
    timezone: &str,
) -> Result<(), DBError> {
    db.insert(
        user_id.to_string(),
        UserConfig {
            user_id: user_id.to_string(),
            timezone: timezone.to_string(),
        },
    );
    save_db(&get_db_location(), db)
}

pub fn get_user_timezone(db: &DB<UserConfig>, user_id: &str) -> Option<String> {
    db.get(user_id).map(|cfg| cfg.timezone.clone())
}
