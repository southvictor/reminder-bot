use crate::events::queue::{Event, EventBus};
use crate::service::reminder_service::{pending_buttons, render_pending_message, PendingReminder};
use crate::models::reminder;
use crate::service::openai_service::OpenAIService;
use crate::service::reminder_service::ReminderService;
use memory_db::DB;
use serde::Serialize;
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
    CreateModal,
    CreateInputText,
};
use serenity::all::InputTextStyle;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct ErrorMessage {
    pub error: String,
}

pub struct BotHandler {
    db: Arc<Mutex<DB<reminder::Reminder>>>,
    openai_api_key: Arc<String>,
    pending: Arc<Mutex<HashMap<String, PendingReminder>>>,
    event_bus: EventBus,
}

impl BotHandler {
    pub fn new(
        db: Arc<Mutex<DB<reminder::Reminder>>>,
        openai_api_key: Arc<String>,
        event_bus: EventBus,
        pending: Arc<Mutex<HashMap<String, PendingReminder>>>,
    ) -> Self {
        BotHandler {
            db,
            openai_api_key,
            pending,
            event_bus,
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
        self.event_bus
            .emit(Event::NotifyRequested {
                text: text.clone(),
                user_id: user_id.clone(),
                channel_id: channel_id.clone(),
            })
            .await;
        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Got it — processing your reminder.")
                        .ephemeral(true),
                ),
            )
            .await;

    }

    async fn handle_pending_confirm(
        &self,
        ctx: &Context,
        interaction: serenity::all::ComponentInteraction,
        pending_id: &str,
    ) {
        let maybe_pending = {
            let pending = self.pending.lock().await;
            pending.get(pending_id).cloned()
        };

        let Some(pending_item) = maybe_pending else {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("This reminder is no longer available.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        if pending_item.user_id != format!("@{}", interaction.user.id) {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Only the original requester can confirm this reminder.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        if pending_item.expires_at < Utc::now() {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content("This reminder request has expired.")
                            .components(vec![]),
                    ),
                )
                .await;
            let mut pending = self.pending.lock().await;
            pending.remove(pending_id);
            return;
        }

        let final_content = pending_item.content.clone();
        eprintln!(
            "Confirming pending {} -> content='{}' time='{}'",
            pending_id,
            pending_item.content,
            pending_item.time
        );

        let mut db = self.db.lock().await;
        if let Err(e) = ReminderService::create(
            &mut db,
            &final_content,
            &pending_item.user_id,
            &pending_item.time,
            &pending_item.channel_id,
        )
        .await
        {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("Failed to persist reminder: {}", e))
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        {
            let mut pending = self.pending.lock().await;
            pending.remove(pending_id);
        }

        let _ = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content(format!(
                            "Confirmed! I'll remind you: \"{}\" at {}",
                            final_content, pending_item.time
                        ))
                        .components(vec![]),
                ),
            )
            .await;
    }

    async fn handle_pending_cancel(
        &self,
        ctx: &Context,
        interaction: serenity::all::ComponentInteraction,
        pending_id: &str,
    ) {
        let maybe_pending = {
            let pending = self.pending.lock().await;
            pending.get(pending_id).cloned()
        };

        let Some(pending_item) = maybe_pending else {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("This reminder is no longer available.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        if pending_item.user_id != format!("@{}", interaction.user.id) {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Only the original requester can cancel this reminder.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        {
            let mut pending = self.pending.lock().await;
            pending.remove(pending_id);
        }

        let _ = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content("Canceled reminder request.")
                        .components(vec![]),
                ),
            )
            .await;
    }

    async fn handle_pending_context(
        &self,
        ctx: &Context,
        interaction: serenity::all::ComponentInteraction,
        pending_id: &str,
    ) {
        let maybe_pending = {
            let pending = self.pending.lock().await;
            pending.get(pending_id).cloned()
        };

        let Some(pending_item) = maybe_pending else {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("This reminder is no longer available.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        if pending_item.user_id != format!("@{}", interaction.user.id) {
            let _ = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Only the original requester can edit this reminder.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let modal = CreateModal::new(
            format!("reminder_context_modal:{}", pending_id),
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

        let _ = interaction
            .create_response(&ctx.http, CreateInteractionResponse::Modal(modal))
            .await;
    }
}

#[async_trait]
impl EventHandler for BotHandler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let builder = CreateCommand::new("notify")
            .description("Create a notification reminder")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "text",
                    "What should I remind you about?",
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
                        "reminder_confirm" => {
                            self.handle_pending_confirm(&ctx, component, pending_id).await;
                        }
                        "reminder_cancel" => {
                            self.handle_pending_cancel(&ctx, component, pending_id).await;
                        }
                        "reminder_context" => {
                            self.handle_pending_context(&ctx, component, pending_id).await;
                        }
                        _ => {}
                    }
                }
            }
            other => {
                if let Some(modal) = other.modal_submit() {
                    let custom_id = modal.data.custom_id.as_str();
                    let Some((_, pending_id)) = custom_id.split_once(':') else {
                        return;
                    };
                    let mut context_value: Option<String> = None;
                    for row in &modal.data.components {
                        for component in &row.components {
                            if let serenity::all::ActionRowComponent::InputText(input) = component {
                                if input.custom_id == "context" {
                                    context_value = Some(input.value.clone().unwrap_or_default());
                                }
                            }
                        }
                    }

                    let maybe_pending: Option<PendingReminder> = {
                        let pending = self.pending.lock().await;
                        pending.get(pending_id).cloned()
                    };

                    let Some(mut pending_item) = maybe_pending else {
                        let _ = modal
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("This reminder is no longer available.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    };

                    if pending_item.user_id != format!("@{}", modal.user.id) {
                        let _ = modal
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Only the original requester can edit this reminder.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }

                    if let Some(ctx_value) = context_value {
                        pending_item.extra_context = Some(ctx_value);
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

                    let openai = OpenAIService::new(self.openai_api_key.as_ref().to_string());
                    let refreshed = match openai
                        .generate_prompt(&combined_prompt, "notification_correction")
                        .await
                    {
                        Ok(payload) => {
                            eprintln!("Correction OpenAI response: {}", payload);
                            match serde_json::from_str::<reminder::AIReminder>(&payload) {
                            Ok(parsed) => Some(parsed),
                            Err(err) => {
                                eprintln!("Failed to parse reminder JSON: {}", err);
                                None
                            }
                        }
                        }
                        Err(err) => {
                            eprintln!("Failed to call OpenAI for reminder: {}", err);
                            None
                        }
                    };

                    if let Some(updated) = refreshed {
                        pending_item.content = updated.content;
                        pending_item.time = updated.time;
                    }

                    {
                        let mut pending = self.pending.lock().await;
                        pending.insert(pending_id.to_string(), pending_item.clone());
                    }
                    eprintln!(
                        "Pending update {} -> content='{}' time='{}'",
                        pending_id,
                        pending_item.content,
                        pending_item.time
                    );

                    let updated_message = render_pending_message(&pending_item);
                    if let Some(message) = &modal.message {
                        let _ = modal
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::UpdateMessage(
                                    CreateInteractionResponseMessage::new()
                                        .content(updated_message)
                                        .components(vec![pending_buttons(pending_id)]),
                                ),
                            )
                            .await;
                        let mut pending = self.pending.lock().await;
                        if let Some(entry) = pending.get_mut(pending_id) {
                            entry.message_id = Some(message.id.get());
                        }
                        return;
                    }

                    let channel_id = match pending_item.channel_id.parse::<u64>() {
                        Ok(id) => serenity::model::id::ChannelId::new(id),
                        Err(_) => {
                            let _ = modal
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("Failed to refresh confirmation.")
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };
                    match channel_id
                        .send_message(
                            &ctx.http,
                            serenity::builder::CreateMessage::new()
                                .content(updated_message)
                                .components(vec![pending_buttons(pending_id)]),
                        )
                        .await
                    {
                        Ok(message) => {
                            let mut pending = self.pending.lock().await;
                            if let Some(entry) = pending.get_mut(pending_id) {
                                entry.message_id = Some(message.id.get());
                            }
                            let _ = modal
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("Context updated.")
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                        }
                        Err(err) => {
                            eprintln!("Failed to send refreshed confirmation: {:?}", err);
                            let _ = modal
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("Context updated, but failed to refresh the preview.")
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

// Minimal Discord "interaction" types for application commands
