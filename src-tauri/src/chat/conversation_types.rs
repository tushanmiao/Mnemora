//! 本地会话文件的数据合同。
//!
//! `StoredConversation` 对应单个 `conv_<id>.json`，`ConversationListItem` 对应轻量索引。
//! 类型只包含 Chat 展示与恢复所需数据，不包含 Provider Base URL 或 API Key。

use serde::{Deserialize, Serialize};

use crate::{
    ai::types::{ModelRole, ModelUsage},
    settings::types::validate_stable_id,
};

const MAX_MESSAGES: usize = 500;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_TITLE_CHARS: usize = 500;
const MAX_SYSTEM_PROMPT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageStatus {
    Pending,
    Streaming,
    Completed,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiPermissionMode {
    AskEveryTime,
    AskSensitive,
    FullAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSnapshot {
    pub id: String,
    pub api_model: String,
    pub display_name: String,
    pub provider_id: String,
    pub provider_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChatMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: ModelRole,
    pub content: String,
    pub status: MessageStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_snapshot: Option<ModelSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredConversation {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub messages: Vec<StoredChatMessage>,
    pub assistant_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    #[serde(default)]
    pub system_prompt: String,
    pub permission_mode: AiPermissionMode,
    pub project_id: Option<String>,
    pub collection_id: Option<String>,
    pub pinned: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListItem {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub message_count: usize,
    pub assistant_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub project_id: Option<String>,
    pub collection_id: Option<String>,
    pub pinned: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

pub fn validate_conversation_id(label: &str, value: &str) -> Result<(), String> {
    validate_stable_id(label, value)?;
    if value.contains(':') {
        return Err(format!("{label} cannot contain ':'"));
    }
    Ok(())
}

impl StoredConversation {
    pub fn validate(&self) -> Result<(), String> {
        validate_conversation_id("Conversation ID", &self.id)?;
        if self.title.trim().is_empty() || self.title.chars().count() > MAX_TITLE_CHARS {
            return Err("Conversation title is empty or too long".to_string());
        }
        if self.messages.len() > MAX_MESSAGES {
            return Err(format!("Conversation cannot exceed {MAX_MESSAGES} messages"));
        }
        if self.system_prompt.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err("Conversation System Prompt is too long".to_string());
        }
        validate_optional_id("Assistant ID", self.assistant_id.as_deref())?;
        validate_optional_id("Provider ID", self.provider_id.as_deref())?;
        validate_optional_id("Model ID", self.model_id.as_deref())?;
        if self.provider_id.is_some() && self.model_id.is_none() {
            return Err("Conversation provider ID requires a model ID".to_string());
        }
        validate_optional_id("Project ID", self.project_id.as_deref())?;
        validate_optional_id("Collection ID", self.collection_id.as_deref())?;

        for message in &self.messages {
            validate_stable_id("Message ID", &message.id)?;
            if message.conversation_id != self.id {
                return Err("Message conversation ID does not match its conversation".to_string());
            }
            if message.content.len() > MAX_MESSAGE_BYTES {
                return Err("Chat message is too long".to_string());
            }
            if message.error_message.as_ref().is_some_and(|value| value.len() > 64 * 1024) {
                return Err("Chat error message is too long".to_string());
            }
            validate_optional_id("Message model ID", message.model_id.as_deref())?;
            if let Some(snapshot) = &message.model_snapshot {
                validate_stable_id("Snapshot model ID", &snapshot.id)?;
                validate_stable_id("Snapshot provider ID", &snapshot.provider_id)?;
            }
        }
        Ok(())
    }

    pub fn to_list_item(&self) -> ConversationListItem {
        let preview = self
            .messages
            .iter()
            .rev()
            .find_map(|message| {
                let text = if message.content.trim().is_empty() {
                    message.error_message.as_deref().unwrap_or_default()
                } else {
                    &message.content
                };
                (!text.trim().is_empty()).then(|| text.chars().take(160).collect::<String>())
            })
            .unwrap_or_else(|| "暂无消息".to_string());
        ConversationListItem {
            id: self.id.clone(),
            title: self.title.clone(),
            preview,
            message_count: self.messages.len(),
            assistant_id: self.assistant_id.clone(),
            provider_id: self.provider_id.clone(),
            model_id: self.model_id.clone(),
            project_id: self.project_id.clone(),
            collection_id: self.collection_id.clone(),
            pinned: self.pinned,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

fn validate_optional_id(label: &str, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        validate_stable_id(label, value)?;
    }
    Ok(())
}
