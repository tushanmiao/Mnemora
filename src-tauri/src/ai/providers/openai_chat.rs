//! OpenAI Chat Completions 非流式适配器。
//!
//! `complete` 依次完成：统一消息转 `messages`、Bearer/自定义认证、单次 POST、
//! 错误转换、`choices[0].message` 与 `usage` 解析。这里不重试，也不处理 SSE。

use reqwest::Client;
use serde_json::{json, Map, Value};

use crate::ai::{
    error::ModelError,
    http::{endpoint_url, read_json_response},
    types::{ModelRequest, ModelResponse, ModelRole, ModelUsage, ProviderRequestContext},
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
    parse_response(&value)
}

fn request_body(request: &ModelRequest) -> Value {
    let mut messages = Vec::with_capacity(request.messages.len() + 1);
    if let Some(system_prompt) = request.system_prompt.as_deref() {
        messages.push(json!({ "role": "system", "content": system_prompt }));
    }
    messages.extend(request.messages.iter().map(|message| {
        let role = match message.role {
            ModelRole::User => "user",
            ModelRole::Assistant => "assistant",
        };
        json!({ "role": role, "content": message.content })
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
    Value::Object(body)
}

fn parse_response(value: &Value) -> Result<ModelResponse, ModelError> {
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
        finish_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage: value.get("usage").map(parse_usage),
    })
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

    use crate::ai::types::{ModelMessage, ModelOptions, ModelRequest, ModelRole};

    #[test]
    fn maps_system_prompt_and_messages() {
        let body = super::request_body(&ModelRequest {
            model: "gpt-test".to_string(),
            system_prompt: Some("Be concise".to_string()),
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "Hello".to_string(),
            }],
            options: ModelOptions::default(),
        });

        assert_eq!(body["model"], "gpt-test");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn parses_text_finish_reason_and_usage() {
        let response = super::parse_response(&json!({
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
        }))
        .unwrap();

        assert_eq!(response.text, "Hello");
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
        assert_eq!(response.usage.unwrap().reasoning_tokens, Some(2));
    }
}
