//! 模型请求调试记录。
//!
//! 调试功能默认关闭，只在内存中保留最近 30 条记录。请求头中的凭据始终脱敏，
//! 请求体和响应预览都有硬上限，避免调试功能长期占用内存或意外保存敏感密钥。

use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::{
    ai::{
        error::ModelError,
        providers,
        types::{ModelRequest, ModelUsage, ProviderRequestContext},
    },
    state::AppState,
};

const MAX_RECORDS: usize = 30;
const MAX_REQUEST_BODY_BYTES: usize = 128 * 1024;
const MAX_RESPONSE_PREVIEW_CHARS: usize = 4_000;
static RECORD_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDebugRequest {
    pub method: String,
    pub url: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub body: Value,
    pub body_truncated: bool,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDebugResponse {
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub body: Option<Value>,
    pub body_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDebugRecord {
    pub id: String,
    pub created_at_ms: u64,
    pub duration_ms: u64,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub api_model: String,
    pub display_name: String,
    pub protocol: String,
    pub status: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    pub request: RequestDebugRequest,
    pub response: RequestDebugResponse,
    #[serde(default)]
    pub usage: Option<ModelUsage>,
}

#[derive(Debug)]
pub struct RequestDebugRecordInput {
    pub created_at_ms: u64,
    pub duration_ms: u64,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub api_model: String,
    pub display_name: String,
    pub protocol: String,
    pub status: String,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub request: RequestDebugRequest,
    pub response: RequestDebugResponse,
    pub usage: Option<ModelUsage>,
}

pub fn is_enabled(state: &AppState) -> bool {
    state
        .app_settings
        .read()
        .map(|settings| settings.request_debug_enabled)
        .unwrap_or(false)
}

pub fn build_request(
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
    stream: bool,
) -> Result<RequestDebugRequest, String> {
    // 记忆正文只用于本次模型请求，调试记录中只保留脱敏占位符。
    let mut debug_request = request.clone();
    if let Some(system_prompt) = debug_request.system_prompt.as_mut() {
        *system_prompt = redact_memory_sections(system_prompt);
    }
    let raw = providers::build_debug_request(context, &debug_request, stream)?;
    let serialized_size = serde_json::to_vec(&raw.body)
        .map(|body| body.len())
        .unwrap_or(MAX_REQUEST_BODY_BYTES.saturating_add(1));
    let (body, body_truncated) = if serialized_size > MAX_REQUEST_BODY_BYTES {
        (
            json!({
                "_mnemora": "请求体超过调试记录上限，未保存完整内容。",
                "sizeBytes": serialized_size,
                "limitBytes": MAX_REQUEST_BODY_BYTES,
            }),
            true,
        )
    } else {
        (raw.body, false)
    };

    Ok(RequestDebugRequest {
        method: "POST".to_string(),
        url: raw.url,
        headers: raw.headers,
        body,
        body_truncated,
        stream,
    })
}

fn redact_memory_sections(value: &str) -> String {
    const START: &str = "<mnemora_memory_l1>";
    const END: &str = "</mnemora_memory_l1>";
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find(START) {
        output.push_str(&remaining[..start]);
        let after_start = &remaining[start + START.len()..];
        let Some(end) = after_start.find(END) else {
            output.push_str("<mnemora_memory_l1>[已隐藏]</mnemora_memory_l1>");
            return output;
        };
        output.push_str("<mnemora_memory_l1>[已隐藏]</mnemora_memory_l1>");
        remaining = &after_start[end + END.len()..];
    }
    output.push_str(remaining);
    output
}

pub fn success_response(
    status_code: Option<u16>,
    text: &str,
    reasoning: Option<&str>,
    finish_reason: Option<&str>,
    usage: Option<&ModelUsage>,
) -> RequestDebugResponse {
    let (text, body_truncated) = truncate_chars(text, MAX_RESPONSE_PREVIEW_CHARS);
    let (reasoning, reasoning_truncated) = reasoning
        .map(|value| truncate_chars(value, MAX_RESPONSE_PREVIEW_CHARS))
        .unwrap_or_default();
    RequestDebugResponse {
        status_code,
        body: Some(json!({
            "text": text,
            "reasoning": (!reasoning.is_empty()).then_some(reasoning),
            "finishReason": finish_reason,
            "usage": usage,
        })),
        body_truncated: body_truncated || reasoning_truncated,
    }
}

pub fn stopped_response(text: &str) -> RequestDebugResponse {
    let (text, body_truncated) = truncate_chars(text, MAX_RESPONSE_PREVIEW_CHARS);
    RequestDebugResponse {
        status_code: None,
        body: Some(json!({ "stopped": true, "partialText": text })),
        body_truncated,
    }
}

pub fn error_response(error: &ModelError, partial_text: &str) -> RequestDebugResponse {
    let (partial_text, body_truncated) = truncate_chars(partial_text, MAX_RESPONSE_PREVIEW_CHARS);
    RequestDebugResponse {
        status_code: error.status_code,
        body: Some(json!({
            "error": error,
            "partialText": partial_text,
        })),
        body_truncated,
    }
}

pub fn append_preview(preview: &mut String, delta: &str) {
    if preview.chars().count() >= MAX_RESPONSE_PREVIEW_CHARS {
        return;
    }
    let remaining = MAX_RESPONSE_PREVIEW_CHARS.saturating_sub(preview.chars().count());
    preview.extend(delta.chars().take(remaining));
}

pub fn record(state: &AppState, input: RequestDebugRecordInput) {
    let record = RequestDebugRecord {
        id: format!(
            "debug_{}_{}",
            input.created_at_ms,
            RECORD_COUNTER.fetch_add(1, Ordering::Relaxed)
        ),
        created_at_ms: input.created_at_ms,
        duration_ms: input.duration_ms,
        provider_id: input.provider_id,
        provider_name: input.provider_name,
        model_id: input.model_id,
        api_model: input.api_model,
        display_name: input.display_name,
        protocol: input.protocol,
        status: input.status,
        conversation_id: input.conversation_id,
        message_id: input.message_id,
        request: input.request,
        response: input.response,
        usage: input.usage,
    };
    let Ok(mut records) = state.request_debug_records.lock() else {
        return;
    };
    records.push_front(record);
    records.truncate(MAX_RECORDS);
}

#[tauri::command]
pub fn request_debug_get_records(
    state: State<'_, AppState>,
) -> Result<Vec<RequestDebugRecord>, String> {
    state
        .request_debug_records
        .lock()
        .map(|records| records.iter().cloned().collect())
        .map_err(|_| "请求调试记录暂时不可用。".to_string())
}

#[tauri::command]
pub fn request_debug_clear(state: State<'_, AppState>) -> Result<(), String> {
    state
        .request_debug_records
        .lock()
        .map(|mut records| records.clear())
        .map_err(|_| "请求调试记录暂时不可用。".to_string())
}

pub fn empty_store() -> VecDeque<RequestDebugRecord> {
    VecDeque::with_capacity(MAX_RECORDS)
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value.to_string(), false);
    }
    (value.chars().take(max_chars).collect(), true)
}

#[cfg(test)]
mod tests {
    use super::{
        append_preview, redact_memory_sections, success_response, truncate_chars,
        MAX_RESPONSE_PREVIEW_CHARS,
    };

    #[test]
    fn response_preview_has_a_hard_character_limit() {
        let (value, truncated) = truncate_chars(&"中".repeat(4_001), 4_000);
        assert_eq!(value.chars().count(), 4_000);
        assert!(truncated);
    }

    #[test]
    fn streaming_preview_stops_at_the_same_limit() {
        let mut preview = "a".repeat(MAX_RESPONSE_PREVIEW_CHARS - 1);
        append_preview(&mut preview, "xyz");
        assert_eq!(preview.chars().count(), MAX_RESPONSE_PREVIEW_CHARS);
        assert!(preview.ends_with('x'));
    }

    #[test]
    fn successful_debug_response_keeps_provider_visible_reasoning() {
        let response = success_response(
            Some(200),
            "Answer",
            Some("Visible reasoning summary"),
            Some("stop"),
            None,
        );

        assert_eq!(
            response
                .body
                .as_ref()
                .and_then(|body| body.get("reasoning"))
                .and_then(serde_json::Value::as_str),
            Some("Visible reasoning summary")
        );
        assert!(!response.body_truncated);
    }

    #[test]
    fn debug_request_redacts_l1_memory_sections() {
        let value = redact_memory_sections(
            "before\n<mnemora_memory_l1>private preference</mnemora_memory_l1>\nafter",
        );
        assert!(!value.contains("private preference"));
        assert!(value.contains("[已隐藏]"));
    }
}
