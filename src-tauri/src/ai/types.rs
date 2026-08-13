//! AI 层的供应商无关数据合同。
//!
//! - `ModelMessage` / `ModelRequest`：Chat 业务层交给协议适配器的统一输入。
//! - `ModelResponse` / `ModelUsage`：四种协议转换后的统一输出。
//! - `ModelStreamChunk` / `ModelStreamOutcome`：四种流式协议转换后的统一增量和终态。
//! - `ProviderRequestContext`：一次请求所需的协议、地址和临时密钥引用，不会返回前端。
//! - `ProviderConnectionInput` / `ConnectionTestResult`：设置页手动网络操作使用的独立合同。

use serde::{Deserialize, Serialize};

pub use crate::settings::types::{ApiProtocol, AuthScheme};

/** 统一消息角色；System Prompt 保持在请求顶层，工具结果使用 Tool。 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelRole {
    User,
    Assistant,
    Tool,
}

/** 请求生命周期内使用的图片正文；Base64 不进入会话持久化。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelImage {
    pub name: String,
    pub media_type: String,
    pub data_base64: String,
}

/** 发送给模型的一条纯文本历史消息。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMessage {
    pub role: ModelRole,
    pub content: String,
    #[serde(default)]
    pub images: Vec<ModelImage>,
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
    #[serde(default)]
    pub tool_result: Option<ModelToolResult>,
}

/** 提供给模型的供应商无关函数工具定义。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/** 模型产生的结构化工具调用；`provider_signature` 用于回放 Gemini 签名。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_signature: Option<String>,
}

/** 工具执行完成后回传模型的有界结果。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelToolResult {
    pub call_id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
}

/** 四种协议都能合理映射的最小公共生成参数。 */
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptions {
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/** 已解析为 API Model 后的统一模型请求。 */
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<ModelMessage>,
    pub options: ModelOptions,
    pub tools: Vec<ModelTool>,
}

/** 一次请求使用的供应商运行时上下文；API Key 只以短生命周期引用存在。 */
pub struct ProviderRequestContext<'a> {
    pub protocol: ApiProtocol,
    pub auth_scheme: AuthScheme,
    pub base_url: &'a str,
    pub api_key: &'a str,
}

/** 四种协议统一后的 Token 用量与本地耗时。 */
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSource {
    ProviderReported,
    GatewayNormalized,
    Estimated,
    #[default]
    Missing,
}

/** 一次调用计算成本时使用的价格副本，避免历史成本随设置变化。 */
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingSnapshot {
    pub input_per_million: Option<f64>,
    pub output_per_million: Option<f64>,
    pub cache_read_per_million: Option<f64>,
    pub cache_write_per_million: Option<f64>,
    pub currency: String,
    pub captured_at_ms: u64,
    pub settings_version: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_cached_input_tokens: Option<u64>,
    /** 最后一次成功模型调用的有效输入，用于上下文圆环，不是多轮累计值。 */
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_input_tokens: Option<u64>,
    #[serde(default)]
    pub usage_source: UsageSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_first_token_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_snapshot: Option<PricingSnapshot>,
    #[serde(default = "default_call_count")]
    pub call_count: u32,
}

fn default_call_count() -> u32 {
    1
}

/** 非流式模型调用的统一结果。 */
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponse {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ModelToolCall>,
}

/** 流式适配器产生的正文或思考增量，两者在界面和存储中保持独立。 */
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum ModelStreamChunk {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
        provider_signature: Option<String>,
    },
}

/** 供应商流正常结束时汇总的停止原因和用量。 */
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelStreamSummary {
    pub finish_reason: Option<String>,
    pub usage: Option<ModelUsage>,
    pub tool_calls: Vec<ModelToolCall>,
}

/** 区分供应商正常结束和用户主动取消，不把取消伪装成模型错误。 */
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ModelStreamOutcome {
    Completed(ModelStreamSummary),
    Cancelled,
}

/** 手动网络操作的临时输入，不属于普通 Provider 配置返回值。 */
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionInput {
    #[serde(default)]
    pub provider_id: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub api_key: Option<String>,
    pub protocol: ApiProtocol,
    pub auth_scheme: AuthScheme,
}

/** 手动连接测试结果。成功只代表本次请求成功，不代表持续在线。 */
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub success: bool,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
