use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use memory_db::DB;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::models::notification::{self, Notification};
use crate::service::approval_prompt::ApprovalPromptService;
use crate::service::notification_service::NotificationService;
use crate::service::openai_service::OpenAIClient;

pub type ActionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType {
    Unknown,
    CreateNotification,
    CreateTodo,
    ToolUse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionStatus {
    Pending,
    AwaitingApproval,
    Approved,
    Rejected,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationDraft {
    pub user_id: String,
    pub channel_id: String,
    pub content: String,
    pub time: DateTime<Utc>,
    pub original_text: String,
    pub extra_context: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub message_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionPayload {
    NotificationDraft(NotificationDraft),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: ActionId,
    pub action_type: ActionType,
    pub status: ActionStatus,
    pub user_id: String,
    pub channel_id: String,
    pub payload: Option<ActionPayload>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Action {
    pub fn notification_draft(&self) -> Option<&NotificationDraft> {
        match &self.payload {
            Some(ActionPayload::NotificationDraft(draft)) => Some(draft),
            _ => None,
        }
    }

    pub fn notification_draft_mut(&mut self) -> Option<&mut NotificationDraft> {
        match &mut self.payload {
            Some(ActionPayload::NotificationDraft(draft)) => Some(draft),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActionStore {
    actions: HashMap<ActionId, Action>,
}

impl ActionStore {
    pub fn new() -> Self {
        Self {
            actions: HashMap::new(),
        }
    }

    pub fn insert(&mut self, action: Action) {
        self.actions.insert(action.id.clone(), action);
    }

    pub fn get(&self, id: &str) -> Option<&Action> {
        self.actions.get(id)
    }

    #[allow(dead_code)]
    pub fn ids(&self) -> Vec<ActionId> {
        self.actions.keys().cloned().collect()
    }
}

#[derive(Debug)]
pub enum ActionEvent {
    NotifyRequested {
        text: String,
        user_id: String,
        channel_id: String,
    },
    ApprovalConfirmed {
        action_id: String,
        user_id: String,
    },
    ApprovalCanceled {
        action_id: String,
        user_id: String,
    },
    ContextSubmitted {
        action_id: String,
        user_id: String,
        context: String,
    },
}

pub struct ActionEngine {
    store: Arc<Mutex<ActionStore>>,
    openai: Arc<dyn OpenAIClient>,
    approval: Arc<dyn ApprovalPromptService>,
    notification_db: Arc<Mutex<DB<Notification>>>,
}

impl ActionEngine {
    pub fn new(
        store: Arc<Mutex<ActionStore>>,
        openai: Arc<dyn OpenAIClient>,
        approval: Arc<dyn ApprovalPromptService>,
        notification_db: Arc<Mutex<DB<Notification>>>,
    ) -> Self {
        Self {
            store,
            openai,
            approval,
            notification_db,
        }
    }

    pub async fn handle_event(&self, event: ActionEvent) {
        match event {
            ActionEvent::NotifyRequested {
                text,
                user_id,
                channel_id,
            } => {
                let payload = match self.openai.generate_prompt(&text, "notification").await {
                    Ok(p) => p,
                    Err(err) => {
                        let _ = self.approval.update_status_message(
                            &channel_id,
                            &user_id,
                            &format!("Failed to call OpenAI for notification: {}", err),
                        ).await;
                        return;
                    }
                };

                let ai_notification: notification::AINotification = match serde_json::from_str(&payload) {
                    Ok(r) => r,
                    Err(err) => {
                        let _ = self.approval.update_status_message(
                            &channel_id,
                            &user_id,
                            &format!("Failed to parse notification JSON: {}", err),
                        ).await;
                        return;
                    }
                };

                let now = Utc::now();
                let pending_id = Uuid::new_v4().to_string();
                let mut action = Action {
                    id: pending_id,
                    action_type: ActionType::CreateNotification,
                    status: ActionStatus::AwaitingApproval,
                    user_id: user_id.clone(),
                    channel_id: channel_id.clone(),
                    payload: Some(ActionPayload::NotificationDraft(NotificationDraft {
                        user_id: user_id.clone(),
                        channel_id: channel_id.clone(),
                        content: ai_notification.content,
                        time: ai_notification.time,
                        original_text: text.clone(),
                        extra_context: None,
                        expires_at: now + Duration::minutes(5),
                        message_id: None,
                    })),
                    created_at: now,
                    updated_at: now,
                };

                if self.approval.prompt(&mut action).await.is_err() {
                    action.status = ActionStatus::Failed;
                }

                let mut store = self.store.lock().await;
                store.insert(action);
            }
            ActionEvent::ApprovalConfirmed { action_id, user_id } => {
                let action_snapshot = {
                    let store = self.store.lock().await;
                    store.get(&action_id).cloned()
                };

                let Some(mut action) = action_snapshot else {
                    return;
                };

                if action.user_id != user_id || action.status != ActionStatus::AwaitingApproval {
                    return;
                }

                action.status = ActionStatus::Approved;
                action.updated_at = Utc::now();

                let Some(draft) = action.notification_draft() else {
                    action.status = ActionStatus::Failed;
                    action.updated_at = Utc::now();
                    let _ = self
                        .approval
                        .update_status_message(
                            &action.channel_id,
                            &action.user_id,
                            "Failed to persist notification.",
                        )
                        .await;
                    let mut store = self.store.lock().await;
                    store.insert(action);
                    return;
                };

                let mut db = self.notification_db.lock().await;
                let result = NotificationService::create(
                    &mut db,
                    &draft.content,
                    &action.user_id,
                    &draft.time,
                    &action.channel_id,
                )
                .await;

                if result.is_ok() {
                    action.status = ActionStatus::Completed;
                    action.updated_at = Utc::now();
                    let message = if let Some(draft) = action.notification_draft() {
                        format!(
                            "Confirmed! I'll notify you: \"{}\" at {}",
                            draft.content, draft.time
                        )
                    } else {
                        "Confirmed notification.".to_string()
                    };
                    let _ = self.approval.update_status(&action, &message).await;
                } else {
                    action.status = ActionStatus::Failed;
                    action.updated_at = Utc::now();
                    let _ = self.approval.update_status_message(
                        &action.channel_id,
                        &action.user_id,
                        "Failed to persist notification.",
                    ).await;
                }

                let mut store = self.store.lock().await;
                store.insert(action);
            }
            ActionEvent::ApprovalCanceled { action_id, user_id } => {
                let action_snapshot = {
                    let store = self.store.lock().await;
                    store.get(&action_id).cloned()
                };

                let Some(mut action) = action_snapshot else {
                    return;
                };

                if action.user_id != user_id || action.status != ActionStatus::AwaitingApproval {
                    return;
                }

                action.status = ActionStatus::Rejected;
                action.updated_at = Utc::now();
                let _ = self.approval.update_status(&action, "Canceled notification request.").await;

                let mut store = self.store.lock().await;
                store.insert(action);
            }
            ActionEvent::ContextSubmitted {
                action_id,
                user_id,
                context,
            } => {
                let action_snapshot = {
                    let store = self.store.lock().await;
                    store.get(&action_id).cloned()
                };

                let Some(mut action) = action_snapshot else {
                    return;
                };

                if action.user_id != user_id || action.status != ActionStatus::AwaitingApproval {
                    return;
                }

                let mut combined_prompt = if let Some(draft) = action.notification_draft() {
                    draft.original_text.clone()
                } else {
                    return;
                };

                if !context.trim().is_empty() {
                    combined_prompt = format!(
                        "Original request: {original}\nCorrection note: {context}",
                        original = combined_prompt,
                        context = context.trim()
                    );
                }

                let refreshed = match self
                    .openai
                    .generate_prompt(&combined_prompt, "notification_correction")
                    .await
                {
                    Ok(payload) => serde_json::from_str::<notification::AINotification>(&payload).ok(),
                    Err(_) => None,
                };

                if let Some(updated) = refreshed {
                    if let Some(draft) = action.notification_draft_mut() {
                        if !context.trim().is_empty() {
                            draft.extra_context = Some(context.trim().to_string());
                        }
                        draft.content = updated.content;
                        draft.time = updated.time;
                    }

                    let _ = self.approval.prompt(&mut action).await;
                    action.updated_at = Utc::now();

                    let mut store = self.store.lock().await;
                    store.insert(action);
                }
            }
        }
    }
}
