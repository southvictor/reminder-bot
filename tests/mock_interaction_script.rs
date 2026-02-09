use std::collections::HashMap;
use std::fs;
use std::sync::Arc;

use reminderBot::action::ActionEvent;
use reminderBot::events::queue::EventBus;
use reminderBot::handlers::discord::BotHandler;
use reminderBot::models::todo::TodoItem;
use reminderBot::service::routing::HeuristicRouter;
use serde::Deserialize;
use tokio::sync::Mutex;

#[derive(Deserialize)]
struct ScriptLine {
    #[serde(rename = "type")]
    kind: String,
    user_id: String,
    channel_id: String,
    text: String,
}

#[tokio::test]
async fn script_drives_notify_flow() {
    let temp_dir = std::env::temp_dir().join(format!("notificationbot_script_{}", uuid::Uuid::new_v4()));
    let script_path = temp_dir.join("script.jsonl");
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(
        &script_path,
        r#"{"type":"notify","user_id":"@u","channel_id":"123","text":"animal safari"}
{"type":"notify","user_id":"@u","channel_id":"123","text":"tomorrow at 3"}"#,
    )
    .unwrap();

    let (bus, mut rx) = EventBus::new(8);
    let router = Arc::new(HeuristicRouter);
    let todo_db = Arc::new(Mutex::new(HashMap::<String, TodoItem>::new()));
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handler = BotHandler::new(
        todo_db,
        bus.clone(),
        sessions,
        router,
    );

    let content = fs::read_to_string(&script_path).unwrap();
    for line in content.lines() {
        let step: ScriptLine = serde_json::from_str(line).unwrap();
        if step.kind == "notify" {
            let _ = handler
                .handle_notify_internal(&step.text, &step.user_id, &step.channel_id)
                .await;
        }
    }

    let event = rx.recv().await.expect("should emit notify");
    match event {
        ActionEvent::NotifyRequested { text, user_id, channel_id } => {
            assert!(text.contains("tomorrow"));
            assert_eq!(user_id, "@u");
            assert_eq!(channel_id, "123");
        }
        other => panic!("unexpected event: {:?}", other),
    }
}
