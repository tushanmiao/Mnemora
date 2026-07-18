use serde::{Deserialize, Serialize};

/** 网络协议与供应商名称分离，同一协议可以配置多个中转站。 */
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiProtocol {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
    GeminiGenerateContent,
}

/** API Key 的认证方式。 */
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthScheme {
    ProtocolDefault,
    Bearer,
    XApiKey,
    XGoogApiKey,
}

/** 手动网络操作的临时输入，不属于普通 Provider 配置返回值。 */
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnectionInput {
    pub base_url: String,
    pub api_key: String,
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
