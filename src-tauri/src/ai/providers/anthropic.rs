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
        ModelStreamSummary, ModelToolCall, ModelUsage, ProviderRequestContext,
    },
};

use super::{apply_model_auth, DefaultAuth};

pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";
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
    let url =
        endpoint_url(context.base_url, "messages").map_err(ModelError::invalid_configuration)?;
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
    let mut pending_tool_calls = Vec::<PendingToolCall>::new();
    let outcome = send_sse_request(
        request_builder.json(&body),
        context.api_key,
        cancellation,
        |event| {
            let value: Value = serde_json::from_str(&event.data)
                .map_err(|_| ModelError::invalid_response("Anthropic SSE 事件不是有效 JSON。"))?;
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
                "content_block_delta" => match value.pointer("/delta/type").and_then(Value::as_str)
                {
                    Some("text_delta") => {
                        if let Some(delta) = value.pointer("/delta/text").and_then(Value::as_str) {
                            if !delta.is_empty() {
                                saw_text = true;
                                on_chunk(ModelStreamChunk::TextDelta(delta.to_string()))?;
                            }
                        }
                    }
                    Some("thinking_delta") if request.options.thinking_enabled => {
                        if let Some(delta) =
                            value.pointer("/delta/thinking").and_then(Value::as_str)
                        {
                            if !delta.is_empty() {
                                on_chunk(ModelStreamChunk::ReasoningDelta(delta.to_string()))?;
                            }
                        }
                    }
                    Some("input_json_delta") => {
                        let index =
                            value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        ensure_pending(&mut pending_tool_calls, index);
                        let delta = value
                            .pointer("/delta/partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        pending_tool_calls[index].arguments.push_str(&delta);
                        on_chunk(ModelStreamChunk::ToolCallDelta {
                            index,
                            id: None,
                            name: None,
                            arguments_delta: delta,
                            provider_signature: None,
                        })?;
                    }
                    _ => {}
                },
                "content_block_start" => {
                    let block = value.get("content_block").unwrap_or(&Value::Null);
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let index =
                            value.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        ensure_pending(&mut pending_tool_calls, index);
                        let call = &mut pending_tool_calls[index];
                        call.id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        call.name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        // Anthropic 通常在 start 事件中发送空对象，真正参数随后由
                        // input_json_delta 给出。空对象不能先拼成 "{}"，否则会得到
                        // "{}{...}" 这种无效 JSON。
                        let initial = block.get("input").filter(|value| {
                            value.as_object().is_some_and(|object| !object.is_empty())
                        });
                        if let Some(initial) = initial {
                            call.arguments.push_str(&initial.to_string());
                        }
                        on_chunk(ModelStreamChunk::ToolCallDelta {
                            index,
                            id: (!call.id.is_empty()).then(|| call.id.clone()),
                            name: (!call.name.is_empty()).then(|| call.name.clone()),
                            arguments_delta: initial.map(Value::to_string).unwrap_or_default(),
                            provider_signature: None,
                        })?;
                    }
                }
                "message_delta" => {
                    if let Some(reason) =
                        value.pointer("/delta/stop_reason").and_then(Value::as_str)
                    {
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
                    return Err(ModelError::provider(format!(
                        "Anthropic 流式错误：{message}"
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
    let tool_calls = finish_pending_tool_calls(pending_tool_calls)?;
    if !saw_text && tool_calls.is_empty() {
        return Err(ModelError::invalid_response(
            "Anthropic 流式响应没有可显示的文本内容。",
        ));
    }
    Ok(ModelStreamOutcome::Completed(ModelStreamSummary {
        finish_reason,
        usage: has_usage.then_some(usage),
        tool_calls,
    }))
}

pub(crate) fn request_body(request: &ModelRequest) -> Value {
    let max_tokens = request
        .options
        .max_output_tokens
        .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS);
    let mut body = Map::from_iter([
        ("model".to_string(), Value::String(request.model.clone())),
        ("max_tokens".to_string(), json!(max_tokens)),
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
    if request.options.thinking_enabled && max_tokens >= 2_048 {
        let budget_tokens = (max_tokens / 4).clamp(1_024, 8_192).min(max_tokens - 1);
        body.insert(
            "thinking".to_string(),
            json!({ "type": "enabled", "budget_tokens": budget_tokens }),
        );
    } else if let Some(temperature) = request.options.temperature {
        body.insert("temperature".to_string(), json!(temperature));
    }
    if !request.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.input_schema
                        })
                    })
                    .collect(),
            ),
        );
    }
    Value::Object(body)
}

fn merged_messages(request: &ModelRequest) -> Vec<Value> {
    let mut merged = Vec::<Value>::new();
    for message in &request.messages {
        let role = if message.role == ModelRole::Assistant {
            "assistant"
        } else {
            "user"
        };
        let mut content = Vec::new();
        if message.role == ModelRole::Tool {
            if let Some(result) = message.tool_result.as_ref() {
                content.push(json!({
                    "type": "tool_result",
                    "tool_use_id": result.call_id,
                    "content": result.content,
                    "is_error": result.is_error
                }));
            }
        } else {
            if !message.content.trim().is_empty() {
                content.push(json!({ "type": "text", "text": message.content }));
            }
            content.extend(message.images.iter().map(|image| {
                json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": image.media_type,
                        "data": image.data_base64
                    }
                })
            }));
            content.extend(message.tool_calls.iter().map(|call| {
                json!({
                    "type": "tool_use",
                    "id": call.id,
                    "name": call.name,
                    "input": call.arguments
                })
            }));
        }
        if let Some(last) = merged
            .last_mut()
            .filter(|last| last.get("role").and_then(Value::as_str) == Some(role))
        {
            if let Some(parts) = last.get_mut("content").and_then(Value::as_array_mut) {
                merge_content_parts(parts, content);
                continue;
            }
        }
        merged.push(json!({ "role": role, "content": content }));
    }
    merged
}

fn merge_content_parts(target: &mut Vec<Value>, mut incoming: Vec<Value>) {
    let can_merge_text = target
        .last()
        .and_then(|part| part.get("type"))
        .and_then(Value::as_str)
        == Some("text")
        && incoming
            .first()
            .and_then(|part| part.get("type"))
            .and_then(Value::as_str)
            == Some("text");
    if can_merge_text {
        let next = incoming
            .remove(0)
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if let Some(text_value) = target.last_mut().and_then(|part| part.get_mut("text")) {
            let text = text_value.as_str().unwrap_or_default().to_string();
            *text_value = Value::String(format!("{text}\n\n{next}"));
        }
    }
    target.extend(incoming);
}

fn parse_response(value: &Value, include_reasoning: bool) -> Result<ModelResponse, ModelError> {
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ModelError::invalid_response("Anthropic 响应缺少 content 数组。"))?;
    let text = content
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    let tool_calls = parse_tool_calls(content)?;
    if text.is_empty() && tool_calls.is_empty() {
        return Err(ModelError::invalid_response(
            "Anthropic 响应没有可显示的文本内容。",
        ));
    }

    Ok(ModelResponse {
        text,
        reasoning: include_reasoning
            .then(|| {
                let reasoning = content
                    .iter()
                    .filter(|part| part.get("type").and_then(Value::as_str) == Some("thinking"))
                    .filter_map(|part| part.get("thinking").and_then(Value::as_str))
                    .collect::<String>();
                (!reasoning.is_empty()).then_some(reasoning)
            })
            .flatten(),
        finish_reason: value
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage: value.get("usage").map(parse_usage),
        tool_calls,
    })
}

#[derive(Default)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn ensure_pending(calls: &mut Vec<PendingToolCall>, index: usize) {
    while calls.len() <= index {
        calls.push(PendingToolCall::default());
    }
}

fn parse_tool_calls(content: &[Value]) -> Result<Vec<ModelToolCall>, ModelError> {
    content
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|part| {
            let id = part.get("id").and_then(Value::as_str).unwrap_or_default();
            let name = part.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = part.get("input").cloned().unwrap_or_else(|| json!({}));
            build_tool_call(id, name, arguments)
        })
        .collect()
}

fn finish_pending_tool_calls(
    calls: Vec<PendingToolCall>,
) -> Result<Vec<ModelToolCall>, ModelError> {
    calls
        .into_iter()
        .filter(|call| !call.id.is_empty() || !call.name.is_empty())
        .map(|call| {
            let raw_arguments = if call.arguments.trim().is_empty() {
                "{}"
            } else {
                call.arguments.as_str()
            };
            let arguments = serde_json::from_str(raw_arguments).map_err(|_| {
                ModelError::invalid_response("Anthropic 工具调用参数不是有效 JSON 对象。")
            })?;
            build_tool_call(&call.id, &call.name, arguments)
        })
        .collect()
}

fn build_tool_call(id: &str, name: &str, arguments: Value) -> Result<ModelToolCall, ModelError> {
    if id.trim().is_empty() || name.trim().is_empty() || !arguments.is_object() {
        return Err(ModelError::invalid_response(
            "Anthropic 工具调用缺少 ID、名称或对象参数。",
        ));
    }
    Ok(ModelToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
        provider_signature: None,
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

    use crate::ai::types::{
        ModelImage, ModelMessage, ModelOptions, ModelRequest, ModelRole, ModelTool, ModelToolCall,
        ModelToolResult,
    };

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
                    images: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_result: None,
                },
                ModelMessage {
                    role: ModelRole::User,
                    content: "Two".to_string(),
                    images: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_result: None,
                },
            ],
            options: ModelOptions::default(),
            tools: Vec::new(),
        });

        assert_eq!(body["system"], "Be concise");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["content"][0]["text"], "One\n\nTwo");
        assert_eq!(body["max_tokens"], 4_096);
    }

    #[test]
    fn maps_image_as_anthropic_source_block() {
        let body = super::request_body(&ModelRequest {
            model: "claude-vision".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Describe it".to_string(),
                images: vec![ModelImage {
                    name: "capture.png".to_string(),
                    media_type: "image/png".to_string(),
                    data_base64: "aGVsbG8=".to_string(),
                }],
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions::default(),
            tools: Vec::new(),
        });

        assert_eq!(body["messages"][0]["content"][1]["type"], "image");
        assert_eq!(
            body["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(
            body["messages"][0]["content"][1]["source"]["data"],
            "aGVsbG8="
        );
    }

    #[test]
    fn parses_text_and_cache_usage() {
        let response = super::parse_response(
            &json!({
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
            }),
            false,
        )
        .unwrap();

        assert_eq!(response.text, "Hello there");
        let usage = response.usage.unwrap();
        assert_eq!(usage.total_tokens, Some(17));
        assert_eq!(usage.cache_read_tokens, Some(4));
    }

    #[test]
    fn enables_thinking_and_parses_reasoning_blocks() {
        let body = super::request_body(&ModelRequest {
            model: "claude-test".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Solve it".to_string(),
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions {
                max_output_tokens: Some(4_096),
                thinking_enabled: true,
                ..ModelOptions::default()
            },
            tools: Vec::new(),
        });
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 1_024);
        assert!(body.get("temperature").is_none());

        let response = super::parse_response(
            &json!({
                "content": [
                    { "type": "thinking", "thinking": "Plan first." },
                    { "type": "text", "text": "Answer" }
                ]
            }),
            true,
        )
        .unwrap();
        assert_eq!(response.text, "Answer");
        assert_eq!(response.reasoning.as_deref(), Some("Plan first."));
    }

    #[test]
    fn maps_tool_use_and_adjacent_tool_result() {
        let body = super::request_body(&ModelRequest {
            model: "claude-test".to_string(),
            system_prompt: None,
            messages: vec![
                ModelMessage {
                    role: ModelRole::Assistant,
                    content: String::new(),
                    images: Vec::new(),
                    tool_calls: vec![ModelToolCall {
                        id: "toolu_1".to_string(),
                        name: "skill".to_string(),
                        arguments: json!({ "id": "summarize" }),
                        provider_signature: None,
                    }],
                    tool_result: None,
                },
                ModelMessage {
                    role: ModelRole::Tool,
                    content: String::new(),
                    images: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_result: Some(ModelToolResult {
                        call_id: "toolu_1".to_string(),
                        name: "skill".to_string(),
                        content: "loaded".to_string(),
                        is_error: false,
                    }),
                },
            ],
            options: ModelOptions::default(),
            tools: vec![ModelTool {
                name: "skill".to_string(),
                description: "load skill".to_string(),
                input_schema: json!({ "type": "object" }),
            }],
        });

        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["messages"][0]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], "toolu_1");
    }

    #[test]
    fn accepts_tool_only_response_and_empty_stream_start_input() {
        let response = super::parse_response(
            &json!({
                "content": [
                    { "type": "tool_use", "id": "toolu_1", "name": "first", "input": { "value": 1 } },
                    { "type": "tool_use", "id": "toolu_2", "name": "second", "input": { "value": 2 } }
                ],
                "stop_reason": "tool_use"
            }),
            false,
        )
        .unwrap();
        assert!(response.text.is_empty());
        assert_eq!(response.tool_calls.len(), 2);

        let calls = super::finish_pending_tool_calls(vec![super::PendingToolCall {
            id: "toolu_1".to_string(),
            name: "empty".to_string(),
            arguments: String::new(),
        }])
        .unwrap();
        assert_eq!(calls[0].arguments, json!({}));
    }
}
