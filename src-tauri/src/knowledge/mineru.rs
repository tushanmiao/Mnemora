//! MinerU Cloud v4 integration and untrusted-result handling.
//!
//! The knowledge module deliberately keeps the provider boundary in the
//! Tauri host.  A PDF is validated and hashed before it leaves the machine,
//! the upload uses MinerU's presigned URL flow, and the result is treated as
//! untrusted input until every ZIP entry and JSON document has been checked.
//! No API token is accepted from a persisted knowledge record or returned to
//! the WebView by this module.

use std::{
    collections::HashSet,
    fmt,
    fs::{self, File, OpenOptions},
    future::Future,
    io::{self, Cursor, Read},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures::StreamExt;
use lopdf::Document as PdfDocument;
use reqwest::{
    header::{HeaderMap, RETRY_AFTER},
    Client, Response, StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zip::ZipArchive;

/// MinerU's documented cloud v4 endpoint.  The setting stores the `/api/v4`
/// suffix so custom compatible gateways can use the same route layout.
pub const DEFAULT_MINERU_ENDPOINT: &str = "https://mineru.net/api/v4";
pub const MINERU_PROVIDER_ID: &str = "mineru-cloud";

/// Cloud limits are enforced before a request is made.  They are intentionally
/// constants rather than values supplied by the renderer or a remote response.
pub const MAX_PDF_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_PDF_PAGES: u32 = 200;
pub const MAX_UPLOAD_URL_FILES: usize = 50;

/// A single HTTP request may be slow, but must not live forever.  The complete
/// remote task has a separate deadline below.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
pub const JOB_DEADLINE: Duration = Duration::from_secs(600);
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
pub const MAX_REQUEST_ATTEMPTS: usize = 3;

/// The result URL is presigned and may contain a query string.  It is not
/// passed through the API-token-bearing request path.
pub const MAX_RESULT_ZIP_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_RESULT_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_RESULT_ASSET_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_RESULT_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_RESULT_ENTRIES: usize = 10_000;
pub const MAX_RESULT_PATH_CHARS: usize = 512;
pub const MAX_RESULT_JSON_TEXT_CHARS: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PdfTextLayerStatus {
    Present,
    Absent,
    Unknown,
}

impl PdfTextLayerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfPreflight {
    pub byte_size: u64,
    pub sha256: String,
    pub page_count: u32,
    pub text_layer: PdfTextLayerStatus,
    pub encrypted: bool,
}

impl PdfPreflight {
    pub fn has_usable_text_layer(&self) -> bool {
        self.text_layer == PdfTextLayerStatus::Present
    }
}

/// Stable error codes are persisted with a knowledge job.  The human message
/// is bounded and never contains the token or the complete remote response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MineruError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

impl MineruError {
    fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.to_string(),
            message: bound_message(&message.into()),
            retryable,
            status_code: None,
        }
    }

    fn with_status(mut self, status: StatusCode) -> Self {
        self.status_code = Some(status.as_u16());
        self
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl fmt::Display for MineruError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MineruError {}

#[derive(Debug, Clone)]
pub struct MineruConfig {
    pub endpoint: String,
    pub model: String,
    pub language: String,
    pub ocr_enabled: bool,
    pub formula_enabled: bool,
    pub table_enabled: bool,
    pub figure_enabled: bool,
    /// These two values are still capped by the provider-safe constants.
    pub request_timeout: Duration,
    pub job_deadline: Duration,
    pub poll_interval: Duration,
    pub max_attempts: usize,
}

impl Default for MineruConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_MINERU_ENDPOINT.to_string(),
            model: "vlm".to_string(),
            language: "ch".to_string(),
            ocr_enabled: true,
            formula_enabled: true,
            table_enabled: true,
            figure_enabled: true,
            request_timeout: REQUEST_TIMEOUT,
            job_deadline: JOB_DEADLINE,
            poll_interval: POLL_INTERVAL,
            max_attempts: MAX_REQUEST_ATTEMPTS,
        }
    }
}

impl MineruConfig {
    pub fn from_knowledge_settings(
        settings: &crate::settings::app_types::KnowledgeSettings,
    ) -> Self {
        let mut config = Self {
            endpoint: settings.mineru_endpoint.clone(),
            model: settings.mineru_model.clone(),
            language: settings.mineru_language.clone(),
            ocr_enabled: settings.mineru_ocr_enabled,
            formula_enabled: settings.mineru_formula_enabled,
            table_enabled: settings.mineru_table_enabled,
            figure_enabled: settings.mineru_figure_enabled,
            request_timeout: Duration::from_secs(u64::from(settings.network_timeout_seconds)),
            ..Self::default()
        };
        // A user setting may shorten the limits, never extend the provider
        // safety envelope.  Zero is treated as the documented default.
        if config.request_timeout.is_zero() {
            config.request_timeout = REQUEST_TIMEOUT;
        }
        config.request_timeout = config.request_timeout.min(REQUEST_TIMEOUT);
        config
    }

    fn validate(&self) -> Result<reqwest::Url, MineruError> {
        let endpoint = validate_api_endpoint(&self.endpoint)?;
        if self.model != "vlm" && self.model != "pipeline" {
            return Err(MineruError::new(
                "MINERU_INVALID_CONFIGURATION",
                "MinerU 解析模式无效。",
                false,
            ));
        }
        if self.language.trim().is_empty()
            || self.language.chars().count() > 32
            || !self.language.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, ',' | '+' | '-' | '_' | ' ')
            })
        {
            return Err(MineruError::new(
                "MINERU_INVALID_CONFIGURATION",
                "MinerU 语言配置无效。",
                false,
            ));
        }
        if self.request_timeout.is_zero() || self.request_timeout > REQUEST_TIMEOUT {
            return Err(MineruError::new(
                "MINERU_INVALID_CONFIGURATION",
                "MinerU 单请求超时必须在 1 到 120 秒之间。",
                false,
            ));
        }
        if self.job_deadline.is_zero() || self.job_deadline > JOB_DEADLINE {
            return Err(MineruError::new(
                "MINERU_INVALID_CONFIGURATION",
                "MinerU 任务截止时间不能超过 600 秒。",
                false,
            ));
        }
        if self.poll_interval.is_zero() || self.poll_interval > Duration::from_secs(60) {
            return Err(MineruError::new(
                "MINERU_INVALID_CONFIGURATION",
                "MinerU 轮询间隔必须在 1 到 60 秒之间。",
                false,
            ));
        }
        if !(1..=MAX_REQUEST_ATTEMPTS).contains(&self.max_attempts) {
            return Err(MineruError::new(
                "MINERU_INVALID_CONFIGURATION",
                "MinerU 网络重试次数超出安全范围。",
                false,
            ));
        }
        Ok(endpoint)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MineruProgress {
    pub stage: String,
    pub extracted_pages: Option<u32>,
    pub total_pages: Option<u32>,
    pub attempt: u32,
}

#[derive(Debug, Clone)]
pub struct MineruExtraction {
    pub batch_id: String,
    pub provider_task_id: Option<String>,
    pub file_name: String,
    pub preflight: PdfPreflight,
    pub result_zip: Vec<u8>,
    pub result_zip_sha256: String,
}

#[derive(Clone)]
pub struct MineruClient {
    client: Client,
}

impl MineruClient {
    pub fn new() -> Result<Self, MineruError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(REQUEST_TIMEOUT)
            .pool_idle_timeout(Duration::from_secs(60))
            .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| {
                MineruError::new(
                    "MINERU_CLIENT_INIT_FAILED",
                    format!("无法创建 MinerU 网络客户端：{error}"),
                    false,
                )
            })?;
        Ok(Self { client })
    }

    /// Internal constructor used by deterministic HTTP tests.  Endpoint
    /// validation remains mandatory; callers cannot use this to bypass the
    /// production HTTPS gate.
    #[cfg(test)]
    fn with_client(client: Client) -> Self {
        Self { client }
    }

    pub async fn extract_pdf(
        &self,
        pdf_path: &Path,
        original_name: &str,
        token: &str,
        config: &MineruConfig,
        cancellation: &CancellationToken,
        progress: Option<&(dyn Fn(MineruProgress) + Send + Sync)>,
    ) -> Result<MineruExtraction, MineruError> {
        let endpoint = config.validate()?;
        let token = normalize_token(token)?;
        check_cancelled(cancellation)?;
        emit_progress(progress, "validating", None, None, 0);

        let preflight = preflight_pdf(pdf_path)?;
        let (pdf_bytes, upload_hash) = read_file_with_hash(pdf_path)?;
        if upload_hash != preflight.sha256 {
            return Err(MineruError::new(
                "MINERU_SOURCE_CHANGED",
                "PDF 在上传前发生变化，已拒绝使用不一致的内容。",
                true,
            ));
        }
        check_cancelled(cancellation)?;
        let file_name = safe_upload_file_name(original_name);
        let force_ocr = config.ocr_enabled && !preflight.has_usable_text_layer();

        emit_progress(progress, "requestingUploadUrl", None, None, 0);
        let ticket = self
            .request_upload_ticket(
                &endpoint,
                &token,
                &file_name,
                config,
                force_ocr,
                cancellation,
            )
            .await?;

        check_cancelled(cancellation)?;
        emit_progress(progress, "uploading", None, None, 0);
        self.upload_pdf(&ticket.upload_url, pdf_bytes, config, cancellation)
            .await?;

        emit_progress(
            progress,
            "remotePending",
            None,
            Some(preflight.page_count),
            0,
        );
        let remote = self
            .poll_result(
                &endpoint,
                &token,
                &ticket.batch_id,
                &file_name,
                config,
                cancellation,
                progress,
            )
            .await?;

        check_cancelled(cancellation)?;
        emit_progress(
            progress,
            "downloading",
            remote.extracted_pages,
            remote.total_pages,
            0,
        );
        let result_zip = self
            .download_result(&remote.result_url, config, cancellation)
            .await?;
        // Validate the untrusted archive before returning it to a repository
        // writer.  Parsing is repeated only for the eventual normalization;
        // this early pass prevents an invalid ZIP from being persisted as a
        // successful remote revision.
        inspect_result_archive(&result_zip)?;
        let result_zip_sha256 = sha256_hex(&result_zip);
        emit_progress(
            progress,
            "done",
            remote.extracted_pages,
            remote.total_pages,
            0,
        );
        Ok(MineruExtraction {
            batch_id: ticket.batch_id,
            provider_task_id: remote.provider_task_id,
            file_name,
            preflight,
            result_zip,
            result_zip_sha256,
        })
    }

    async fn request_upload_ticket(
        &self,
        endpoint: &reqwest::Url,
        token: &str,
        file_name: &str,
        config: &MineruConfig,
        force_ocr: bool,
        cancellation: &CancellationToken,
    ) -> Result<UploadTicket, MineruError> {
        let url = api_url(endpoint, "file-urls/batch")?;
        let body = json!({
            "files": [{ "name": file_name, "is_ocr": force_ocr }],
            "language": config.language.trim(),
            "model_version": config.model,
            "enable_formula": config.formula_enabled,
            "enable_table": config.table_enabled,
        });
        let response = self
            .send_with_retries(
                || {
                    self.client
                        .post(url.clone())
                        .header("Authorization", format!("Bearer {token}"))
                        .json(&body)
                        .timeout(config.request_timeout)
                        .send()
                },
                cancellation,
                config.max_attempts,
            )
            .await?;
        let status = response.status();
        let text = read_response_limited(response, 2 * 1024 * 1024).await?;
        let value = parse_json_response(status, &text, "申请 MinerU 上传地址")?;
        ensure_business_success(status, &value, token, "申请 MinerU 上传地址")?;
        let data = value
            .get("data")
            .ok_or_else(|| invalid_response("MinerU 上传地址响应缺少 data。"))?;
        let batch_id = first_string(data, &["batch_id", "batchId"])
            .ok_or_else(|| invalid_response("MinerU 上传地址响应缺少 batch_id。"))?;
        let batch_id = validate_remote_id(&batch_id, "batch_id")?;
        let urls = data
            .get("file_urls")
            .or_else(|| data.get("fileUrls"))
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response("MinerU 上传地址响应缺少 file_urls。"))?;
        if urls.len() != 1 || urls.len() > MAX_UPLOAD_URL_FILES {
            return Err(invalid_response("MinerU 上传地址数量与文件数量不一致。"));
        }
        let upload_url = urls
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("MinerU 上传地址无效。"))?;
        let upload_url = validate_presigned_url(upload_url, "上传地址")?;
        Ok(UploadTicket {
            batch_id,
            upload_url,
        })
    }

    async fn upload_pdf(
        &self,
        upload_url: &str,
        pdf_bytes: Vec<u8>,
        config: &MineruConfig,
        cancellation: &CancellationToken,
    ) -> Result<(), MineruError> {
        // The signature of the presigned URL may not cover Content-Type.  Do
        // not add one here; this is an intentional part of the MinerU v4
        // contract.
        let response = self
            .send_with_retries(
                || {
                    self.client
                        .put(upload_url)
                        .body(pdf_bytes.clone())
                        .timeout(config.request_timeout)
                        .send()
                },
                cancellation,
                config.max_attempts.min(2),
            )
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = read_response_limited(response, 64 * 1024).await?;
            return Err(http_error(status, &text, "上传 PDF 到 MinerU", ""));
        }
        Ok(())
    }

    async fn poll_result(
        &self,
        endpoint: &reqwest::Url,
        token: &str,
        batch_id: &str,
        file_name: &str,
        config: &MineruConfig,
        cancellation: &CancellationToken,
        progress: Option<&(dyn Fn(MineruProgress) + Send + Sync)>,
    ) -> Result<RemoteResult, MineruError> {
        let url = api_url(endpoint, &format!("extract-results/batch/{batch_id}"))?;
        let started = Instant::now();
        let mut first_poll = true;
        loop {
            check_cancelled(cancellation)?;
            if started.elapsed() >= config.job_deadline {
                return Err(MineruError::new(
                    "MINERU_TIMEOUT",
                    "MinerU 云端解析任务超过 600 秒截止时间。",
                    true,
                ));
            }
            if !first_poll {
                wait_with_cancellation(config.poll_interval, cancellation).await?;
            }
            first_poll = false;
            let response = self
                .send_with_retries(
                    || {
                        self.client
                            .get(url.clone())
                            .header("Authorization", format!("Bearer {token}"))
                            .timeout(config.request_timeout)
                            .send()
                    },
                    cancellation,
                    config.max_attempts,
                )
                .await?;
            let status = response.status();
            let text = read_response_limited(response, 4 * 1024 * 1024).await?;
            let value = parse_json_response(status, &text, "查询 MinerU 解析任务")?;
            ensure_business_success(status, &value, token, "查询 MinerU 解析任务")?;
            let result = find_extract_result(&value, file_name);
            let Some(result) = result else {
                emit_progress(progress, "remotePending", None, None, 0);
                continue;
            };
            let state = first_string(&result, &["state", "status"])
                .unwrap_or_else(|| "unknown".to_string())
                .to_ascii_lowercase();
            let (extracted_pages, total_pages) = extract_progress(&result);
            emit_progress(progress, &state, extracted_pages, total_pages, 0);
            if matches!(state.as_str(), "done" | "success" | "completed") {
                let result_url = first_string(
                    &result,
                    &[
                        "full_zip_url",
                        "fullZipUrl",
                        "zip_url",
                        "zipUrl",
                        "result_url",
                    ],
                )
                .ok_or_else(|| invalid_response("MinerU 任务已完成但没有返回结果 ZIP 地址。"))?;
                let result_url = validate_presigned_url(&result_url, "结果地址")?;
                let provider_task_id =
                    first_string(&result, &["task_id", "taskId", "extract_id", "extractId"])
                        .and_then(|value| validate_remote_id(&value, "task_id").ok());
                return Ok(RemoteResult {
                    result_url,
                    provider_task_id,
                    extracted_pages,
                    total_pages,
                });
            }
            if matches!(
                state.as_str(),
                "failed" | "error" | "cancelled" | "canceled"
            ) {
                let message =
                    first_string(&result, &["err_msg", "errMsg", "error", "message", "msg"])
                        .unwrap_or_else(|| "MinerU 云端解析失败。".to_string());
                let code = if matches!(state.as_str(), "cancelled" | "canceled") {
                    "MINERU_REMOTE_CANCELLED"
                } else {
                    "MINERU_REMOTE_FAILED"
                };
                return Err(MineruError::new(
                    code,
                    sanitize_remote_message(&message, token),
                    false,
                ));
            }
        }
    }

    async fn download_result(
        &self,
        result_url: &str,
        config: &MineruConfig,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, MineruError> {
        let response = self
            .send_with_retries(
                || {
                    // A presigned object-store URL must not receive the
                    // MinerU bearer token.
                    self.client
                        .get(result_url)
                        .timeout(config.request_timeout)
                        .send()
                },
                cancellation,
                config.max_attempts,
            )
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = read_response_limited(response, 64 * 1024).await?;
            return Err(http_error(status, &text, "下载 MinerU 结果", ""));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESULT_ZIP_BYTES)
        {
            return Err(MineruError::new(
                "MINERU_RESULT_TOO_LARGE",
                "MinerU 结果 ZIP 超过本地安全上限。",
                false,
            ));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(item) = stream.next().await {
            check_cancelled(cancellation)?;
            let item = item.map_err(|error| {
                let code = if error.is_timeout() {
                    "MINERU_TIMEOUT"
                } else {
                    "MINERU_CONNECTION"
                };
                MineruError::new(code, "读取 MinerU 结果失败。", true)
            })?;
            if bytes.len() as u64 + item.len() as u64 > MAX_RESULT_ZIP_BYTES {
                return Err(MineruError::new(
                    "MINERU_RESULT_TOO_LARGE",
                    "MinerU 结果 ZIP 超过本地安全上限。",
                    false,
                ));
            }
            bytes.extend_from_slice(&item);
        }
        if bytes.is_empty() {
            return Err(invalid_response("MinerU 返回了空的结果 ZIP。"));
        }
        Ok(bytes)
    }

    async fn send_with_retries<F, Fut>(
        &self,
        mut make_request: F,
        cancellation: &CancellationToken,
        max_attempts: usize,
    ) -> Result<Response, MineruError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<Response, reqwest::Error>>,
    {
        let attempts = max_attempts.clamp(1, MAX_REQUEST_ATTEMPTS);
        let mut last_error = None;
        for attempt in 0..attempts {
            check_cancelled(cancellation)?;
            match make_request().await {
                Ok(response) if is_retryable_status(response.status()) => {
                    let status = response.status();
                    let delay = retry_delay(response.headers(), attempt);
                    if attempt + 1 < attempts {
                        drop(response);
                        wait_with_cancellation(delay, cancellation).await?;
                        continue;
                    }
                    let error = http_error(status, "", "MinerU 网络请求", "");
                    last_error = Some(error);
                }
                Ok(response) => return Ok(response),
                Err(error) => {
                    let mapped = map_transport_error(&error);
                    if mapped.retryable && attempt + 1 < attempts {
                        wait_with_cancellation(backoff_delay(attempt), cancellation).await?;
                        continue;
                    }
                    last_error = Some(mapped);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            MineruError::new("MINERU_CONNECTION", "MinerU 网络请求失败。", true)
        }))
    }
}

#[derive(Debug, Clone)]
struct UploadTicket {
    batch_id: String,
    upload_url: String,
}

#[derive(Debug, Clone)]
struct RemoteResult {
    result_url: String,
    provider_task_id: Option<String>,
    extracted_pages: Option<u32>,
    total_pages: Option<u32>,
}

fn emit_progress(
    callback: Option<&(dyn Fn(MineruProgress) + Send + Sync)>,
    stage: &str,
    extracted_pages: Option<u32>,
    total_pages: Option<u32>,
    attempt: u32,
) {
    if let Some(callback) = callback {
        callback(MineruProgress {
            stage: stage.to_string(),
            extracted_pages,
            total_pages,
            attempt,
        });
    }
}

fn normalize_token(token: &str) -> Result<String, MineruError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(MineruError::new(
            "MINERU_TOKEN_MISSING",
            "尚未配置 MinerU Cloud Token。",
            false,
        ));
    }
    if token.len() > 16_384 || token.chars().any(char::is_control) {
        return Err(MineruError::new(
            "MINERU_TOKEN_INVALID",
            "MinerU Cloud Token 无效。",
            false,
        ));
    }
    Ok(token.to_string())
}

fn validate_api_endpoint(value: &str) -> Result<reqwest::Url, MineruError> {
    let value = value.trim();
    let mut url = reqwest::Url::parse(value)
        .map_err(|_| MineruError::new("MINERU_ENDPOINT_INVALID", "MinerU API 地址无效。", false))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(MineruError::new(
            "MINERU_ENDPOINT_INVALID",
            "MinerU API 地址必须是无凭据、无查询参数的 HTTPS 地址。",
            false,
        ));
    }
    let path = url.path().trim_end_matches('/').to_string();
    if path.is_empty() {
        url.set_path("/api/v4");
    } else {
        if path.split('/').any(|part| part == "..") {
            return Err(MineruError::new(
                "MINERU_ENDPOINT_INVALID",
                "MinerU API 地址不能包含路径回退。",
                false,
            ));
        }
        url.set_path(&path);
    }
    Ok(url)
}

fn api_url(endpoint: &reqwest::Url, route: &str) -> Result<reqwest::Url, MineruError> {
    let route = route.trim_start_matches('/');
    if route.is_empty() || route.contains("..") || route.chars().any(char::is_control) {
        return Err(MineruError::new(
            "MINERU_ENDPOINT_INVALID",
            "MinerU API 路径无效。",
            false,
        ));
    }
    endpoint.join(route).map_err(|_| {
        MineruError::new(
            "MINERU_ENDPOINT_INVALID",
            "MinerU API 路径无法构造。",
            false,
        )
    })
}

fn validate_presigned_url(value: &str, label: &str) -> Result<String, MineruError> {
    let url = reqwest::Url::parse(value.trim()).map_err(|_| {
        MineruError::new(
            "MINERU_RESULT_URL_INVALID",
            format!("MinerU {label}无效。"),
            false,
        )
    })?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(MineruError::new(
            "MINERU_RESULT_URL_INVALID",
            format!("MinerU {label}必须使用无凭据 HTTPS 地址。"),
            false,
        ));
    }
    Ok(url.to_string())
}

fn safe_upload_file_name(value: &str) -> String {
    let candidate = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("document.pdf")
        .trim();
    let mut result = candidate
        .chars()
        .filter(|character| !character.is_control() && *character != '/' && *character != '\\')
        .take(200)
        .collect::<String>();
    if result.is_empty() {
        result = "document.pdf".to_string();
    }
    if !result.to_ascii_lowercase().ends_with(".pdf") {
        result.push_str(".pdf");
    }
    result
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), MineruError> {
    if cancellation.is_cancelled() {
        Err(MineruError::new(
            "MINERU_CANCELLED",
            "MinerU 解析任务已取消。",
            false,
        ))
    } else {
        Ok(())
    }
}

async fn wait_with_cancellation(
    duration: Duration,
    cancellation: &CancellationToken,
) -> Result<(), MineruError> {
    tokio::select! {
        _ = cancellation.cancelled() => Err(MineruError::new("MINERU_CANCELLED", "MinerU 解析任务已取消。", false)),
        _ = tokio::time::sleep(duration) => Ok(()),
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::TOO_EARLY
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    ) || status.as_u16() == 524
}

fn retry_delay(headers: &HeaderMap, attempt: usize) -> Duration {
    let retry_after = headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds.min(30)));
    retry_after.unwrap_or_else(|| backoff_delay(attempt))
}

fn backoff_delay(attempt: usize) -> Duration {
    let multiplier = 1u64 << attempt.min(4);
    Duration::from_millis(500u64.saturating_mul(multiplier).min(10_000))
}

fn map_transport_error(error: &reqwest::Error) -> MineruError {
    if error.is_timeout() {
        MineruError::new("MINERU_TIMEOUT", "MinerU 网络请求超时。", true)
    } else if error.is_connect() {
        MineruError::new("MINERU_CONNECTION", "无法连接 MinerU 云服务。", true)
    } else {
        MineruError::new("MINERU_NETWORK", "MinerU 网络请求失败。", true)
    }
}

async fn read_response_limited(response: Response, limit: u64) -> Result<String, MineruError> {
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(item) = stream.next().await {
        let item = item.map_err(|error| map_transport_error(&error))?;
        if bytes.len() as u64 + item.len() as u64 > limit {
            return Err(MineruError::new(
                "MINERU_RESPONSE_TOO_LARGE",
                "MinerU 响应超过本地安全上限。",
                false,
            ));
        }
        bytes.extend_from_slice(&item);
    }
    String::from_utf8(bytes).map_err(|_| invalid_response("MinerU 响应不是合法 UTF-8。"))
}

fn parse_json_response(
    status: StatusCode,
    text: &str,
    context: &str,
) -> Result<Value, MineruError> {
    serde_json::from_str(text).map_err(|_| {
        if status.is_success() {
            invalid_response(format!("{context}返回了无法解析的 JSON。"))
        } else {
            http_error(status, text, context, "")
        }
    })
}

fn ensure_business_success(
    status: StatusCode,
    value: &Value,
    token: &str,
    context: &str,
) -> Result<(), MineruError> {
    if !status.is_success() {
        return Err(http_error(status, &value.to_string(), context, token));
    }
    let code = response_code(value);
    let success = value.get("success").and_then(Value::as_bool);
    if code.as_deref().is_some_and(|code| code != "0") || success == Some(false) {
        let provider_code = code.unwrap_or_else(|| "unknown".to_string());
        let message = first_string(value, &["msg", "message", "error"])
            .unwrap_or_else(|| "MinerU 业务请求失败。".to_string());
        return Err(business_error(
            &provider_code,
            &message,
            status,
            token,
            context,
        ));
    }
    Ok(())
}

fn response_code(value: &Value) -> Option<String> {
    for key in ["code", "msgCode", "msg_code"] {
        if let Some(value) = value.get(key) {
            if let Some(text) = value.as_str() {
                return Some(text.trim().to_string());
            }
            if let Some(number) = value.as_i64() {
                return Some(number.to_string());
            }
            if let Some(number) = value.as_u64() {
                return Some(number.to_string());
            }
        }
    }
    None
}

fn business_error(
    code: &str,
    message: &str,
    status: StatusCode,
    token: &str,
    context: &str,
) -> MineruError {
    let normalized = message.to_ascii_lowercase();
    let (mapped, retryable) = if matches!(code, "A0202" | "A0211")
        || status == StatusCode::UNAUTHORIZED
        || normalized.contains("token") && normalized.contains("invalid")
        || normalized.contains("token") && normalized.contains("expired")
    {
        ("MINERU_AUTH", false)
    } else if status == StatusCode::TOO_MANY_REQUESTS
        || normalized.contains("rate limit")
        || normalized.contains("频率")
    {
        ("MINERU_RATE_LIMITED", true)
    } else if status == StatusCode::PAYMENT_REQUIRED
        || normalized.contains("quota")
        || normalized.contains("balance")
        || normalized.contains("额度")
        || normalized.contains("余额")
    {
        ("MINERU_QUOTA_EXCEEDED", false)
    } else if is_retryable_status(status) {
        ("MINERU_REMOTE_UNAVAILABLE", true)
    } else {
        ("MINERU_REMOTE_ERROR", false)
    };
    let safe_message = sanitize_remote_message(message, token);
    MineruError::new(
        mapped,
        format!("{context}失败（{code}）：{safe_message}"),
        retryable,
    )
    .with_status(status)
}

fn http_error(status: StatusCode, body: &str, context: &str, token: &str) -> MineruError {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let code = parsed
        .as_ref()
        .and_then(response_code)
        .unwrap_or_else(|| status.as_u16().to_string());
    let message = parsed
        .as_ref()
        .and_then(|value| first_string(value, &["msg", "message", "error"]))
        .unwrap_or_else(|| "远端服务未提供错误详情。".to_string());
    if !status.is_success() {
        return business_error(&code, &message, status, token, context);
    }
    MineruError::new(
        "MINERU_REMOTE_ERROR",
        format!("{context}失败：{message}"),
        false,
    )
}

fn sanitize_remote_message(message: &str, token: &str) -> String {
    let mut result = message.replace('\n', " ").replace('\r', " ");
    if !token.is_empty() {
        result = result.replace(token, "[REDACTED]");
    }
    // Avoid accidentally persisting a bearer credential if a proxy echoes the
    // Authorization header in its diagnostic text.
    if let Some(index) = result.to_ascii_lowercase().find("bearer ") {
        let end = result[index..]
            .find(char::is_whitespace)
            .map(|offset| index + offset)
            .unwrap_or(result.len());
        result.replace_range(index..end, "Bearer [REDACTED]");
    }
    result
}

fn bound_message(value: &str) -> String {
    let value = value.trim();
    let mut result = value.chars().take(1_000).collect::<String>();
    if value.chars().count() > 1_000 {
        result.push_str("...");
    }
    result
}

fn invalid_response(message: impl Into<String>) -> MineruError {
    MineruError::new("MINERU_INVALID_RESPONSE", message, false)
}

fn validate_remote_id(value: &str, label: &str) -> Result<String, MineruError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(MineruError::new(
            "MINERU_INVALID_RESPONSE",
            format!("MinerU 响应中的 {label} 无效。"),
            false,
        ));
    }
    Ok(value.to_string())
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn find_extract_result(value: &Value, file_name: &str) -> Option<Value> {
    let list = value
        .get("data")
        .and_then(|data| {
            data.get("extract_result")
                .or_else(|| data.get("extractResult"))
        })
        .and_then(Value::as_array)
        .or_else(|| value.get("extract_result").and_then(Value::as_array))?;
    list.iter()
        .find(|item| first_string(item, &["file_name", "fileName"]).as_deref() == Some(file_name))
        .cloned()
        .or_else(|| list.first().cloned())
}

fn extract_progress(value: &Value) -> (Option<u32>, Option<u32>) {
    let progress = value
        .get("extract_progress")
        .or_else(|| value.get("extractProgress"))
        .unwrap_or(value);
    let extracted = first_u32(progress, &["extracted_pages", "extractedPages"]);
    let total = first_u32(progress, &["total_pages", "totalPages"]);
    (extracted, total)
}

fn first_u32(value: &Value, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_u64)
            .and_then(|number| u32::try_from(number).ok())
    })
}

/// Inspect a PDF without uploading it.  The parser is used only for page and
/// text-layer metadata; MinerU remains the authoritative layout extractor.
pub fn preflight_pdf(path: &Path) -> Result<PdfPreflight, MineruError> {
    let metadata = fs::metadata(path).map_err(|error| {
        MineruError::new(
            "MINERU_PDF_READ_FAILED",
            format!("读取 PDF 文件失败：{error}"),
            false,
        )
    })?;
    if !metadata.is_file() {
        return Err(MineruError::new(
            "MINERU_PDF_INVALID",
            "PDF 路径不是普通文件。",
            false,
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_PDF_BYTES {
        return Err(MineruError::new(
            "MINERU_PDF_TOO_LARGE",
            "PDF 必须大于 0 且不超过 200 MB。",
            false,
        ));
    }
    let mut header = [0u8; 5];
    let mut file = File::open(path).map_err(|error| {
        MineruError::new(
            "MINERU_PDF_READ_FAILED",
            format!("打开 PDF 文件失败：{error}"),
            false,
        )
    })?;
    file.read_exact(&mut header).map_err(|_| {
        MineruError::new(
            "MINERU_PDF_INVALID",
            "PDF 文件缺少有效的 %PDF- 文件签名。",
            false,
        )
    })?;
    if &header != b"%PDF-" {
        return Err(MineruError::new(
            "MINERU_PDF_INVALID",
            "文件不是有效的 PDF。",
            false,
        ));
    }
    drop(file);

    let (bytes, sha256) = read_file_with_hash(path)?;
    let document = PdfDocument::load_mem(&bytes).map_err(|error| {
        MineruError::new(
            "MINERU_PDF_INVALID",
            format!("PDF 结构无法读取：{error}"),
            false,
        )
    })?;
    let pages = document.get_pages();
    let page_count = u32::try_from(pages.len())
        .map_err(|_| MineruError::new("MINERU_PDF_INVALID", "PDF 页数超出支持范围。", false))?;
    if page_count == 0 {
        return Err(MineruError::new(
            "MINERU_PDF_INVALID",
            "PDF 不包含可读取的页面。",
            false,
        ));
    }
    if page_count > MAX_PDF_PAGES {
        return Err(MineruError::new(
            "MINERU_PDF_TOO_MANY_PAGES",
            format!("单个 MinerU 任务最多支持 {MAX_PDF_PAGES} 页，当前 PDF 有 {page_count} 页。"),
            false,
        ));
    }
    let encrypted = document.is_encrypted();
    let text_layer = detect_text_layer(&document, &pages);
    Ok(PdfPreflight {
        byte_size: metadata.len(),
        sha256,
        page_count,
        text_layer,
        encrypted,
    })
}

/// A deliberately small local fallback.  It only exposes text that `lopdf`
/// can actually extract; it never labels a scanned page as OCR text and never
/// fabricates figures, tables, formulas, or captions.
#[derive(Debug, Clone)]
pub struct LocalPdfPage {
    pub page_index: u32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct LocalPdfExtraction {
    pub preflight: PdfPreflight,
    pub full_markdown: String,
    pub pages: Vec<LocalPdfPage>,
    pub warnings: Vec<String>,
    pub quality_flags: Vec<String>,
}

pub fn extract_local_text_pdf(path: &Path) -> Result<LocalPdfExtraction, MineruError> {
    let preflight = preflight_pdf(path)?;
    let (bytes, current_hash) = read_file_with_hash(path)?;
    if current_hash != preflight.sha256 {
        return Err(MineruError::new(
            "MINERU_SOURCE_CHANGED",
            "PDF 在本地解析前发生变化，已拒绝使用不一致的内容。",
            true,
        ));
    }
    let document = PdfDocument::load_mem(&bytes).map_err(|error| {
        MineruError::new(
            "MINERU_LOCAL_PDF_READ_FAILED",
            format!("无法读取本地 PDF：{error}"),
            false,
        )
    })?;
    let page_numbers = document.get_pages().keys().copied().collect::<Vec<_>>();
    let mut pages = Vec::with_capacity(page_numbers.len());
    let mut warnings = Vec::new();
    let mut full_markdown = String::new();
    let mut saw_text = false;
    for (page_index, page_number) in page_numbers.into_iter().enumerate() {
        let page_index = u32::try_from(page_index).unwrap_or(u32::MAX);
        let text = match document.extract_text(&[page_number]) {
            Ok(text) => normalize_local_page_text(&text),
            Err(_) => {
                warnings.push(format!("LOCAL_PDF_PAGE_TEXT_FAILED:{page_index}"));
                String::new()
            }
        };
        if text.is_empty() {
            warnings.push(format!("LOCAL_PDF_PAGE_TEXT_MISSING:{page_index}"));
        } else {
            saw_text = true;
        }
        if !full_markdown.is_empty() {
            full_markdown.push_str("\n\n");
        }
        full_markdown.push_str(&format!(
            "<!-- page {} -->\n\n",
            page_index.saturating_add(1)
        ));
        full_markdown.push_str(&text);
        pages.push(LocalPdfPage { page_index, text });
        if full_markdown.chars().count() > MAX_RESULT_JSON_TEXT_CHARS.saturating_mul(4) {
            return Err(MineruError::new(
                "MINERU_LOCAL_TEXT_TOO_LARGE",
                "本地 PDF 文本超过安全上限。",
                false,
            ));
        }
    }
    if !saw_text {
        return Err(MineruError::new(
            "MINERU_LOCAL_TEXT_UNAVAILABLE",
            "PDF 没有可提取的文本层；本地 fallback 不提供 OCR。",
            false,
        ));
    }
    let mut quality_flags = vec!["textOnly".to_string(), "ocrUnavailable".to_string()];
    if !warnings.is_empty() {
        quality_flags.push("partialPages".to_string());
    }
    Ok(LocalPdfExtraction {
        preflight,
        full_markdown,
        pages,
        warnings,
        quality_flags,
    })
}

fn normalize_local_page_text(value: &str) -> String {
    value
        .replace('\0', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn detect_text_layer(
    document: &PdfDocument,
    pages: &std::collections::BTreeMap<u32, (u32, u16)>,
) -> PdfTextLayerStatus {
    if document.is_encrypted() {
        return PdfTextLayerStatus::Unknown;
    }
    let page_numbers = pages.keys().take(3).copied().collect::<Vec<_>>();
    if page_numbers.is_empty() {
        return PdfTextLayerStatus::Unknown;
    }
    let mut saw_text_operator = false;
    let mut extraction_error = false;
    for page in page_numbers {
        match document.extract_text(&[page]) {
            Ok(text) if !text.trim().is_empty() => return PdfTextLayerStatus::Present,
            Ok(_) => {}
            Err(_) => extraction_error = true,
        }
        if let Some((object_id, _)) = pages.get(&page) {
            if document
                .get_and_decode_page_content((*object_id, 0))
                .ok()
                .is_some_and(|content| {
                    content.operations.iter().any(|operation| {
                        matches!(operation.operator.as_str(), "Tj" | "TJ" | "'" | "\"")
                    })
                })
            {
                saw_text_operator = true;
            }
        }
    }
    if saw_text_operator {
        PdfTextLayerStatus::Present
    } else if extraction_error {
        PdfTextLayerStatus::Unknown
    } else {
        PdfTextLayerStatus::Absent
    }
}

fn read_file_with_hash(path: &Path) -> Result<(Vec<u8>, String), MineruError> {
    let mut file = File::open(path).map_err(|error| {
        MineruError::new(
            "MINERU_PDF_READ_FAILED",
            format!("读取 PDF 文件失败：{error}"),
            false,
        )
    })?;
    let mut bytes = Vec::new();
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            MineruError::new(
                "MINERU_PDF_READ_FAILED",
                format!("读取 PDF 文件失败：{error}"),
                false,
            )
        })?;
        if read == 0 {
            break;
        }
        if bytes.len() as u64 + read as u64 > MAX_PDF_BYTES {
            return Err(MineruError::new(
                "MINERU_PDF_TOO_LARGE",
                "PDF 超过 200 MB 安全上限。",
                false,
            ));
        }
        hasher.update(&buffer[..read]);
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok((bytes, format!("{:x}", hasher.finalize())))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultArchiveEntry {
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub directory: bool,
}

#[derive(Debug, Clone)]
pub struct ResultArchiveManifest {
    pub entries: Vec<ResultArchiveEntry>,
    pub full_markdown_entry: String,
    pub content_list_entry: String,
    pub layout_entry: String,
    pub full_markdown: String,
    pub content_list: Value,
    pub layout: Value,
    pub sha256: String,
}

/// Validate the complete ZIP and parse the three required MinerU artifacts.
/// Every entry is inspected, not only the files we currently understand, so a
/// later asset consumer cannot accidentally reintroduce path traversal.
pub fn inspect_result_archive(bytes: &[u8]) -> Result<ResultArchiveManifest, MineruError> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_RESULT_ZIP_BYTES {
        return Err(MineruError::new(
            "MINERU_RESULT_TOO_LARGE",
            "MinerU 结果 ZIP 为空或超过安全上限。",
            false,
        ));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        MineruError::new(
            "MINERU_ARCHIVE_INVALID",
            format!("无法打开 MinerU 结果 ZIP：{error}"),
            false,
        )
    })?;
    if archive.len() > MAX_RESULT_ENTRIES {
        return Err(MineruError::new(
            "MINERU_ARCHIVE_TOO_MANY_ENTRIES",
            "MinerU 结果 ZIP 包含过多条目。",
            false,
        ));
    }
    let mut names = HashSet::new();
    let mut entries = Vec::with_capacity(archive.len());
    let mut total_uncompressed = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            MineruError::new(
                "MINERU_ARCHIVE_INVALID",
                format!("读取 MinerU ZIP 条目失败：{error}"),
                false,
            )
        })?;
        let name = validate_zip_entry_name(entry.name(), entry.is_dir())?;
        if !names.insert(name.clone()) {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_DUPLICATE_ENTRY",
                format!("MinerU 结果 ZIP 存在重复条目：{name}"),
                false,
            ));
        }
        if entry.encrypted() {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_ENCRYPTED",
                "不支持加密的 MinerU 结果 ZIP 条目。",
                false,
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_SYMLINK",
                "不允许 MinerU 结果 ZIP 包含符号链接。",
                false,
            ));
        }
        let uncompressed_size = entry.size();
        let max_entry = if entry.is_dir() {
            0
        } else if is_json_or_markdown_name(&name) {
            MAX_RESULT_ENTRY_BYTES
        } else {
            MAX_RESULT_ASSET_BYTES
        };
        if !entry.is_dir() && uncompressed_size > max_entry {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_ENTRY_TOO_LARGE",
                format!("MinerU ZIP 条目超过安全上限：{name}"),
                false,
            ));
        }
        total_uncompressed = total_uncompressed
            .checked_add(uncompressed_size)
            .ok_or_else(|| invalid_response("MinerU ZIP 解压总量溢出。"))?;
        if total_uncompressed > MAX_RESULT_UNCOMPRESSED_BYTES {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_BOMB",
                "MinerU 结果 ZIP 的解压总量超过安全上限。",
                false,
            ));
        }
        entries.push(ResultArchiveEntry {
            name,
            compressed_size: entry.compressed_size(),
            uncompressed_size,
            directory: entry.is_dir(),
        });
    }

    let full_markdown_entry = select_required_entry(&names, &["full.md"])?;
    let content_list_entry = select_required_entry(&names, &["content_list.json"])?;
    let layout_entry = select_required_entry(&names, &["layout.json", "middle.json"])?;
    let full_markdown_bytes =
        read_zip_entry(&mut archive, &full_markdown_entry, MAX_RESULT_ENTRY_BYTES)?;
    let full_markdown = String::from_utf8(full_markdown_bytes)
        .map_err(|_| invalid_response("MinerU full.md 不是合法 UTF-8。"))?;
    let content_list_bytes =
        read_zip_entry(&mut archive, &content_list_entry, MAX_RESULT_ENTRY_BYTES)?;
    let content_list: Value = serde_json::from_slice(&content_list_bytes)
        .map_err(|_| invalid_response("MinerU content_list.json 不是合法 JSON。"))?;
    if !content_list.is_array() {
        return Err(invalid_response("MinerU content_list.json 必须是数组。"));
    }
    let layout_bytes = read_zip_entry(&mut archive, &layout_entry, MAX_RESULT_ENTRY_BYTES)?;
    let layout: Value = serde_json::from_slice(&layout_bytes)
        .map_err(|_| invalid_response("MinerU layout.json/middle.json 不是合法 JSON。"))?;
    validate_layout_shape(&layout)?;
    Ok(ResultArchiveManifest {
        entries,
        full_markdown_entry,
        content_list_entry,
        layout_entry,
        full_markdown,
        content_list,
        layout,
        sha256: sha256_hex(bytes),
    })
}

fn is_json_or_markdown_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".json")
        || name.to_ascii_lowercase().ends_with(".md")
        || name.to_ascii_lowercase().ends_with(".txt")
}

fn validate_zip_entry_name(raw: &str, directory: bool) -> Result<String, MineruError> {
    if raw.is_empty()
        || raw.chars().count() > MAX_RESULT_PATH_CHARS
        || raw.chars().any(char::is_control)
        || raw.contains('\\')
        || raw.starts_with('/')
        || raw.starts_with("//")
        || raw.starts_with("\\\\")
        || raw.as_bytes().get(1).is_some_and(|byte| *byte == b':')
    {
        return Err(MineruError::new(
            "MINERU_ARCHIVE_PATH_UNSAFE",
            format!("MinerU ZIP 条目路径不安全：{raw}"),
            false,
        ));
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        if part.is_empty() {
            continue;
        }
        if part == "." || part == ".." || part.contains(':') {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_PATH_UNSAFE",
                format!("MinerU ZIP 条目路径不安全：{raw}"),
                false,
            ));
        }
        parts.push(part);
    }
    if parts.is_empty() || (!directory && raw.ends_with('/')) {
        return Err(MineruError::new(
            "MINERU_ARCHIVE_PATH_UNSAFE",
            format!("MinerU ZIP 条目路径不安全：{raw}"),
            false,
        ));
    }
    Ok(parts.join("/"))
}

fn select_required_entry(
    names: &HashSet<String>,
    candidates: &[&str],
) -> Result<String, MineruError> {
    for candidate in candidates {
        let exact = names
            .iter()
            .filter(|name| *name == candidate)
            .cloned()
            .collect::<Vec<_>>();
        if exact.len() == 1 {
            return Ok(exact[0].clone());
        }
    }
    for candidate in candidates {
        let matches = names
            .iter()
            .filter(|name| {
                let leaf = name.rsplit('/').next().unwrap_or(name);
                leaf == *candidate
                    || leaf
                        .strip_suffix(candidate)
                        .is_some_and(|prefix| prefix.ends_with('_'))
            })
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            return Ok(matches[0].clone());
        }
        if matches.len() > 1 {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_AMBIGUOUS_ENTRY",
                format!("MinerU ZIP 中存在多个候选条目：{candidate}"),
                false,
            ));
        }
    }
    Err(MineruError::new(
        "MINERU_ARCHIVE_REQUIRED_ENTRY_MISSING",
        format!("MinerU 结果 ZIP 缺少必要条目：{}。", candidates.join(" / ")),
        false,
    ))
}

fn read_zip_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, MineruError> {
    let mut entry = archive.by_name(name).map_err(|error| {
        MineruError::new(
            "MINERU_ARCHIVE_REQUIRED_ENTRY_MISSING",
            format!("读取 MinerU ZIP 条目失败：{error}"),
            false,
        )
    })?;
    if entry.size() > limit {
        return Err(MineruError::new(
            "MINERU_ARCHIVE_ENTRY_TOO_LARGE",
            format!("MinerU ZIP 条目超过安全上限：{name}"),
            false,
        ));
    }
    let capacity = usize::try_from(entry.size().min(limit)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    let mut limited = entry.by_ref().take(limit.saturating_add(1));
    limited.read_to_end(&mut bytes).map_err(|error| {
        MineruError::new(
            "MINERU_ARCHIVE_READ_FAILED",
            format!("读取 MinerU ZIP 条目失败：{error}"),
            false,
        )
    })?;
    if bytes.len() as u64 > limit {
        return Err(MineruError::new(
            "MINERU_ARCHIVE_ENTRY_TOO_LARGE",
            format!("MinerU ZIP 条目超过安全上限：{name}"),
            false,
        ));
    }
    Ok(bytes)
}

fn validate_layout_shape(layout: &Value) -> Result<(), MineruError> {
    let pages = layout
        .get("pdf_info")
        .or_else(|| layout.get("pdfInfo"))
        .or_else(|| layout.get("pages"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_response("MinerU layout 缺少 pdf_info/pages 数组。"))?;
    if pages.is_empty() {
        return Err(invalid_response("MinerU layout 不包含页面信息。"));
    }
    for page in pages {
        let size = page
            .get("page_size")
            .or_else(|| page.get("pageSize"))
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response("MinerU layout 页面缺少 page_size。"))?;
        if size.len() != 2
            || !size.iter().all(|value| {
                value
                    .as_f64()
                    .is_some_and(|number| number.is_finite() && number > 0.0 && number <= 100_000.0)
            })
        {
            return Err(invalid_response("MinerU layout 页面尺寸无效。"));
        }
    }
    Ok(())
}

/// Extract a validated result into a newly-created directory.  The operation
/// is staged beside the destination and committed with one rename, so a crash
/// cannot leave a revision pointing at half an archive.  Existing destinations
/// are never overwritten.
pub fn extract_result_archive_atomic(
    bytes: &[u8],
    destination: &Path,
) -> Result<ResultArchiveManifest, MineruError> {
    let manifest = inspect_result_archive(bytes)?;
    let parent = destination.parent().ok_or_else(|| {
        MineruError::new(
            "MINERU_ARCHIVE_DESTINATION_INVALID",
            "结果归档目录没有父目录。",
            false,
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        MineruError::new(
            "MINERU_ARCHIVE_DESTINATION_INVALID",
            format!("创建结果归档父目录失败：{error}"),
            false,
        )
    })?;
    let parent = fs::canonicalize(parent).map_err(|error| {
        MineruError::new(
            "MINERU_ARCHIVE_DESTINATION_INVALID",
            format!("校验结果归档父目录失败：{error}"),
            false,
        )
    })?;
    let destination_name = destination.file_name().ok_or_else(|| {
        MineruError::new(
            "MINERU_ARCHIVE_DESTINATION_INVALID",
            "结果归档目录名无效。",
            false,
        )
    })?;
    let destination = parent.join(destination_name);
    if destination.exists() {
        return Err(MineruError::new(
            "MINERU_ARCHIVE_DESTINATION_EXISTS",
            "结果归档目录已经存在，拒绝覆盖。",
            false,
        ));
    }
    let staging = parent.join(format!(
        ".{}.staging-{}",
        destination_name.to_string_lossy(),
        Uuid::new_v4()
    ));
    fs::create_dir(&staging).map_err(|error| {
        MineruError::new(
            "MINERU_ARCHIVE_DESTINATION_INVALID",
            format!("创建结果归档暂存目录失败：{error}"),
            false,
        )
    })?;
    let result = extract_into_directory(bytes, &staging, &manifest);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        let _ = fs::remove_dir_all(&staging);
        return Err(MineruError::new(
            "MINERU_ARCHIVE_COMMIT_FAILED",
            format!("提交 MinerU 结果归档失败：{error}"),
            false,
        ));
    }
    Ok(manifest)
}

fn extract_into_directory(
    bytes: &[u8],
    destination: &Path,
    manifest: &ResultArchiveManifest,
) -> Result<(), MineruError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        MineruError::new(
            "MINERU_ARCHIVE_INVALID",
            format!("无法重新打开 MinerU 结果 ZIP：{error}"),
            false,
        )
    })?;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            MineruError::new(
                "MINERU_ARCHIVE_READ_FAILED",
                format!("读取 MinerU ZIP 条目失败：{error}"),
                false,
            )
        })?;
        let name = validate_zip_entry_name(entry.name(), entry.is_dir())?;
        let target = destination.join(&name);
        if !target
            .parent()
            .is_some_and(|parent| parent.starts_with(destination))
        {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_PATH_UNSAFE",
                format!("MinerU ZIP 条目无法安全落盘：{name}"),
                false,
            ));
        }
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| {
                MineruError::new(
                    "MINERU_ARCHIVE_WRITE_FAILED",
                    format!("创建 MinerU 结果目录失败：{error}"),
                    false,
                )
            })?;
            continue;
        }
        let max_entry = if is_json_or_markdown_name(&name) {
            MAX_RESULT_ENTRY_BYTES
        } else {
            MAX_RESULT_ASSET_BYTES
        };
        if entry.size() > max_entry {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_ENTRY_TOO_LARGE",
                format!("MinerU ZIP 条目超过安全上限：{name}"),
                false,
            ));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| invalid_response("MinerU ZIP 解压总量溢出。"))?;
        if total > MAX_RESULT_UNCOMPRESSED_BYTES {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_BOMB",
                "MinerU 结果 ZIP 的解压总量超过安全上限。",
                false,
            ));
        }
        let parent = target.parent().ok_or_else(|| {
            MineruError::new(
                "MINERU_ARCHIVE_WRITE_FAILED",
                "MinerU 结果文件父目录无效。",
                false,
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            MineruError::new(
                "MINERU_ARCHIVE_WRITE_FAILED",
                format!("创建 MinerU 结果文件目录失败：{error}"),
                false,
            )
        })?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| {
                MineruError::new(
                    "MINERU_ARCHIVE_WRITE_FAILED",
                    format!("创建 MinerU 结果文件失败：{error}"),
                    false,
                )
            })?;
        let mut limited = entry.by_ref().take(max_entry.saturating_add(1));
        io::copy(&mut limited, &mut output).map_err(|error| {
            MineruError::new(
                "MINERU_ARCHIVE_WRITE_FAILED",
                format!("写入 MinerU 结果文件失败：{error}"),
                false,
            )
        })?;
        if output
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(max_entry + 1)
            > max_entry
        {
            return Err(MineruError::new(
                "MINERU_ARCHIVE_ENTRY_TOO_LARGE",
                format!("MinerU ZIP 条目超过安全上限：{name}"),
                false,
            ));
        }
        output.sync_all().map_err(|error| {
            MineruError::new(
                "MINERU_ARCHIVE_WRITE_FAILED",
                format!("同步 MinerU 结果文件失败：{error}"),
                false,
            )
        })?;
    }
    // Keep this assertion close to the writer: if a future change skips an
    // entry, the manifest and on-disk archive cannot silently diverge.
    if manifest.entries.len() != archive.len() {
        return Err(invalid_response("MinerU 结果归档条目清单不一致。"));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MineruPageGeometry {
    pub page_index: u32,
    pub width: f64,
    pub height: f64,
}

pub fn parse_page_geometries(layout: &Value) -> Result<Vec<MineruPageGeometry>, MineruError> {
    validate_layout_shape(layout)?;
    let pages = layout
        .get("pdf_info")
        .or_else(|| layout.get("pdfInfo"))
        .or_else(|| layout.get("pages"))
        .and_then(Value::as_array)
        .expect("validate_layout_shape checked pages");
    pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            let size = page
                .get("page_size")
                .or_else(|| page.get("pageSize"))
                .and_then(Value::as_array)
                .expect("validate_layout_shape checked page size");
            Ok(MineruPageGeometry {
                page_index: u32::try_from(index).unwrap_or(u32::MAX),
                width: size[0].as_f64().unwrap_or_default(),
                height: size[1].as_f64().unwrap_or_default(),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MineruElement {
    pub ordinal: usize,
    pub provider_element_id: Option<String>,
    pub element_type: String,
    pub page_index: Option<u32>,
    pub page_end: Option<u32>,
    /// Coordinates are normalized to a 0..1 top-left-origin rectangle.
    pub bbox: Option<[f64; 4]>,
    pub text: String,
    pub caption: String,
    pub formula_latex: String,
    pub table_html: String,
    pub table_json: String,
    pub asset_names: Vec<String>,
    pub metadata: Value,
}

/// Convert the provider's content list into the stable element vocabulary used
/// by the SQLite schema.  Unknown provider types remain `unknown` with their
/// original type in metadata; they are never silently discarded.
pub fn parse_content_elements(
    content_list: &Value,
    page_count: usize,
    archive_entries: &HashSet<String>,
) -> Result<(Vec<MineruElement>, Vec<String>), MineruError> {
    let items = content_list
        .as_array()
        .ok_or_else(|| invalid_response("MinerU content_list 必须是数组。"))?;
    let mut warnings = Vec::new();
    let mut elements = Vec::with_capacity(items.len());
    for (ordinal, item) in items.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| invalid_response("MinerU content_list 包含非对象元素。"))?;
        let provider_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .trim();
        let element_type = map_element_type(provider_type, object);
        let page_index = first_u32(item, &["page_idx", "pageIndex", "page"]);
        if page_index.is_some_and(|page| page as usize >= page_count) {
            warnings.push(format!("MINERU_PAGE_INDEX_OUT_OF_RANGE:{ordinal}"));
        }
        let bbox = parse_bbox(item.get("bbox"), ordinal, &mut warnings)?;
        let text = first_string_value(
            item,
            &["text", "content", "text_content", "textContent", "latex"],
        );
        let caption = first_string_value(
            item,
            &["img_caption", "image_caption", "table_caption", "caption"],
        );
        let formula_latex = first_string_value(
            item,
            &["latex", "equation", "formula", "interline_equation"],
        );
        let table_html = first_string_value(item, &["table_body", "table_html", "html"]);
        let table_json = item
            .get("table")
            .filter(|value| !value.is_string())
            .map(|value| value.to_string())
            .unwrap_or_default();
        let asset_names = collect_asset_names(item, archive_entries, &mut warnings, ordinal);
        let provider_element_id = first_string(item, &["id", "element_id", "elementId"]);
        let metadata = json!({
            "providerType": provider_type,
            "textLevel": item.get("text_level").or_else(|| item.get("textLevel")),
            "raw": item,
        });
        let text = bound_element_text(text, ordinal, &mut warnings)?;
        let caption = bound_element_text(caption, ordinal, &mut warnings)?;
        let formula_latex = bound_element_text(formula_latex, ordinal, &mut warnings)?;
        let table_html = bound_element_text(table_html, ordinal, &mut warnings)?;
        elements.push(MineruElement {
            ordinal,
            provider_element_id,
            element_type,
            page_index,
            page_end: page_index,
            bbox,
            text,
            caption,
            formula_latex,
            table_html,
            table_json,
            asset_names,
            metadata,
        });
    }
    Ok((elements, warnings))
}

fn map_element_type(provider_type: &str, item: &serde_json::Map<String, Value>) -> String {
    match provider_type.to_ascii_lowercase().as_str() {
        "text" => {
            let level = item
                .get("text_level")
                .or_else(|| item.get("textLevel"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if level > 0 {
                "title".to_string()
            } else {
                "paragraph".to_string()
            }
        }
        "title" | "paragraph_title" | "section_title" => "title".to_string(),
        "list" | "list_item" => "list".to_string(),
        "table" => "table".to_string(),
        "image" | "figure" => "figure".to_string(),
        "chart" => "chart".to_string(),
        "equation" | "interline_equation" | "formula" => "formula".to_string(),
        "algorithm" => "algorithm".to_string(),
        "code" => "code".to_string(),
        "caption" | "image_caption" | "table_caption" => "caption".to_string(),
        "ref_text" | "reference" | "bibliography" => "reference".to_string(),
        "header" | "page_header" => "header".to_string(),
        "footer" | "page_footer" => "footer".to_string(),
        "footnote" | "page_footnote" => "footnote".to_string(),
        "page_image" => "page_image".to_string(),
        _ => "unknown".to_string(),
    }
}

fn first_string_value(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn bound_element_text(
    value: String,
    ordinal: usize,
    warnings: &mut Vec<String>,
) -> Result<String, MineruError> {
    if value.chars().count() > MAX_RESULT_JSON_TEXT_CHARS {
        warnings.push(format!("MINERU_ELEMENT_TEXT_TOO_LARGE:{ordinal}"));
        return Err(MineruError::new(
            "MINERU_ELEMENT_TEXT_TOO_LARGE",
            "MinerU 元素文本超过安全上限。",
            false,
        ));
    }
    Ok(value)
}

fn parse_bbox(
    value: Option<&Value>,
    ordinal: usize,
    warnings: &mut Vec<String>,
) -> Result<Option<[f64; 4]>, MineruError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let values = value
        .as_array()
        .ok_or_else(|| invalid_response(format!("MinerU 元素 {ordinal} 的 bbox 不是数组。")))?;
    if values.len() != 4 {
        return Err(invalid_response(format!(
            "MinerU 元素 {ordinal} 的 bbox 长度无效。"
        )));
    }
    let numbers = values
        .iter()
        .map(|value| value.as_f64())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| invalid_response(format!("MinerU 元素 {ordinal} 的 bbox 含有非数字。")))?;
    if numbers
        .iter()
        .any(|number| !number.is_finite() || *number < 0.0 || *number > 1_000.0)
        || numbers[2] < numbers[0]
        || numbers[3] < numbers[1]
    {
        return Err(invalid_response(format!(
            "MinerU 元素 {ordinal} 的 bbox 无效。"
        )));
    }
    // MinerU content_list coordinates are 0..1000.  Be conservative for
    // compatible providers that already send 0..1 coordinates.
    let scale = if numbers.iter().copied().fold(0.0_f64, f64::max) <= 1.0 {
        1.0
    } else {
        1_000.0
    };
    let bbox = [
        numbers[0] / scale,
        numbers[1] / scale,
        numbers[2] / scale,
        numbers[3] / scale,
    ];
    if bbox[2] < bbox[0] || bbox[3] < bbox[1] {
        warnings.push(format!("MINERU_BBOX_INVALID:{ordinal}"));
        return Ok(None);
    }
    Ok(Some(bbox))
}

fn collect_asset_names(
    value: &Value,
    archive_entries: &HashSet<String>,
    warnings: &mut Vec<String>,
    ordinal: usize,
) -> Vec<String> {
    let mut candidates = Vec::new();
    for key in [
        "img_path",
        "imgPath",
        "image_path",
        "imagePath",
        "table_img_path",
        "tableImgPath",
        "equation_img_path",
        "equationImgPath",
        "asset_path",
        "assetPath",
    ] {
        collect_string_or_array(value.get(key), &mut candidates);
    }
    let mut assets = Vec::new();
    for candidate in candidates {
        let safe = match validate_zip_entry_name(&candidate, false) {
            Ok(value) => value,
            Err(_) => {
                warnings.push(format!("MINERU_ASSET_PATH_REJECTED:{ordinal}"));
                continue;
            }
        };
        let match_name = if archive_entries.contains(&safe) {
            Some(safe)
        } else {
            archive_entries
                .iter()
                .find(|name| name.rsplit('/').next() == Some(safe.as_str()))
                .cloned()
        };
        if let Some(name) = match_name {
            if !assets.contains(&name) {
                assets.push(name);
            }
        } else {
            warnings.push(format!("MINERU_ASSET_MISSING:{ordinal}"));
        }
    }
    assets
}

fn collect_string_or_array(value: Option<&Value>, output: &mut Vec<String>) {
    match value {
        Some(Value::String(value)) if !value.trim().is_empty() => {
            output.push(value.trim().to_string())
        }
        Some(Value::Array(values)) => {
            for value in values {
                if let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) {
                    output.push(value.trim().to_string());
                }
            }
        }
        _ => {}
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::PathBuf};

    use super::*;
    use zip::write::SimpleFileOptions;

    fn test_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("mnemora-mineru-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixture_zip(extra: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = SimpleFileOptions::default();
            writer.start_file("task_full.md", options).unwrap();
            writer.write_all(b"# Paper\n\nA result.").unwrap();
            writer
                .start_file("task_content_list.json", options)
                .unwrap();
            writer
                .write_all(
                    br#"[{"type":"text","page_idx":0,"bbox":[0,0,1000,100],"text":"hello"}]"#,
                )
                .unwrap();
            writer.start_file("task_layout.json", options).unwrap();
            writer
                .write_all(br#"{"pdf_info":[{"page_size":[612,792]}]}"#)
                .unwrap();
            for (name, bytes) in extra {
                writer.start_file(*name, options).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn endpoint_validation_requires_https_and_normalizes_api_path() {
        let endpoint = validate_api_endpoint("https://mineru.net/").unwrap();
        assert_eq!(endpoint.as_str(), "https://mineru.net/api/v4");
        assert!(validate_api_endpoint("http://mineru.net/api/v4").is_err());
        assert!(validate_api_endpoint("https://user:pass@mineru.net/api/v4").is_err());
        assert!(validate_api_endpoint("https://mineru.net/api/v4?token=secret").is_err());
    }

    #[test]
    fn upload_names_cannot_escape_or_become_non_pdf() {
        assert_eq!(safe_upload_file_name("..\\paper"), "paper.pdf");
        assert_eq!(safe_upload_file_name("paper.PDF"), "paper.PDF");
        assert_eq!(safe_upload_file_name(""), "document.pdf");
    }

    #[test]
    fn business_errors_map_auth_quota_and_transient_states_without_token() {
        let auth = business_error(
            "A0202",
            "token secret-token invalid",
            StatusCode::BAD_REQUEST,
            "secret-token",
            "submit",
        );
        assert_eq!(auth.code(), "MINERU_AUTH");
        assert!(!auth.message.contains("secret-token"));
        let quota = business_error(
            "-1",
            "quota exceeded",
            StatusCode::BAD_REQUEST,
            "token",
            "submit",
        );
        assert_eq!(quota.code(), "MINERU_QUOTA_EXCEEDED");
        let transient = business_error(
            "-1",
            "busy",
            StatusCode::SERVICE_UNAVAILABLE,
            "token",
            "submit",
        );
        assert!(transient.is_retryable());
    }

    #[test]
    fn zip_path_gate_rejects_windows_and_posix_escape_forms() {
        for value in [
            "../outside.txt",
            "a/../../outside.txt",
            "C:/outside.txt",
            "\\\\server\\share\\x",
            "/absolute.txt",
            "a\\b.txt",
            "a:stream",
        ] {
            assert!(validate_zip_entry_name(value, false).is_err(), "{value}");
        }
        assert_eq!(
            validate_zip_entry_name("task/assets/a.png", false).unwrap(),
            "task/assets/a.png"
        );
    }

    #[test]
    fn archive_requires_unique_safe_required_entries_and_parses_v4_layout() {
        let bytes = fixture_zip(&[("task/images/figure.png", b"png")]);
        let manifest = inspect_result_archive(&bytes).unwrap();
        assert_eq!(manifest.full_markdown, "# Paper\n\nA result.");
        assert_eq!(manifest.layout_entry, "task_layout.json");
        assert_eq!(manifest.entries.len(), 4);
        assert_eq!(manifest.sha256, sha256_hex(&bytes));
        let names = manifest
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"task/images/figure.png"));
    }

    #[test]
    fn archive_rejects_ambiguous_or_missing_required_entries() {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = SimpleFileOptions::default();
            writer.start_file("a/full.md", options).unwrap();
            writer.write_all(b"a").unwrap();
            writer.start_file("b/full.md", options).unwrap();
            writer.write_all(b"b").unwrap();
            writer.start_file("content_list.json", options).unwrap();
            writer.write_all(b"[]").unwrap();
            writer.start_file("layout.json", options).unwrap();
            writer
                .write_all(br#"{"pdf_info":[{"page_size":[1,1]}]}"#)
                .unwrap();
            writer.finish().unwrap();
        }
        let error = inspect_result_archive(bytes.get_ref()).unwrap_err();
        assert_eq!(error.code(), "MINERU_ARCHIVE_AMBIGUOUS_ENTRY");
        let missing = fixture_zip(&[]).into_iter().collect::<Vec<_>>();
        // The normal fixture has all required files; mutate the ZIP through a
        // separately-built archive to make the missing-artifact assertion
        // independent of string replacement in compressed bytes.
        let mut only_full = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut only_full);
            let options = SimpleFileOptions::default();
            writer.start_file("full.md", options).unwrap();
            writer.write_all(b"x").unwrap();
            writer.finish().unwrap();
        }
        let error = inspect_result_archive(only_full.get_ref()).unwrap_err();
        assert_eq!(error.code(), "MINERU_ARCHIVE_REQUIRED_ENTRY_MISSING");
        assert!(!missing.is_empty());
    }

    #[test]
    fn archive_atomic_extract_never_writes_outside_destination() {
        let root = test_dir("atomic");
        let destination = root.join("revision");
        let bytes = fixture_zip(&[("task/images/figure.png", b"png")]);
        extract_result_archive_atomic(&bytes, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("task/images/figure.png")).unwrap(),
            b"png"
        );
        assert!(!root.join("images/figure.png").exists());
        assert!(extract_result_archive_atomic(&bytes, &destination).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn content_elements_keep_multimodal_types_bbox_and_missing_asset_warning() {
        let content: Value = serde_json::json!([
            {"type":"text","text_level":1,"page_idx":0,"bbox":[100,200,900,400],"text":"Title"},
            {"type":"table","page_idx":0,"bbox":[0,0,1000,1000],"table_body":"<table></table>","img_path":"missing.png"},
            {"type":"interline_equation","page_idx":0,"latex":"x^2"}
        ]);
        let entries = ["present.png".to_string()].into_iter().collect();
        let (elements, warnings) = parse_content_elements(&content, 1, &entries).unwrap();
        assert_eq!(elements[0].element_type, "title");
        assert_eq!(elements[0].bbox, Some([0.1, 0.2, 0.9, 0.4]));
        assert_eq!(elements[1].element_type, "table");
        assert_eq!(elements[2].element_type, "formula");
        assert!(warnings
            .iter()
            .any(|warning| warning.starts_with("MINERU_ASSET_MISSING")));
    }

    #[test]
    fn preflight_rejects_non_pdf_signature_without_network() {
        let root = test_dir("preflight");
        let path = root.join("bad.pdf");
        fs::write(&path, b"not a pdf").unwrap();
        let error = preflight_pdf(&path).unwrap_err();
        assert_eq!(error.code(), "MINERU_PDF_INVALID");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn response_code_accepts_numeric_and_new_auth_field_shapes() {
        assert_eq!(response_code(&json!({"code": 0})).as_deref(), Some("0"));
        assert_eq!(
            response_code(&json!({"msgCode":"A0211"})).as_deref(),
            Some("A0211")
        );
    }
}
