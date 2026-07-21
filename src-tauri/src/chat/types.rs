//! Chat 命令输入与校验。
//!
//! 前端提交的是 Mnemora 内部 ID 和供应商无关历史消息。API Model、协议、Base URL 与
//! API Key 均由 Rust 根据已保存设置补齐，避免前端把展示名称或旧配置当成真实请求参数。

use serde::{Deserialize, Serialize};

use super::storage::ConversationRepository;
use crate::{
    ai::{
        error::ModelError,
        types::{ModelMessage, ModelOptions, ModelRequest, ModelRole, ModelUsage},
    },
    chat::{attachments::load_model_image, conversation_types::StoredChatAttachment},
    settings::types::validate_stable_id,
};

const MAX_MESSAGES: usize = 500;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_SYSTEM_PROMPT_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 131_072;
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 8;
const MAX_MODEL_IMAGES: usize = 4;
const MAX_MODEL_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

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
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_stable_id("Provider ID", self.provider_id.trim())
            .map_err(ModelError::invalid_configuration)?;
        validate_stable_id("Model ID", self.model_id.trim())
            .map_err(ModelError::invalid_configuration)?;
        if self
            .operation
            .as_deref()
            .is_some_and(|operation| !matches!(operation, "chatComplete" | "contextCompression"))
        {
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
        if self.system_prompt.len() > MAX_SYSTEM_PROMPT_BYTES {
            return Err(ModelError::invalid_configuration("System Prompt 过长。"));
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
            if self.operation.as_deref() == Some("contextCompression")
                && !message.attachments.is_empty()
            {
                return Err(ModelError::invalid_configuration(
                    "上下文压缩请求不能包含附件。",
                ));
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
    ) -> Result<ModelRequest, ModelError> {
        let conversation_id = self.conversation_id.as_deref();
        let mut image_bytes = 0u64;
        let mut image_count = 0usize;
        let last_user_index = self
            .messages
            .iter()
            .rposition(|message| message.role == ModelRole::User);
        let mut messages = Vec::with_capacity(self.messages.len());
        for (message_index, message) in self.messages.into_iter().enumerate() {
            let mut content = message.content;
            let mut images = Vec::new();
            let mut files = Vec::new();
            let mut historical_images = Vec::new();
            for attachment in message.attachments {
                if attachment.kind == "image" {
                    if Some(message_index) != last_user_index {
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
            });
        }
        Ok(ModelRequest {
            model: api_model,
            system_prompt: (!self.system_prompt.trim().is_empty())
                .then(|| self.system_prompt.trim().to_string()),
            messages,
            options: self.options,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use crate::ai::{
        error::ModelErrorKind,
        types::{ModelOptions, ModelRole},
    };

    use super::{ChatCompletionRequest, ModelStreamEvent};
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
            .into_model_request("vision-model".to_string(), &repository)
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
