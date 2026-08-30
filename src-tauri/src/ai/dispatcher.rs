//! AI 协议分派器。
//!
//! Chat 业务层只调用 `complete`。本模块根据 `ApiProtocol` 选择四个固定适配器，并在统一
//! 响应上补充本地总耗时。第一版不使用动态插件、自动重试或 API Key 轮换。

use std::time::Instant;

use reqwest::Client;
use tokio_util::sync::CancellationToken;

use super::{
    error::ModelError,
    providers::{anthropic, gemini, openai_chat, openai_responses},
    stream,
    types::{
        ApiProtocol, ModelRequest, ModelResponse, ModelStreamChunk, ModelStreamOutcome, ModelUsage,
        ProviderRequestContext,
    },
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

/// 按目标协议构建请求体并返回其序列化后的字节数。
///
/// 中转站限制的是 **body 字节数**，不是 token。两者在中文与 base64 图片上差异极大：
/// 一个汉字按 `estimate_text_tokens` 算 1 token，UTF-8 编码却是 3 字节；一张图片的
/// base64 几乎不占 token 估算却能轻易吃掉几 MB body。所以体积保护必须量字节，
/// 而且必须量**真正会发出去的那个 body**，不能量 prompt 字符串。
///
/// 代价是多做一次序列化。相对于一次动辄数分钟的模型调用可以忽略，因此只在需要闸门的
/// 调用方（深度笔记）使用，普通聊天不付这个开销。
#[cfg(test)]
pub fn request_body_bytes(
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
) -> Result<usize, ModelError> {
    request_body_bytes_for_transport(context, request, false)
}

/// 按实际传输形态计算请求体字节数。
///
/// 流式与非流式不是同一份 JSON：OpenAI Chat 还会多出 `stream_options`。P1 的
/// 上游请求遥测要按每个**物理请求**留痕，因此这里不能继续用非流式 body 近似流式
/// body，否则控制器拟合出来的包线会系统性偏小或偏大。
pub fn request_body_bytes_for_transport(
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
    streaming: bool,
) -> Result<usize, ModelError> {
    let mut body = match context.protocol {
        ApiProtocol::OpenAiChatCompletions => openai_chat::request_body(request),
        ApiProtocol::OpenAiResponses => openai_responses::request_body(request),
        ApiProtocol::AnthropicMessages => anthropic::request_body(request),
        ApiProtocol::GeminiGenerateContent => gemini::request_body(request),
    };
    if streaming {
        match context.protocol {
            ApiProtocol::OpenAiChatCompletions => {
                body["stream"] = serde_json::Value::Bool(true);
                body["stream_options"] = serde_json::json!({ "include_usage": true });
            }
            ApiProtocol::OpenAiResponses | ApiProtocol::AnthropicMessages => {
                body["stream"] = serde_json::Value::Bool(true);
            }
            // Gemini 通过不同 URL 选择流式传输，请求 body 不变。
            ApiProtocol::GeminiGenerateContent => {}
        }
    }
    serde_json::to_vec(&body)
        .map(|bytes| bytes.len())
        .map_err(|error| ModelError::invalid_response(format!("序列化模型请求体失败：{error}")))
}

/// 用流式请求换取一个非流式的 `ModelResponse`。
///
/// 存在的唯一理由是**保活**：非流式请求在生成期间连接完全静默，中转站的 idle 超时
/// 通常远短于长文生成需要的时间，于是我们主动撞上网关的 504。流式请求每来一个 token
/// 就有字节流动，idle 计时器被持续重置。
///
/// 这里不引入任何新的协议适配器 —— `ai::stream::stream` 已经覆盖四种协议、已接
/// `CancellationToken`、并统计 `first_token_ms`。本函数只把增量累积回一个完整响应，
/// 让调用方与 `complete` 完全同构、无感切换。
pub async fn complete_via_stream(
    client: &Client,
    context: &ProviderRequestContext<'_>,
    request: &ModelRequest,
    cancellation: &CancellationToken,
) -> Result<ModelResponse, ModelError> {
    let mut text = String::new();
    let mut reasoning = String::new();
    // 闭包对 text/reasoning 持可变借用，收进块里确保借用在读取之前结束。
    let outcome = {
        let mut on_chunk = |chunk: ModelStreamChunk| {
            match chunk {
                ModelStreamChunk::TextDelta(delta) => text.push_str(&delta),
                ModelStreamChunk::ReasoningDelta(delta) => reasoning.push_str(&delta),
                // 工具调用增量由各 provider 自行累积，结束时随 summary 一并返回，
                // 这里不需要重复拼装。
                ModelStreamChunk::ToolCallDelta { .. } => {}
            }
            Ok(())
        };
        stream::stream(client, context, request, cancellation, &mut on_chunk).await?
    };
    let summary = match outcome {
        ModelStreamOutcome::Completed(summary) => summary,
        ModelStreamOutcome::Cancelled => return Err(ModelError::cancelled()),
    };
    if text.trim().is_empty() && summary.tool_calls.is_empty() {
        return Err(ModelError::invalid_response(
            "模型流式响应没有返回任何正文或工具调用。",
        ));
    }
    // `stream::stream` 内部已经调用过 `apply_stream_metrics` 与 `normalize_usage`，
    // 这里不再重复归一化，否则会二次改写同一份用量。
    Ok(ModelResponse {
        text,
        reasoning: (!reasoning.trim().is_empty()).then_some(reasoning),
        finish_reason: summary.finish_reason,
        usage: summary.usage,
        tool_calls: summary.tool_calls,
    })
}

#[cfg(test)]
mod tests {
    use super::{request_body_bytes, request_body_bytes_for_transport};
    use crate::ai::types::{
        ApiProtocol, AuthScheme, ModelMessage, ModelOptions, ModelRequest, ModelRole,
        ProviderRequestContext,
    };

    const PROTOCOLS: [ApiProtocol; 4] = [
        ApiProtocol::OpenAiChatCompletions,
        ApiProtocol::OpenAiResponses,
        ApiProtocol::AnthropicMessages,
        ApiProtocol::GeminiGenerateContent,
    ];

    fn context(protocol: ApiProtocol) -> ProviderRequestContext<'static> {
        ProviderRequestContext {
            protocol,
            auth_scheme: AuthScheme::ProtocolDefault,
            base_url: "https://example.test/v1",
            api_key: "test-key",
        }
    }

    fn request(content: &str) -> ModelRequest {
        ModelRequest {
            model: "model-1".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: content.to_string(),
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions::default(),
            tools: Vec::new(),
        }
    }

    #[test]
    fn every_protocol_can_be_weighed_before_sending() {
        for protocol in PROTOCOLS {
            let bytes = request_body_bytes(&context(protocol), &request("hello")).unwrap();
            assert!(bytes > 0, "{protocol:?} 的请求体字节数应当可测");
            let streaming =
                request_body_bytes_for_transport(&context(protocol), &request("hello"), true)
                    .unwrap();
            assert!(streaming > 0, "{protocol:?} 的流式请求体字节数应当可测");
        }
    }

    #[test]
    fn streaming_openai_chat_body_includes_stream_options() {
        let protocol = ApiProtocol::OpenAiChatCompletions;
        let plain = request_body_bytes(&context(protocol), &request("hello")).unwrap();
        let streaming =
            request_body_bytes_for_transport(&context(protocol), &request("hello"), true).unwrap();
        assert!(
            streaming > plain,
            "流式 OpenAI Chat body 应包含 stream_options：{plain} -> {streaming}"
        );
    }

    #[test]
    fn byte_count_grows_with_payload_on_every_protocol() {
        for protocol in PROTOCOLS {
            let small = request_body_bytes(&context(protocol), &request("hi")).unwrap();
            let large =
                request_body_bytes(&context(protocol), &request(&"hi".repeat(10_000))).unwrap();
            assert!(
                large > small + 10_000,
                "{protocol:?} 的字节数没有随载荷增长：{small} → {large}"
            );
        }
    }

    /// 这是选字节而非 token 的全部理由：一个中文字符是 1 token、3 字节。
    /// 用 token 估算去挡网关的字节上限，会系统性低估约三倍。
    #[test]
    fn multibyte_text_is_weighed_in_bytes_not_characters() {
        let protocol = ApiProtocol::OpenAiChatCompletions;
        let ascii = request_body_bytes(&context(protocol), &request(&"a".repeat(1_000))).unwrap();
        let chinese =
            request_body_bytes(&context(protocol), &request(&"中".repeat(1_000))).unwrap();
        assert!(
            chinese >= ascii + 2_000,
            "等字符数的中文应当重出约两倍字节：{ascii} vs {chinese}"
        );
    }
}
