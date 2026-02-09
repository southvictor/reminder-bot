use crate::service::routing::{Intent, IntentRouter};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

pub type SessionKey = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Unknown,
    PendingNotification,
}

#[derive(Debug, Clone)]
pub struct PendingSession {
    pub state: SessionState,
    pub original_text: String,
    pub last_prompt_at: DateTime<Utc>,
}

pub enum NotifyDecision {
    EmitNotify { normalized_text: String },
    NeedClarification,
}

pub async fn route_notify(
    router: &dyn IntentRouter,
    sessions: &mut HashMap<SessionKey, PendingSession>,
    session_key: SessionKey,
    text: String,
    now: DateTime<Utc>,
) -> NotifyDecision {
    let mut combined_text = text;
    if let Some(session) = sessions.get(&session_key) {
        if now - session.last_prompt_at > Duration::minutes(5) {
            sessions.remove(&session_key);
        } else if session.state == SessionState::Unknown {
            combined_text = format!("{} {}", session.original_text, combined_text);
        }
    }

    let routing = router.route(&combined_text).await;
    match routing.intent {
        Intent::Notification => {
            let session = PendingSession {
                state: SessionState::PendingNotification,
                original_text: combined_text,
                last_prompt_at: now,
            };
            sessions.insert(session_key, session);
            NotifyDecision::EmitNotify {
                normalized_text: routing.normalized_text,
            }
        }
        Intent::Unknown => {
            let session = PendingSession {
                state: SessionState::Unknown,
                original_text: combined_text,
                last_prompt_at: now,
            };
            sessions.insert(session_key, session);
            NotifyDecision::NeedClarification
        }
    }
}
