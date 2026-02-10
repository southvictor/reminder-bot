use crate::action::ActionEvent;
use crate::events::queue::EventBus;
use crate::handlers::discord_responder::{InteractionResponder, SerenityResponder};
use crate::service::notify_flow::{route_notify, NotifyDecision, PendingSession, SessionKey};
use crate::service::routing::IntentRouter;
use crate::models::todo::{self, TodoItem};
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
    todo_db: Arc<Mutex<DB<todo::TodoItem>>>,
    sessions: Arc<Mutex<HashMap<SessionKey, PendingSession>>>,
    router: Arc<dyn IntentRouter>,
    event_bus: EventBus,
}

impl BotHandler {
    pub fn new(
        todo_db: Arc<Mutex<DB<todo::TodoItem>>>,
        event_bus: EventBus,
        sessions: Arc<Mutex<HashMap<SessionKey, PendingSession>>>,
        router: Arc<dyn IntentRouter>,
    ) -> Self {
        BotHandler {
            todo_db,
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
        let responder = SerenityResponder::for_command(ctx, &command);
        self.handle_notify_with(&responder, &text, &user_id, &channel_id)
            .await;

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

        if let NotifyDecision::EmitNotify { normalized_text } = &decision {
            self.event_bus
                .emit(ActionEvent::NotifyRequested {
                    text: normalized_text.clone(),
                    user_id: user_id.to_string(),
                    channel_id: channel_id.to_string(),
                })
                .await;
        }

        decision
    }

    pub fn notify_response(decision: &NotifyDecision) -> &'static str {
        match decision {
            NotifyDecision::EmitNotify { .. } => {
                "Got it — processing your notification."
            }
            NotifyDecision::NeedClarification => {
                "I can set notifications. What should I notify you about, and when? Re-run /notify with a time."
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
        responder.reply_ephemeral(Self::notify_response(&decision)).await;
        decision
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
                }
            }
            other => {
                if let Some(modal) = other.modal_submit() {
                    let custom_id = modal.data.custom_id.as_str();
                    let Some((prefix, action_id)) = custom_id.split_once(':') else {
                        return;
                    };
                    if prefix != "action_context_modal" {
                        return;
                    }
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

                    self.event_bus
                        .emit(ActionEvent::ContextSubmitted {
                            action_id: action_id.to_string(),
                            user_id: format!("@{}", modal.user.id),
                            context: context_value.unwrap_or_default(),
                        })
                        .await;

                    let _ = modal
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Thanks! Updating your notification preview.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
            }
        }
    }
}

// Minimal Discord "interaction" types for application commands
