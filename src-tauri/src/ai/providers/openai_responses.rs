//! OpenAI Responses 非流式适配器。
//!
//! System Prompt 映射到顶层 `instructions`，历史消息映射到 `input`。响应只提取
//! `output[].content[type=output_text]`、完成状态和统一用量；SDK 专属便利字段仅作为兼容回退。

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
    let url =
        endpoint_url(context.base_url, "responses").map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(client.post(url), context, DefaultAuth::Bearer)?;
    let response = request_builder
        .json(&request_body(request))
        .send()
        .await
        .map_err(ModelError::from_reqwest)?;

    if !response.status().is_success() {
        return Err(ModelError::from_response(response, context.api_key).await);
    }

    let value = read_json_response(response, "OpenAI Responses")
        .await
        .map_err(ModelError::invalid_response)?;
    parse_response(&value)
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
    let url =
        endpoint_url(context.base_url, "responses").map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(client.post(url), context, DefaultAuth::Bearer)?;
    let mut body = request_body(request);
    body["stream"] = Value::Bool(true);

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
            let value: Value = serde_json::from_str(&event.data).map_err(|_| {
                ModelError::invalid_response("OpenAI Responses SSE 事件不是有效 JSON。")
            })?;
            let event_type = event
                .event_type
                .as_deref()
                .or_else(|| value.get("type").and_then(Value::as_str))
                .unwrap_or_default();
            match event_type {
                "response.output_text.delta" | "response.refusal.delta" => {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            saw_text = true;
                            on_chunk(ModelStreamChunk::TextDelta(delta.to_string()))?;
                        }
                    }
                }
                "response.completed" | "response.incomplete" => {
                    let response = value.get("response").unwrap_or(&value);
                    finish_reason = response
                        .pointer("/incomplete_details/reason")
                        .and_then(Value::as_str)
                        .or_else(|| response.get("status").and_then(Value::as_str))
                        .map(str::to_string);
                    if let Some(raw_usage) = response.get("usage").filter(|usage| !usage.is_null())
                    {
                        usage = Some(parse_usage(raw_usage));
                    }
                }
                "response.failed" | "error" => {
                    let message = value
                        .pointer("/response/error/message")
                        .or_else(|| value.pointer("/error/message"))
                        .and_then(Value::as_str)
                        .unwrap_or("供应商返回了未知流式错误");
                    return Err(ModelError::provider(format!(
                        "OpenAI Responses 流式错误：{message}"
                    )));
                }
                _ => {}
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
            "OpenAI Responses 流式响应没有 output_text 内容。",
        ));
    }
    Ok(ModelStreamOutcome::Completed(ModelStreamSummary {
        finish_reason,
        usage,
    }))
}

pub(crate) fn request_body(request: &ModelRequest) -> Value {
    let input = request
        .messages
        .iter()
        .map(|message| {
            let role = match message.role {
                ModelRole::User => "user",
                ModelRole::Assistant => "assistant",
            };
            json!({ "role": role, "content": message.content })
        })
        .collect::<Vec<_>>();
    let mut body = Map::from_iter([
        ("model".to_string(), Value::String(request.model.clone())),
        ("input".to_string(), Value::Array(input)),
        ("stream".to_string(), Value::Bool(false)),
        ("store".to_string(), Value::Bool(false)),
    ]);
    if let Some(system_prompt) = request.system_prompt.as_deref() {
        body.insert(
            "instructions".to_string(),
            Value::String(system_prompt.to_string()),
        );
    }
    if let Some(temperature) = request.options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(max_output_tokens) = request.options.max_output_tokens {
        body.insert("max_output_tokens".to_string(), json!(max_output_tokens));
    }
    Value::Object(body)
}

fn parse_response(value: &Value) -> Result<ModelResponse, ModelError> {
    let mut parts = Vec::new();
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            let Some(content) = item.get("content").and_then(Value::as_array) else {
                continue;
            };
            for part in content {
                match part.get("type").and_then(Value::as_str) {
                    Some("output_text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            parts.push(text.to_string());
                        }
                    }
                    Some("refusal") => {
                        if let Some(refusal) = part.get("refusal").and_then(Value::as_str) {
                            parts.push(refusal.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if parts.is_empty() {
        if let Some(output_text) = value.get("output_text").and_then(Value::as_str) {
            parts.push(output_text.to_string());
        }
    }
    let text = parts.join("");
    if text.is_empty() {
        return Err(ModelError::invalid_response(
            "OpenAI Responses 响应没有 output_text 内容。",
        ));
    }

    let finish_reason = value
        .pointer("/incomplete_details/reason")
        .and_then(Value::as_str)
        .or_else(|| value.get("status").and_then(Value::as_str))
        .map(str::to_string);
    Ok(ModelResponse {
        text,
        finish_reason,
        usage: value.get("usage").map(parse_usage),
    })
}

fn parse_usage(value: &Value) -> ModelUsage {
    ModelUsage {
        input_tokens: value.get("input_tokens").and_then(Value::as_u64),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: value
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        cache_read_tokens: value
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_write_tokens: value
            .pointer("/input_tokens_details/cache_write_tokens")
            .and_then(Value::as_u64),
        ..ModelUsage::default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::ai::types::{ModelMessage, ModelOptions, ModelRequest, ModelRole};

    #[test]
    fn maps_instructions_and_disables_server_storage() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-test".to_string(),
            system_prompt: Some("Be concise".to_string()),
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Hello".to_string(),
            }],
            options: ModelOptions::default(),
        });

        assert_eq!(body["instructions"], "Be concise");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["store"], false);
    }

    #[test]
    fn parses_output_items_and_usage_details() {
        let response = super::parse_response(&json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": "Hello" }]
            }],
            "usage": {
                "input_tokens": 8,
                "output_tokens": 3,
                "total_tokens": 11,
                "input_tokens_details": { "cached_tokens": 4 },
                "output_tokens_details": { "reasoning_tokens": 1 }
            }
        }))
        .unwrap();

        assert_eq!(response.text, "Hello");
        assert_eq!(response.finish_reason.as_deref(), Some("completed"));
        assert_eq!(response.usage.unwrap().cache_read_tokens, Some(4));
    }
}
