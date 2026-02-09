use std::collections::HashMap;

use chrono::Utc;
use reminderBot::service::notify_flow::{route_notify, NotifyDecision, PendingSession, SessionKey};
use reminderBot::service::routing::{Intent, IntentResult, IntentRouter};

struct ScriptedRouter {
    intents: std::sync::Mutex<Vec<IntentResult>>,
}

#[serenity::async_trait]
impl IntentRouter for ScriptedRouter {
    async fn route(&self, _text: &str) -> IntentResult {
        let mut intents = self.intents.lock().unwrap();
        intents
            .pop()
            .unwrap_or(IntentResult {
                intent: Intent::Unknown,
                normalized_text: "".to_string(),
            })
    }
}

#[tokio::test]
async fn unknown_then_notification_routes_on_followup() {
    let router = ScriptedRouter {
        intents: std::sync::Mutex::new(vec![
            IntentResult {
                intent: Intent::Notification,
                normalized_text: "notify me tomorrow at 5 to call mom".to_string(),
            },
            IntentResult {
                intent: Intent::Unknown,
                normalized_text: "call mom".to_string(),
            },
        ]),
    };

    let mut sessions: HashMap<SessionKey, PendingSession> = HashMap::new();
    let key: SessionKey = ("@user".to_string(), "channel".to_string());

    let first = route_notify(
        &router,
        &mut sessions,
        key.clone(),
        "call mom".to_string(),
        Utc::now(),
    )
    .await;
    assert!(matches!(first, NotifyDecision::NeedClarification));
    assert!(sessions.contains_key(&key));

    let second = route_notify(
        &router,
        &mut sessions,
        key.clone(),
        "tomorrow at 5".to_string(),
        Utc::now(),
    )
    .await;

    match second {
        NotifyDecision::EmitNotify { normalized_text } => {
            assert!(normalized_text.contains("tomorrow"));
        }
        _ => panic!("expected emit notify on follow-up"),
    }
}
