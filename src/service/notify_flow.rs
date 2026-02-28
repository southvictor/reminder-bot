use crate::service::routing::{Intent, IntentRouter};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

pub type SessionKey = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Unknown,
    PendingCalendarEvent,
    PendingTodo,
    PendingConfig,
}

#[derive(Debug, Clone)]
pub struct ConfigUpdate {
    pub kind: ConfigKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKind {
    Timezone,
}

#[derive(Debug, Clone)]
pub struct PendingSession {
    pub state: SessionState,
    pub original_text: String,
    pub last_prompt_at: DateTime<Utc>,
    pub pending_config: Option<ConfigUpdate>,
}

#[derive(Debug)]
pub enum NotifyDecision {
    EmitCalendarEvent { text: String },
    EmitTodo { text: String },
    EmitConfig { text: String },
    ApplyConfig { update: ConfigUpdate },
    ConfirmConfig { message: String },
    TodoFailed { error: String },
    TimezoneFailed { error: String },
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
        } else if session.state == SessionState::PendingConfig {
            if is_confirmation(&combined_text) {
                if let Some(update) = session.pending_config.clone() {
                    return NotifyDecision::ApplyConfig { update };
                }
            }
        } else if session.state == SessionState::Unknown {
            combined_text = format!("{} {}", session.original_text, combined_text);
        }
    }

    let routing = router.route(&combined_text).await;
    match routing.intent {
        Intent::CalendarEvent => {
            let session = PendingSession {
                state: SessionState::PendingCalendarEvent,
                original_text: combined_text.clone(),
                last_prompt_at: now,
                pending_config: None,
            };
            sessions.insert(session_key, session);
            NotifyDecision::EmitCalendarEvent {
                text: combined_text,
            }
        }
        Intent::Todolist => {
            let session = PendingSession {
                state: SessionState::PendingTodo,
                original_text: combined_text.clone(),
                last_prompt_at: now,
                pending_config: None,
            };
            sessions.insert(session_key, session);
            NotifyDecision::EmitTodo {
                text: combined_text,
            }
        }
        Intent::Config => {
            let session = PendingSession {
                state: SessionState::PendingConfig,
                original_text: combined_text.clone(),
                last_prompt_at: now,
                pending_config: None,
            };
            sessions.insert(session_key, session);
            NotifyDecision::EmitConfig {
                text: combined_text,
            }
        }
        Intent::Unknown => {
            let session = PendingSession {
                state: SessionState::Unknown,
                original_text: combined_text.clone(),
                last_prompt_at: now,
                pending_config: None,
            };
            sessions.insert(session_key, session);
            NotifyDecision::NeedClarification
        }
    }
}

fn is_confirmation(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    matches!(lower.as_str(), "yes" | "y" | "confirm" | "ok" | "okay")
}
