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
        ModelStreamSummary, ModelToolCall, ModelUsage, ProviderRequestContext,
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
        endpoint_url(context.base_url, "responses").map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(client.post(url), context, DefaultAuth::Bearer)?;
    let mut body = request_body(request);
    body["stream"] = Value::Bool(true);

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
            let value: Value = serde_json::from_str(&event.data).map_err(|_| {
                ModelError::invalid_response("OpenAI Responses SSE 事件不是有效 JSON。")
            })?;
            let event_type = event
                .event_type
                .as_deref()
                .or_else(|| value.get("type").and_then(Value::as_str))
                .unwrap_or_default();
            match event_type {
                "response.reasoning_summary_text.delta"
                | "response.reasoning_text.delta"
                | "response.reasoning_summary.delta"
                | "response.reasoning.delta"
                    if request.options.thinking_enabled =>
                {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            on_chunk(ModelStreamChunk::ReasoningDelta(delta.to_string()))?;
                        }
                    }
                }
                "response.output_text.delta" | "response.refusal.delta" => {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        if !delta.is_empty() {
                            saw_text = true;
                            on_chunk(ModelStreamChunk::TextDelta(delta.to_string()))?;
                        }
                    }
                }
                "response.output_item.added" => {
                    let item = value.get("item").unwrap_or(&Value::Null);
                    if item.get("type").and_then(Value::as_str) == Some("function_call") {
                        let index = value
                            .get("output_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize;
                        ensure_pending(&mut pending_tool_calls, index);
                        let call = &mut pending_tool_calls[index];
                        call.id = item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        call.name = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        call.arguments.push_str(
                            item.get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        );
                        on_chunk(ModelStreamChunk::ToolCallDelta {
                            index,
                            id: (!call.id.is_empty()).then(|| call.id.clone()),
                            name: (!call.name.is_empty()).then(|| call.name.clone()),
                            arguments_delta: item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            provider_signature: None,
                        })?;
                    }
                }
                "response.function_call_arguments.delta" => {
                    let index = value
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    ensure_pending(&mut pending_tool_calls, index);
                    let delta = value
                        .get("delta")
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
    let tool_calls = finish_pending_tool_calls(pending_tool_calls)?;
    if !saw_text && tool_calls.is_empty() {
        return Err(ModelError::invalid_response(
            "OpenAI Responses 流式响应没有 output_text 内容。",
        ));
    }
    Ok(ModelStreamOutcome::Completed(ModelStreamSummary {
        finish_reason,
        usage,
        tool_calls,
    }))
}

pub(crate) fn request_body(request: &ModelRequest) -> Value {
    let mut input = Vec::new();
    for message in &request.messages {
        if message.role == ModelRole::Tool {
            if let Some(result) = message.tool_result.as_ref() {
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": result.call_id,
                    "output": result.content
                }));
            }
            continue;
        }
        let role = if message.role == ModelRole::User {
            "user"
        } else {
            "assistant"
        };
        if !message.content.trim().is_empty() || !message.images.is_empty() {
            let value = if message.images.is_empty() {
                json!({ "role": role, "content": message.content })
            } else {
                let mut content = Vec::new();
                if !message.content.trim().is_empty() {
                    content.push(json!({ "type": "input_text", "text": message.content }));
                }
                content.extend(message.images.iter().map(|image| {
                    json!({
                        "type": "input_image",
                        "image_url": format!("data:{};base64,{}", image.media_type, image.data_base64)
                    })
                }));
                json!({ "role": role, "content": content })
            };
            input.push(value);
        }
        for call in &message.tool_calls {
            input.push(json!({
                "type": "function_call",
                "call_id": call.id,
                "name": call.name,
                "arguments": call.arguments.to_string()
            }));
        }
    }
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
    if request.options.thinking_enabled && supports_reasoning(&request.model) {
        let effort = request
            .options
            .reasoning_effort
            .as_deref()
            .unwrap_or("medium");
        body.insert(
            "reasoning".to_string(),
            json!({ "effort": effort, "summary": "auto" }),
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
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.input_schema,
                            "strict": true
                        })
                    })
                    .collect(),
            ),
        );
    }
    Value::Object(body)
}

fn parse_response(value: &Value, include_reasoning: bool) -> Result<ModelResponse, ModelError> {
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
    let tool_calls = parse_tool_calls(value)?;
    if text.is_empty() && tool_calls.is_empty() {
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
        reasoning: include_reasoning
            .then(|| extract_reasoning(value))
            .flatten(),
        finish_reason,
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

fn parse_tool_calls(value: &Value) -> Result<Vec<ModelToolCall>, ModelError> {
    value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .map(|item| {
            let id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            parse_tool_call(id, name, arguments)
        })
        .collect()
}

fn finish_pending_tool_calls(
    calls: Vec<PendingToolCall>,
) -> Result<Vec<ModelToolCall>, ModelError> {
    calls
        .into_iter()
        .filter(|call| !call.id.is_empty() || !call.name.is_empty())
        .map(|call| parse_tool_call(&call.id, &call.name, &call.arguments))
        .collect()
}

fn parse_tool_call(id: &str, name: &str, arguments: &str) -> Result<ModelToolCall, ModelError> {
    if id.trim().is_empty() || name.trim().is_empty() {
        return Err(ModelError::invalid_response(
            "OpenAI Responses 工具调用缺少 ID 或名称。",
        ));
    }
    let arguments = serde_json::from_str::<Value>(arguments).map_err(|_| {
        ModelError::invalid_response("OpenAI Responses 工具调用参数不是有效 JSON 对象。")
    })?;
    if !arguments.is_object() {
        return Err(ModelError::invalid_response(
            "OpenAI Responses 工具参数必须是 JSON 对象。",
        ));
    }
    Ok(ModelToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
        provider_signature: None,
    })
}

fn extract_reasoning(value: &Value) -> Option<String> {
    let mut parts = Vec::new();
    for item in value
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            continue;
        }
        for field in ["summary", "content"] {
            for part in item
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join(""))
}

fn supports_reasoning(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.starts_with("gpt-5")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
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

    use crate::ai::types::{
        ModelImage, ModelMessage, ModelOptions, ModelRequest, ModelRole, ModelTool, ModelToolCall,
        ModelToolResult,
    };

    #[test]
    fn maps_instructions_and_disables_server_storage() {
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

        assert_eq!(body["instructions"], "Be concise");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["store"], false);
    }

    #[test]
    fn maps_image_as_responses_input_part() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-vision".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: String::new(),
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

        assert_eq!(body["input"][0]["content"][0]["type"], "input_image");
        assert_eq!(
            body["input"][0]["content"][0]["image_url"],
            "data:image/png;base64,aGVsbG8="
        );
    }

    #[test]
    fn parses_output_items_and_usage_details() {
        let response = super::parse_response(
            &json!({
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
            }),
            false,
        )
        .unwrap();

        assert_eq!(response.text, "Hello");
        assert_eq!(response.finish_reason.as_deref(), Some("completed"));
        assert_eq!(response.usage.unwrap().cache_read_tokens, Some(4));
    }

    #[test]
    fn requests_and_parses_reasoning_summary() {
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
        assert_eq!(body["reasoning"]["effort"], "medium");
        assert_eq!(body["reasoning"]["summary"], "auto");

        let response = super::parse_response(
            &json!({
                "status": "completed",
                "output": [
                    {
                        "type": "reasoning",
                        "summary": [{ "type": "summary_text", "text": "Plan first." }]
                    },
                    {
                        "type": "message",
                        "content": [{ "type": "output_text", "text": "Answer" }]
                    }
                ]
            }),
            true,
        )
        .unwrap();
        assert_eq!(response.text, "Answer");
        assert_eq!(response.reasoning.as_deref(), Some("Plan first."));
    }

    #[test]
    fn maps_flat_tools_calls_and_outputs() {
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

        assert_eq!(body["tools"][0]["name"], "skill");
        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["call_id"], "call_1");
    }

    #[test]
    fn accepts_multiple_tool_only_items_and_stream_accumulation() {
        let response = super::parse_response(
            &json!({
                "status": "completed",
                "output": [
                    { "type": "function_call", "call_id": "call_1", "name": "first", "arguments": "{\"value\":1}" },
                    { "type": "function_call", "call_id": "call_2", "name": "second", "arguments": "{\"value\":2}" }
                ]
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
}
