use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde_json;
use serenity::http::Http;
use serenity::model::id::ChannelId;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::events::queue::Event;
use crate::service::reminder_service::{pending_buttons, render_pending_message, PendingReminder};
use crate::models::reminder;
use crate::service::openai_service::OpenAIClient;

pub async fn run_event_worker(
    mut rx: mpsc::Receiver<Event>,
    openai: Arc<dyn OpenAIClient>,
    discord_token: Arc<String>,
    pending: Arc<Mutex<HashMap<String, PendingReminder>>>,
) {
    while let Some(event) = rx.recv().await {
        match event {
            Event::NotifyRequested {
                text,
                user_id,
                channel_id,
            } => {
                let payload = match openai.generate_prompt(&text, "notification").await {
                    Ok(p) => p,
                    Err(err) => {
                        send_channel_error(&discord_token, &channel_id, &user_id, &format!(
                            "Failed to call OpenAI for reminder: {}",
                            err
                        ))
                        .await;
                        continue;
                    }
                };

                let ai_reminder: reminder::AIReminder = match serde_json::from_str(&payload) {
                    Ok(r) => r,
                    Err(err) => {
                        send_channel_error(&discord_token, &channel_id, &user_id, &format!(
                            "Failed to parse reminder JSON: {}",
                            err
                        ))
                        .await;
                        continue;
                    }
                };

                let pending_id = Uuid::new_v4().to_string();
                let pending_item = PendingReminder {
                    user_id: user_id.clone(),
                    channel_id: channel_id.clone(),
                    content: ai_reminder.content,
                    time: ai_reminder.time,
                    original_text: text.clone(),
                    extra_context: None,
                    expires_at: Utc::now() + Duration::minutes(5),
                    message_id: None,
                };
                let message_body = render_pending_message(&pending_item);
                let buttons = pending_buttons(&pending_id);

                {
                    let mut pending_map = pending.lock().await;
                    pending_map.insert(pending_id.clone(), pending_item);
                }

                let http: Http = Http::new(discord_token.as_ref());
                let channel = match channel_id.parse::<u64>() {
                    Ok(id) => ChannelId::new(id),
                    Err(_) => {
                        send_channel_error(
                            discord_token.as_ref(),
                            &channel_id,
                            &user_id,
                            "Invalid channel id for reminder.",
                        )
                        .await;
                        continue;
                    }
                };
                let _ = channel
                    .send_message(
                        &http,
                        serenity::builder::CreateMessage::new()
                            .content(message_body)
                            .components(vec![buttons]),
                    )
                    .await;
            }
            Event::PendingConfirmed { .. } => {
                // TODO: handle confirmation
            }
            Event::PendingCanceled { .. } => {
                // TODO: handle cancel
            }
            Event::ContextSubmitted {
                pending_id,
                user_id,
                context,
            } => {
                let mut pending_map = pending.lock().await;
                let Some(pending_item) = pending_map.get_mut(&pending_id) else {
                    continue;
                };
                if pending_item.user_id != user_id {
                    continue;
                }
                if !context.trim().is_empty() {
                    pending_item.extra_context = Some(context.trim().to_string());
                }
                let mut combined_prompt = pending_item.original_text.clone();
                if let Some(ctx_value) = &pending_item.extra_context {
                    if !ctx_value.trim().is_empty() {
                        combined_prompt = format!(
                            "Original request: {original}\nCorrection note: {context}",
                            original = pending_item.original_text,
                            context = ctx_value.trim()
                        );
                    }
                }

                let refreshed = match openai
                    .generate_prompt(&combined_prompt, "notification_correction")
                    .await
                {
                    Ok(payload) => serde_json::from_str::<reminder::AIReminder>(&payload).ok(),
                    Err(_) => None,
                };
                if let Some(updated) = refreshed {
                    pending_item.content = updated.content;
                    pending_item.time = updated.time;
                }
            }
        }
    }
}

async fn send_channel_error(token: &str, channel_id: &str, user_id: &str, message: &str) {
    let http: Http = Http::new(token);
    let channel = match channel_id.parse::<u64>() {
        Ok(id) => ChannelId::new(id),
        Err(_) => return,
    };
    let _ = channel
        .say(&http, format!("<{}> {}", user_id, message))
        .await;
}
