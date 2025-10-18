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
    EditInteractionResponse,
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct ErrorMessage {
    pub error: String,
}

pub struct BotHandler {
    db: Arc<Mutex<DB<reminder::Reminder>>>,
    client_secret: Arc<String>,
    openai_api_key: Arc<String>,
}

impl BotHandler {
    pub fn new(
        db: Arc<Mutex<DB<reminder::Reminder>>>,
        client_secret: Arc<String>,
        openai_api_key: Arc<String>,
    ) -> Self {
        BotHandler { db, client_secret, openai_api_key }
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

        // Defer quickly so we don't hit the Discord interaction timeout.
        let _ = command.defer_ephemeral(&ctx.http).await;

        // Load DB, create reminder via OpenAI, and persist.
        let mut db = self.db.lock().await;
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
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content(msg),
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
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content(msg),
                    )
                    .await;
                return;
            }
        };

        let user_id = format!("@{}", command.user.id.to_string());
        let channel_id = command.channel_id.to_string();
        println!("Persisted ai reminder {:?}", ai_reminder);
        if let Err(e) = reminder::create_reminder(
            &mut db,
            &ai_reminder.content,
            &user_id,
            &ai_reminder.time,
            &channel_id,
        )
        .await
        {
            let _ = command
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new()
                        .content(format!("Failed to persist reminder: {}", e)),
                )
                .await;
            return;
        }

        let _ = command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(format!(
                    "Got it! I'll remind you: \"{}\" at {}",
                    ai_reminder.content, ai_reminder.time
                )),
            )
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
        if let DiscordInteraction::Command(command) = interaction {
            match command.data.name.as_str() {
                "notify" => self.handle_notify(&ctx, command).await,
                _ => {
                    // Unknown or unhandled command; ignore for now.
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
