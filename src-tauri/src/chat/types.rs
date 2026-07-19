//! Chat 命令输入与校验。
//!
//! 前端提交的是 Mnemora 内部 ID 和供应商无关历史消息。API Model、协议、Base URL 与
//! API Key 均由 Rust 根据已保存设置补齐，避免前端把展示名称或旧配置当成真实请求参数。

use serde::{Deserialize, Serialize};

use crate::{
    ai::{
        error::ModelError,
        types::{ModelMessage, ModelOptions, ModelRequest, ModelUsage},
    },
    settings::types::validate_stable_id,
};

const MAX_MESSAGES: usize = 500;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_SYSTEM_PROMPT_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 131_072;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionRequest {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub system_prompt: String,
    pub messages: Vec<ModelMessage>,
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
            if message.content.trim().is_empty() {
                return Err(ModelError::invalid_configuration("聊天消息内容不能为空。"));
            }
            if message.content.len() > MAX_MESSAGE_BYTES {
                return Err(ModelError::invalid_configuration("单条聊天消息过长。"));
            }
            total_bytes = total_bytes.saturating_add(message.content.len());
            if total_bytes > MAX_CONTEXT_BYTES {
                return Err(ModelError::invalid_configuration("本次聊天上下文过长。"));
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

    pub fn into_model_request(self, api_model: String) -> ModelRequest {
        ModelRequest {
            model: api_model,
            system_prompt: (!self.system_prompt.trim().is_empty())
                .then(|| self.system_prompt.trim().to_string()),
            messages: self.messages,
            options: self.options,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ai::{
        error::ModelErrorKind,
        types::{ModelMessage, ModelOptions, ModelRole},
    };

    use super::{ChatCompletionRequest, ModelStreamEvent};

    fn request(content: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            provider_id: "provider-1".to_string(),
            model_id: "model-1".to_string(),
            system_prompt: String::new(),
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: content.to_string(),
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
