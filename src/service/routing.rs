use crate::service::openai_service::OpenAIClient;
use serde::Deserialize;
use serenity::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    CalendarEvent,
    Todolist,
    Config,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct IntentResult {
    pub intent: Intent,
}

#[async_trait]
pub trait IntentRouter: Send + Sync {
    async fn route(&self, text: &str) -> IntentResult;
}

pub struct HeuristicRouter;

#[async_trait]
impl IntentRouter for HeuristicRouter {
    async fn route(&self, text: &str) -> IntentResult {
        route_intent(text)
    }
}

pub struct OpenAIRouter {
    openai: Arc<dyn OpenAIClient>,
}

impl OpenAIRouter {
    pub fn new(openai: Arc<dyn OpenAIClient>) -> Self {
        Self { openai }
    }
}

#[async_trait]
impl IntentRouter for OpenAIRouter {
    async fn route(&self, text: &str) -> IntentResult {
        match self
            .openai
            .generate_prompt(text, "intent_router", "America/New_York")
            .await
        {
            Ok(payload) => {
                eprintln!("Intent router payload: {}", payload);
                if let Some(result) = parse_router_payload(&payload) {
                    return result;
                }
                eprintln!("Intent router invalid payload: {}", payload);
                IntentResult {
                    intent: Intent::Unknown,
                }
            }
            Err(err) => {
                eprintln!("Intent router call failed: {}", err);
                IntentResult {
                    intent: Intent::Unknown,
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct RouterPayload {
    intent: String,
}

fn parse_router_payload(payload: &str) -> Option<IntentResult> {
    let parsed: RouterPayload = serde_json::from_str(payload).ok()?;
    let intent_value = parsed.intent.trim().to_lowercase();
    let intent = match intent_value.as_str() {
        "calendar_event" => Intent::CalendarEvent,
        "todolist" => Intent::Todolist,
        "config" => Intent::Config,
        _ => Intent::Unknown,
    };
    Some(IntentResult {
        intent,
    })
}

pub fn route_intent(text: &str) -> IntentResult {
    let normalized = text.trim().to_string();
    if normalized.is_empty() {
        return IntentResult {
            intent: Intent::Unknown,
        };
    }

    if has_time_tokens(&normalized) {
        return IntentResult {
            intent: Intent::CalendarEvent,
        };
    }

    if is_config_message(&normalized) {
        return IntentResult {
            intent: Intent::Config,
        };
    }

    IntentResult {
        intent: Intent::Todolist,
    }
}

fn is_config_message(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("timezone")
        || lower.contains("time zone")
        || lower.contains("tz")
        || lower.contains("eastern")
        || lower.contains("central")
        || lower.contains("mountain")
        || lower.contains("pacific")
        || lower.contains("est")
        || lower.contains("edt")
        || lower.contains("cst")
        || lower.contains("cdt")
        || lower.contains("mst")
        || lower.contains("mdt")
        || lower.contains("pst")
        || lower.contains("pdt")
        || lower.contains("utc")
        || lower.contains("gmt")
        || lower.contains("configure")
        || lower.contains("config")
}

fn has_time_tokens(text: &str) -> bool {
    let lower = text.to_lowercase();
    let tokens = [
        "today",
        "tomorrow",
        "tonight",
        "morning",
        "afternoon",
        "evening",
        "next ",
        "this ",
        "at ",
        "in ",
        "on ",
    ];
    if tokens.iter().any(|t| lower.contains(t)) {
        return true;
    }

    let weekdays = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    if weekdays.iter().any(|d| lower.contains(d)) {
        return true;
    }

    let months = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    if months.iter().any(|m| lower.contains(m)) {
        return true;
    }

    if lower.contains('/') || lower.contains(':') {
        return lower.chars().any(|c| c.is_ascii_digit());
    }

    has_am_pm(&lower)
}

fn has_am_pm(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        let first = bytes[i];
        let second = bytes[i + 1];
        if (first == b'a' || first == b'p') && second == b'm' {
            let before = if i == 0 { None } else { Some(bytes[i - 1]) };
            let after = if i + 2 >= bytes.len() { None } else { Some(bytes[i + 2]) };
            let boundary_before = before.map_or(true, |b| !b.is_ascii_alphabetic());
            let boundary_after = after.map_or(true, |b| !b.is_ascii_alphabetic());
            if boundary_before && boundary_after {
                return true;
            }
        }
    }
    false
}
