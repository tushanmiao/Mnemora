use reqwest::{Response, Url};

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
    let body = response.text().await.unwrap_or_default();
    let snippet = body.chars().take(400).collect::<String>();
    let message = if snippet.trim().is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {snippet}")
    };
    (status, message)
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
