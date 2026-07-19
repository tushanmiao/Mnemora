//! AI 层的供应商无关数据合同。
//!
//! - `ModelMessage` / `ModelRequest`：Chat 业务层交给协议适配器的统一输入。
//! - `ModelResponse` / `ModelUsage`：四种协议转换后的统一输出。
//! - `ModelStreamChunk` / `ModelStreamOutcome`：四种流式协议转换后的统一增量和终态。
//! - `ProviderRequestContext`：一次请求所需的协议、地址和临时密钥引用，不会返回前端。
//! - `ProviderConnectionInput` / `ConnectionTestResult`：设置页手动网络操作使用的独立合同。

use serde::{Deserialize, Serialize};

pub use crate::settings::types::{ApiProtocol, AuthScheme};

/** 模型历史消息只保留第一版实际支持的用户和助手角色。 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelRole {
    User,
    Assistant,
}

/** 发送给模型的一条纯文本历史消息。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMessage {
    pub role: ModelRole,
    pub content: String,
}

/** 四种协议都能合理映射的最小公共生成参数。 */
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptions {
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
}

/** 已解析为 API Model 后的统一模型请求。 */
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<ModelMessage>,
    pub options: ModelOptions,
}

/** 一次请求使用的供应商运行时上下文；API Key 只以短生命周期引用存在。 */
pub struct ProviderRequestContext<'a> {
    pub protocol: ApiProtocol,
    pub auth_scheme: AuthScheme,
    pub base_url: &'a str,
    pub api_key: &'a str,
}

/** 四种协议统一后的 Token 用量与本地耗时。 */
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    pub total_duration_ms: Option<u64>,
}

/** 非流式模型调用的统一结果。 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponse {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
}

/** 流式适配器产生的增量；第一版只向界面暴露纯文本。 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelStreamChunk {
    TextDelta(String),
}

/** 供应商流正常结束时汇总的停止原因和用量。 */
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelStreamSummary {
    pub finish_reason: Option<String>,
    pub usage: Option<ModelUsage>,
}

/** 区分供应商正常结束和用户主动取消，不把取消伪装成模型错误。 */
#[derive(Debug, Clone, PartialEq, Eq)]
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
