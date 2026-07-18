//! 统一模型错误。
//!
//! 本模块把 HTTP 状态、四家错误 JSON 和 `reqwest` 错误转换为稳定分类，前端不需要解析
//! 供应商原始响应。错误正文有长度限制，并会再次替换当前 API Key，避免密钥进入界面或日志。

use std::fmt;

use reqwest::{Response, StatusCode};
use serde::Serialize;
use serde_json::Value;

use super::http::read_response_text_limited;

const MAX_PROVIDER_ERROR_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_MESSAGE_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelErrorKind {
    InvalidConfiguration,
    MissingApiKey,
    Authentication,
    PermissionDenied,
    RateLimited,
    ModelNotFound,
    ContextLengthExceeded,
    ContentFiltered,
    Timeout,
    Connection,
    InvalidResponse,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelError {
    pub kind: ModelErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

impl ModelError {
    pub fn invalid_configuration(message: impl Into<String>) -> Self {
        Self::new(ModelErrorKind::InvalidConfiguration, message)
    }

    pub fn missing_api_key() -> Self {
        Self::new(
            ModelErrorKind::MissingApiKey,
            "当前供应商尚未配置 API Key。",
        )
    }

    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self::new(ModelErrorKind::InvalidResponse, message)
    }

    pub fn content_filtered(message: impl Into<String>) -> Self {
        Self::new(ModelErrorKind::ContentFiltered, message)
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(ModelErrorKind::Provider, message)
    }

    pub fn from_reqwest(error: reqwest::Error) -> Self {
        let (kind, message) = if error.is_timeout() {
            (ModelErrorKind::Timeout, "模型请求超时，请稍后重试。")
        } else if error.is_connect() {
            (
                ModelErrorKind::Connection,
                "无法连接模型服务，请检查网络和 API Base URL。",
            )
        } else {
            (ModelErrorKind::Connection, "模型网络请求失败。")
        };

        Self::new(kind, message).with_status(error.status().map(|status| status.as_u16()))
    }

    pub async fn from_response(response: Response, api_key: &str) -> Self {
        let status = response.status();
        let retry_after_ms = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|seconds| seconds.checked_mul(1_000));
        let raw_body = read_response_text_limited(response, MAX_PROVIDER_ERROR_BYTES)
            .await
            .unwrap_or_default();
        let parsed = serde_json::from_str::<Value>(&raw_body).ok();
        let provider_code = parsed.as_ref().and_then(extract_provider_code);
        let provider_message = parsed
            .as_ref()
            .and_then(extract_provider_message)
            .or_else(|| (!raw_body.trim().is_empty()).then_some(raw_body.as_str()))
            .unwrap_or("供应商未返回错误详情");
        let provider_message = redact_and_limit(provider_message, api_key);
        let kind = classify_error(status, provider_code.as_deref(), &provider_message);
        let message = format!("模型服务返回 HTTP {}：{provider_message}", status.as_u16());

        Self {
            kind,
            message,
            status_code: Some(status.as_u16()),
            provider_code,
            retry_after_ms,
        }
    }

    fn new(kind: ModelErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status_code: None,
            provider_code: None,
            retry_after_ms: None,
        }
    }

    fn with_status(mut self, status_code: Option<u16>) -> Self {
        self.status_code = status_code;
        self
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn extract_provider_message(value: &Value) -> Option<&str> {
    value
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .or_else(|| value.get("message").and_then(Value::as_str))
}

fn extract_provider_code(value: &Value) -> Option<String> {
    let code = value
        .get("error")
        .and_then(|error| error.get("code").or_else(|| error.get("type")))
        .or_else(|| value.get("code"))
        .or_else(|| value.get("type"))?;

    code.as_str()
        .map(str::to_string)
        .or_else(|| code.as_i64().map(|number| number.to_string()))
}

fn redact_and_limit(message: &str, api_key: &str) -> String {
    let redacted = if api_key.is_empty() {
        message.to_string()
    } else {
        message.replace(api_key, "[REDACTED]")
    };
    let mut snippet = redacted
        .chars()
        .take(MAX_PROVIDER_MESSAGE_CHARS)
        .collect::<String>();
    if redacted.chars().count() > MAX_PROVIDER_MESSAGE_CHARS {
        snippet.push_str("...");
    }
    snippet
}

fn classify_error(
    status: StatusCode,
    provider_code: Option<&str>,
    message: &str,
) -> ModelErrorKind {
    match status {
        StatusCode::UNAUTHORIZED => return ModelErrorKind::Authentication,
        StatusCode::FORBIDDEN => return ModelErrorKind::PermissionDenied,
        StatusCode::TOO_MANY_REQUESTS => return ModelErrorKind::RateLimited,
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
            return ModelErrorKind::Timeout;
        }
        status if status.is_server_error() => return ModelErrorKind::Provider,
        _ => {}
    }

    let details = format!("{} {message}", provider_code.unwrap_or_default()).to_lowercase();
    if details.contains("context_length")
        || details.contains("context window")
        || details.contains("too many tokens")
    {
        ModelErrorKind::ContextLengthExceeded
    } else if details.contains("content_filter")
        || details.contains("content filter")
        || details.contains("safety")
    {
        ModelErrorKind::ContentFiltered
    } else if status == StatusCode::NOT_FOUND
        || details.contains("model_not_found")
        || details.contains("model not found")
    {
        ModelErrorKind::ModelNotFound
    } else if status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY {
        ModelErrorKind::InvalidConfiguration
    } else {
        ModelErrorKind::Provider
    }
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::{classify_error, redact_and_limit, ModelErrorKind};

    #[test]
    fn classifies_common_provider_errors() {
        assert_eq!(
            classify_error(StatusCode::UNAUTHORIZED, None, "invalid key"),
            ModelErrorKind::Authentication
        );
        assert_eq!(
            classify_error(
                StatusCode::BAD_REQUEST,
                Some("context_length_exceeded"),
                "too long"
            ),
            ModelErrorKind::ContextLengthExceeded
        );
        assert_eq!(
            classify_error(StatusCode::TOO_MANY_REQUESTS, None, "slow down"),
            ModelErrorKind::RateLimited
        );
    }

    #[test]
    fn redacts_api_key_from_provider_message() {
        assert_eq!(
            redact_and_limit("invalid key secret-value", "secret-value"),
            "invalid key [REDACTED]"
        );
    }
}
