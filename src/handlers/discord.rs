use crate::events::queue::{Event, EventBus};
use crate::service::notify_flow::{route_notify, NotifyDecision, PendingSession, SessionKey};
use crate::service::notification_service::{pending_buttons, render_pending_message, PendingNotification};
use crate::service::routing::IntentRouter;
use crate::models::notification;
use crate::models::todo::{self, TodoItem};
use crate::service::openai_service::OpenAIClient;
use crate::service::openai_service::OpenAIService;
use crate::service::notification_service::NotificationService;
use memory_db::{DB, save_db};
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
    db: Arc<Mutex<DB<notification::Notification>>>,
    todo_db: Arc<Mutex<DB<todo::TodoItem>>>,
    openai_api_key: Arc<String>,
    pending: Arc<Mutex<HashMap<String, PendingNotification>>>,
    sessions: Arc<Mutex<HashMap<SessionKey, PendingSession>>>,
    router: Arc<dyn IntentRouter>,
    event_bus: EventBus,
}

impl BotHandler {
    pub fn new(
        db: Arc<Mutex<DB<notification::Notification>>>,
        todo_db: Arc<Mutex<DB<todo::TodoItem>>>,
        openai_api_key: Arc<String>,
        event_bus: EventBus,
        pending: Arc<Mutex<HashMap<String, PendingNotification>>>,
        sessions: Arc<Mutex<HashMap<SessionKey, PendingSession>>>,
        router: Arc<dyn IntentRouter>,
    ) -> Self {
        BotHandler {
            db,
            todo_db,
            openai_api_key,
            pending,
            sessions,
            router,
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
        let session_key = (user_id.clone(), channel_id.clone());
        let now = Utc::now();

        let decision = {
            let mut sessions = self.sessions.lock().await;
            route_notify(
                self.router.as_ref(),
                &mut sessions,
                session_key,
                text.clone(),
                now,
            )
            .await
        };

        match decision {
            NotifyDecision::EmitNotify { normalized_text } => {
                self.event_bus
                    .emit(Event::NotifyRequested {
                        text: normalized_text,
                        user_id: user_id.clone(),
                        channel_id: channel_id.clone(),
                    })
                    .await;
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Got it — processing your notification.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
            }
            NotifyDecision::NeedClarification => {
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("I can set notifications. What should I notify you about, and when? Re-run /notify with a time.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
            }
        }

    }

    async fn handle_todo_add(&self, ctx: &Context, command: serenity::all::CommandInteraction) {
        let text = command
            .data
            .options
            .iter()
            .find(|opt| opt.name == "add")
            .and_then(|opt| match &opt.value {
                serenity::all::CommandDataOptionValue::SubCommand(sub) => Some(sub),
                _ => None,
            })
            .and_then(|sub| {
                sub.iter()
                    .find(|opt| opt.name == "text")
                    .and_then(|opt| match &opt.value {
                        serenity::all::CommandDataOptionValue::String(s) => Some(s.as_str()),
                        _ => None,
                    })
            })
            .unwrap_or("")
            .to_string();

        if text.trim().is_empty() {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Missing `text` argument for /todo add")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let user_id = command.user.id.to_string();
        let mut db = self.todo_db.lock().await;
        if let Err(err) = todo::create_todo(&mut db, &user_id, &text) {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("Failed to create todo: {}", err))
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Added to your todo list.")
                        .ephemeral(true),
                ),
            )
            .await;
    }

    async fn handle_todo_list(&self, ctx: &Context, command: serenity::all::CommandInteraction) {
        let user_id = command.user.id.to_string();
        let db = self.todo_db.lock().await;
        let mut items: Vec<TodoItem> = db
            .values()
            .filter(|item| item.user_id == user_id && item.completed_at.is_none())
            .cloned()
            .collect();
        items.sort_by_key(|item| item.created_at);

        let content = if items.is_empty() {
            "You have no open todos.".to_string()
        } else {
            let mut body = String::from("Your open todos:\n");
            for (idx, item) in items.iter().enumerate() {
                body.push_str(&format!("{}) {}\n", idx + 1, item.content));
            }
            body.trim_end().to_string()
        };

        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(content)
                        .ephemeral(true),
                ),
            )
            .await;
    }

    async fn handle_todo_done(&self, ctx: &Context, command: serenity::all::CommandInteraction) {
        let index = command
            .data
            .options
            .iter()
            .find(|opt| opt.name == "done")
            .and_then(|opt| match &opt.value {
                serenity::all::CommandDataOptionValue::SubCommand(sub) => Some(sub),
                _ => None,
            })
            .and_then(|sub| {
                sub.iter()
                    .find(|opt| opt.name == "index")
                    .and_then(|opt| match opt.value {
                        serenity::all::CommandDataOptionValue::Integer(v) => Some(v),
                        _ => None,
                    })
            })
            .unwrap_or(0);

        if index <= 0 {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Provide a valid index for /todo done.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let user_id = command.user.id.to_string();
        let mut db = self.todo_db.lock().await;
        let mut items: Vec<TodoItem> = db
            .values()
            .filter(|item| item.user_id == user_id && item.completed_at.is_none())
            .cloned()
            .collect();
        items.sort_by_key(|item| item.created_at);

        let idx = (index - 1) as usize;
        let Some(item) = items.get(idx) else {
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("That todo index does not exist.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        };

        if let Some(entry) = db.get_mut(&item.id) {
            entry.completed_at = Some(Utc::now());
        }
        let _ = save_db(&todo::get_db_location(), &db);

        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Marked as done.")
                        .ephemeral(true),
                ),
            )
            .await;
    }

    async fn handle_todo_clear(&self, ctx: &Context, command: serenity::all::CommandInteraction) {
        let user_id = command.user.id.to_string();
        let mut db = self.todo_db.lock().await;
        let to_remove: Vec<String> = db
            .values()
            .filter(|item| item.user_id == user_id && item.completed_at.is_some())
            .map(|item| item.id.clone())
            .collect();

        for id in to_remove {
            db.remove(id.as_str());
        }
        let _ = save_db(&todo::get_db_location(), &db);

        let _ = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("Cleared completed todos.")
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
                            .content("This notification is no longer available.")
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
                            .content("Only the original requester can confirm this notification.")
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
                            .content("This notification request has expired.")
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
        if let Err(e) = NotificationService::create(
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
                            .content(format!("Failed to persist notification: {}", e))
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
                            "Confirmed! I'll notify you: \"{}\" at {}",
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
                            .content("This notification is no longer available.")
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
                            .content("Only the original requester can cancel this notification.")
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
                        .content("Canceled notification request.")
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
                            .content("This notification is no longer available.")
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
                            .content("Only the original requester can edit this notification.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let modal = CreateModal::new(
            format!("notification_context_modal:{}", pending_id),
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

        let todo_builder = CreateCommand::new("todo")
            .description("Manage your todo list")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "add",
                    "Add a new todo item",
                )
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "text",
                        "Todo text",
                    )
                    .required(true),
                ),
            )
            .add_option(CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "list",
                "List open todos",
            ))
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "done",
                    "Mark a todo as done by index",
                )
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::Integer,
                        "index",
                        "Index from /todo list",
                    )
                    .required(true),
                ),
            )
            .add_option(CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "clear",
                "Clear completed todos",
            ));

        let _ = Command::create_global_command(&ctx.http, todo_builder).await;

    }

    async fn interaction_create(&self, ctx: Context, interaction: DiscordInteraction) {
        match interaction {
            DiscordInteraction::Command(command) => {
                match command.data.name.as_str() {
                    "notify" => self.handle_notify(&ctx, command).await,
                    "todo" => {
                        let sub = command
                            .data
                            .options
                            .first()
                            .map(|opt| opt.name.as_str())
                            .unwrap_or("");
                        match sub {
                            "add" => self.handle_todo_add(&ctx, command).await,
                            "list" => self.handle_todo_list(&ctx, command).await,
                            "done" => self.handle_todo_done(&ctx, command).await,
                            "clear" => self.handle_todo_clear(&ctx, command).await,
                            _ => {}
                        }
                    }
                    _ => {
                        // Unknown or unhandled command; ignore for now.
                    }
                }
            }
            DiscordInteraction::Component(component) => {
                let custom_id = component.data.custom_id.clone();
                if let Some((action, pending_id)) = custom_id.split_once(':') {
                    match action {
                        "notification_confirm" => {
                            self.handle_pending_confirm(&ctx, component, pending_id).await;
                        }
                        "notification_cancel" => {
                            self.handle_pending_cancel(&ctx, component, pending_id).await;
                        }
                        "notification_context" => {
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

                    let maybe_pending: Option<PendingNotification> = {
                        let pending = self.pending.lock().await;
                        pending.get(pending_id).cloned()
                    };

                    let Some(mut pending_item) = maybe_pending else {
                        let _ = modal
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("This notification is no longer available.")
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
                                        .content("Only the original requester can edit this notification.")
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
                            match serde_json::from_str::<notification::AINotification>(&payload) {
                            Ok(parsed) => Some(parsed),
                            Err(err) => {
                                eprintln!("Failed to parse notification JSON: {}", err);
                                None
                            }
                        }
                        }
                        Err(err) => {
                            eprintln!("Failed to call OpenAI for notification: {}", err);
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
