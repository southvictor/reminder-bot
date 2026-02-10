use serenity::async_trait;
use serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage, CreateModal};
use serenity::all::{CommandInteraction, ComponentInteraction};
use serenity::prelude::Context;

#[async_trait]
pub trait InteractionResponder: Send + Sync {
    async fn reply_ephemeral(&self, content: &str);
    async fn reply_update(&self, content: &str);
    async fn show_modal(&self, modal: CreateModal);
}

pub struct SerenityResponder<'a> {
    ctx: &'a Context,
    command: Option<&'a CommandInteraction>,
    component: Option<&'a ComponentInteraction>,
}

impl<'a> SerenityResponder<'a> {
    pub fn for_command(ctx: &'a Context, command: &'a CommandInteraction) -> Self {
        Self {
            ctx,
            command: Some(command),
            component: None,
        }
    }

    pub fn for_component(ctx: &'a Context, component: &'a ComponentInteraction) -> Self {
        Self {
            ctx,
            command: None,
            component: Some(component),
        }
    }
}

#[async_trait]
impl InteractionResponder for SerenityResponder<'_> {
    async fn reply_ephemeral(&self, content: &str) {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(content)
                .ephemeral(true),
        );
        if let Some(command) = self.command {
            let _ = command.create_response(&self.ctx.http, response).await;
            return;
        }
        if let Some(component) = self.component {
            let _ = component.create_response(&self.ctx.http, response).await;
        }
    }

    async fn reply_update(&self, content: &str) {
        if let Some(component) = self.component {
            let _ = component
                .create_response(
                    &self.ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .content(content)
                            .components(vec![]),
                    ),
                )
                .await;
            return;
        }
        if let Some(command) = self.command {
            let _ = command
                .create_response(
                    &self.ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content(content),
                    ),
                )
                .await;
        }
    }

    async fn show_modal(&self, modal: CreateModal) {
        if let Some(component) = self.component {
            let _ = component
                .create_response(&self.ctx.http, CreateInteractionResponse::Modal(modal))
                .await;
        }
    }
}
