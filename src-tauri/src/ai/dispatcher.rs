//! AI 协议分派器。
//!
//! Chat 业务层只调用 `complete`。本模块根据 `ApiProtocol` 选择四个固定适配器，并在统一
//! 响应上补充本地总耗时。第一版不使用动态插件、自动重试或 API Key 轮换。

use std::time::Instant;

use reqwest::Client;

use super::{
    error::ModelError,
    providers::{anthropic, gemini, openai_chat, openai_responses},
    types::{ApiProtocol, ModelRequest, ModelResponse, ModelUsage, ProviderRequestContext},
};
use crate::usage::normalize::normalize_usage;

pub async fn complete(
    client: &Client,
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
) -> Result<ModelResponse, ModelError> {
    let started_at = Instant::now();
    let mut response = match context.protocol {
        ApiProtocol::OpenAiChatCompletions => openai_chat::complete(client, context, request).await,
        ApiProtocol::OpenAiResponses => openai_responses::complete(client, context, request).await,
        ApiProtocol::AnthropicMessages => anthropic::complete(client, context, request).await,
        ApiProtocol::GeminiGenerateContent => gemini::complete(client, context, request).await,
    }?;

    let duration_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
    response
        .usage
        .get_or_insert_with(ModelUsage::default)
        .total_duration_ms = Some(duration_ms);
    if let Some(usage) = response.usage.take() {
        response.usage = Some(normalize_usage(context.protocol, usage));
    }
    Ok(response)
}
