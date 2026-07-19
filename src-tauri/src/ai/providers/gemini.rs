//! Gemini 原生 GenerateContent 协议适配器。
//!
//! System Prompt 映射到 `systemInstruction`，助手角色映射为 `model`，模型名称进入
//! `models/{model}:generateContent` 路径。响应解析 candidates 文本、finishReason 和 usageMetadata。

use reqwest::{Client, RequestBuilder, Url};
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

pub fn model_list_request(client: &Client, base_url: &str) -> Result<RequestBuilder, String> {
    Ok(client.get(endpoint_url(base_url, "models")?))
}

pub async fn complete(
    client: &Client,
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
) -> Result<ModelResponse, ModelError> {
    let url = generate_content_url(context.base_url, &request.model)
        .map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(client.post(url), context, DefaultAuth::XGoogApiKey)?;
    let response = request_builder
        .json(&request_body(request))
        .send()
        .await
        .map_err(ModelError::from_reqwest)?;

    if !response.status().is_success() {
        return Err(ModelError::from_response(response, context.api_key).await);
    }

    let value = read_json_response(response, "Gemini GenerateContent")
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
    let url = stream_generate_content_url(context.base_url, &request.model)
        .map_err(ModelError::invalid_configuration)?;
    let request_builder = apply_model_auth(client.post(url), context, DefaultAuth::XGoogApiKey)?;
    let mut saw_text = false;
    let mut finish_reason = None;
    let mut usage = None;
    let outcome = send_sse_request(
        request_builder.json(&request_body(request)),
        context.api_key,
        cancellation,
        |event| {
            let value: Value = serde_json::from_str(&event.data)
                .map_err(|_| ModelError::invalid_response("Gemini SSE 事件不是有效 JSON。"))?;
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                return Err(ModelError::provider(format!("Gemini 流式错误：{message}")));
            }
            if let Some(block_reason) = value
                .pointer("/promptFeedback/blockReason")
                .and_then(Value::as_str)
            {
                return Err(ModelError::content_filtered(format!(
                    "Gemini 因安全策略拒绝了请求：{block_reason}。"
                )));
            }
            if let Some(candidate) = value
                .get("candidates")
                .and_then(Value::as_array)
                .and_then(|candidates| candidates.first())
            {
                let delta = candidate
                    .pointer("/content/parts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<String>();
                if !delta.is_empty() {
                    saw_text = true;
                    on_chunk(ModelStreamChunk::TextDelta(delta))?;
                }
                if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
                    finish_reason = Some(reason.to_string());
                }
            }
            if let Some(raw_usage) = value.get("usageMetadata") {
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
            "Gemini 流式响应没有可显示的文本内容。",
        ));
    }
    Ok(ModelStreamOutcome::Completed(ModelStreamSummary {
        finish_reason,
        usage,
    }))
}

fn generate_content_url(base_url: &str, model: &str) -> Result<Url, String> {
    let model = model.trim().strip_prefix("models/").unwrap_or(model.trim());
    if model.is_empty() {
        return Err("Gemini API Model cannot be empty".to_string());
    }

    let mut url = endpoint_url(base_url, "models")?;
    url.path_segments_mut()
        .map_err(|_| "Gemini Base URL cannot be used as a path base".to_string())?
        .push(&format!("{model}:generateContent"));
    Ok(url)
}

fn stream_generate_content_url(base_url: &str, model: &str) -> Result<Url, String> {
    let model = model.trim().strip_prefix("models/").unwrap_or(model.trim());
    if model.is_empty() {
        return Err("Gemini API Model cannot be empty".to_string());
    }

    let mut url = endpoint_url(base_url, "models")?;
    url.path_segments_mut()
        .map_err(|_| "Gemini Base URL cannot be used as a path base".to_string())?
        .push(&format!("{model}:streamGenerateContent"));
    url.query_pairs_mut().append_pair("alt", "sse");
    Ok(url)
}

fn request_body(request: &ModelRequest) -> Value {
    let contents = request
        .messages
        .iter()
        .map(|message| {
            let role = match message.role {
                ModelRole::User => "user",
                ModelRole::Assistant => "model",
            };
            json!({ "role": role, "parts": [{ "text": message.content }] })
        })
        .collect::<Vec<_>>();
    let mut body = Map::from_iter([("contents".to_string(), Value::Array(contents))]);
    if let Some(system_prompt) = request.system_prompt.as_deref() {
        body.insert(
            "systemInstruction".to_string(),
            json!({ "parts": [{ "text": system_prompt }] }),
        );
    }

    let mut generation_config = Map::new();
    if let Some(temperature) = request.options.temperature {
        generation_config.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(max_output_tokens) = request.options.max_output_tokens {
        generation_config.insert("maxOutputTokens".to_string(), json!(max_output_tokens));
    }
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }
    Value::Object(body)
}

fn parse_response(value: &Value) -> Result<ModelResponse, ModelError> {
    if let Some(block_reason) = value
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
    {
        return Err(ModelError::content_filtered(format!(
            "Gemini 因安全策略拒绝了请求：{block_reason}。"
        )));
    }

    let candidate = value
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .ok_or_else(|| ModelError::invalid_response("Gemini 响应缺少 candidates[0]。"))?;
    let finish_reason = candidate
        .get("finishReason")
        .and_then(Value::as_str)
        .map(str::to_string);
    let text = candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>();
    if text.is_empty() {
        if finish_reason.as_deref() == Some("SAFETY") {
            return Err(ModelError::content_filtered(
                "Gemini 因安全策略没有返回文本内容。",
            ));
        }
        return Err(ModelError::invalid_response(
            "Gemini 响应没有可显示的文本内容。",
        ));
    }

    Ok(ModelResponse {
        text,
        finish_reason,
        usage: value.get("usageMetadata").map(parse_usage),
    })
}

fn parse_usage(value: &Value) -> ModelUsage {
    let input_tokens = value.get("promptTokenCount").and_then(Value::as_u64);
    let output_tokens = value.get("candidatesTokenCount").and_then(Value::as_u64);
    ModelUsage {
        input_tokens,
        output_tokens,
        total_tokens: value
            .get("totalTokenCount")
            .and_then(Value::as_u64)
            .or_else(|| {
                input_tokens
                    .zip(output_tokens)
                    .map(|(input, output)| input + output)
            }),
        reasoning_tokens: value.get("thoughtsTokenCount").and_then(Value::as_u64),
        cache_read_tokens: value.get("cachedContentTokenCount").and_then(Value::as_u64),
        ..ModelUsage::default()
    }
}

pub fn parse_models(value: &Value) -> Result<Vec<String>, String> {
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "Invalid Gemini model list: missing models array".to_string())?;

    Ok(models
        .iter()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(|name| name.strip_prefix("models/").unwrap_or(name).to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::ai::types::{ModelMessage, ModelOptions, ModelRequest, ModelRole};

    #[test]
    fn parses_gemini_models_without_models_prefix() {
        let models = super::parse_models(&json!({
            "models": [
                { "name": "models/gemini-pro" },
                { "name": "models/gemini-flash" }
            ]
        }))
        .unwrap();
        assert_eq!(models, vec!["gemini-pro", "gemini-flash"]);
    }

    #[test]
    fn builds_native_generate_content_request() {
        let request = ModelRequest {
            model: "gemini-test".to_string(),
            system_prompt: Some("Be concise".to_string()),
            messages: vec![ModelMessage {
                role: ModelRole::Assistant,
                content: "Previous".to_string(),
            }],
            options: ModelOptions::default(),
        };
        let url =
            super::generate_content_url("https://example.com/v1beta", &request.model).unwrap();
        let body = super::request_body(&request);

        assert_eq!(
            url.as_str(),
            "https://example.com/v1beta/models/gemini-test:generateContent"
        );
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be concise");
        assert_eq!(body["contents"][0]["role"], "model");
    }

    #[test]
    fn parses_text_and_usage_metadata() {
        let response = super::parse_response(&json!({
            "candidates": [{
                "content": { "parts": [{ "text": "Hello" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 6,
                "candidatesTokenCount": 3,
                "totalTokenCount": 9,
                "cachedContentTokenCount": 2,
                "thoughtsTokenCount": 1
            }
        }))
        .unwrap();

        assert_eq!(response.text, "Hello");
        let usage = response.usage.unwrap();
        assert_eq!(usage.total_tokens, Some(9));
        assert_eq!(usage.reasoning_tokens, Some(1));
    }
}
