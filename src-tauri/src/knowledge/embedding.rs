//! Optional embedding transport and deterministic vector primitives.
//!
//! Source text is sent only by an explicit caller after the knowledge privacy
//! gates have been checked. API keys and vector bodies are never included in
//! errors or diagnostics. Stored vectors are finite, L2-normalized f32 values
//! encoded in little-endian order.

use std::{collections::HashMap, future::Future, pin::Pin, time::Duration};

use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::{
    ai::http::{endpoint_url, read_response_bytes_limited},
    ai::model::{database_embedding_dimensions, database_supports_embedding},
    settings::{
        app_types::KnowledgeSettings,
        types::{ApiProtocol, AuthScheme, ModelSettings},
    },
};

pub const EMBEDDING_PIPELINE_VERSION: &str = "embedding-v1";
pub const EMBEDDING_SCHEMA_VERSION: &str = "embedding-schema-v1";
pub const EMBEDDING_NORMALIZATION: &str = "l2";
pub const DEFAULT_EMBEDDING_BATCH_SIZE: usize = 64;
pub const MAX_EMBEDDING_BATCH_SIZE: usize = 256;
pub const MAX_EMBEDDING_DIMENSIONS: usize = 65_536;
const MAX_EMBEDDING_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

pub type EmbeddingFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>, EmbeddingError>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl EmbeddingError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }

    pub fn cancelled() -> Self {
        Self::new(
            "EMBEDDING_CANCELLED",
            "Embedding request was cancelled.",
            false,
        )
    }

    pub fn timeout() -> Self {
        Self::new("EMBEDDING_TIMEOUT", "Embedding request timed out.", true)
    }
}

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EmbeddingError {}

/// Runtime route resolved from the existing model-provider settings. It does
/// not contain the credential; the key remains a short-lived SecretStore read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingProviderSpec {
    pub provider_id: String,
    pub model_id: String,
    pub model_revision: String,
    pub base_url: String,
    pub protocol: ApiProtocol,
    pub auth_scheme: AuthScheme,
    pub credential_revision: u64,
    pub expected_dimensions: Option<usize>,
    pub embedding_key: String,
}

impl EmbeddingProviderSpec {
    pub fn resolve(
        knowledge: &KnowledgeSettings,
        models: &ModelSettings,
    ) -> Result<Self, EmbeddingError> {
        if !knowledge.enabled {
            return Err(EmbeddingError::new(
                "EMBEDDING_KNOWLEDGE_DISABLED",
                "The knowledge base is disabled.",
                false,
            ));
        }
        if !knowledge.embedding_enabled {
            return Err(EmbeddingError::new(
                "EMBEDDING_DISABLED",
                "Embedding is disabled in knowledge settings.",
                false,
            ));
        }
        if !knowledge.allow_remote_embedding {
            return Err(EmbeddingError::new(
                "EMBEDDING_REMOTE_NOT_ALLOWED",
                "Remote embedding transfer has not been allowed.",
                false,
            ));
        }
        let provider_id = knowledge.embedding_provider.trim();
        let model_id = knowledge.embedding_model.trim();
        if provider_id.is_empty() || model_id.is_empty() {
            return Err(EmbeddingError::new(
                "EMBEDDING_CONFIG_MISSING",
                "Embedding provider and model are required.",
                false,
            ));
        }
        let provider = models
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .ok_or_else(|| {
                EmbeddingError::new(
                    "EMBEDDING_PROVIDER_NOT_FOUND",
                    "The configured embedding provider does not exist.",
                    false,
                )
            })?;
        if !provider.enabled {
            return Err(EmbeddingError::new(
                "EMBEDDING_PROVIDER_DISABLED",
                "The configured embedding provider is disabled.",
                false,
            ));
        }
        if !matches!(
            provider.protocol,
            ApiProtocol::OpenAiChatCompletions | ApiProtocol::OpenAiResponses
        ) {
            return Err(EmbeddingError::new(
                "EMBEDDING_PROTOCOL_UNSUPPORTED",
                "The selected provider is not configured with an OpenAI-compatible protocol.",
                false,
            ));
        }

        // An explicit provider-model override wins over the shared database.
        // This supports relays that expose a known model under a custom name,
        // while a known non-embedding model remains rejected by default.
        let configured_model = provider
            .models
            .iter()
            .find(|model| model.id == model_id || model.api_model == model_id);
        let configured_embedding_capability = configured_model
            .and_then(|model| model.capabilities.as_ref())
            .and_then(|capabilities| capabilities.embedding);
        let metadata_model_id = configured_model
            .map(|model| model.api_model.as_str())
            .unwrap_or(model_id);
        let database_embedding_capability = database_supports_embedding(model_id).or_else(|| {
            (metadata_model_id != model_id)
                .then(|| database_supports_embedding(metadata_model_id))
                .flatten()
        });
        let embedding_capability =
            configured_embedding_capability.or(database_embedding_capability);
        if embedding_capability == Some(false) {
            return Err(EmbeddingError::new(
                "EMBEDDING_MODEL_UNSUPPORTED",
                "The selected model is not marked as an embedding model.",
                false,
            ));
        }
        let expected_dimensions = database_embedding_dimensions(model_id)
            .or_else(|| database_embedding_dimensions(metadata_model_id));
        let normalized_base_url = provider.base_url.trim().trim_end_matches('/');
        let protocol_name = protocol_name(provider.protocol);
        let auth_name = auth_scheme_name(provider.auth_scheme);

        let model_revision = format!(
            "{}:{}:{}:{}",
            EMBEDDING_PIPELINE_VERSION,
            EMBEDDING_SCHEMA_VERSION,
            protocol_name,
            short_digest(normalized_base_url.as_bytes())
        );
        let identity = format!(
            "schema={}\nprovider={}\nbase_url={}\nmodel={}\nmodel_revision={}\nprotocol={}\nauth={}\ncredential_revision={}\ndimensions={}\nnormalization={}",
            EMBEDDING_SCHEMA_VERSION,
            provider.id,
            normalized_base_url,
            model_id,
            model_revision,
            protocol_name,
            auth_name,
            provider.credential_revision,
            expected_dimensions
                .map(|dimensions| dimensions.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            EMBEDDING_NORMALIZATION,
        );
        Ok(Self {
            provider_id: provider.id.clone(),
            model_id: model_id.to_string(),
            model_revision,
            base_url: normalized_base_url.to_string(),
            protocol: provider.protocol,
            auth_scheme: provider.auth_scheme,
            credential_revision: provider.credential_revision,
            expected_dimensions,
            embedding_key: format!("sha256:{:x}", Sha256::digest(identity.as_bytes())),
        })
    }
}

fn protocol_name(protocol: ApiProtocol) -> &'static str {
    match protocol {
        ApiProtocol::OpenAiChatCompletions => "openai-chat-completions",
        ApiProtocol::OpenAiResponses => "openai-responses",
        ApiProtocol::AnthropicMessages => "anthropic-messages",
        ApiProtocol::GeminiGenerateContent => "gemini-generate-content",
    }
}

fn auth_scheme_name(auth_scheme: AuthScheme) -> &'static str {
    match auth_scheme {
        AuthScheme::ProtocolDefault => "protocol-default",
        AuthScheme::Bearer => "bearer",
        AuthScheme::XApiKey => "x-api-key",
        AuthScheme::XGoogApiKey => "x-goog-api-key",
    }
}

fn short_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
        .chars()
        .take(16)
        .collect()
}

/// Minimal provider contract used by both indexing and query embedding.
pub trait EmbeddingProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn model_id(&self) -> &str;
    fn model_revision(&self) -> &str;
    fn dimensions(&self) -> Option<usize>;
    fn max_batch_size(&self) -> usize;
    fn document_prefix(&self) -> &str;
    fn query_prefix(&self) -> &str;

    fn embed_documents<'a>(
        &'a self,
        texts: &'a [String],
        cancellation: &'a CancellationToken,
    ) -> EmbeddingFuture<'a>;

    fn embed_query<'a>(
        &'a self,
        text: &'a str,
        cancellation: &'a CancellationToken,
    ) -> EmbeddingFuture<'a>;

    fn health_check<'a>(&'a self, cancellation: &'a CancellationToken) -> EmbeddingFuture<'a>;
}

pub struct OpenAiCompatibleEmbeddingProvider {
    client: Client,
    spec: EmbeddingProviderSpec,
    api_key: Zeroizing<String>,
    max_batch_size: usize,
}

impl OpenAiCompatibleEmbeddingProvider {
    pub fn new(
        client: Client,
        spec: EmbeddingProviderSpec,
        api_key: String,
    ) -> Result<Self, EmbeddingError> {
        if api_key.trim().is_empty() {
            return Err(EmbeddingError::new(
                "EMBEDDING_API_KEY_MISSING",
                "The embedding provider API key is missing.",
                false,
            ));
        }
        Ok(Self {
            client,
            spec,
            api_key: Zeroizing::new(api_key),
            max_batch_size: DEFAULT_EMBEDDING_BATCH_SIZE,
        })
    }

    async fn request(
        &self,
        input: Vec<String>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if input.is_empty() || input.len() > self.max_batch_size() {
            return Err(EmbeddingError::new(
                "EMBEDDING_BATCH_INVALID",
                "Embedding batch size is invalid.",
                false,
            ));
        }
        let url = endpoint_url(&self.spec.base_url, "embeddings").map_err(|_| {
            EmbeddingError::new(
                "EMBEDDING_CONFIG_INVALID",
                "Embedding provider URL is invalid.",
                false,
            )
        })?;
        let body = OpenAiEmbeddingRequest {
            model: &self.spec.model_id,
            input,
            encoding_format: "float",
        };
        let request = apply_auth(
            self.client.post(url).json(&body),
            self.spec.auth_scheme,
            self.api_key.as_str(),
        )?;
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(EmbeddingError::cancelled()),
            response = request.send() => response.map_err(|error| {
                if error.is_timeout() {
                    EmbeddingError::timeout()
                } else {
                    EmbeddingError::new(
                        "EMBEDDING_TRANSPORT",
                        "Embedding provider request failed.",
                        true,
                    )
                }
            })?,
        };
        if !response.status().is_success() {
            return Err(http_status_error(response.status()));
        }
        let bytes = tokio::select! {
            _ = cancellation.cancelled() => return Err(EmbeddingError::cancelled()),
            body = read_response_bytes_limited(response, MAX_EMBEDDING_RESPONSE_BYTES) => {
                body.map_err(|_| EmbeddingError::new(
                    "EMBEDDING_RESPONSE_TOO_LARGE",
                    "Embedding provider response is invalid or too large.",
                    false,
                ))?
            }
        };
        let expected = body.input.len();
        let response: OpenAiEmbeddingResponse = serde_json::from_slice(&bytes).map_err(|_| {
            EmbeddingError::new(
                "EMBEDDING_RESPONSE_INVALID",
                "Embedding provider returned invalid JSON.",
                false,
            )
        })?;
        ordered_response_vectors(response.data, expected)
    }
}

impl EmbeddingProvider for OpenAiCompatibleEmbeddingProvider {
    fn provider_id(&self) -> &str {
        &self.spec.provider_id
    }

    fn model_id(&self) -> &str {
        &self.spec.model_id
    }

    fn model_revision(&self) -> &str {
        &self.spec.model_revision
    }

    fn dimensions(&self) -> Option<usize> {
        self.spec.expected_dimensions
    }

    fn max_batch_size(&self) -> usize {
        self.max_batch_size
    }

    fn document_prefix(&self) -> &str {
        ""
    }

    fn query_prefix(&self) -> &str {
        ""
    }

    fn embed_documents<'a>(
        &'a self,
        texts: &'a [String],
        cancellation: &'a CancellationToken,
    ) -> EmbeddingFuture<'a> {
        Box::pin(async move {
            let prefix = self.document_prefix();
            let input = texts.iter().map(|text| format!("{prefix}{text}")).collect();
            self.request(input, cancellation).await
        })
    }

    fn embed_query<'a>(
        &'a self,
        text: &'a str,
        cancellation: &'a CancellationToken,
    ) -> EmbeddingFuture<'a> {
        Box::pin(async move {
            self.request(vec![format!("{}{text}", self.query_prefix())], cancellation)
                .await
        })
    }

    fn health_check<'a>(&'a self, cancellation: &'a CancellationToken) -> EmbeddingFuture<'a> {
        self.embed_query("health check", cancellation)
    }
}

#[derive(Serialize)]
struct OpenAiEmbeddingRequest<'a> {
    model: &'a str,
    input: Vec<String>,
    encoding_format: &'static str,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

fn ordered_response_vectors(
    data: Vec<OpenAiEmbeddingData>,
    expected: usize,
) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    if data.len() != expected {
        return Err(EmbeddingError::new(
            "EMBEDDING_COUNT_MISMATCH",
            "Embedding provider returned an unexpected vector count.",
            false,
        ));
    }
    let mut ordered = vec![None; expected];
    for item in data {
        if item.index >= expected || ordered[item.index].is_some() {
            return Err(EmbeddingError::new(
                "EMBEDDING_INDEX_INVALID",
                "Embedding provider returned invalid vector indexes.",
                false,
            ));
        }
        ordered[item.index] = Some(item.embedding);
    }
    ordered
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| {
                EmbeddingError::new(
                    "EMBEDDING_INDEX_INVALID",
                    "Embedding provider omitted a vector index.",
                    false,
                )
            })
        })
        .collect()
}

fn apply_auth(
    request: RequestBuilder,
    auth_scheme: AuthScheme,
    api_key: &str,
) -> Result<RequestBuilder, EmbeddingError> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(EmbeddingError::new(
            "EMBEDDING_API_KEY_MISSING",
            "The embedding provider API key is missing.",
            false,
        ));
    }
    Ok(match auth_scheme {
        AuthScheme::ProtocolDefault | AuthScheme::Bearer => request.bearer_auth(api_key),
        AuthScheme::XApiKey => request.header("x-api-key", api_key),
        AuthScheme::XGoogApiKey => request.header("x-goog-api-key", api_key),
    })
}

fn http_status_error(status: StatusCode) -> EmbeddingError {
    let code = match status.as_u16() {
        401 | 403 => "EMBEDDING_AUTH_FAILED",
        404 => "EMBEDDING_MODEL_NOT_FOUND",
        408 => "EMBEDDING_TIMEOUT",
        429 => "EMBEDDING_RATE_LIMITED",
        500..=599 => "EMBEDDING_PROVIDER_UNAVAILABLE",
        _ => "EMBEDDING_HTTP_ERROR",
    };
    EmbeddingError::new(
        code,
        format!("Embedding provider returned HTTP {}.", status.as_u16()),
        status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::TOO_MANY_REQUESTS
            || status.is_server_error(),
    )
}

#[derive(Debug, Clone, Copy)]
pub struct EmbeddingRetryPolicy {
    pub max_retries: usize,
    pub request_timeout: Duration,
    pub initial_backoff: Duration,
}

impl Default for EmbeddingRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            request_timeout: Duration::from_secs(120),
            initial_backoff: Duration::from_millis(500),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmbeddingBatchProgress {
    pub completed: usize,
    pub total: usize,
    pub retries: usize,
}

/// Embed all documents in bounded batches. Validation happens before a batch
/// is appended, so a dimension mismatch rejects that whole batch atomically.
pub async fn embed_documents_batched<F>(
    provider: &dyn EmbeddingProvider,
    texts: &[String],
    cancellation: &CancellationToken,
    policy: EmbeddingRetryPolicy,
    mut progress: F,
) -> Result<Vec<Vec<f32>>, EmbeddingError>
where
    F: FnMut(EmbeddingBatchProgress),
{
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let batch_size = provider.max_batch_size().clamp(1, MAX_EMBEDDING_BATCH_SIZE);
    let mut output = Vec::with_capacity(texts.len());
    let mut expected_dimensions = provider.dimensions();
    let mut retry_count = 0usize;

    for batch in texts.chunks(batch_size) {
        let mut attempt = 0usize;
        let vectors = loop {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::cancelled());
            }
            let result = tokio::select! {
                _ = cancellation.cancelled() => return Err(EmbeddingError::cancelled()),
                result = tokio::time::timeout(
                    policy.request_timeout,
                    provider.embed_documents(batch, cancellation),
                ) => match result {
                    Ok(value) => value,
                    Err(_) => Err(EmbeddingError::timeout()),
                },
            };
            match result {
                Ok(vectors) => break vectors,
                Err(error) if error.retryable && attempt < policy.max_retries => {
                    attempt += 1;
                    retry_count += 1;
                    let multiplier = 1u32 << attempt.saturating_sub(1).min(8);
                    let delay = policy.initial_backoff.saturating_mul(multiplier);
                    tokio::select! {
                        _ = cancellation.cancelled() => return Err(EmbeddingError::cancelled()),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        };
        if vectors.len() != batch.len() {
            return Err(EmbeddingError::new(
                "EMBEDDING_COUNT_MISMATCH",
                "Embedding provider returned an unexpected vector count.",
                false,
            ));
        }
        let normalized = validate_and_normalize_batch(vectors, &mut expected_dimensions)?;
        output.extend(normalized);
        progress(EmbeddingBatchProgress {
            completed: output.len(),
            total: texts.len(),
            retries: retry_count,
        });
    }
    Ok(output)
}

pub async fn embed_query_with_retry(
    provider: &dyn EmbeddingProvider,
    text: &str,
    cancellation: &CancellationToken,
    policy: EmbeddingRetryPolicy,
) -> Result<Vec<f32>, EmbeddingError> {
    let mut attempt = 0usize;
    loop {
        if cancellation.is_cancelled() {
            return Err(EmbeddingError::cancelled());
        }
        let result = tokio::select! {
            _ = cancellation.cancelled() => return Err(EmbeddingError::cancelled()),
            result = tokio::time::timeout(
                policy.request_timeout,
                provider.embed_query(text, cancellation),
            ) => match result {
                Ok(value) => value,
                Err(_) => Err(EmbeddingError::timeout()),
            },
        };
        match result {
            Ok(vectors) => {
                let mut expected = provider.dimensions();
                let mut normalized = validate_and_normalize_batch(vectors, &mut expected)?;
                if normalized.len() != 1 {
                    return Err(EmbeddingError::new(
                        "EMBEDDING_COUNT_MISMATCH",
                        "Embedding provider returned an unexpected query vector count.",
                        false,
                    ));
                }
                return Ok(normalized.remove(0));
            }
            Err(error) if error.retryable && attempt < policy.max_retries => {
                attempt += 1;
                let multiplier = 1u32 << attempt.saturating_sub(1).min(8);
                let delay = policy.initial_backoff.saturating_mul(multiplier);
                tokio::select! {
                    _ = cancellation.cancelled() => return Err(EmbeddingError::cancelled()),
                    _ = tokio::time::sleep(delay) => {}
                }
            }
            Err(error) => return Err(error),
        }
    }
}

fn validate_and_normalize_batch(
    vectors: Vec<Vec<f32>>,
    expected_dimensions: &mut Option<usize>,
) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    if vectors.is_empty() {
        return Err(EmbeddingError::new(
            "EMBEDDING_RESPONSE_EMPTY",
            "Embedding provider returned no vectors.",
            false,
        ));
    }
    let batch_dimensions = vectors[0].len();
    if batch_dimensions == 0 || batch_dimensions > MAX_EMBEDDING_DIMENSIONS {
        return Err(EmbeddingError::new(
            "EMBEDDING_DIMENSIONS_INVALID",
            "Embedding vector dimensions are invalid.",
            false,
        ));
    }
    if expected_dimensions.is_some_and(|expected| expected != batch_dimensions)
        || vectors
            .iter()
            .any(|vector| vector.len() != batch_dimensions)
    {
        return Err(EmbeddingError::new(
            "EMBEDDING_DIMENSION_MISMATCH",
            "Embedding vectors have inconsistent dimensions.",
            false,
        ));
    }
    *expected_dimensions = Some(batch_dimensions);
    vectors.into_iter().map(normalize_l2).collect()
}

pub fn normalize_l2(vector: Vec<f32>) -> Result<Vec<f32>, EmbeddingError> {
    if vector.is_empty() || vector.len() > MAX_EMBEDDING_DIMENSIONS {
        return Err(EmbeddingError::new(
            "EMBEDDING_DIMENSIONS_INVALID",
            "Embedding vector dimensions are invalid.",
            false,
        ));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::new(
            "EMBEDDING_NON_FINITE",
            "Embedding vector contains a non-finite value.",
            false,
        ));
    }
    let norm_squared = vector
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>();
    if !norm_squared.is_finite() || norm_squared <= f64::EPSILON {
        return Err(EmbeddingError::new(
            "EMBEDDING_ZERO_VECTOR",
            "Embedding vector has zero magnitude.",
            false,
        ));
    }
    let norm = norm_squared.sqrt();
    let normalized = vector
        .into_iter()
        .map(|value| (f64::from(value) / norm) as f32)
        .collect::<Vec<_>>();
    if normalized.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::new(
            "EMBEDDING_NON_FINITE",
            "Embedding normalization produced a non-finite value.",
            false,
        ));
    }
    Ok(normalized)
}

pub fn encode_f32_le(vector: &[f32]) -> Result<Vec<u8>, EmbeddingError> {
    if vector.is_empty() || vector.len() > MAX_EMBEDDING_DIMENSIONS {
        return Err(EmbeddingError::new(
            "EMBEDDING_DIMENSIONS_INVALID",
            "Embedding vector dimensions are invalid.",
            false,
        ));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::new(
            "EMBEDDING_NON_FINITE",
            "Embedding vector contains a non-finite value.",
            false,
        ));
    }
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Ok(bytes)
}

pub fn decode_f32_le(bytes: &[u8], dimensions: usize) -> Result<Vec<f32>, EmbeddingError> {
    if dimensions == 0
        || dimensions > MAX_EMBEDDING_DIMENSIONS
        || bytes.len() != dimensions.saturating_mul(4)
    {
        return Err(EmbeddingError::new(
            "EMBEDDING_BLOB_INVALID",
            "Stored embedding BLOB length does not match its dimensions.",
            false,
        ));
    }
    let values = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    if values.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::new(
            "EMBEDDING_NON_FINITE",
            "Stored embedding contains a non-finite value.",
            false,
        ));
    }
    Ok(values)
}

pub fn cosine_similarity_normalized(left: &[f32], right: &[f32]) -> Result<f64, EmbeddingError> {
    if left.is_empty() || left.len() != right.len() {
        return Err(EmbeddingError::new(
            "EMBEDDING_DIMENSION_MISMATCH",
            "Embedding vectors have inconsistent dimensions.",
            false,
        ));
    }
    if left
        .iter()
        .chain(right.iter())
        .any(|value| !value.is_finite())
    {
        return Err(EmbeddingError::new(
            "EMBEDDING_NON_FINITE",
            "Embedding vector contains a non-finite value.",
            false,
        ));
    }
    let score = left
        .iter()
        .zip(right)
        .map(|(left, right)| f64::from(*left) * f64::from(*right))
        .sum::<f64>();
    if !score.is_finite() {
        return Err(EmbeddingError::new(
            "EMBEDDING_NON_FINITE",
            "Cosine similarity is not finite.",
            false,
        ));
    }
    Ok(score.clamp(-1.0, 1.0))
}

#[derive(Debug, Clone, PartialEq)]
pub struct FusedRank {
    pub chunk_id: String,
    pub score: f64,
    pub lexical_rank: Option<usize>,
    pub vector_rank: Option<usize>,
}

pub fn reciprocal_rank_fusion(
    lexical: &[String],
    vector: &[String],
    rank_constant: usize,
) -> Vec<FusedRank> {
    let rank_constant = rank_constant.max(1) as f64;
    let mut fused = HashMap::<String, FusedRank>::new();
    for (index, chunk_id) in lexical.iter().enumerate() {
        let entry = fused.entry(chunk_id.clone()).or_insert_with(|| FusedRank {
            chunk_id: chunk_id.clone(),
            score: 0.0,
            lexical_rank: None,
            vector_rank: None,
        });
        if entry.lexical_rank.is_none() {
            let rank = index + 1;
            entry.lexical_rank = Some(rank);
            entry.score += 1.0 / (rank_constant + rank as f64);
        }
    }
    for (index, chunk_id) in vector.iter().enumerate() {
        let entry = fused.entry(chunk_id.clone()).or_insert_with(|| FusedRank {
            chunk_id: chunk_id.clone(),
            score: 0.0,
            lexical_rank: None,
            vector_rank: None,
        });
        if entry.vector_rank.is_none() {
            let rank = index + 1;
            entry.vector_rank = Some(rank);
            entry.score += 1.0 / (rank_constant + rank as f64);
        }
    }
    let mut output = fused.into_values().collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| {
                left.lexical_rank
                    .unwrap_or(usize::MAX)
                    .cmp(&right.lexical_rank.unwrap_or(usize::MAX))
            })
            .then_with(|| {
                left.vector_rank
                    .unwrap_or(usize::MAX)
                    .cmp(&right.vector_rank.unwrap_or(usize::MAX))
            })
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    output
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        sync::{Arc, Mutex},
    };

    use super::*;
    use serde_json::Value as JsonValue;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    struct MockProvider {
        responses: Arc<Mutex<VecDeque<Result<Vec<Vec<f32>>, EmbeddingError>>>>,
        calls: Arc<Mutex<Vec<usize>>>,
        batch_size: usize,
    }

    impl MockProvider {
        fn new(responses: Vec<Result<Vec<Vec<f32>>, EmbeddingError>>, batch_size: usize) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into())),
                calls: Arc::new(Mutex::new(Vec::new())),
                batch_size,
            }
        }
    }

    impl EmbeddingProvider for MockProvider {
        fn provider_id(&self) -> &str {
            "mock"
        }
        fn model_id(&self) -> &str {
            "mock-model"
        }
        fn model_revision(&self) -> &str {
            "1"
        }
        fn dimensions(&self) -> Option<usize> {
            None
        }
        fn max_batch_size(&self) -> usize {
            self.batch_size
        }
        fn document_prefix(&self) -> &str {
            "passage: "
        }
        fn query_prefix(&self) -> &str {
            "query: "
        }

        fn embed_documents<'a>(
            &'a self,
            texts: &'a [String],
            _cancellation: &'a CancellationToken,
        ) -> EmbeddingFuture<'a> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(texts.len());
                self.responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("mock response")
            })
        }

        fn embed_query<'a>(
            &'a self,
            _text: &'a str,
            _cancellation: &'a CancellationToken,
        ) -> EmbeddingFuture<'a> {
            Box::pin(async move {
                self.responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("mock response")
            })
        }

        fn health_check<'a>(&'a self, cancellation: &'a CancellationToken) -> EmbeddingFuture<'a> {
            self.embed_query("health", cancellation)
        }
    }

    fn fast_policy() -> EmbeddingRetryPolicy {
        EmbeddingRetryPolicy {
            max_retries: 2,
            request_timeout: Duration::from_secs(1),
            initial_backoff: Duration::from_millis(1),
        }
    }

    #[derive(Clone)]
    struct HttpMockResponse {
        status: u16,
        body: String,
        delay: Duration,
    }

    #[derive(Debug, Clone)]
    struct CapturedHttpRequest {
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    async fn read_http_request(stream: &mut TcpStream) -> CapturedHttpRequest {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "mock client closed before sending headers");
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break end + 4;
            }
            assert!(
                bytes.len() < 1_048_576,
                "mock request headers are too large"
            );
        };
        let header_text = String::from_utf8_lossy(&bytes[..header_end]);
        let mut lines = header_text.split("\r\n");
        let request_line = lines.next().unwrap_or_default();
        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        let mut headers = HashMap::new();
        let mut content_length = 0usize;
        for line in lines.filter(|line| !line.is_empty()) {
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if name == "content-length" {
                    content_length = value.parse::<usize>().unwrap_or(0);
                }
                headers.insert(name, value);
            }
        }
        while bytes.len() < header_end.saturating_add(content_length) {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "mock client closed before sending request body");
            bytes.extend_from_slice(&buffer[..read]);
            assert!(
                bytes.len() < 16 * 1024 * 1024,
                "mock request body is too large"
            );
        }
        CapturedHttpRequest {
            path,
            headers,
            body: bytes[header_end..header_end + content_length].to_vec(),
        }
    }

    fn status_reason(status: u16) -> &'static str {
        match status {
            200 => "OK",
            401 => "Unauthorized",
            404 => "Not Found",
            408 => "Request Timeout",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            _ => "Test Response",
        }
    }

    async fn spawn_http_mock(
        responses: Vec<HttpMockResponse>,
    ) -> (
        String,
        Arc<Mutex<Vec<CapturedHttpRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_for_task = captured.clone();
        let task = tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut stream).await;
                captured_for_task.lock().unwrap().push(request);
                if !response.delay.is_zero() {
                    tokio::time::sleep(response.delay).await;
                }
                let body = response.body.into_bytes();
                let header = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    status_reason(response.status),
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            }
        });
        (format!("http://{address}/v1"), captured, task)
    }

    fn http_response_body(vectors: &[(usize, &[f32])]) -> String {
        serde_json::json!({
            "object": "list",
            "data": vectors
                .iter()
                .map(|(index, vector)| serde_json::json!({
                    "object": "embedding",
                    "index": index,
                    "embedding": vector,
                }))
                .collect::<Vec<_>>(),
            "model": "test-embedding",
            "usage": {"prompt_tokens": 1, "total_tokens": 1},
        })
        .to_string()
    }

    fn http_spec(base_url: String, auth_scheme: AuthScheme) -> EmbeddingProviderSpec {
        EmbeddingProviderSpec {
            provider_id: "http-test".to_string(),
            model_id: "test-embedding".to_string(),
            model_revision: "embedding-v1:http-test".to_string(),
            base_url,
            protocol: ApiProtocol::OpenAiChatCompletions,
            auth_scheme,
            credential_revision: 3,
            expected_dimensions: None,
            embedding_key: "sha256:http-test".to_string(),
        }
    }

    #[test]
    fn float32_blob_is_little_endian_and_strictly_sized() {
        let vector = vec![1.0, -2.5, 0.25];
        let bytes = encode_f32_le(&vector).unwrap();
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(decode_f32_le(&bytes, 3).unwrap(), vector);
        assert!(decode_f32_le(&bytes, 2).is_err());
    }

    #[test]
    fn normalization_rejects_zero_and_non_finite_vectors() {
        assert!(normalize_l2(vec![0.0, 0.0]).is_err());
        assert!(normalize_l2(vec![f32::NAN, 1.0]).is_err());
        assert!(normalize_l2(vec![f32::INFINITY, 1.0]).is_err());
        let vector = normalize_l2(vec![3.0, 4.0]).unwrap();
        assert!((vector[0] - 0.6).abs() < 1e-6);
        assert!((vector[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn cosine_requires_equal_finite_dimensions() {
        let first = normalize_l2(vec![1.0, 0.0]).unwrap();
        let second = normalize_l2(vec![0.0, 1.0]).unwrap();
        assert!(cosine_similarity_normalized(&first, &second).unwrap().abs() < 1e-6);
        assert!(cosine_similarity_normalized(&first, &[1.0]).is_err());
    }

    #[test]
    fn rrf_merges_duplicate_routes_without_comparing_raw_scores() {
        let fused = reciprocal_rank_fusion(
            &["a".into(), "b".into(), "c".into()],
            &["c".into(), "b".into(), "d".into()],
            60,
        );
        assert_eq!(fused[0].chunk_id, "c");
        assert_eq!(fused[1].chunk_id, "b");
        assert_eq!(fused.iter().filter(|item| item.chunk_id == "b").count(), 1);
    }

    #[tokio::test]
    async fn provider_mock_batches_retries_and_reports_progress() {
        let transient = EmbeddingError::new("EMBEDDING_RATE_LIMITED", "rate limited", true);
        let provider = MockProvider::new(
            vec![
                Err(transient),
                Ok(vec![vec![1.0, 0.0], vec![0.0, 1.0]]),
                Ok(vec![vec![1.0, 1.0]]),
            ],
            2,
        );
        let texts = vec!["a".into(), "b".into(), "c".into()];
        let mut progress = Vec::new();
        let vectors = embed_documents_batched(
            &provider,
            &texts,
            &CancellationToken::new(),
            fast_policy(),
            |update| progress.push(update),
        )
        .await
        .unwrap();
        assert_eq!(vectors.len(), 3);
        assert_eq!(*provider.calls.lock().unwrap(), vec![2, 2, 1]);
        assert_eq!(progress.last().unwrap().completed, 3);
        assert_eq!(progress.last().unwrap().retries, 1);
    }

    #[tokio::test]
    async fn batch_dimension_mismatch_rejects_the_batch() {
        let provider = MockProvider::new(vec![Ok(vec![vec![1.0, 0.0], vec![1.0, 0.0, 0.0]])], 8);
        let error = embed_documents_batched(
            &provider,
            &["a".into(), "b".into()],
            &CancellationToken::new(),
            fast_policy(),
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "EMBEDDING_DIMENSION_MISMATCH");
    }

    #[tokio::test]
    async fn cancellation_stops_before_provider_call() {
        let provider = MockProvider::new(vec![], 8);
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = embed_documents_batched(
            &provider,
            &["a".into()],
            &cancellation,
            fast_policy(),
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "EMBEDDING_CANCELLED");
        assert!(provider.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn openai_response_indexes_are_reordered_and_validated() {
        let vectors = ordered_response_vectors(
            vec![
                OpenAiEmbeddingData {
                    index: 1,
                    embedding: vec![0.0, 1.0],
                },
                OpenAiEmbeddingData {
                    index: 0,
                    embedding: vec![1.0, 0.0],
                },
            ],
            2,
        )
        .unwrap();
        assert_eq!(vectors[0], vec![1.0, 0.0]);
        assert!(ordered_response_vectors(
            vec![OpenAiEmbeddingData {
                index: 2,
                embedding: vec![1.0]
            }],
            1,
        )
        .is_err());
    }

    #[test]
    fn http_errors_have_stable_retry_classification_without_response_bodies() {
        assert!(http_status_error(StatusCode::TOO_MANY_REQUESTS).retryable);
        assert!(http_status_error(StatusCode::BAD_GATEWAY).retryable);
        assert!(!http_status_error(StatusCode::UNAUTHORIZED).retryable);
        assert!(!http_status_error(StatusCode::NOT_FOUND).retryable);
    }

    #[tokio::test]
    async fn real_http_provider_sends_openai_embedding_contract_and_auth_headers() {
        let cases = [
            (
                AuthScheme::ProtocolDefault,
                "authorization",
                "Bearer test-secret",
            ),
            (AuthScheme::Bearer, "authorization", "Bearer test-secret"),
            (AuthScheme::XApiKey, "x-api-key", "test-secret"),
            (AuthScheme::XGoogApiKey, "x-goog-api-key", "test-secret"),
        ];
        for (auth_scheme, expected_header, expected_value) in cases {
            let (base_url, captured, server) = spawn_http_mock(vec![HttpMockResponse {
                status: 200,
                body: http_response_body(&[(1, &[0.0, 1.0]), (0, &[3.0, 4.0])]),
                delay: Duration::ZERO,
            }])
            .await;
            let provider = OpenAiCompatibleEmbeddingProvider::new(
                Client::builder().build().unwrap(),
                http_spec(base_url, auth_scheme),
                "test-secret".to_string(),
            )
            .unwrap();
            let texts = vec!["document text".to_string(), "second text".to_string()];
            let vectors = provider
                .embed_documents(&texts, &CancellationToken::new())
                .await
                .unwrap();
            server.await.unwrap();

            assert_eq!(vectors.len(), 2);
            assert_eq!(vectors[0], vec![3.0, 4.0]);
            assert_eq!(vectors[1], vec![0.0, 1.0]);
            let requests = captured.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].path, "/v1/embeddings");
            assert_eq!(
                requests[0].headers.get(expected_header).map(String::as_str),
                Some(expected_value)
            );
            let body: JsonValue = serde_json::from_slice(&requests[0].body).unwrap();
            assert_eq!(body["model"], "test-embedding");
            assert_eq!(
                body["input"],
                serde_json::json!(["document text", "second text"])
            );
            assert_eq!(body["encoding_format"], "float");
        }
    }

    #[tokio::test]
    async fn real_http_provider_reorders_indexes_and_retries_rate_limits() {
        let (base_url, captured, server) = spawn_http_mock(vec![
            HttpMockResponse {
                status: 429,
                body: r#"{"error":"rate limited"}"#.to_string(),
                delay: Duration::ZERO,
            },
            HttpMockResponse {
                status: 200,
                body: http_response_body(&[(1, &[0.0, 1.0]), (0, &[1.0, 0.0])]),
                delay: Duration::ZERO,
            },
        ])
        .await;
        let provider = OpenAiCompatibleEmbeddingProvider::new(
            Client::builder().build().unwrap(),
            http_spec(base_url, AuthScheme::Bearer),
            "test-secret".to_string(),
        )
        .unwrap();
        let vectors = embed_documents_batched(
            &provider,
            &["one".to_string(), "two".to_string()],
            &CancellationToken::new(),
            EmbeddingRetryPolicy {
                max_retries: 1,
                request_timeout: Duration::from_secs(1),
                initial_backoff: Duration::from_millis(1),
            },
            |_| {},
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        assert_eq!(captured.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn real_http_provider_classifies_http_errors_without_leaking_body_or_key() {
        let cases = [
            (401, "EMBEDDING_AUTH_FAILED", false),
            (404, "EMBEDDING_MODEL_NOT_FOUND", false),
            (408, "EMBEDDING_TIMEOUT", true),
            (429, "EMBEDDING_RATE_LIMITED", true),
            (500, "EMBEDDING_PROVIDER_UNAVAILABLE", true),
        ];
        for (status, expected_code, retryable) in cases {
            let secret = "secret-that-must-not-appear";
            let (base_url, _captured, server) = spawn_http_mock(vec![HttpMockResponse {
                status,
                body: format!(r#"{{"error":"{secret}"}}"#),
                delay: Duration::ZERO,
            }])
            .await;
            let provider = OpenAiCompatibleEmbeddingProvider::new(
                Client::builder().build().unwrap(),
                http_spec(base_url, AuthScheme::Bearer),
                secret.to_string(),
            )
            .unwrap();
            let error = provider
                .embed_documents(&["text".to_string()], &CancellationToken::new())
                .await
                .unwrap_err();
            server.await.unwrap();
            assert_eq!(error.code, expected_code);
            assert_eq!(error.retryable, retryable);
            assert!(!error.message.contains(secret));
        }
    }

    #[tokio::test]
    async fn real_http_provider_timeout_is_cancellable_and_does_not_hang() {
        let (base_url, _captured, server) = spawn_http_mock(vec![HttpMockResponse {
            status: 200,
            body: http_response_body(&[(0, &[1.0, 0.0])]),
            delay: Duration::from_millis(100),
        }])
        .await;
        let provider = OpenAiCompatibleEmbeddingProvider::new(
            Client::builder().build().unwrap(),
            http_spec(base_url, AuthScheme::Bearer),
            "test-secret".to_string(),
        )
        .unwrap();
        let error = embed_documents_batched(
            &provider,
            &["slow".to_string()],
            &CancellationToken::new(),
            EmbeddingRetryPolicy {
                max_retries: 0,
                request_timeout: Duration::from_millis(10),
                initial_backoff: Duration::ZERO,
            },
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "EMBEDDING_TIMEOUT");
        tokio::time::timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
    }

    fn embedding_settings(model: &str) -> KnowledgeSettings {
        KnowledgeSettings {
            enabled: true,
            embedding_enabled: true,
            allow_remote_embedding: true,
            embedding_provider: "official-openai".to_string(),
            embedding_model: model.to_string(),
            ..KnowledgeSettings::default()
        }
    }

    #[test]
    fn route_resolution_validates_embedding_capability_and_dimensions() {
        let models = ModelSettings::default();
        let known =
            EmbeddingProviderSpec::resolve(&embedding_settings("text-embedding-3-small"), &models)
                .unwrap();
        assert_eq!(known.expected_dimensions, Some(1536));
        assert_eq!(known.protocol, ApiProtocol::OpenAiResponses);

        let error =
            EmbeddingProviderSpec::resolve(&embedding_settings("gpt-5.5"), &models).unwrap_err();
        assert_eq!(error.code, "EMBEDDING_MODEL_UNSUPPORTED");

        let unknown = EmbeddingProviderSpec::resolve(
            &embedding_settings("relay-new-embedding-model"),
            &models,
        )
        .unwrap();
        assert_eq!(unknown.expected_dimensions, None);
    }

    #[test]
    fn explicit_provider_model_embedding_override_can_allow_a_known_relay_name() {
        let mut models = ModelSettings::default();
        models.providers[0]
            .models
            .push(crate::settings::types::ProviderModelConfig {
                id: "relay-embedding".to_string(),
                api_model: "gpt-5.5".to_string(),
                display_name: "Relay embedding alias".to_string(),
                context_window_tokens: None,
                pricing: None,
                capabilities: Some(crate::settings::types::ModelCapabilities {
                    embedding: Some(true),
                    ..crate::settings::types::ModelCapabilities::default()
                }),
                enabled: true,
            });
        let settings = embedding_settings("gpt-5.5");
        let spec = EmbeddingProviderSpec::resolve(&settings, &models).unwrap();
        assert_eq!(spec.expected_dimensions, None);
    }

    #[test]
    fn disabled_knowledge_base_cannot_resolve_a_remote_embedding_route() {
        let mut settings = embedding_settings("relay-new-embedding-model");
        settings.enabled = false;
        let error =
            EmbeddingProviderSpec::resolve(&settings, &ModelSettings::default()).unwrap_err();
        assert_eq!(error.code, "EMBEDDING_KNOWLEDGE_DISABLED");
    }

    #[test]
    fn embedding_cache_identity_includes_route_and_credential_revision_without_secret() {
        let settings = embedding_settings("text-embedding-3-small");
        let first = EmbeddingProviderSpec::resolve(&settings, &ModelSettings::default()).unwrap();
        let mut changed_models = ModelSettings::default();
        changed_models.providers[0].credential_revision = 1;
        let second = EmbeddingProviderSpec::resolve(&settings, &changed_models).unwrap();
        assert_ne!(first.embedding_key, second.embedding_key);
        assert!(!first.embedding_key.contains("secret"));
        assert!(!second.embedding_key.contains("secret"));

        changed_models.providers[0].credential_revision = 0;
        changed_models.providers[0].auth_scheme = AuthScheme::XApiKey;
        let third = EmbeddingProviderSpec::resolve(&settings, &changed_models).unwrap();
        assert_ne!(first.embedding_key, third.embedding_key);
    }
}
