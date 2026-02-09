use std::sync::Arc;

use serenity::http::Http;
use serenity::model::id::ChannelId;

use crate::action::{Action, ActionPayload};
use crate::service::notification_service::{pending_buttons, render_pending_message};

#[serenity::async_trait]
pub trait ApprovalPromptService: Send + Sync {
    async fn prompt(&self, action: &mut Action) -> Result<(), String>;
    async fn update_status(&self, action: &Action, message: &str) -> Result<(), String>;
    async fn update_status_message(
        &self,
        channel_id: &str,
        user_id: &str,
        message: &str,
    ) -> Result<(), String>;
}

pub struct DiscordApprovalPromptService {
    token: Arc<String>,
}

impl DiscordApprovalPromptService {
    pub fn new(token: Arc<String>) -> Self {
        Self { token }
    }

    fn channel_from(&self, channel_id: &str) -> Result<ChannelId, String> {
        let id = channel_id
            .parse::<u64>()
            .map_err(|_| "Invalid channel id".to_string())?;
        Ok(ChannelId::new(id))
    }
}

#[serenity::async_trait]
impl ApprovalPromptService for DiscordApprovalPromptService {
    async fn prompt(&self, action: &mut Action) -> Result<(), String> {
        let draft = match action.payload.as_mut() {
            Some(ActionPayload::NotificationDraft(draft)) => draft,
            _ => return Err("unsupported action payload".to_string()),
        };

        let message_body = render_pending_message(draft);
        let buttons = pending_buttons(&action.id);
        let http: Http = Http::new(self.token.as_ref());
        let channel = self.channel_from(&draft.channel_id)?;

        let message = channel
            .send_message(
                &http,
                serenity::builder::CreateMessage::new()
                    .content(message_body)
                    .components(vec![buttons]),
            )
            .await
            .map_err(|err| format!("Failed to send approval prompt: {err}"))?;

        draft.message_id = Some(message.id.get());
        Ok(())
    }

    async fn update_status(&self, action: &Action, message: &str) -> Result<(), String> {
        let http: Http = Http::new(self.token.as_ref());
        let channel = self.channel_from(&action.channel_id)?;
        channel
            .send_message(
                &http,
                serenity::builder::CreateMessage::new().content(message),
            )
            .await
            .map_err(|err| format!("Failed to send status message: {err}"))?;
        Ok(())
    }

    async fn update_status_message(
        &self,
        channel_id: &str,
        user_id: &str,
        message: &str,
    ) -> Result<(), String> {
        let http: Http = Http::new(self.token.as_ref());
        let channel = self.channel_from(channel_id)?;
        let content = format!("<{}> {}", user_id, message);
        channel
            .say(&http, content)
            .await
            .map_err(|err| format!("Failed to send status message: {err}"))?;
        Ok(())
    }
}
