use chrono::{DateTime, Utc};
use memory_db::{DB, DBError, save_db};
use serde::{Deserialize, Serialize};
use std::env;
use uuid::Uuid;

// Returns the directory where todo DB + backups live.
// Defaults to a relative "./data/todo" directory.
pub fn get_db_location() -> String {
    if let Ok(path) = env::var("TODO_DB_LOCATION") {
        return path;
    }
    let base = env::var("DB_LOCATION").unwrap_or("./data".to_string());
    format!("{}/todo", base)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TodoItem {
    pub id: String,
    pub user_id: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub fn create_todo(
    db: &mut DB<TodoItem>,
    user_id: &str,
    content: &str,
) -> Result<String, DBError> {
    let id = Uuid::new_v4().to_string();
    db.insert(
        id.clone(),
        TodoItem {
            id: id.clone(),
            user_id: user_id.to_string(),
            content: content.to_string(),
            created_at: Utc::now(),
            completed_at: None,
        },
    );
    save_db(&get_db_location(), db)?;
    Ok(id)
}
