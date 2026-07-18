use serde::{Deserialize, Serialize};

pub use crate::settings::types::{ApiProtocol, AuthScheme};

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
