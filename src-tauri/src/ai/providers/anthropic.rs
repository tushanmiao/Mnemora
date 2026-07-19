//! Anthropic Messages 协议适配器。
//!
//! 模型列表和非流式生成共享 `anthropic-version` Header。生成时把 System Prompt 放在
//! 顶层 `system`，合并连续同角色消息，并把文本块、停止原因和缓存用量转换为统一结果。

use reqwest::{Client, RequestBuilder};
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

const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 4_096;

pub fn model_list_request(client: &Client, base_url: &str) -> Result<RequestBuilder, String> {
    let mut url = endpoint_url(base_url, "models")?;
    url.query_pairs_mut().append_pair("limit", "1000");
    Ok(client
        .get(url)
        .header("anthropic-version", ANTHROPIC_VERSION))
}

pub async fn complete(
    client: &Client,
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
) -> Result<ModelResponse, ModelError> {
    let url =
        endpoint_url(context.base_url, "messages").map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(
        client
            .post(url)
            .header("anthropic-version", ANTHROPIC_VERSION),
        context,
        DefaultAuth::XApiKey,
    )?;
    let response = request_builder
        .json(&request_body(request))
        .send()
        .await
        .map_err(ModelError::from_reqwest)?;

    if !response.status().is_success() {
        return Err(ModelError::from_response(response, context.api_key).await);
    }

    let value = read_json_response(response, "Anthropic Messages")
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
    let url = endpoint_url(context.base_url, "messages")
        .map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(
        client
            .post(url)
            .header("anthropic-version", ANTHROPIC_VERSION),
        context,
        DefaultAuth::XApiKey,
    )?;
    let mut body = request_body(request);
    body["stream"] = Value::Bool(true);

    let mut saw_text = false;
    let mut finish_reason = None;
    let mut usage = ModelUsage::default();
    let mut has_usage = false;
    let outcome = send_sse_request(
        request_builder.json(&body),
        context.api_key,
        cancellation,
        |event| {
            let value: Value = serde_json::from_str(&event.data).map_err(|_| {
                ModelError::invalid_response("Anthropic SSE 事件不是有效 JSON。")
            })?;
            let event_type = event
                .event_type
                .as_deref()
                .or_else(|| value.get("type").and_then(Value::as_str))
                .unwrap_or_default();
            match event_type {
                "message_start" => {
                    if let Some(raw_usage) = value.pointer("/message/usage") {
                        merge_usage(&mut usage, parse_usage(raw_usage));
                        has_usage = true;
                    }
                }
                "content_block_delta" => {
                    if value.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") {
                        if let Some(delta) = value.pointer("/delta/text").and_then(Value::as_str) {
                            if !delta.is_empty() {
                                saw_text = true;
                                on_chunk(ModelStreamChunk::TextDelta(delta.to_string()))?;
                            }
                        }
                    }
                }
                "message_delta" => {
                    if let Some(reason) = value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                        finish_reason = Some(reason.to_string());
                    }
                    if let Some(raw_usage) = value.get("usage") {
                        merge_usage(&mut usage, parse_usage(raw_usage));
                        has_usage = true;
                    }
                }
                "error" => {
                    let message = value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("供应商返回了未知流式错误");
                    return Err(ModelError::provider(format!("Anthropic 流式错误：{message}")));
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
            "Anthropic 流式响应没有可显示的文本内容。",
        ));
    }
    Ok(ModelStreamOutcome::Completed(ModelStreamSummary {
        finish_reason,
        usage: has_usage.then_some(usage),
    }))
}

fn request_body(request: &ModelRequest) -> Value {
    let mut body = Map::from_iter([
        ("model".to_string(), Value::String(request.model.clone())),
        (
            "max_tokens".to_string(),
            json!(request
                .options
                .max_output_tokens
                .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)),
        ),
        (
            "messages".to_string(),
            Value::Array(merged_messages(request)),
        ),
        ("stream".to_string(), Value::Bool(false)),
    ]);
    if let Some(system_prompt) = request.system_prompt.as_deref() {
        body.insert(
            "system".to_string(),
            Value::String(system_prompt.to_string()),
        );
    }
    if let Some(temperature) = request.options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    Value::Object(body)
}

fn merged_messages(request: &ModelRequest) -> Vec<Value> {
    let mut merged = Vec::<(ModelRole, String)>::new();
    for message in &request.messages {
        if let Some((last_role, last_content)) = merged.last_mut() {
            if *last_role == message.role {
                last_content.push_str("\n\n");
                last_content.push_str(&message.content);
                continue;
            }
        }
        merged.push((message.role, message.content.clone()));
    }

    merged
        .into_iter()
        .map(|(role, content)| {
            let role = match role {
                ModelRole::User => "user",
                ModelRole::Assistant => "assistant",
            };
            json!({ "role": role, "content": [{ "type": "text", "text": content }] })
        })
        .collect()
}

fn parse_response(value: &Value) -> Result<ModelResponse, ModelError> {
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ModelError::invalid_response("Anthropic 响应缺少 content 数组。"))?;
    let text = content
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    if text.is_empty() {
        return Err(ModelError::invalid_response(
            "Anthropic 响应没有可显示的文本内容。",
        ));
    }

    Ok(ModelResponse {
        text,
        finish_reason: value
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage: value.get("usage").map(parse_usage),
    })
}

fn parse_usage(value: &Value) -> ModelUsage {
    let input_tokens = value.get("input_tokens").and_then(Value::as_u64);
    let output_tokens = value.get("output_tokens").and_then(Value::as_u64);
    ModelUsage {
        input_tokens,
        output_tokens,
        total_tokens: value
            .get("total_tokens")
            .and_then(Value::as_u64)
            .or_else(|| {
                input_tokens
                    .zip(output_tokens)
                    .map(|(input, output)| input + output)
            }),
        cache_read_tokens: value.get("cache_read_input_tokens").and_then(Value::as_u64),
        cache_write_tokens: value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64),
        ..ModelUsage::default()
    }
}

fn merge_usage(target: &mut ModelUsage, incoming: ModelUsage) {
    if incoming.input_tokens.is_some() {
        target.input_tokens = incoming.input_tokens;
    }
    if incoming.output_tokens.is_some() {
        target.output_tokens = incoming.output_tokens;
    }
    if incoming.cache_read_tokens.is_some() {
        target.cache_read_tokens = incoming.cache_read_tokens;
    }
    if incoming.cache_write_tokens.is_some() {
        target.cache_write_tokens = incoming.cache_write_tokens;
    }
    target.total_tokens = target
        .input_tokens
        .zip(target.output_tokens)
        .map(|(input, output)| input + output);
}

pub fn parse_models(value: &Value) -> Result<Vec<String>, String> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Invalid Anthropic model list: missing data array".to_string())?;

    Ok(data
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::ai::types::{ModelMessage, ModelOptions, ModelRequest, ModelRole};

    #[test]
    fn parses_anthropic_models() {
        let models = super::parse_models(&json!({
            "data": [{ "id": "claude-sonnet" }, { "id": "claude-haiku" }]
        }))
        .unwrap();
        assert_eq!(models, vec!["claude-sonnet", "claude-haiku"]);
    }

    #[test]
    fn maps_system_and_merges_consecutive_roles() {
        let body = super::request_body(&ModelRequest {
            model: "claude-test".to_string(),
            system_prompt: Some("Be concise".to_string()),
            messages: vec![
                ModelMessage {
                    role: ModelRole::User,
                    content: "One".to_string(),
                },
                ModelMessage {
                    role: ModelRole::User,
                    content: "Two".to_string(),
                },
            ],
            options: ModelOptions::default(),
        });

        assert_eq!(body["system"], "Be concise");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["content"][0]["text"], "One\n\nTwo");
        assert_eq!(body["max_tokens"], 4_096);
    }

    #[test]
    fn parses_text_and_cache_usage() {
        let response = super::parse_response(&json!({
            "content": [
                { "type": "text", "text": "Hello " },
                { "type": "text", "text": "there" }
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 5,
                "cache_read_input_tokens": 4,
                "cache_creation_input_tokens": 2
            }
        }))
        .unwrap();

        assert_eq!(response.text, "Hello there");
        let usage = response.usage.unwrap();
        assert_eq!(usage.total_tokens, Some(17));
        assert_eq!(usage.cache_read_tokens, Some(4));
    }
}
