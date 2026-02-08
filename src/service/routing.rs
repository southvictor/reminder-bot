use crate::service::openai_service::OpenAIClient;
use serde::Deserialize;
use serenity::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Notification,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct IntentResult {
    pub intent: Intent,
    pub normalized_text: String,
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
        match self.openai.generate_prompt(text, "intent_router").await {
            Ok(payload) => parse_router_payload(&payload).unwrap_or_else(|| route_intent(text)),
            Err(_) => route_intent(text),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RouterPayload {
    intent: String,
    normalized_text: String,
}

fn parse_router_payload(payload: &str) -> Option<IntentResult> {
    let parsed: RouterPayload = serde_json::from_str(payload).ok()?;
    let intent = match parsed.intent.as_str() {
        "notification" => Intent::Notification,
        _ => Intent::Unknown,
    };
    Some(IntentResult {
        intent,
        normalized_text: parsed.normalized_text.trim().to_string(),
    })
}

pub fn route_intent(text: &str) -> IntentResult {
    let normalized = text.trim().to_string();
    if normalized.is_empty() {
        return IntentResult {
            intent: Intent::Unknown,
            normalized_text: normalized,
        };
    }

    if has_time_tokens(&normalized) {
        return IntentResult {
            intent: Intent::Notification,
            normalized_text: normalized,
        };
    }

    IntentResult {
        intent: Intent::Unknown,
        normalized_text: normalized,
    }
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
