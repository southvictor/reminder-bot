use clap::{Parser, Subcommand};
use chrono::DateTime;
use chrono::Utc;
use serde_json;
use crate::models::notification;
use crate::service::openai_service::OpenAIService;
use crate::service::openai_service::OpenAIClient;
use crate::service::notification_service::NotificationService;
use inquire::Text;
use memory_db::DB;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Create {
        content: String,
        notify_users: String,
        expires_at: DateTime<Utc>,
        channel: String,
    },
    CreatePrompt {}
}

pub async fn cli(
    shared_db: Arc<Mutex<DB<notification::Notification>>>,
    default_user: String,
    default_channel: String,
    openai_api_key: String,
) {
    // Fine to panic here
    let cli = Cli::parse();
    let mut db = shared_db.lock().await;
    match &cli.command {
        Commands::Create { content, notify_users, expires_at , channel} => {
            if let Err(e) = NotificationService::create(&mut db, &content, &notify_users, &expires_at, &channel).await {
                println!("Failed to create notification: {}", e);
            }
        }
        Commands::CreatePrompt {  } => {
            if let Err(e) = create_notification_from_prompt(
                &mut db,
                &default_user,
                &default_channel,
                &openai_api_key,
            )
            .await
            {
                println!("Failed to create notification from prompt {}", e);
            }
        }
    }
}

async fn create_notification_from_prompt(
    db: &mut DB<notification::Notification>,
    default_user: &str,
    default_channel: &str,
    openai_api_key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let user_prompt: String;
    if let Ok(prompt) = specify_prompt() {
        user_prompt = prompt;
    } else {
        println!("No user prompt supplied");
        return Err("No user prompt provided".into());
    }
    
    let openai = OpenAIService::new(openai_api_key.to_string());
    let payload = openai
        .generate_prompt(&user_prompt, "notification")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { format!("{}", e).into() })?;
    println!("{}", payload);
    let ai_notification: notification::AINotification = serde_json::from_str(&payload)?;
    if let Err(e) = NotificationService::create(
        db,
        &ai_notification.content,
        &default_user.to_string(),
        &ai_notification.time,
        &default_channel.to_string(),
    )
    .await
    {
        println!("Failed to create notification: {}", e);
    }
    Ok(())
}

fn specify_prompt() -> Result<String, Box<dyn std::error::Error>> {
    Ok(Text::new("Enter your notifications.").prompt()?)
}

// Env lookups for defaults are handled in main.rs and passed into cli().
