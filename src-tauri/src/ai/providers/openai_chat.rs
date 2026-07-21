//! OpenAI Chat Completions 非流式适配器。
//!
//! `complete` 依次完成：统一消息转 `messages`、Bearer/自定义认证、单次 POST、
//! 错误转换、`choices[0].message` 与 `usage` 解析。这里不重试，也不处理 SSE。

use reqwest::Client;
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;

use crate::ai::{
    error::ModelError,
    http::{endpoint_url, read_json_response},
    stream::{send_sse_request, sse::SseReadOutcome},
    types::{
        ModelRequest, ModelResponse, ModelRole, ModelStreamChunk, ModelStreamOutcome,
        ModelStreamSummary, ModelUsage, ProviderRequestContext,
    },
};

use super::{apply_model_auth, DefaultAuth};

pub async fn complete(
    client: &Client,
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
) -> Result<ModelResponse, ModelError> {
    let url = endpoint_url(context.base_url, "chat/completions")
        .map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(client.post(url), context, DefaultAuth::Bearer)?;
    let response = request_builder
        .json(&request_body(request))
        .send()
        .await
        .map_err(ModelError::from_reqwest)?;

    if !response.status().is_success() {
        return Err(ModelError::from_response(response, context.api_key).await);
    }

    let value = read_json_response(response, "OpenAI Chat Completions")
        .await
        .map_err(ModelError::invalid_response)?;
    parse_response(&value, request.options.thinking_enabled)
}

pub async fn stream<F>(
    client: &Client,
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
    cancellation: &CancellationToken,
    on_chunk: &mut F,
) -> Result<ModelStreamOutcome, ModelError>
where
    F: FnMut(ModelStreamChunk) -> Result<(), ModelError>,
{
    let url = endpoint_url(context.base_url, "chat/completions")
        .map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(client.post(url), context, DefaultAuth::Bearer)?;
    let mut body = request_body(request);
    body["stream"] = Value::Bool(true);
    body["stream_options"] = json!({ "include_usage": true });

    let mut saw_text = false;
    let mut finish_reason = None;
    let mut usage = None;
    let outcome = send_sse_request(
        request_builder.json(&body),
        context.api_key,
        cancellation,
        |event| {
            if event.data.trim() == "[DONE]" {
                return Ok(());
            }
            let value: Value = serde_json::from_str(&event.data)
                .map_err(|_| ModelError::invalid_response("OpenAI Chat SSE 事件不是有效 JSON。"))?;
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                return Err(ModelError::provider(format!(
                    "OpenAI Chat 流式错误：{message}"
                )));
            }
            if let Some(choice) = value
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
            {
                if request.options.thinking_enabled {
                    let reasoning =
                        extract_reasoning_text(choice.get("delta").unwrap_or(&Value::Null));
                    if !reasoning.is_empty() {
                        on_chunk(ModelStreamChunk::ReasoningDelta(reasoning))?;
                    }
                }
                let delta = extract_delta_text(choice.get("delta").unwrap_or(&Value::Null));
                if !delta.is_empty() {
                    saw_text = true;
                    on_chunk(ModelStreamChunk::TextDelta(delta))?;
                }
                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    finish_reason = Some(reason.to_string());
                }
            }
            if let Some(raw_usage) = value.get("usage").filter(|usage| !usage.is_null()) {
                usage = Some(parse_usage(raw_usage));
            }
            Ok(())
        },
    )
    .await?;

    if outcome == SseReadOutcome::Cancelled {
        return Ok(ModelStreamOutcome::Cancelled);
    }
    if !saw_text {
        return Err(ModelError::invalid_response(
            "OpenAI Chat 流式响应没有可显示的文本内容。",
        ));
    }
    Ok(ModelStreamOutcome::Completed(ModelStreamSummary {
        finish_reason,
        usage,
    }))
}

pub(crate) fn request_body(request: &ModelRequest) -> Value {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system_prompt) = request.system_prompt.as_deref() {
        messages.push(json!({ "role": "system", "content": system_prompt }));
    }
    messages.extend(request.messages.iter().map(|message| {
        let role = match message.role {
            ModelRole::User => "user",
            ModelRole::Assistant => "assistant",
        };
        if message.images.is_empty() {
            json!({ "role": role, "content": message.content })
        } else {
            let mut content = Vec::new();
            if !message.content.trim().is_empty() {
                content.push(json!({ "type": "text", "text": message.content }));
            }
            content.extend(message.images.iter().map(|image| {
                json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", image.media_type, image.data_base64)
                    }
                })
            }));
            json!({ "role": role, "content": content })
        }
    }));

    let mut body = Map::from_iter([
        ("model".to_string(), Value::String(request.model.clone())),
        ("messages".to_string(), Value::Array(messages)),
        ("stream".to_string(), Value::Bool(false)),
    ]);
    if let Some(temperature) = request.options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(max_output_tokens) = request.options.max_output_tokens {
        body.insert(
            "max_completion_tokens".to_string(),
            json!(max_output_tokens),
        );
    }
    if request.options.thinking_enabled && supports_reasoning_effort(&request.model) {
        body.insert(
            "reasoning_effort".to_string(),
            Value::String("medium".to_string()),
        );
    }
    Value::Object(body)
}

fn parse_response(value: &Value, include_reasoning: bool) -> Result<ModelResponse, ModelError> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| ModelError::invalid_response("OpenAI Chat 响应缺少 choices[0]。"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| ModelError::invalid_response("OpenAI Chat 响应缺少 message。"))?;
    let text = extract_message_text(message);
    if text.is_empty() {
        return Err(ModelError::invalid_response(
            "OpenAI Chat 响应没有可显示的文本内容。",
        ));
    }

    Ok(ModelResponse {
        text,
        reasoning: include_reasoning
            .then(|| request_reasoning(message))
            .flatten(),
        finish_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage: value.get("usage").map(parse_usage),
    })
}

fn request_reasoning(message: &Value) -> Option<String> {
    message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn extract_reasoning_text(delta: &Value) -> String {
    delta
        .get("reasoning_content")
        .or_else(|| delta.get("reasoning"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn supports_reasoning_effort(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.starts_with("gpt-5")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
}

fn extract_message_text(message: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            parts.push(text.to_string());
        } else if let Some(items) = content.as_array() {
            parts.extend(
                items.iter().filter_map(|item| {
                    item.get("text").and_then(Value::as_str).map(str::to_string)
                }),
            );
        }
    }
    if let Some(refusal) = message.get("refusal").and_then(Value::as_str) {
        parts.push(refusal.to_string());
    }
    parts.join("")
}

fn extract_delta_text(delta: &Value) -> String {
    let Some(content) = delta.get("content") else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect()
}

fn parse_usage(value: &Value) -> ModelUsage {
    ModelUsage {
        input_tokens: value.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: value.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: value
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        cache_read_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        ..ModelUsage::default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::ai::types::{ModelImage, ModelMessage, ModelOptions, ModelRequest, ModelRole};

    #[test]
    fn maps_system_prompt_and_messages() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-test".to_string(),
            system_prompt: Some("Be concise".to_string()),
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Hello".to_string(),
                images: Vec::new(),
            }],
            options: ModelOptions::default(),
        });

        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn maps_image_as_chat_content_part() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-vision".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Describe it".to_string(),
                images: vec![ModelImage {
                    name: "capture.png".to_string(),
                    media_type: "image/png".to_string(),
                    data_base64: "aGVsbG8=".to_string(),
                }],
            }],
            options: ModelOptions::default(),
        });

        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(
            body["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,aGVsbG8="
        );
    }

    #[test]
    fn parses_text_finish_reason_and_usage() {
        let response = super::parse_response(
            &json!({
                "choices": [{
                    "message": { "content": "Hello" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 4,
                    "total_tokens": 14,
                    "completion_tokens_details": { "reasoning_tokens": 2 }
                }
            }),
            false,
        )
        .unwrap();

        assert_eq!(response.text, "Hello");
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
        assert_eq!(response.usage.unwrap().reasoning_tokens, Some(2));
    }

    #[test]
    fn requests_and_parses_reasoning_content() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-5".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Solve it".to_string(),
                images: Vec::new(),
            }],
            options: ModelOptions {
                thinking_enabled: true,
                ..ModelOptions::default()
            },
        });
        assert_eq!(body["reasoning_effort"], "medium");

        let response = super::parse_response(
            &json!({
                "choices": [{
                    "message": {
                        "content": "Answer",
                        "reasoning_content": "Plan first."
                    }
                }]
            }),
            true,
        )
        .unwrap();
        assert_eq!(response.text, "Answer");
        assert_eq!(response.reasoning.as_deref(), Some("Plan first."));
    }
}
