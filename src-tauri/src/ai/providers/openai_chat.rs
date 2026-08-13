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
        ModelStreamSummary, ModelToolCall, ModelUsage, ProviderRequestContext,
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
    let mut pending_tool_calls = Vec::<PendingToolCall>::new();
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
                if let Some(tool_calls) = choice
                    .pointer("/delta/tool_calls")
                    .and_then(Value::as_array)
                {
                    for raw_call in tool_calls {
                        let index =
                            raw_call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        while pending_tool_calls.len() <= index {
                            pending_tool_calls.push(PendingToolCall::default());
                        }
                        let (id, name, arguments_delta) =
                            accumulate_tool_call_delta(raw_call, &mut pending_tool_calls[index]);
                        on_chunk(ModelStreamChunk::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments_delta,
                            provider_signature: None,
                        })?;
                    }
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
    let tool_calls = finish_pending_tool_calls(pending_tool_calls)?;
    if !saw_text && tool_calls.is_empty() {
        return Err(ModelError::invalid_response(
            "OpenAI Chat 流式响应没有可显示的文本内容。",
        ));
    }
    Ok(ModelStreamOutcome::Completed(ModelStreamSummary {
        finish_reason,
        usage,
        tool_calls,
    }))
}

pub(crate) fn request_body(request: &ModelRequest) -> Value {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system_prompt) = request.system_prompt.as_deref() {
        messages.push(json!({ "role": "system", "content": system_prompt }));
    }
    messages.extend(request.messages.iter().map(|message| {
        if message.role == ModelRole::Tool {
            let result = message.tool_result.as_ref();
            return json!({
                "role": "tool",
                "tool_call_id": result.map(|value| value.call_id.as_str()).unwrap_or_default(),
                "content": result.map(|value| value.content.as_str()).unwrap_or(message.content.as_str())
            });
        }
        let role = if message.role == ModelRole::User { "user" } else { "assistant" };
        let mut value = if message.images.is_empty() {
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
        };
        if !message.tool_calls.is_empty() {
            value["tool_calls"] = Value::Array(message.tool_calls.iter().map(|call| json!({
                "id": call.id,
                "type": "function",
                "function": { "name": call.name, "arguments": call.arguments.to_string() }
            })).collect());
        }
        value
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
        let effort = request
            .options
            .reasoning_effort
            .as_deref()
            .unwrap_or("medium");
        body.insert(
            "reasoning_effort".to_string(),
            Value::String(effort.to_string()),
        );
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
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.input_schema,
                                "strict": true
                            }
                        })
                    })
                    .collect(),
            ),
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
    let tool_calls = parse_tool_calls(message)?;
    if text.is_empty() && tool_calls.is_empty() {
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
        tool_calls,
    })
}

#[derive(Default)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn parse_tool_calls(message: &Value) -> Result<Vec<ModelToolCall>, ModelError> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, call)| {
            let id = non_empty_string(call.get("id").and_then(Value::as_str))
                .unwrap_or_else(|| fallback_tool_call_id(index));
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            parse_tool_call(&id, name, arguments)
        })
        .collect()
}

fn accumulate_tool_call_delta(
    raw_call: &Value,
    pending: &mut PendingToolCall,
) -> (Option<String>, Option<String>, String) {
    // 部分 OpenAI 兼容网关会在后续参数分片中重复发送空 ID 或空名称。
    // 空值只能表示“本分片没有更新”，不能覆盖首个分片中已经收到的真实值。
    let id = non_empty_string(raw_call.get("id").and_then(Value::as_str));
    let name = non_empty_string(raw_call.pointer("/function/name").and_then(Value::as_str));
    let arguments_delta = raw_call
        .pointer("/function/arguments")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if let Some(value) = id.as_ref() {
        pending.id = value.clone();
    }
    if let Some(value) = name.as_ref() {
        pending.name = value.clone();
    }
    pending.arguments.push_str(&arguments_delta);

    (id, name, arguments_delta)
}

fn finish_pending_tool_calls(
    calls: Vec<PendingToolCall>,
) -> Result<Vec<ModelToolCall>, ModelError> {
    calls
        .into_iter()
        .enumerate()
        .filter(|(_, call)| {
            !call.id.trim().is_empty()
                || !call.name.trim().is_empty()
                || !call.arguments.trim().is_empty()
        })
        .map(|(index, call)| {
            let id =
                non_empty_string(Some(&call.id)).unwrap_or_else(|| fallback_tool_call_id(index));
            parse_tool_call(&id, &call.name, &call.arguments)
        })
        .collect()
}

fn parse_tool_call(id: &str, name: &str, arguments: &str) -> Result<ModelToolCall, ModelError> {
    let id = id.trim();
    let name = name.trim();
    if id.is_empty() || name.is_empty() {
        return Err(ModelError::invalid_response(
            "OpenAI Chat 工具调用缺少 ID 或名称。",
        ));
    }
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    let arguments = serde_json::from_str::<Value>(arguments).map_err(|_| {
        ModelError::invalid_response("OpenAI Chat 工具调用参数不是有效 JSON 对象。")
    })?;
    if !arguments.is_object() {
        return Err(ModelError::invalid_response(
            "OpenAI Chat 工具调用参数必须是 JSON 对象。",
        ));
    }
    Ok(ModelToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
        provider_signature: None,
    })
}

fn non_empty_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn fallback_tool_call_id(index: usize) -> String {
    format!("call_mnemora_{index}_{}", uuid::Uuid::new_v4().simple())
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

    use crate::ai::types::{
        ModelImage, ModelMessage, ModelOptions, ModelRequest, ModelRole, ModelTool, ModelToolCall,
        ModelToolResult,
    };

    #[test]
    fn maps_system_prompt_and_messages() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-test".to_string(),
            system_prompt: Some("Be concise".to_string()),
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Hello".to_string(),
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions::default(),
            tools: Vec::new(),
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
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions::default(),
            tools: Vec::new(),
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
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions {
                thinking_enabled: true,
                ..ModelOptions::default()
            },
            tools: Vec::new(),
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

    #[test]
    fn maps_tools_calls_and_results() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-test".to_string(),
            system_prompt: None,
            messages: vec![
                ModelMessage {
                    role: ModelRole::Assistant,
                    content: String::new(),
                    images: Vec::new(),
                    tool_calls: vec![ModelToolCall {
                        id: "call_1".to_string(),
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
                        call_id: "call_1".to_string(),
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

        assert_eq!(body["tools"][0]["function"]["name"], "skill");
        assert_eq!(body["messages"][0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(body["messages"][1]["tool_call_id"], "call_1");
    }

    #[test]
    fn accepts_tool_only_response_and_keeps_stream_arguments_separate() {
        let response = super::parse_response(
            &json!({
                "choices": [{
                    "message": {
                        "content": null,
                        "tool_calls": [
                            { "id": "call_1", "function": { "name": "first", "arguments": "{\"value\":1}" } },
                            { "id": "call_2", "function": { "name": "second", "arguments": "{\"value\":2}" } }
                        ]
                    },
                    "finish_reason": "tool_calls"
                }]
            }),
            false,
        )
        .unwrap();
        assert!(response.text.is_empty());
        assert_eq!(response.tool_calls.len(), 2);

        let calls = super::finish_pending_tool_calls(vec![
            super::PendingToolCall {
                id: "call_1".to_string(),
                name: "first".to_string(),
                arguments: "{\"value\":1}".to_string(),
            },
            super::PendingToolCall {
                id: "call_2".to_string(),
                name: "second".to_string(),
                arguments: "{\"value\":2}".to_string(),
            },
        ])
        .unwrap();
        assert_eq!(calls[0].arguments["value"], 1);
        assert_eq!(calls[1].arguments["value"], 2);
    }

    #[test]
    fn stream_tool_call_keeps_real_id_and_name_over_empty_continuations() {
        let mut pending = super::PendingToolCall::default();
        let first = json!({
            "id": " call_real ",
            "function": {
                "name": " code-explanation ",
                "arguments": "{\"path\":"
            }
        });
        let continuation = json!({
            "id": "",
            "function": {
                "name": "   ",
                "arguments": "\"src/main.rs\"}"
            }
        });

        let (first_id, first_name, _) = super::accumulate_tool_call_delta(&first, &mut pending);
        let (continuation_id, continuation_name, _) =
            super::accumulate_tool_call_delta(&continuation, &mut pending);

        assert_eq!(first_id.as_deref(), Some("call_real"));
        assert_eq!(first_name.as_deref(), Some("code-explanation"));
        assert_eq!(continuation_id, None);
        assert_eq!(continuation_name, None);

        let calls = super::finish_pending_tool_calls(vec![pending]).unwrap();
        assert_eq!(calls[0].id, "call_real");
        assert_eq!(calls[0].name, "code-explanation");
        assert_eq!(calls[0].arguments["path"], "src/main.rs");
    }

    #[test]
    fn stream_tool_call_generates_id_when_gateway_omits_it() {
        let mut pending = super::PendingToolCall::default();
        let chunk = json!({
            "id": "",
            "function": {
                "name": "code-explanation",
                "arguments": "{}"
            }
        });

        let (id, name, _) = super::accumulate_tool_call_delta(&chunk, &mut pending);
        assert_eq!(id, None);
        assert_eq!(name.as_deref(), Some("code-explanation"));

        let calls = super::finish_pending_tool_calls(vec![pending]).unwrap();
        assert!(calls[0].id.starts_with("call_mnemora_0_"));
        assert_eq!(calls[0].name, "code-explanation");
    }

    #[test]
    fn non_stream_tool_call_generates_id_but_still_requires_name() {
        let response = super::parse_response(
            &json!({
                "choices": [{
                    "message": {
                        "content": null,
                        "tool_calls": [{
                            "id": " ",
                            "function": { "name": "code-explanation", "arguments": "" }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }]
            }),
            false,
        )
        .unwrap();
        assert!(response.tool_calls[0].id.starts_with("call_mnemora_0_"));
        assert_eq!(response.tool_calls[0].arguments, json!({}));

        let error = super::parse_response(
            &json!({
                "choices": [{
                    "message": {
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "function": { "name": "", "arguments": "{}" }
                        }]
                    }
                }]
            }),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("缺少 ID 或名称"));
    }
}
