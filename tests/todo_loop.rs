use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use memory_db::DB;
use tokio::sync::Mutex;
use serenity::async_trait;

use reminderBot::models::config::UserConfig;
use reminderBot::models::todo::TodoItem;
use reminderBot::tasks::todo_loop::{daily_checklist_tick, DmSender};

struct SentMessage {
    user_id: String,
    content: String,
    component_rows: usize,
}

struct FakeSender {
    messages: Arc<Mutex<Vec<SentMessage>>>,
}

impl FakeSender {
    fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl DmSender for FakeSender {
    async fn send_dm_with_components(
        &self,
        user_id: &str,
        content: &str,
        components: Vec<serenity::builder::CreateActionRow>,
    ) -> Result<(), String> {
        let mut messages = self.messages.lock().await;
        messages.push(SentMessage {
            user_id: user_id.to_string(),
            content: content.to_string(),
            component_rows: components.len(),
        });
        Ok(())
    }
}

#[tokio::test]
async fn daily_checklist_sends_once_and_updates_timestamp() {
    let now = Utc::now();
    let mut todo_db: DB<TodoItem> = HashMap::new();
    todo_db.insert(
        "todo-1".to_string(),
        TodoItem {
            id: "todo-1".to_string(),
            user_id: "@1".to_string(),
            content: "Call mom".to_string(),
            created_at: now - Duration::hours(2),
            completed_at: None,
        },
    );
    todo_db.insert(
        "todo-2".to_string(),
        TodoItem {
            id: "todo-2".to_string(),
            user_id: "@1".to_string(),
            content: "Send report".to_string(),
            created_at: now - Duration::hours(1),
            completed_at: None,
        },
    );

    let mut config_db: DB<UserConfig> = HashMap::new();
    let sender = FakeSender::new();

    daily_checklist_tick(&mut todo_db, &mut config_db, &sender, now)
        .await
        .expect("first checklist should send");

    let messages = sender.messages.lock().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].user_id, "1");
    assert!(messages[0].content.contains("Daily todo check-in"));
    assert!(messages[0].content.contains("1. Call mom"));
    assert_eq!(messages[0].component_rows, 2);
    drop(messages);

    let cfg = config_db.get("@1").expect("config should exist");
    assert!(cfg.last_todo_prompt_at.is_some());

    daily_checklist_tick(&mut todo_db, &mut config_db, &sender, now + Duration::hours(1))
        .await
        .expect("second checklist should be skipped");

    let messages = sender.messages.lock().await;
    assert_eq!(messages.len(), 1);
}
