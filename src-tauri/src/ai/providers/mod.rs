//! 模型供应商 wire protocol 适配器。
//!
//! - `openai_chat`：OpenAI Chat Completions。
//! - `openai_responses`：OpenAI Responses。
//! - `anthropic`：Anthropic Messages。
//! - `gemini`：Gemini 原生 GenerateContent。
//! - `openai`：两个 OpenAI 协议共用的模型列表解析。
//!
//! 本文件只保留认证和手动模型列表等少量公共分派，非流式生成由 `ai::dispatcher` 调用各协议文件。

pub(crate) mod anthropic;
pub(crate) mod gemini;
mod openai;
pub(crate) mod openai_chat;
pub(crate) mod openai_responses;

use std::{
    collections::{BTreeMap, HashSet},
    time::Instant,
};

use reqwest::{Client, RequestBuilder};
use serde_json::Value;

use crate::ai::{
    error::ModelError,
    http::{read_json_response, response_error},
    types::{
        ApiProtocol, AuthScheme, ConnectionTestResult, ProviderConnectionInput,
        ProviderRequestContext,
    },
};

#[derive(Clone, Copy)]
pub(super) enum DefaultAuth {
    Bearer,
    XApiKey,
    XGoogApiKey,
}

fn apply_api_key(
    request: RequestBuilder,
    auth_scheme: AuthScheme,
    api_key: &str,
    default_auth: DefaultAuth,
) -> Result<RequestBuilder, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("API Key is required".to_string());
    }

    let scheme = match auth_scheme {
        AuthScheme::ProtocolDefault => default_auth,
        AuthScheme::Bearer => DefaultAuth::Bearer,
        AuthScheme::XApiKey => DefaultAuth::XApiKey,
        AuthScheme::XGoogApiKey => DefaultAuth::XGoogApiKey,
    };

    Ok(match scheme {
        DefaultAuth::Bearer => request.bearer_auth(api_key),
        DefaultAuth::XApiKey => request.header("x-api-key", api_key),
        DefaultAuth::XGoogApiKey => request.header("x-goog-api-key", api_key),
    })
}

pub(super) fn apply_model_auth(
    request: RequestBuilder,
    context: &ProviderRequestContext<'_>,
    default_auth: DefaultAuth,
) -> Result<RequestBuilder, ModelError> {
    apply_api_key(request, context.auth_scheme, context.api_key, default_auth)
        .map_err(|_| ModelError::missing_api_key())
}

pub(crate) struct DebugHttpRequest {
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

/**
 * 构造与真实协议适配器一致的请求快照，但只写入脱敏后的认证 Header。
 * 该函数不会发送网络请求，也不会把 API Key 放入返回值。
 */
pub(crate) fn build_debug_request(
    context: &ProviderRequestContext<'_>,
    request: &crate::ai::types::ModelRequest,
    stream: bool,
) -> Result<DebugHttpRequest, String> {
    let (url, body, default_auth) = match context.protocol {
        ApiProtocol::OpenAiChatCompletions => {
            let url = crate::ai::http::endpoint_url(context.base_url, "chat/completions")?;
            let mut body = openai_chat::request_body(request);
            if stream {
                body["stream"] = Value::Bool(true);
                body["stream_options"] = serde_json::json!({ "include_usage": true });
            }
            (url, body, DefaultAuth::Bearer)
        }
        ApiProtocol::OpenAiResponses => {
            let url = crate::ai::http::endpoint_url(context.base_url, "responses")?;
            let mut body = openai_responses::request_body(request);
            if stream {
                body["stream"] = Value::Bool(true);
            }
            (url, body, DefaultAuth::Bearer)
        }
        ApiProtocol::AnthropicMessages => {
            let url = crate::ai::http::endpoint_url(context.base_url, "messages")?;
            let mut body = anthropic::request_body(request);
            if stream {
                body["stream"] = Value::Bool(true);
            }
            (url, body, DefaultAuth::XApiKey)
        }
        ApiProtocol::GeminiGenerateContent => {
            let url = if stream {
                gemini::stream_generate_content_url(context.base_url, &request.model)?
            } else {
                gemini::generate_content_url(context.base_url, &request.model)?
            };
            (url, gemini::request_body(request), DefaultAuth::XGoogApiKey)
        }
    };

    let mut headers =
        BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
    if context.protocol == ApiProtocol::AnthropicMessages {
        headers.insert(
            "anthropic-version".to_string(),
            anthropic::ANTHROPIC_VERSION.to_string(),
        );
    }
    let auth = match context.auth_scheme {
        AuthScheme::ProtocolDefault => default_auth,
        AuthScheme::Bearer => DefaultAuth::Bearer,
        AuthScheme::XApiKey => DefaultAuth::XApiKey,
        AuthScheme::XGoogApiKey => DefaultAuth::XGoogApiKey,
    };
    match auth {
        DefaultAuth::Bearer => {
            headers.insert("authorization".to_string(), "Bearer [REDACTED]".to_string());
        }
        DefaultAuth::XApiKey => {
            headers.insert("x-api-key".to_string(), "[REDACTED]".to_string());
        }
        DefaultAuth::XGoogApiKey => {
            headers.insert("x-goog-api-key".to_string(), "[REDACTED]".to_string());
        }
    }

    Ok(DebugHttpRequest {
        url: url.to_string(),
        headers,
        body,
    })
}

fn model_list_request(
    client: &Client,
    input: &ProviderConnectionInput,
) -> Result<RequestBuilder, String> {
    match input.protocol {
        ApiProtocol::OpenAiChatCompletions | ApiProtocol::OpenAiResponses => {
            let request = openai::model_list_request(client, &input.base_url)?;
            apply_api_key(
                request,
                input.auth_scheme,
                input.api_key.as_deref().unwrap_or_default(),
                DefaultAuth::Bearer,
            )
        }
        ApiProtocol::AnthropicMessages => {
            let request = anthropic::model_list_request(client, &input.base_url)?;
            apply_api_key(
                request,
                input.auth_scheme,
                input.api_key.as_deref().unwrap_or_default(),
                DefaultAuth::XApiKey,
            )
        }
        ApiProtocol::GeminiGenerateContent => {
            let request = gemini::model_list_request(client, &input.base_url)?;
            apply_api_key(
                request,
                input.auth_scheme,
                input.api_key.as_deref().unwrap_or_default(),
                DefaultAuth::XGoogApiKey,
            )
        }
    }
}

fn parse_models(protocol: ApiProtocol, value: &Value) -> Result<Vec<String>, String> {
    match protocol {
        ApiProtocol::OpenAiChatCompletions | ApiProtocol::OpenAiResponses => {
            openai::parse_models(value)
        }
        ApiProtocol::AnthropicMessages => anthropic::parse_models(value),
        ApiProtocol::GeminiGenerateContent => gemini::parse_models(value),
    }
}

/** 获取模型只执行一次请求，不使用重试或 Key 轮换。 */
pub async fn fetch_models(
    client: &Client,
    input: &ProviderConnectionInput,
) -> Result<Vec<String>, String> {
    let response = model_list_request(client, input)?
        .send()
        .await
        .map_err(|error| format!("Model list request failed: {error}"))?;

    if !response.status().is_success() {
        let (_, message) = response_error(response).await;
        return Err(message);
    }

    let value = read_json_response(response, "model list").await?;
    let models = parse_models(input.protocol, &value)?;

    let mut seen = HashSet::new();
    let mut unique = models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty() && seen.insert(model.clone()))
        .collect::<Vec<_>>();
    unique.sort_by_key(|model| model.to_lowercase());
    Ok(unique)
}

/**
 * 手动测试复用该协议的模型列表端点，只检查一次 HTTP 结果。
 * 不解析模型、不重试、不切换 Key，也不会在后台自动执行。
 */
pub async fn test_connection(
    client: &Client,
    input: &ProviderConnectionInput,
) -> ConnectionTestResult {
    let started_at = Instant::now();
    let request = match model_list_request(client, input) {
        Ok(request) => request,
        Err(error) => {
            return ConnectionTestResult {
                success: false,
                latency_ms: started_at.elapsed().as_millis() as u64,
                status_code: None,
                error: Some(error),
            };
        }
    };

    match request.send().await {
        Ok(response) if response.status().is_success() => ConnectionTestResult {
            success: true,
            latency_ms: started_at.elapsed().as_millis() as u64,
            status_code: Some(response.status().as_u16()),
            error: None,
        },
        Ok(response) => {
            let (status_code, error) = response_error(response).await;
            ConnectionTestResult {
                success: false,
                latency_ms: started_at.elapsed().as_millis() as u64,
                status_code: Some(status_code),
                error: Some(error),
            }
        }
        Err(error) => ConnectionTestResult {
            success: false,
            latency_ms: started_at.elapsed().as_millis() as u64,
            status_code: error.status().map(|status| status.as_u16()),
            error: Some(format!("Connection request failed: {error}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Client;

    use super::model_list_request;
    use crate::ai::types::{ApiProtocol, AuthScheme, ProviderConnectionInput};

    fn input(protocol: ApiProtocol, auth_scheme: AuthScheme) -> ProviderConnectionInput {
        ProviderConnectionInput {
            provider_id: Some("provider-test".to_string()),
            base_url: "https://example.com/v1".to_string(),
            api_key: Some("secret-key".to_string()),
            protocol,
            auth_scheme,
        }
    }

    #[test]
    fn openai_responses_uses_models_and_bearer_auth() {
        let request = model_list_request(
            &Client::new(),
            &input(ApiProtocol::OpenAiResponses, AuthScheme::ProtocolDefault),
        )
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(request.url().as_str(), "https://example.com/v1/models");
        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Bearer secret-key"
        );
    }

    #[test]
    fn anthropic_uses_version_query_and_x_api_key() {
        let request = model_list_request(
            &Client::new(),
            &input(ApiProtocol::AnthropicMessages, AuthScheme::ProtocolDefault),
        )
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(
            request.url().as_str(),
            "https://example.com/v1/models?limit=1000"
        );
        assert_eq!(request.headers().get("x-api-key").unwrap(), "secret-key");
        assert_eq!(
            request.headers().get("anthropic-version").unwrap(),
            "2023-06-01"
        );
    }

    #[test]
    fn gemini_uses_google_api_key_header() {
        let request = model_list_request(
            &Client::new(),
            &input(
                ApiProtocol::GeminiGenerateContent,
                AuthScheme::ProtocolDefault,
            ),
        )
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(request.url().as_str(), "https://example.com/v1/models");
        assert_eq!(
            request.headers().get("x-goog-api-key").unwrap(),
            "secret-key"
        );
    }

    #[test]
    fn custom_auth_scheme_overrides_protocol_default() {
        let request = model_list_request(
            &Client::new(),
            &input(ApiProtocol::AnthropicMessages, AuthScheme::Bearer),
        )
        .unwrap()
        .build()
        .unwrap();

        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Bearer secret-key"
        );
        assert!(request.headers().get("x-api-key").is_none());
    }
}
