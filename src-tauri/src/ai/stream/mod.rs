//! 四协议流式调用统一入口。
//!
//! `sse` 负责有界分帧；各 provider 负责请求 JSON 和事件解析；本文件只做协议分派、
//! 取消期间的 HTTP 建连和本地总耗时统计。流式路径与非流式路径都不自动重试。

pub(crate) mod sse;

use std::time::Instant;

use reqwest::{Client, RequestBuilder};
use tokio_util::sync::CancellationToken;

use super::{
    error::ModelError,
    providers::{anthropic, gemini, openai_chat, openai_responses},
    types::{
        ApiProtocol, ModelRequest, ModelStreamChunk, ModelStreamOutcome, ModelUsage,
        ProviderRequestContext,
    },
};
use sse::SseReadOutcome;

pub(crate) async fn send_sse_request<F>(
    request: RequestBuilder,
    api_key: &str,
    cancellation: &CancellationToken,
    on_event: F,
) -> Result<SseReadOutcome, ModelError>
where
    F: FnMut(sse::SseEvent) -> Result<(), ModelError>,
{
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Ok(SseReadOutcome::Cancelled),
        response = request.send() => response.map_err(ModelError::from_reqwest)?,
    };
    if !response.status().is_success() {
        return Err(ModelError::from_response(response, api_key).await);
    }
    sse::consume(response, cancellation, on_event).await
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
    let started_at = Instant::now();
    let mut outcome = match context.protocol {
        ApiProtocol::OpenAiChatCompletions => {
            openai_chat::stream(client, context, request, cancellation, on_chunk).await
        }
        ApiProtocol::OpenAiResponses => {
            openai_responses::stream(client, context, request, cancellation, on_chunk).await
        }
        ApiProtocol::AnthropicMessages => {
            anthropic::stream(client, context, request, cancellation, on_chunk).await
        }
        ApiProtocol::GeminiGenerateContent => {
            gemini::stream(client, context, request, cancellation, on_chunk).await
        }
    }?;

    if let ModelStreamOutcome::Completed(summary) = &mut outcome {
        let duration_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
        summary
            .usage
            .get_or_insert_with(ModelUsage::default)
            .total_duration_ms = Some(duration_ms);
    }
    Ok(outcome)
}
