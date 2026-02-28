use crate::handlers::action::ActionEvent;
use crate::events::queue::EventBus;
use crate::handlers::discord_responder::{InteractionResponder, SerenityResponder};
use crate::service::notify_flow::{route_notify, ConfigKind, ConfigUpdate, NotifyDecision, PendingSession, SessionKey};
use crate::service::routing::IntentRouter;
use crate::models::{config, todo};
use memory_db::DB;
use serde::{Deserialize, Serialize};
use serenity::prelude::*;
use serenity::async_trait;
use serenity::model::gateway::Ready;
use serenity::all::{Command, CommandOptionType, Interaction as DiscordInteraction};
use serenity::builder::{
    CreateCommand,
    CreateCommandOption,
    CreateInteractionResponse,
    CreateInteractionResponseMessage,
    CreateActionRow,
    CreateButton,
    CreateModal,
    CreateInputText,
};
use serenity::all::InputTextStyle;
use chrono::Utc;
use chrono_tz::Tz;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct ErrorMessage {
    pub error: String,
}

#[derive(Debug, Deserialize)]
struct ConfigPayload {
    kind: String,
    value: String,
}

fn config_buttons() -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new("config_confirm").label("Confirm").style(serenity::all::ButtonStyle::Success),
        CreateButton::new("config_cancel").label("Cancel").style(serenity::all::ButtonStyle::Danger),
    ])]
}

pub struct BotHandler {
    todo_db: Arc<Mutex<DB<todo::TodoItem>>>,
    config_db: Arc<Mutex<DB<config::UserConfig>>>,
    sessions: Arc<Mutex<HashMap<SessionKey, PendingSession>>>,
    router: Arc<dyn IntentRouter>,
    event_bus: EventBus,
    openai: Arc<dyn crate::service::openai_service::OpenAIClient>,
}

impl BotHandler {
    pub fn new(
        todo_db: Arc<Mutex<DB<todo::TodoItem>>>,
        config_db: Arc<Mutex<DB<config::UserConfig>>>,
        event_bus: EventBus,
        sessions: Arc<Mutex<HashMap<SessionKey, PendingSession>>>,
        router: Arc<dyn IntentRouter>,
        openai: Arc<dyn crate::service::openai_service::OpenAIClient>,
    ) -> Self {
        BotHandler {
            todo_db,
            config_db,
            sessions,
            router,
            event_bus,
            openai,
        }
    }
}

impl BotHandler {
    async fn handle_notify(&self, ctx: &Context, command: serenity::all::CommandInteraction) {
        let text = command
            .data
            .options
            .iter()
            .find(|opt| opt.name == "text")
            .and_then(|opt| match &opt.value {
                serenity::all::CommandDataOptionValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or("")
            .to_string();

        if text.trim().is_empty() {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Missing `text` argument for /notify")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let user_id = format!("@{}", command.user.id.to_string());
        let channel_id = command.channel_id.to_string();
        let responder = SerenityResponder::for_command(ctx, &command);
        self.handle_notify_with(&responder, &text, &user_id, &channel_id)
            .await;

    }

    async fn get_user_timezone(&self, user_id: &str) -> String {
        let db = self.config_db.lock().await;
        config::get_user_timezone(&db, user_id).unwrap_or_else(|| "America/New_York".to_string())
    }

    async fn set_user_timezone(&self, user_id: &str, timezone: &str) -> Result<(), String> {
        let tz = timezone.parse::<Tz>().map_err(|_| "Invalid timezone".to_string())?;
        let mut db = self.config_db.lock().await;
        config::set_user_timezone(&mut db, user_id, tz.name())
            .map_err(|e| e.to_string())
    }

    async fn parse_config_update(&self, text: &str) -> Result<ConfigUpdate, String> {
        let payload = self
            .openai
            .generate_prompt(text, "config_parser", "America/New_York")
            .await
            .map_err(|e| e.to_string())?;
        let parsed: ConfigPayload = serde_json::from_str(&payload)
            .map_err(|e| format!("Invalid config response: {}", e))?;
        let kind = match parsed.kind.trim().to_lowercase().as_str() {
            "timezone" => ConfigKind::Timezone,
            _ => return Err("Unsupported config kind".to_string()),
        };
        Ok(ConfigUpdate {
            kind,
            value: parsed.value.trim().to_string(),
        })
    }

    pub async fn handle_notify_internal(
        &self,
        text: &str,
        user_id: &str,
        channel_id: &str,
    ) -> NotifyDecision {
        let session_key = (user_id.to_string(), channel_id.to_string());
        let now = Utc::now();
        let decision = {
            let mut sessions = self.sessions.lock().await;
            route_notify(
                self.router.as_ref(),
                &mut sessions,
                session_key,
                text.to_string(),
                now,
            )
            .await
        };

        if let NotifyDecision::EmitCalendarEvent { text } = &decision {
            let timezone = self.get_user_timezone(user_id).await;
            let payload = match self.openai.generate_prompt(text, "calendar_event_parser", &timezone).await {
                Ok(p) => p,
                Err(err) => return NotifyDecision::TimezoneFailed { error: err.to_string() },
            };
            let parsed: crate::models::notification::AINotification = match serde_json::from_str(&payload) {
                Ok(p) => p,
                Err(err) => return NotifyDecision::TimezoneFailed { error: err.to_string() },
            };
            self.event_bus
                .emit(ActionEvent::NotifyRequested {
                    content: parsed.content,
                    time: parsed.time,
                    user_id: user_id.to_string(),
                    channel_id: channel_id.to_string(),
                    timezone,
                })
                .await;
        }
        if let NotifyDecision::EmitTodo { text } = &decision {
            let payload = match self.openai.generate_prompt(text, "todo_parser", "America/New_York").await {
                Ok(p) => p,
                Err(err) => return NotifyDecision::TodoFailed { error: err.to_string() },
            };
            let parsed: serde_json::Value = match serde_json::from_str(&payload) {
                Ok(p) => p,
                Err(err) => return NotifyDecision::TodoFailed { error: err.to_string() },
            };
            let content = parsed.get("content").and_then(|v| v.as_str()).unwrap_or(text);
            let mut db = self.todo_db.lock().await;
            if let Err(err) = todo::create_todo(&mut db, user_id, content) {
                return NotifyDecision::TodoFailed {
                    error: err.to_string(),
                };
            }
        }
        if let NotifyDecision::EmitConfig { text } = &decision {
            let update = match self.parse_config_update(text).await {
                Ok(update) => update,
                Err(err) => return NotifyDecision::TimezoneFailed { error: err },
            };
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get_mut(&(user_id.to_string(), channel_id.to_string())) {
                session.pending_config = Some(update.clone());
                session.state = crate::service::notify_flow::SessionState::PendingConfig;
            }
            return NotifyDecision::ConfirmConfig {
                message: format!(
                    "I can set your timezone to {}. Use the buttons to confirm.",
                    update.value
                ),
            };
        }
        if let NotifyDecision::ApplyConfig { update } = &decision {
            match update.kind {
                ConfigKind::Timezone => {
                    if let Err(err) = self.set_user_timezone(user_id, &update.value).await {
                        return NotifyDecision::TimezoneFailed { error: err };
                    }
                }
            }
        }

        decision
    }

    pub fn notify_response(decision: &NotifyDecision) -> String {
        match decision {
            NotifyDecision::EmitCalendarEvent { .. } => {
                "Got it — processing your calendar event.".to_string()
            }
            NotifyDecision::EmitTodo { .. } => "Added to your todo list.".to_string(),
            NotifyDecision::EmitConfig { .. } => {
                "I can update your settings. Use the buttons to confirm.".to_string()
            }
            NotifyDecision::ApplyConfig { update } => match update.kind {
                ConfigKind::Timezone => format!("Timezone set to {}.", update.value),
            },
            NotifyDecision::ConfirmConfig { message } => message.clone(),
            NotifyDecision::NeedClarification => {
                "I can help with calendar events, todo items, or settings. What should I do?".to_string()
            }
            NotifyDecision::TodoFailed { error } => {
                format!("Failed to create todo: {}", error)
            }
            NotifyDecision::TimezoneFailed { error } => {
                format!("Failed to update config: {}", error)
            }
        }
    }

    pub async fn handle_notify_with(
        &self,
        responder: &dyn InteractionResponder,
        text: &str,
        user_id: &str,
        channel_id: &str,
    ) -> NotifyDecision {
        let decision = self.handle_notify_internal(text, user_id, channel_id).await;
        if let NotifyDecision::ConfirmConfig { .. } = &decision {
            responder
                .reply_ephemeral_with_components(&Self::notify_response(&decision), config_buttons())
                .await;
            return decision;
        }
        responder.reply_ephemeral(&Self::notify_response(&decision)).await;
        decision
    }

    async fn handle_config_confirm(&self, ctx: &Context, interaction: serenity::all::ComponentInteraction) {
        let user_id = format!("@{}", interaction.user.id);
        let channel_id = interaction.channel_id.to_string();
        let session_key = (user_id.clone(), channel_id);
        let mut sessions = self.sessions.lock().await;
        let Some(session) = sessions.get_mut(&session_key) else {
            let responder = SerenityResponder::for_component(ctx, &interaction);
            responder.reply_update("No pending config update.").await;
            return;
        };
        let Some(update) = session.pending_config.clone() else {
            let responder = SerenityResponder::for_component(ctx, &interaction);
            responder.reply_update("No pending config update.").await;
            return;
        };
        session.pending_config = None;
        session.state = crate::service::notify_flow::SessionState::Unknown;
        drop(sessions);
        let result = match update.kind {
            ConfigKind::Timezone => self.set_user_timezone(&user_id, &update.value).await,
        };
        let responder = SerenityResponder::for_component(ctx, &interaction);
        match result {
            Ok(()) => responder.reply_update(&format!("Timezone set to {}.", update.value)).await,
            Err(err) => responder.reply_update(&format!("Failed to update config: {}", err)).await,
        }
    }

    async fn handle_config_cancel(&self, ctx: &Context, interaction: serenity::all::ComponentInteraction) {
        let user_id = format!("@{}", interaction.user.id);
        let channel_id = interaction.channel_id.to_string();
        let session_key = (user_id.clone(), channel_id);
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get_mut(&session_key) {
            session.pending_config = None;
            session.state = crate::service::notify_flow::SessionState::Unknown;
        }
        let responder = SerenityResponder::for_component(ctx, &interaction);
        responder.reply_update("Canceled config update.").await;
    }

    async fn handle_pending_confirm(
        &self,
        ctx: &Context,
        interaction: serenity::all::ComponentInteraction,
        action_id: &str,
    ) {
        self.event_bus
            .emit(ActionEvent::ApprovalConfirmed {
                action_id: action_id.to_string(),
                user_id: format!("@{}", interaction.user.id),
            })
            .await;

        let responder = SerenityResponder::for_component(ctx, &interaction);
        responder.reply_update("Processing your request.").await;
    }

    async fn handle_pending_cancel(
        &self,
        ctx: &Context,
        interaction: serenity::all::ComponentInteraction,
        action_id: &str,
    ) {
        self.event_bus
            .emit(ActionEvent::ApprovalCanceled {
                action_id: action_id.to_string(),
                user_id: format!("@{}", interaction.user.id),
            })
            .await;

        let responder = SerenityResponder::for_component(ctx, &interaction);
        responder.reply_update("Processing your request.").await;
    }

    async fn handle_pending_context(
        &self,
        ctx: &Context,
        interaction: serenity::all::ComponentInteraction,
        action_id: &str,
    ) {
        let responder = SerenityResponder::for_component(ctx, &interaction);
        self.handle_pending_context_with(&responder, action_id).await;
    }

    pub async fn handle_pending_context_with(
        &self,
        responder: &dyn InteractionResponder,
        action_id: &str,
    ) {
        let modal = CreateModal::new(
            format!("action_context_modal:{}", action_id),
            "Add context",
        )
        .components(vec![CreateActionRow::InputText(
            CreateInputText::new(
                InputTextStyle::Paragraph,
                "Context",
                "context",
            )
            .placeholder("Add any details or corrections (optional)")
            .required(false),
        )]);

        responder.show_modal(modal).await;
    }
}

#[async_trait]
impl EventHandler for BotHandler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let builder = CreateCommand::new("notify")
            .description("Create a notification")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "text",
                    "What should I notify you about?",
                )
                .required(true),
            );

        let _ = Command::create_global_command(&ctx.http, builder).await;

    }

    async fn interaction_create(&self, ctx: Context, interaction: DiscordInteraction) {
        match interaction {
            DiscordInteraction::Command(command) => {
                match command.data.name.as_str() {
                    "notify" => self.handle_notify(&ctx, command).await,
                    _ => {
                        // Unknown or unhandled command; ignore for now.
                    }
                }
            }
            DiscordInteraction::Component(component) => {
                let custom_id = component.data.custom_id.clone();
                if let Some((action, pending_id)) = custom_id.split_once(':') {
                    match action {
                        "action_confirm" => {
                            self.handle_pending_confirm(&ctx, component, pending_id).await;
                        }
                        "action_cancel" => {
                            self.handle_pending_cancel(&ctx, component, pending_id).await;
                        }
                        "action_context" => {
                            self.handle_pending_context(&ctx, component, pending_id).await;
                        }
                        _ => {}
                    }
                } else {
                    match custom_id.as_str() {
                        "config_confirm" => self.handle_config_confirm(&ctx, component).await,
                        "config_cancel" => self.handle_config_cancel(&ctx, component).await,
                        _ => {}
                    }
                }
            }
            other => {
                if let Some(modal) = other.modal_submit() {
                    let custom_id = modal.data.custom_id.as_str();
                    if let Some((action, pending_id)) = custom_id.split_once(':') {
                        if action == "action_context_modal" {
                            let context = modal
                                .data
                                .components
                                .iter()
                                .flat_map(|row| row.components.iter())
                                .find_map(|component| {
                                    if let serenity::all::ActionRowComponent::InputText(input) = component {
                                        if input.custom_id == "context" {
                                            return input.value.clone();
                                        }
                                    }
                                    None
                                })
                                .unwrap_or_default();

                            self.event_bus
                                .emit(ActionEvent::ContextSubmitted {
                                    action_id: pending_id.to_string(),
                                    user_id: format!("@{}", modal.user.id),
                                    context,
                                })
                                .await;

                            let _ = modal
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("Thanks! Updating the notification.")
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                        }
                    }
                }
            }
        }
    }
}
