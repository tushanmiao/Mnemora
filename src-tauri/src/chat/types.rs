//! Chat 命令输入与校验。
//!
//! 前端提交的是 Mnemora 内部 ID 和供应商无关历史消息。API Model、协议、Base URL 与
//! API Key 均由 Rust 根据已保存设置补齐，避免前端把展示名称或旧配置当成真实请求参数。

use serde::{Deserialize, Serialize};

use super::conversation_types::AiPermissionMode;
use super::storage::ConversationRepository;
use crate::{
    ai::{
        error::ModelError,
        types::{ModelMessage, ModelOptions, ModelRequest, ModelResponse, ModelRole, ModelUsage},
    },
    chat::{attachments::load_model_image, conversation_types::StoredChatAttachment},
    settings::types::validate_stable_id,
    skills::SkillRepository,
};

const MAX_MESSAGES: usize = 500;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_SYSTEM_PROMPT_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 131_072;
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 8;
const MAX_MODEL_IMAGES: usize = 4;
const MAX_MODEL_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ACTIVATED_SKILLS: usize = 12;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatWorkspaceMode {
    #[default]
    Chat,
    Work,
    Notes,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ChatWorkspaceContext {
    #[serde(rename = "note")]
    Note {
        note_id: String,
        note_title: String,
        note_revision_hash: String,
        #[serde(default)]
        note_snapshot: Option<String>,
        #[serde(default)]
        source_pdf_id: Option<String>,
        #[serde(default)]
        source_pdf_title: Option<String>,
        #[serde(default)]
        source_page_index: Option<u32>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModelMessage {
    pub role: ModelRole,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<StoredChatAttachment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionRequest {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub activated_skill_ids: Vec<String>,
    #[serde(default)]
    pub slash_skill_id: Option<String>,
    #[serde(default)]
    pub permission_mode: AiPermissionMode,
    #[serde(default)]
    pub workspace_mode: ChatWorkspaceMode,
    #[serde(default)]
    pub workspace_context: Option<ChatWorkspaceContext>,
    pub messages: Vec<ChatModelMessage>,
    #[serde(default)]
    pub options: ModelOptions,
}

/** 一次流式运行的稳定身份和普通 Chat 请求。 */
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStreamRequest {
    pub run_id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub completion: ChatCompletionRequest,
}

/** 非流式 Chat 在模型结果之外返回有界 Agent 元数据。 */
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionResponse {
    #[serde(flatten)]
    pub response: ModelResponse,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activated_skill_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_traces: Vec<crate::chat::agent::ToolTraceSnapshot>,
}

impl ChatStreamRequest {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("Run ID", self.run_id.trim())
            .map_err(ModelError::invalid_configuration)?;
        validate_stable_id("Conversation ID", self.conversation_id.trim())
            .map_err(ModelError::invalid_configuration)?;
        validate_stable_id("Message ID", self.message_id.trim())
            .map_err(ModelError::invalid_configuration)?;
        self.completion.validate()
    }
}

/** Rust 通过 Tauri Channel 发送给当前助手消息的统一流事件。 */
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[allow(clippy::large_enum_variant)]
pub enum ModelStreamEvent {
    Started {
        run_id: String,
        conversation_id: String,
        message_id: String,
    },
    TextDelta {
        run_id: String,
        conversation_id: String,
        message_id: String,
        delta: String,
    },
    ReasoningDelta {
        run_id: String,
        conversation_id: String,
        message_id: String,
        delta: String,
    },
    ToolTrace {
        run_id: String,
        conversation_id: String,
        message_id: String,
        trace: crate::chat::agent::ToolTraceSnapshot,
    },
    ToolApprovalRequested {
        run_id: String,
        conversation_id: String,
        message_id: String,
        approval_id: String,
        trace: crate::chat::agent::ToolTraceSnapshot,
    },
    SkillActivated {
        run_id: String,
        conversation_id: String,
        message_id: String,
        skill_id: String,
        name: String,
        version: String,
        content_hash: String,
    },
    Completed {
        run_id: String,
        conversation_id: String,
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ModelUsage>,
    },
    Stopped {
        run_id: String,
        conversation_id: String,
        message_id: String,
    },
    Error {
        run_id: String,
        conversation_id: String,
        message_id: String,
        error: ModelError,
    },
}

impl ChatCompletionRequest {
    /// 上下文压缩、笔记总结这类内部辅助调用：不进入正常聊天流程，
    /// 统一禁用技能激活、附件和 Agent 工具。
    pub fn is_auxiliary_operation(&self) -> bool {
        matches!(
            self.operation.as_deref(),
            Some("contextCompression") | Some("noteSummary") | Some("deepNote") | Some("noteEdit")
        )
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("Provider ID", self.provider_id.trim())
            .map_err(ModelError::invalid_configuration)?;
        validate_stable_id("Model ID", self.model_id.trim())
            .map_err(ModelError::invalid_configuration)?;
        if self.operation.as_deref().is_some_and(|operation| {
            !matches!(
                operation,
                "chatComplete" | "contextCompression" | "noteSummary" | "deepNote" | "noteEdit"
            )
        }) {
            return Err(ModelError::invalid_configuration(
                "Chat operation is not supported.",
            ));
        }

        if self.messages.is_empty() {
            return Err(ModelError::invalid_configuration("至少需要一条聊天消息。"));
        }
        if self.messages.len() > MAX_MESSAGES {
            return Err(ModelError::invalid_configuration(format!(
                "聊天历史不能超过 {MAX_MESSAGES} 条消息。"
            )));
        }
        if let Some(ChatWorkspaceContext::Note {
            note_id,
            note_title,
            note_revision_hash,
            note_snapshot,
            source_pdf_id,
            source_pdf_title,
            ..
        }) = &self.workspace_context
        {
            validate_stable_id("Workspace note ID", note_id)
                .map_err(ModelError::invalid_configuration)?;
            if note_title.trim().is_empty() || note_title.chars().count() > 500 {
                return Err(ModelError::invalid_configuration("当前笔记标题无效。"));
            }
            if note_revision_hash.trim().is_empty() || note_revision_hash.len() > 160 {
                return Err(ModelError::invalid_configuration("当前笔记版本无效。"));
            }
            if note_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.len() > 32 * 1024 + 128)
            {
                return Err(ModelError::invalid_configuration(
                    "当前笔记快照超过 32 KB。",
                ));
            }
            if let Some(source_pdf_id) = source_pdf_id {
                validate_stable_id("Workspace source PDF ID", source_pdf_id)
                    .map_err(ModelError::invalid_configuration)?;
            }
            if source_pdf_title
                .as_ref()
                .is_some_and(|title| title.chars().count() > 500)
            {
                return Err(ModelError::invalid_configuration("来源 PDF 标题过长。"));
            }
        }
        if self.system_prompt.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err(ModelError::invalid_configuration("System Prompt 过长。"));
        }
        if self.activated_skill_ids.len() > MAX_ACTIVATED_SKILLS {
            return Err(ModelError::invalid_configuration(format!(
                "每轮最多激活 {MAX_ACTIVATED_SKILLS} 个技能。"
            )));
        }
        if let Some(slash_skill_id) = self.slash_skill_id.as_deref() {
            crate::skills::validate_skill_id(slash_skill_id)
                .map_err(ModelError::invalid_configuration)?;
            if !self
                .activated_skill_ids
                .iter()
                .any(|skill_id| skill_id == slash_skill_id)
            {
                return Err(ModelError::invalid_configuration(
                    "Slash Skill 必须同时出现在本轮激活技能列表中。",
                ));
            }
        }
        let mut skill_ids = std::collections::HashSet::new();
        for skill_id in &self.activated_skill_ids {
            crate::skills::validate_skill_id(skill_id)
                .map_err(ModelError::invalid_configuration)?;
            if !skill_ids.insert(skill_id) {
                return Err(ModelError::invalid_configuration(
                    "本轮技能列表包含重复 ID。",
                ));
            }
        }
        if self.is_auxiliary_operation()
            && (!self.activated_skill_ids.is_empty() || self.slash_skill_id.is_some())
        {
            return Err(ModelError::invalid_configuration(
                "内部辅助请求不能激活技能。",
            ));
        }

        let mut total_bytes = self.system_prompt.len();
        for message in &self.messages {
            if message.content.trim().is_empty() && message.attachments.is_empty() {
                return Err(ModelError::invalid_configuration("聊天消息内容不能为空。"));
            }
            if message.content.len() > MAX_MESSAGE_BYTES {
                return Err(ModelError::invalid_configuration("单条聊天消息过长。"));
            }
            total_bytes = total_bytes.saturating_add(message.content.len());
            if total_bytes > MAX_CONTEXT_BYTES {
                return Err(ModelError::invalid_configuration("本次聊天上下文过长。"));
            }
            if message.attachments.len() > MAX_ATTACHMENTS_PER_MESSAGE {
                return Err(ModelError::invalid_configuration(format!(
                    "单条消息不能超过 {MAX_ATTACHMENTS_PER_MESSAGE} 个附件。"
                )));
            }
            if message.role != ModelRole::User && !message.attachments.is_empty() {
                return Err(ModelError::invalid_configuration(
                    "只有用户消息可以包含附件。",
                ));
            }
            if self.is_auxiliary_operation() && !message.attachments.is_empty() {
                let deep_note_vision = self.operation.as_deref() == Some("deepNote")
                    && message
                        .attachments
                        .iter()
                        .all(|attachment| attachment.kind == "image");
                if !deep_note_vision {
                    return Err(ModelError::invalid_configuration(
                        "内部辅助请求只能在深度笔记视觉来源节点中携带图片附件。",
                    ));
                }
            }
            for attachment in &message.attachments {
                attachment
                    .validate()
                    .map_err(ModelError::invalid_configuration)?;
            }
        }

        if self.options.temperature.is_some_and(|temperature| {
            !temperature.is_finite() || !(0.0..=1.0).contains(&temperature)
        }) {
            return Err(ModelError::invalid_configuration(
                "Temperature 必须在 0 到 1 之间。",
            ));
        }
        if self
            .options
            .reasoning_effort
            .as_deref()
            .is_some_and(|effort| !matches!(effort, "low" | "medium" | "high" | "xhigh" | "max"))
        {
            return Err(ModelError::invalid_configuration(
                "Reasoning effort must be low, medium, high, xhigh, or max.",
            ));
        }
        if self
            .options
            .max_output_tokens
            .is_some_and(|tokens| tokens == 0 || tokens > MAX_OUTPUT_TOKENS)
        {
            return Err(ModelError::invalid_configuration(format!(
                "最大输出 Token 必须在 1 到 {MAX_OUTPUT_TOKENS} 之间。"
            )));
        }
        Ok(())
    }

    pub fn into_model_request(
        self,
        api_model: String,
        repository: &ConversationRepository,
        skill_repository: &SkillRepository,
    ) -> Result<ModelRequest, ModelError> {
        let conversation_id = self.conversation_id.as_deref();
        let mut image_bytes = 0u64;
        let mut image_count = 0usize;
        let last_user_index = self
            .messages
            .iter()
            .rposition(|message| message.role == ModelRole::User);
        // 未答尾部：最后一条助手消息之后的用户消息。前端只会把 completed 的消息发给
        // 后端，所以出现在这里的助手消息一定是成功回复；尾部消息（含图片）从未被模型
        // 成功消费过。图片正文只对未答尾部发送——已被成功回答的历史图片降级为文字说明，
        // 维持"不重复发送历史图片"的 token/内存设计；而中转站失败后重新提问时，
        // 上一条带图消息仍在尾部，图片不会丢失（修复"失败后再问就胡说"的场景）。
        let last_assistant_index = self
            .messages
            .iter()
            .rposition(|message| message.role == ModelRole::Assistant);
        let in_unanswered_tail = |message_index: usize| {
            last_assistant_index.is_none_or(|assistant_index| message_index > assistant_index)
        };
        let last_user_content = last_user_index.map(|index| self.messages[index].content.clone());
        if let Some(slash_skill_id) = self.slash_skill_id.as_deref() {
            let content = last_user_content.as_deref().ok_or_else(|| {
                ModelError::invalid_configuration("Slash Skill 请求缺少用户消息。")
            })?;
            let resolved = skill_repository
                .resolve_user_content(content, &[slash_skill_id.to_string()])
                .map_err(ModelError::invalid_configuration)?;
            if resolved == content {
                return Err(ModelError::invalid_configuration(
                    "Slash Skill 触发词与当前用户消息不匹配。",
                ));
            }
        }
        let content_skill_ids = self
            .slash_skill_id
            .as_ref()
            .map(|skill_id| vec![skill_id.clone()])
            .unwrap_or_else(|| self.activated_skill_ids.clone());
        let mut messages = Vec::with_capacity(self.messages.len());
        for (message_index, message) in self.messages.into_iter().enumerate() {
            let mut content = message.content;
            if Some(message_index) == last_user_index && message.role == ModelRole::User {
                content = skill_repository
                    .resolve_user_content(&content, &content_skill_ids)
                    .map_err(ModelError::invalid_configuration)?;
            }
            let mut images = Vec::new();
            let mut files = Vec::new();
            let mut historical_images = Vec::new();
            for attachment in message.attachments {
                if attachment.kind == "image" {
                    if !in_unanswered_tail(message_index) {
                        historical_images.push(attachment.name);
                        continue;
                    }
                    image_count += 1;
                    if image_count > MAX_MODEL_IMAGES {
                        return Err(ModelError::invalid_configuration(format!(
                            "本轮最多向模型发送 {MAX_MODEL_IMAGES} 张图片。"
                        )));
                    }
                    image_bytes = image_bytes.saturating_add(attachment.size_bytes);
                    if image_bytes > MAX_MODEL_IMAGE_BYTES {
                        return Err(ModelError::invalid_configuration(
                            "本轮发送给模型的图片总大小不能超过 16 MB。",
                        ));
                    }
                    let conversation_id = conversation_id.ok_or_else(|| {
                        ModelError::invalid_configuration("图片请求缺少会话 ID。")
                    })?;
                    images.push(
                        load_model_image(repository, conversation_id, &attachment)
                            .map_err(ModelError::invalid_configuration)?,
                    );
                } else {
                    files.push(attachment.name);
                }
            }
            if !files.is_empty() {
                let note = format!(
                    "[本地附件尚未解析正文：{}。请不要根据文件名推测文件内容。]",
                    files.join("、")
                );
                if !content.trim().is_empty() {
                    content.push_str("\n\n");
                }
                content.push_str(&note);
            }
            if !historical_images.is_empty() {
                let note = format!(
                    "[历史图片附件：{}。本轮没有重复发送图片正文；如需重新分析，请再次添加或引用图片。]",
                    historical_images.join("、")
                );
                if !content.trim().is_empty() {
                    content.push_str("\n\n");
                }
                content.push_str(&note);
            }
            messages.push(ModelMessage {
                role: message.role,
                content,
                images,
                tool_calls: Vec::new(),
                tool_result: None,
            });
        }
        let mut system_prompt = super::prompt::prepend_core_system_prompt(&self.system_prompt);
        if let Some(ChatWorkspaceContext::Note {
            note_id,
            note_title,
            note_revision_hash,
            note_snapshot,
            source_pdf_id,
            source_pdf_title,
            source_page_index,
        }) = &self.workspace_context
        {
            let source = source_pdf_id
                .as_ref()
                .map(|id| {
                    format!(
                        "\n来源 PDF ID：{}\n来源 PDF：{}{}",
                        id,
                        source_pdf_title.as_deref().unwrap_or("未命名 PDF"),
                        source_page_index
                            .map(|page| format!("\n打开笔记时所在页：第 {} 页", page + 1))
                            .unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            system_prompt.push_str(&format!(
                "\n\n<mnemora_active_note>\n当前右侧是‘当前笔记上下文 Chat’。\n笔记 ID：{note_id}\n笔记标题：{note_title}\n界面版本：{note_revision_hash}{source}\n需要正文时必须调用 note_read 读取该 ID；不要假装已经读取，也不要根据标题或来源 PDF 猜测内容。用户明确加入的选区引用优先于整篇读取。{}\n</mnemora_active_note>",
                note_snapshot.as_ref().map(|snapshot| format!(
                    "\n当前模型没有 Tool 能力，以下是界面显式提供的有界笔记快照；只可依据这份快照回答，截断部分不可猜测：\n<note_snapshot>\n{snapshot}\n</note_snapshot>"
                )).unwrap_or_else(|| "\n当前模型具有 Tool 能力，界面快照未注入；按需调用 note_read。".to_string())
            ));
        }
        let skill_prompt = skill_repository
            .render_activated_skills(&self.activated_skill_ids, last_user_content.as_deref())
            .map_err(ModelError::invalid_configuration)?;
        if !skill_prompt.is_empty() {
            if !system_prompt.is_empty() {
                system_prompt.push_str("\n\n");
            }
            system_prompt.push_str(&skill_prompt);
        }
        if system_prompt.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err(ModelError::invalid_configuration(
                "System Prompt 与技能正文合计过长，请减少技能或自定义指令。",
            ));
        }
        Ok(ModelRequest {
            model: api_model,
            system_prompt: (!system_prompt.is_empty()).then_some(system_prompt),
            messages,
            options: self.options,
            tools: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::chat::conversation_types::AiPermissionMode;
    use uuid::Uuid;

    use crate::ai::{
        error::ModelErrorKind,
        types::{ModelOptions, ModelRole},
    };

    use super::{ChatCompletionRequest, ChatWorkspaceContext, ChatWorkspaceMode, ModelStreamEvent};
    use crate::chat::{conversation_types::StoredChatAttachment, storage::ConversationRepository};

    fn image_attachment(id: &str, path: &str) -> StoredChatAttachment {
        StoredChatAttachment {
            id: id.to_string(),
            kind: "image".to_string(),
            name: format!("{id}.png"),
            mime_type: "image/png".to_string(),
            size_bytes: 80,
            path: path.to_string(),
            preview_path: None,
            width: Some(8),
            height: Some(6),
        }
    }

    fn request(content: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            provider_id: "provider-1".to_string(),
            model_id: "model-1".to_string(),
            conversation_id: None,
            message_id: None,
            operation: None,
            system_prompt: String::new(),
            activated_skill_ids: Vec::new(),
            slash_skill_id: None,
            permission_mode: AiPermissionMode::AskSensitive,
            workspace_mode: ChatWorkspaceMode::Chat,
            workspace_context: None,
            messages: vec![super::ChatModelMessage {
                role: ModelRole::User,
                content: content.to_string(),
                attachments: Vec::new(),
            }],
            options: ModelOptions::default(),
        }
    }

    #[test]
    fn accepts_minimal_text_request() {
        request("Hello").validate().unwrap();
    }

    #[test]
    fn active_note_context_injects_bounded_snapshot() {
        let root = std::env::temp_dir().join(format!("mnemora-active-note-{}", Uuid::new_v4()));
        let repository = ConversationRepository::new(root.join("conversations"));
        let skills = crate::skills::SkillRepository::new(root.join("builtin"), root.join("skills"));
        let mut value = request("解释当前笔记");
        value.workspace_mode = ChatWorkspaceMode::Work;
        value.workspace_context = Some(ChatWorkspaceContext::Note {
            note_id: "note-1".to_string(),
            note_title: "研究笔记".to_string(),
            note_revision_hash: "revision-1".to_string(),
            note_snapshot: Some("# 结论\n\n测试快照".to_string()),
            source_pdf_id: Some("paper-1".to_string()),
            source_pdf_title: Some("Paper".to_string()),
            source_page_index: Some(2),
        });

        let model = value
            .into_model_request("model".to_string(), &repository, &skills)
            .unwrap();
        let prompt = model.system_prompt.unwrap();
        assert!(prompt.contains("笔记 ID：note-1"));
        assert!(prompt.contains("<note_snapshot>"));
        assert!(prompt.contains("测试快照"));
        assert!(prompt.contains("第 3 页"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_empty_message() {
        let error = request("  ").validate().unwrap_err();
        assert_eq!(error.kind, ModelErrorKind::InvalidConfiguration);
    }

    #[test]
    fn accepts_attachment_only_user_message() {
        let mut request = request("");
        request.messages[0]
            .attachments
            .push(image_attachment("attachment-1", "attachment-1_capture.png"));
        request.validate().unwrap();
    }

    #[test]
    fn model_request_only_loads_images_from_last_user_message() {
        // 已被成功回答的历史图片降级为文字说明；只有未答尾部（此处即最后一条用户消息）
        // 携带图片正文。
        let root = std::env::temp_dir().join(format!("mnemora-chat-request-{}", Uuid::new_v4()));
        let repository = ConversationRepository::new(root.clone());
        let directory = repository.attachments_directory("conversation-1").unwrap();
        fs::create_dir_all(&directory).unwrap();
        let old_path = "11111111-1111-4111-8111-111111111111_old.png";
        let new_path = "22222222-2222-4222-8222-222222222222_new.png";
        let image = image::RgbImage::from_pixel(8, 6, image::Rgb([12, 34, 56]));
        image.save(directory.join(old_path)).unwrap();
        image.save(directory.join(new_path)).unwrap();

        let mut request = request("旧问题");
        request.conversation_id = Some("conversation-1".to_string());
        request.messages[0]
            .attachments
            .push(image_attachment("attachment-old", old_path));
        request.messages.push(super::ChatModelMessage {
            role: ModelRole::Assistant,
            content: "旧回答".to_string(),
            attachments: Vec::new(),
        });
        request.messages.push(super::ChatModelMessage {
            role: ModelRole::User,
            content: "新问题".to_string(),
            attachments: vec![image_attachment("attachment-new", new_path)],
        });

        let model_request = request
            .into_model_request(
                "vision-model".to_string(),
                &repository,
                &crate::skills::SkillRepository::new(root.join("builtin"), root.join("skills")),
            )
            .unwrap();
        assert!(model_request.messages[0].images.is_empty());
        assert!(model_request.messages[0].content.contains("历史图片附件"));
        assert_eq!(model_request.messages[2].images.len(), 1);
        assert_eq!(
            model_request.messages[2].images[0].name,
            "attachment-new.png"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_request_keeps_images_for_unanswered_tail_messages() {
        // 中转站失败场景：带图消息没有得到成功回复（失败的助手消息不会被前端发来），
        // 用户随后追问。此时带图消息仍在未答尾部，图片必须重新发送而不是降级为
        // 文字说明——否则模型从未见过图，只能胡猜。
        let root = std::env::temp_dir().join(format!("mnemora-chat-retry-{}", Uuid::new_v4()));
        let repository = ConversationRepository::new(root.clone());
        let directory = repository.attachments_directory("conversation-1").unwrap();
        fs::create_dir_all(&directory).unwrap();
        let image_path = "33333333-3333-4333-8333-333333333333_shot.png";
        let image = image::RgbImage::from_pixel(8, 6, image::Rgb([12, 34, 56]));
        image.save(directory.join(image_path)).unwrap();

        let mut request = request("看看这张图");
        request.conversation_id = Some("conversation-1".to_string());
        request.messages[0]
            .attachments
            .push(image_attachment("attachment-shot", image_path));
        // 没有助手消息（上一轮失败被过滤），用户直接追问。
        request.messages.push(super::ChatModelMessage {
            role: ModelRole::User,
            content: "怎么没回答？再看一次".to_string(),
            attachments: Vec::new(),
        });

        let model_request = request
            .into_model_request(
                "vision-model".to_string(),
                &repository,
                &crate::skills::SkillRepository::new(root.join("builtin"), root.join("skills")),
            )
            .unwrap();
        assert_eq!(model_request.messages[0].images.len(), 1);
        assert_eq!(
            model_request.messages[0].images[0].name,
            "attachment-shot.png"
        );
        assert!(!model_request.messages[0].content.contains("历史图片附件"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_request_injects_skill_body_and_removes_the_slash_trigger() {
        let root = std::env::temp_dir().join(format!("mnemora-chat-skill-{}", Uuid::new_v4()));
        let builtin = root.join("builtin").join("summarize");
        fs::create_dir_all(&builtin).unwrap();
        fs::write(
            builtin.join("SKILL.md"),
            "---\nid: summarize\nname: 内容总结\ndescription: 总结当前内容。\nversion: 1.0.0\ntriggers: [/summary]\n---\n先提取事实，再生成摘要。重点：$ARGUMENTS\n",
        )
        .unwrap();
        let conversation_repository = ConversationRepository::new(root.join("conversations"));
        let skill_repository =
            crate::skills::SkillRepository::new(root.join("builtin"), root.join("skills"));
        let mut request = request("/summary 重点保留结论");
        request.system_prompt = "全局规则".to_string();
        request.activated_skill_ids = vec!["summarize".to_string()];

        let model_request = request
            .into_model_request(
                "test-model".to_string(),
                &conversation_repository,
                &skill_repository,
            )
            .unwrap();

        assert_eq!(model_request.messages[0].content, "重点保留结论");
        let system_prompt = model_request.system_prompt.unwrap();
        assert!(!system_prompt.starts_with("<mnemora_core>"));
        assert!(system_prompt.contains("全局规则"));
        assert!(system_prompt.contains("<mnemora_skill id=\"summarize\""));
        assert!(system_prompt.contains("先提取事实，再生成摘要。"));
        assert!(system_prompt.contains("重点：重点保留结论"));
        assert!(!system_prompt.contains("$ARGUMENTS"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_compression_rejects_attachments() {
        let mut request = request("压缩");
        request.operation = Some("contextCompression".to_string());
        request.messages[0]
            .attachments
            .push(image_attachment("attachment-1", "attachment-1_capture.png"));
        assert_eq!(
            request.validate().unwrap_err().kind,
            ModelErrorKind::InvalidConfiguration
        );
    }

    #[test]
    fn note_summary_is_a_valid_auxiliary_operation() {
        let mut value = request("总结这段对话");
        value.operation = Some("noteSummary".to_string());
        value.validate().unwrap();
        assert!(value.is_auxiliary_operation());
    }

    #[test]
    fn note_summary_rejects_skills_and_attachments() {
        let mut with_skills = request("总结");
        with_skills.operation = Some("noteSummary".to_string());
        with_skills.activated_skill_ids = vec!["summarize".to_string()];
        assert_eq!(
            with_skills.validate().unwrap_err().kind,
            ModelErrorKind::InvalidConfiguration
        );

        let mut with_attachment = request("总结");
        with_attachment.operation = Some("noteSummary".to_string());
        with_attachment.messages[0]
            .attachments
            .push(image_attachment("attachment-1", "attachment-1_capture.png"));
        assert_eq!(
            with_attachment.validate().unwrap_err().kind,
            ModelErrorKind::InvalidConfiguration
        );
    }

    #[test]
    fn deep_note_is_a_valid_auxiliary_operation() {
        let mut value = request("讲解这段对话");
        value.operation = Some("deepNote".to_string());
        value.validate().unwrap();
        assert!(value.is_auxiliary_operation());
    }

    #[test]
    fn note_edit_is_a_valid_auxiliary_operation() {
        let mut value = request("增量更新这篇笔记");
        value.operation = Some("noteEdit".to_string());
        value.validate().unwrap();
        assert!(value.is_auxiliary_operation());
    }

    #[test]
    fn rejects_unknown_operation() {
        let mut value = request("hello");
        value.operation = Some("noteSummarize".to_string());
        assert_eq!(
            value.validate().unwrap_err().kind,
            ModelErrorKind::InvalidConfiguration
        );
    }

    #[test]
    fn serializes_stream_event_identity_as_camel_case() {
        let value = serde_json::to_value(ModelStreamEvent::Started {
            run_id: "run-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            message_id: "message-1".to_string(),
        })
        .unwrap();
        assert_eq!(value["type"], "started");
        assert_eq!(value["runId"], "run-1");
        assert_eq!(value["conversationId"], "conversation-1");
        assert_eq!(value["messageId"], "message-1");
    }
}
