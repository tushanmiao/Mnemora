//! 本地会话文件的数据合同。
//!
//! `StoredConversation` 对应单个 `conv_<id>.json`，`ConversationListItem` 对应轻量索引。
//! 类型只包含 Chat 展示与恢复所需数据，不包含 Provider Base URL 或 API Key。

use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

use crate::{
    ai::types::{ModelRole, ModelUsage},
    settings::types::validate_stable_id,
};

const MAX_MESSAGES: usize = 500;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_TITLE_CHARS: usize = 500;
const MAX_SYSTEM_PROMPT_BYTES: usize = 256 * 1024;
const MAX_CONTEXT_SUMMARY_BYTES: usize = 256 * 1024;
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 8;
const MAX_ATTACHMENT_NAME_CHARS: usize = 255;
const MAX_ATTACHMENT_MIME_BYTES: usize = 128;
const MAX_ATTACHMENT_FILE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_LINKED_LIBRARY_ITEMS: usize = 12;
const MAX_LITERATURE_REFERENCES_PER_MESSAGE: usize = 8;
const MAX_LITERATURE_REFERENCE_TEXT_BYTES: usize = 32 * 1024;
const MAX_LITERATURE_REFERENCE_TOTAL_BYTES: usize = 128 * 1024;
const MAX_LITERATURE_PAGE_INDEX: u32 = 1_000_000;
const MAX_NOTE_REFERENCES_PER_MESSAGE: usize = 10;
const MAX_NOTE_REFERENCE_TEXT_BYTES: usize = 16 * 1024;
const MAX_NOTE_REFERENCE_TOTAL_BYTES: usize = 64 * 1024;

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
pub enum AgentRunStatus {
    Preparing,
    Running,
    WaitingApproval,
    WaitingUser,
    Paused,
    Checkpointing,
    Finalizing,
    Completed,
    Failed,
    Stopped,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkflowSummary {
    pub status: AgentRunStatus,
    pub step_count: u32,
    pub tool_call_count: u32,
    pub skill_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiPermissionMode {
    AskEveryTime,
    #[default]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<crate::settings::types::ApiProtocol>,
}

/** 助手消息生成时实际激活的 Skill 版本快照，不复制完整技能正文。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivatedSkillSnapshot {
    pub id: String,
    pub name: String,
    pub version: String,
    pub content_hash: String,
    pub activation: String,
}

/** 会话消息中附件的轻量元数据；文件正文保存在会话独立目录，不写入 JSON。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChatAttachment {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

impl StoredChatAttachment {
    pub fn validate(&self) -> Result<(), String> {
        validate_stable_id("Attachment ID", &self.id)?;
        if !matches!(self.kind.as_str(), "image" | "file") {
            return Err("Attachment kind is invalid".to_string());
        }
        if self.name.trim().is_empty() || self.name.chars().count() > MAX_ATTACHMENT_NAME_CHARS {
            return Err("Attachment name is empty or too long".to_string());
        }
        if self.mime_type.trim().is_empty() || self.mime_type.len() > MAX_ATTACHMENT_MIME_BYTES {
            return Err("Attachment MIME type is invalid".to_string());
        }
        if self.size_bytes == 0 || self.size_bytes > MAX_ATTACHMENT_FILE_BYTES {
            return Err("Attachment size is invalid".to_string());
        }
        let mut components = Path::new(&self.path).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err("Attachment path must be a stored file name".to_string());
        }
        if let Some(preview_path) = self.preview_path.as_deref() {
            let mut components = Path::new(preview_path).components();
            if !matches!(components.next(), Some(Component::Normal(_)))
                || components.next().is_some()
            {
                return Err("Attachment preview path must be a stored file name".to_string());
            }
            if self.kind != "image" {
                return Err("Only image attachments can have previews".to_string());
            }
        }
        if self.width.is_some() != self.height.is_some() {
            return Err("Attachment dimensions must include width and height".to_string());
        }
        if self
            .width
            .zip(self.height)
            .is_some_and(|(width, height)| width == 0 || height == 0)
        {
            return Err("Attachment dimensions are invalid".to_string());
        }
        Ok(())
    }
}

/** 用户明确从 Work 文献加入某条消息的 PDF 选区或单页引用。 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiteratureReferenceKind {
    Selection,
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredLiteratureReference {
    pub id: String,
    pub library_item_id: String,
    pub title: String,
    pub page_index: u32,
    pub kind: LiteratureReferenceKind,
    pub text: String,
}

impl StoredLiteratureReference {
    fn validate(&self) -> Result<(), String> {
        validate_stable_id("Literature reference ID", &self.id)?;
        validate_stable_id("Literature library item ID", &self.library_item_id)?;
        if self.title.trim().is_empty() || self.title.chars().count() > MAX_TITLE_CHARS {
            return Err("Literature reference title is empty or too long".to_string());
        }
        if self.title.contains('\r') || self.title.contains('\n') {
            return Err("Literature reference title must be a single line".to_string());
        }
        if self.page_index > MAX_LITERATURE_PAGE_INDEX {
            return Err("Literature reference page index is invalid".to_string());
        }
        if self.text.trim().is_empty() || self.text.len() > MAX_LITERATURE_REFERENCE_TEXT_BYTES {
            return Err("Literature reference text is empty or too long".to_string());
        }
        Ok(())
    }
}

/** 用户明确从 Markdown 笔记加入某条消息的选区引用。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredNoteReference {
    pub id: String,
    pub note_id: String,
    pub note_title: String,
    pub revision_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    pub selected_text: String,
}

impl StoredNoteReference {
    fn validate(&self) -> Result<(), String> {
        validate_stable_id("Note reference ID", &self.id)?;
        validate_stable_id("Note ID", &self.note_id)?;
        if self.note_title.trim().is_empty() || self.note_title.chars().count() > MAX_TITLE_CHARS {
            return Err("Note reference title is empty or too long".to_string());
        }
        if self.note_title.contains('\r') || self.note_title.contains('\n') {
            return Err("Note reference title must be a single line".to_string());
        }
        if self.revision_hash.trim().is_empty() || self.revision_hash.len() > 160 {
            return Err("Note reference revision is invalid".to_string());
        }
        if self.selected_text.trim().is_empty()
            || self.selected_text.len() > MAX_NOTE_REFERENCE_TEXT_BYTES
        {
            return Err("Note reference text is empty or too long".to_string());
        }
        if self
            .start_line
            .zip(self.end_line)
            .is_some_and(|(start, end)| start == 0 || end < start)
        {
            return Err("Note reference line range is invalid".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChatMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: ModelRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<StoredChatAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub literature_references: Vec<StoredLiteratureReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub note_references: Vec<StoredNoteReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub status: MessageStatus,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_snapshot: Option<ModelSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activated_skills: Vec<ActivatedSkillSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_traces: Vec<crate::chat::agent::ToolTraceSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_summary: Option<AgentWorkflowSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub thinking_enabled: Option<bool>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub context_summary: String,
    #[serde(default)]
    pub compressed_until_message_id: Option<String>,
    #[serde(default)]
    pub context_compression_count: u32,
    #[serde(default)]
    pub enabled_skill_ids: Vec<String>,
    #[serde(default)]
    pub linked_library_item_ids: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListPage {
    pub items: Vec<ConversationListItem>,
    pub offset: usize,
    pub total: usize,
    pub has_more: bool,
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
            return Err(format!(
                "Conversation cannot exceed {MAX_MESSAGES} messages"
            ));
        }
        if self.system_prompt.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err("Conversation System Prompt is too long".to_string());
        }
        if self.context_summary.len() > MAX_CONTEXT_SUMMARY_BYTES {
            return Err("Conversation context summary is too long".to_string());
        }
        validate_optional_id(
            "Compressed until message ID",
            self.compressed_until_message_id.as_deref(),
        )?;
        if self.compressed_until_message_id.is_some() && self.context_summary.trim().is_empty() {
            return Err("Compressed conversation requires a context summary".to_string());
        }
        validate_optional_id("Assistant ID", self.assistant_id.as_deref())?;
        validate_optional_id("Provider ID", self.provider_id.as_deref())?;
        validate_optional_id("Model ID", self.model_id.as_deref())?;
        if self.provider_id.is_some() && self.model_id.is_none() {
            return Err("Conversation provider ID requires a model ID".to_string());
        }
        validate_optional_id("Project ID", self.project_id.as_deref())?;
        validate_optional_id("Collection ID", self.collection_id.as_deref())?;
        if self.enabled_skill_ids.len() > 64 {
            return Err("Conversation cannot enable more than 64 skills".to_string());
        }
        let mut enabled_skill_ids = std::collections::HashSet::new();
        for skill_id in &self.enabled_skill_ids {
            crate::skills::validate_skill_id(skill_id)?;
            if !enabled_skill_ids.insert(skill_id) {
                return Err("Conversation contains duplicate skill IDs".to_string());
            }
        }
        if self.linked_library_item_ids.len() > MAX_LINKED_LIBRARY_ITEMS {
            return Err(format!(
                "Conversation cannot link more than {MAX_LINKED_LIBRARY_ITEMS} library items"
            ));
        }
        let mut linked_library_item_ids = std::collections::HashSet::new();
        for item_id in &self.linked_library_item_ids {
            validate_stable_id("Linked library item ID", item_id)?;
            if !linked_library_item_ids.insert(item_id) {
                return Err("Conversation contains duplicate linked library item IDs".to_string());
            }
        }

        for message in &self.messages {
            validate_stable_id("Message ID", &message.id)?;
            if message.conversation_id != self.id {
                return Err("Message conversation ID does not match its conversation".to_string());
            }
            if message.content.len() > MAX_MESSAGE_BYTES {
                return Err("Chat message is too long".to_string());
            }
            if message.attachments.len() > MAX_ATTACHMENTS_PER_MESSAGE {
                return Err(format!(
                    "Chat message cannot exceed {MAX_ATTACHMENTS_PER_MESSAGE} attachments"
                ));
            }
            for attachment in &message.attachments {
                attachment.validate()?;
            }
            if message.literature_references.len() > MAX_LITERATURE_REFERENCES_PER_MESSAGE {
                return Err(format!(
                    "Chat message cannot exceed {MAX_LITERATURE_REFERENCES_PER_MESSAGE} literature references"
                ));
            }
            if !message.literature_references.is_empty() && message.role != ModelRole::User {
                return Err("Only user messages can contain literature references".to_string());
            }
            let mut reference_ids = std::collections::HashSet::new();
            let mut reference_text_bytes = 0usize;
            for reference in &message.literature_references {
                reference.validate()?;
                if !reference_ids.insert(&reference.id) {
                    return Err(
                        "Chat message contains duplicate literature reference IDs".to_string()
                    );
                }
                reference_text_bytes = reference_text_bytes.saturating_add(reference.text.len());
            }
            if reference_text_bytes > MAX_LITERATURE_REFERENCE_TOTAL_BYTES {
                return Err("Chat message literature references exceed the text budget".to_string());
            }
            if message.note_references.len() > MAX_NOTE_REFERENCES_PER_MESSAGE {
                return Err(format!(
                    "Chat message cannot exceed {MAX_NOTE_REFERENCES_PER_MESSAGE} note references"
                ));
            }
            if !message.note_references.is_empty() && message.role != ModelRole::User {
                return Err("Only user messages can contain note references".to_string());
            }
            let mut note_reference_ids = std::collections::HashSet::new();
            let mut note_reference_text_bytes = 0usize;
            for reference in &message.note_references {
                reference.validate()?;
                if !note_reference_ids.insert(&reference.id) {
                    return Err("Chat message contains duplicate note reference IDs".to_string());
                }
                note_reference_text_bytes =
                    note_reference_text_bytes.saturating_add(reference.selected_text.len());
            }
            if note_reference_text_bytes > MAX_NOTE_REFERENCE_TOTAL_BYTES {
                return Err("Chat message note references exceed the text budget".to_string());
            }
            if message
                .reasoning
                .as_ref()
                .is_some_and(|value| value.len() > MAX_MESSAGE_BYTES)
            {
                return Err("Chat message reasoning is too long".to_string());
            }
            if message
                .error_message
                .as_ref()
                .is_some_and(|value| value.len() > 64 * 1024)
            {
                return Err("Chat error message is too long".to_string());
            }
            validate_optional_id("Message model ID", message.model_id.as_deref())?;
            if let Some(snapshot) = &message.model_snapshot {
                validate_stable_id("Snapshot model ID", &snapshot.id)?;
                validate_stable_id("Snapshot provider ID", &snapshot.provider_id)?;
            }
            if message.activated_skills.len() > 12 {
                return Err("Chat message cannot activate more than 12 skills".to_string());
            }
            for skill in &message.activated_skills {
                crate::skills::validate_skill_id(&skill.id)?;
                if skill.name.trim().is_empty()
                    || skill.version.trim().is_empty()
                    || !skill.content_hash.starts_with("sha256:")
                    || !matches!(skill.activation.as_str(), "manual" | "slash" | "model")
                {
                    return Err("Chat message contains an invalid skill snapshot".to_string());
                }
            }
            if message.tool_traces.len() > 128 {
                return Err("Chat message cannot contain more than 128 tool traces".to_string());
            }
            if let Some(run_id) = message.agent_run_id.as_deref() {
                validate_stable_id("Agent Run ID", run_id)?;
            }
            if let Some(summary) = message.workflow_summary.as_ref() {
                if summary.step_count > 10_000
                    || summary.tool_call_count > 10_000
                    || summary.skill_count > 10_000
                {
                    return Err("Chat message workflow summary exceeds its bounds".to_string());
                }
            }
            if message.tool_traces.iter().any(|trace| {
                trace.argument_summary.chars().count() > 500
                    || trace
                        .preview
                        .as_deref()
                        .is_some_and(|value| value.chars().count() > 2_000)
            }) {
                return Err("Chat message tool trace is too long".to_string());
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
                if !text.trim().is_empty() {
                    Some(text.chars().take(160).collect::<String>())
                } else if !message.attachments.is_empty() {
                    Some(format!(
                        "附件：{}",
                        message
                            .attachments
                            .iter()
                            .map(|attachment| attachment.name.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ))
                } else if let Some(reference) = message.literature_references.first() {
                    Some(format!(
                        "文献：{}，第 {} 页",
                        reference.title,
                        reference.page_index + 1
                    ))
                } else if let Some(reference) = message.note_references.first() {
                    Some(format!("笔记：{}", reference.note_title))
                } else {
                    None
                }
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn conversation_with_message(role: ModelRole) -> StoredConversation {
        StoredConversation {
            id: "conversation-1".to_string(),
            title: "Literature chat".to_string(),
            messages: vec![StoredChatMessage {
                id: "message-1".to_string(),
                conversation_id: "conversation-1".to_string(),
                role,
                content: "请解释这段内容".to_string(),
                attachments: Vec::new(),
                literature_references: Vec::new(),
                note_references: Vec::new(),
                reasoning: None,
                status: MessageStatus::Completed,
                created_at: 1,
                updated_at: 1,
                model_id: None,
                model_snapshot: None,
                usage: None,
                activated_skills: Vec::new(),
                tool_traces: Vec::new(),
                agent_run_id: None,
                workflow_summary: None,
                error_message: None,
            }],
            assistant_id: None,
            provider_id: None,
            model_id: None,
            thinking_enabled: None,
            reasoning_effort: None,
            system_prompt: String::new(),
            context_summary: String::new(),
            compressed_until_message_id: None,
            context_compression_count: 0,
            enabled_skill_ids: Vec::new(),
            linked_library_item_ids: Vec::new(),
            permission_mode: AiPermissionMode::AskSensitive,
            project_id: None,
            collection_id: None,
            pinned: false,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn reference() -> StoredLiteratureReference {
        StoredLiteratureReference {
            id: "reference-1".to_string(),
            library_item_id: "item-1".to_string(),
            title: "Paper".to_string(),
            page_index: 2,
            kind: LiteratureReferenceKind::Selection,
            text: "Evidence".to_string(),
        }
    }

    #[test]
    fn old_conversation_json_defaults_literature_fields() {
        let value = json!({
            "id": "conversation-1",
            "title": "Legacy",
            "messages": [{
                "id": "message-1",
                "conversationId": "conversation-1",
                "role": "user",
                "content": "hello",
                "status": "completed",
                "createdAt": 1,
                "updatedAt": 1
            }],
            "assistantId": null,
            "providerId": null,
            "modelId": null,
            "systemPrompt": "",
            "contextSummary": "",
            "compressedUntilMessageId": null,
            "contextCompressionCount": 0,
            "enabledSkillIds": [],
            "permissionMode": "askSensitive",
            "projectId": null,
            "collectionId": null,
            "pinned": false,
            "createdAt": 1,
            "updatedAt": 1
        });
        let conversation: StoredConversation = serde_json::from_value(value).unwrap();
        assert!(conversation.linked_library_item_ids.is_empty());
        assert!(conversation.messages[0].literature_references.is_empty());
        conversation.validate().unwrap();
    }

    #[test]
    fn validates_bounded_user_literature_reference() {
        let mut conversation = conversation_with_message(ModelRole::User);
        conversation
            .linked_library_item_ids
            .push("item-1".to_string());
        conversation.messages[0]
            .literature_references
            .push(reference());
        conversation.validate().unwrap();
    }

    #[test]
    fn rejects_literature_references_on_assistant_messages() {
        let mut conversation = conversation_with_message(ModelRole::Assistant);
        conversation.messages[0]
            .literature_references
            .push(reference());
        assert!(conversation.validate().is_err());
    }
}
