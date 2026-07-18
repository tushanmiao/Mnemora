//! AI HTTP 公共辅助函数。
//!
//! 这里负责安全拼接端点和限制响应体大小；协议 JSON、认证方式和用量解析仍由各适配器处理。

use reqwest::{Response, Url};
use serde_json::Value;

pub const MAX_MODEL_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/** 在保留 Base URL 路径前缀的前提下追加协议端点。 */
pub fn endpoint_url(base_url: &str, path: &str) -> Result<Url, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("API Base URL is required".to_string());
    }

    let url = Url::parse(&format!("{base_url}/{}", path.trim_start_matches('/')))
        .map_err(|_| "API Base URL is invalid".to_string())?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err("API Base URL must use http or https".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("API Base URL cannot contain credentials".to_string());
    }

    Ok(url)
}

/** 提取有限长度的供应商错误，避免把完整响应写入界面或日志。 */
pub async fn response_error(response: Response) -> (u16, String) {
    let status = response.status().as_u16();
    let body = read_response_text_limited(response, 64 * 1024)
        .await
        .unwrap_or_default();
    let snippet = body.chars().take(400).collect::<String>();
    let message = if snippet.trim().is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {snippet}")
    };
    (status, message)
}

/** 分块读取响应并执行硬上限，避免异常服务返回超大正文。 */
pub async fn read_response_bytes_limited(
    mut response: Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(format!("Response body exceeds {max_bytes} bytes"));
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(max_bytes as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Failed to read response body: {error}"))?
    {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("Response body exceeds {max_bytes} bytes"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn read_response_text_limited(
    response: Response,
    max_bytes: usize,
) -> Result<String, String> {
    let body = read_response_bytes_limited(response, max_bytes).await?;
    String::from_utf8(body).map_err(|_| "Response body is not valid UTF-8".to_string())
}

pub async fn read_json_response(response: Response, label: &str) -> Result<Value, String> {
    let body = read_response_bytes_limited(response, MAX_MODEL_RESPONSE_BYTES).await?;
    serde_json::from_slice(&body).map_err(|error| format!("Invalid {label} response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::endpoint_url;

    #[test]
    fn appends_path_without_removing_version_prefix() {
        let url = endpoint_url("https://example.com/api/v1/", "/models").unwrap();
        assert_eq!(url.as_str(), "https://example.com/api/v1/models");
    }

    #[test]
    fn rejects_embedded_credentials() {
        let error = endpoint_url("https://user:secret@example.com/v1", "models").unwrap_err();
        assert_eq!(error, "API Base URL cannot contain credentials");
    }
}
