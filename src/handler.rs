use crate::reminder;
use crate::openai_client;
use crate::notification_loop::run_notification_loop;
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
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct ErrorMessage {
    pub error: String,
}

pub struct BotHandler {
    db: Arc<Mutex<DB<reminder::Reminder>>>,
    client_secret: Arc<String>,
    openai_api_key: Arc<String>,
    pending: Arc<Mutex<HashMap<String, PendingReminder>>>,
}

#[derive(Clone)]
struct PendingReminder {
    id: String,
    user_id: String,
    channel_id: String,
    content: String,
    time: DateTime<Utc>,
    original_text: String,
    extra_context: Option<String>,
    expires_at: DateTime<Utc>,
    message_id: Option<u64>,
}

impl BotHandler {
    pub fn new(
        db: Arc<Mutex<DB<reminder::Reminder>>>,
        client_secret: Arc<String>,
        openai_api_key: Arc<String>,
    ) -> Self {
        BotHandler {
            db,
            client_secret,
            openai_api_key,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl BotHandler {
    fn render_pending_message(pending: &PendingReminder) -> String {
        let mut body: String = format!(
            "Please confirm your reminder:\nContent: {}\nTime: {}",
            pending.content,
            pending.time
        );
        if let Some(ctx) = &pending.extra_context {
            if !ctx.trim().is_empty() {
                body.push_str(&format!("\nAdditional context: {}", ctx.trim()));
            }
        }
        body
    }

    fn pending_buttons(pending_id: &str) -> CreateActionRow {
        CreateActionRow::Buttons(vec![
            CreateButton::new(format!("pending_confirm:{}", pending_id))
                .label("Confirm date/time")
                .style(serenity::all::ButtonStyle::Success),
            CreateButton::new(format!("pending_context:{}", pending_id))
                .label("Add context")
                .style(serenity::all::ButtonStyle::Primary),
            CreateButton::new(format!("pending_cancel:{}", pending_id))
                .label("Cancel")
                .style(serenity::all::ButtonStyle::Danger),
        ])
    }

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

        // Create reminder via OpenAI, then always go through pending confirmation.
        let payload = match openai_client::generate_openai_prompt(
            &text,
            "notification",
            &self.openai_api_key,
        )
        .await
        {
            Ok(p) => p,
            Err(err) => {
                let msg = format!("Failed to call OpenAI for reminder: {}", err);
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(msg)
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }
        };

        let ai_reminder: reminder::AIReminder = match serde_json::from_str(&payload) {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("Failed to parse reminder JSON: {}", e);
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(msg)
                                .ephemeral(true),
                        ),
                    )
                    .await;
                return;
            }
        };

        let user_id = format!("@{}", command.user.id.to_string());
        let channel_id = command.channel_id.to_string();
        let pending_id = Uuid::new_v4().to_string();
        let pending_item = PendingReminder {
            id: pending_id.clone(),
            user_id: user_id.clone(),
            channel_id: channel_id.clone(),
            content: ai_reminder.content,
            time: ai_reminder.time,
            original_text: text.clone(),
            extra_context: None,
            expires_at: Utc::now() + Duration::minutes(5),
            message_id: None,
        };
        let message_body = Self::render_pending_message(&pending_item);
        let buttons = Self::pending_buttons(&pending_id);

        {
            let mut pending = self.pending.lock().await;
            pending.insert(pending_id.clone(), pending_item);
        }

        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(message_body)
                        .components(vec![buttons]),
                ),
            )
            .await;

        if let Ok(response) = command.get_response(&ctx.http).await {
            let mut pending = self.pending.lock().await;
            if let Some(entry) = pending.get_mut(&pending_id) {
                entry.message_id = Some(response.id.get());
            }
        }
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
        if let Err(e) = reminder::create_reminder(
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
            format!("pending_context_modal:{}", pending_id),
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

        let db_clone = self.db.clone();
        let secret_clone = self.client_secret.clone();
        let openai_clone = self.openai_api_key.clone();
        tokio::spawn(async move {
            run_notification_loop(db_clone, secret_clone, openai_clone).await;
        });
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
                        "pending_confirm" => {
                            self.handle_pending_confirm(&ctx, component, pending_id).await;
                        }
                        "pending_cancel" => {
                            self.handle_pending_cancel(&ctx, component, pending_id).await;
                        }
                        "pending_context" => {
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

                    let refreshed = match openai_client::generate_openai_prompt(
                        &combined_prompt,
                        "notification_correction",
                        &self.openai_api_key,
                    )
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

                    let updated_message = Self::render_pending_message(&pending_item);
                    if let Some(message) = &modal.message {
                        let _ = modal
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::UpdateMessage(
                                    CreateInteractionResponseMessage::new()
                                        .content(updated_message)
                                        .components(vec![Self::pending_buttons(pending_id)]),
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
                                .components(vec![Self::pending_buttons(pending_id)]),
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
#[derive(Debug, Deserialize)]
pub struct Interaction {
    pub id: String,
    pub application_id: String,
    #[serde(rename = "type")]
    pub kind: u8,
    pub data: Option<ApplicationCommandData>,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub member: Option<InteractionMember>,
    pub user: Option<InteractionUser>, // present in DMs
    pub token: String,
    pub version: u8,
}

#[derive(Debug, Deserialize)]
pub struct ApplicationCommandData {
    pub name: String,
    pub options: Option<Vec<ApplicationCommandDataOption>>,
}

#[derive(Debug, Deserialize)]
pub struct ApplicationCommandDataOption {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u8,
    pub value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InteractionMember {
    pub user: InteractionUser,
}

#[derive(Debug, Deserialize)]
pub struct InteractionUser {
    pub id: String,
    pub username: String,
}
