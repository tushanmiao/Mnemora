//! 用量持久化、聚合和分页查询的数据合同。

use serde::{Deserialize, Serialize};

use crate::ai::types::{ModelUsage, PricingSnapshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: String,
    pub created_at_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub time_to_first_token_ms: Option<u64>,
    #[serde(default)]
    pub generation_duration_ms: Option<u64>,
    #[serde(default)]
    pub output_tokens_per_second: Option<f64>,
    pub source: String,
    pub operation: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub api_model: String,
    pub display_name: String,
    pub protocol: String,
    pub status: String,
    #[serde(default)]
    pub status_code: Option<u16>,
    pub usage_source: String,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub non_cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub context_input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub cost_source: Option<String>,
    #[serde(default)]
    pub pricing_snapshot: Option<PricingSnapshot>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub round_index: Option<u32>,
    #[serde(default)]
    pub call_index: Option<u32>,
    #[serde(default)]
    pub parent_operation: Option<String>,
    #[serde(default)]
    pub activated_skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_definition_count: u32,
    #[serde(default)]
    pub tool_call_count: u32,
    #[serde(default)]
    pub error_kind: Option<String>,
}

impl UsageRecord {
    /// Agent 内部统一记录为 agentModelCall；统计界面优先展示用户可理解的父操作。
    pub fn logical_operation(&self) -> &str {
        self.parent_operation
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.operation)
    }
}

#[derive(Debug, Clone)]
pub struct UsageRecordInput {
    pub created_at_ms: u64,
    pub duration_ms: u64,
    pub source: String,
    pub operation: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub api_model: String,
    pub display_name: String,
    pub protocol: String,
    pub status: String,
    pub status_code: Option<u16>,
    pub usage: Option<ModelUsage>,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub run_id: Option<String>,
    pub round_index: Option<u32>,
    pub call_index: Option<u32>,
    pub parent_operation: Option<String>,
    pub activated_skill_ids: Vec<String>,
    pub tool_definition_count: u32,
    pub tool_call_count: u32,
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatsQuery {
    #[serde(default)]
    pub since_ms: Option<u64>,
    #[serde(default)]
    pub until_ms: Option<u64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub usage_source: Option<String>,
    #[serde(default)]
    pub bucket_ms: Option<u64>,
    #[serde(default)]
    pub bucket_count: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl UsageStatsQuery {
    pub fn matches(&self, record: &UsageRecord) -> bool {
        self.since_ms
            .is_none_or(|value| record.created_at_ms >= value)
            && self
                .until_ms
                .is_none_or(|value| record.created_at_ms < value)
            && matches_optional(&self.source, &record.source)
            && matches_optional(&self.operation, record.logical_operation())
            && matches_optional(&self.status, &record.status)
            && matches_optional(&self.provider_id, &record.provider_id)
            && matches_optional(&self.model_id, &record.model_id)
            && matches_optional(&self.protocol, &record.protocol)
            && matches_optional(&self.usage_source, &record.usage_source)
    }
}

fn matches_optional(expected: &Option<String>, actual: &str) -> bool {
    expected
        .as_deref()
        .is_none_or(|value| value.trim().is_empty() || value == actual)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub stopped_requests: u64,
    pub provider_reported_requests: u64,
    pub gateway_normalized_requests: u64,
    pub estimated_usage_requests: u64,
    pub missing_usage_requests: u64,
    /// 已经拿到 Token（官方、网关归一化或本地估算）的请求数。
    pub known_usage_requests: u64,
    /// 成本只有部分 Token 类别可以计算的请求数。
    pub partial_cost_requests: u64,
    /// 没有可用成本数据的请求数。
    pub missing_cost_requests: u64,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub non_cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub average_duration_ms: Option<f64>,
    pub average_time_to_first_token_ms: Option<f64>,
    pub average_output_tokens_per_second: Option<f64>,
    pub total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTrendPoint {
    pub bucket_index: usize,
    pub started_at_ms: u64,
    pub requests: u64,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageGroupStats {
    pub id: String,
    pub label: String,
    pub provider_id: String,
    pub provider_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_model: Option<String>,
    pub request_count: u64,
    pub success_count: u64,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
    pub average_duration_ms: Option<f64>,
    pub last_used_at_ms: Option<u64>,
}

/// 用于筛选器的稳定选项。它们只按时间范围生成，不会因为其他筛选条件而消失。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilterOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageModelFilterOption {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub api_model: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilterOptions {
    pub providers: Vec<UsageFilterOption>,
    pub models: Vec<UsageModelFilterOption>,
    pub operations: Vec<UsageFilterOption>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryResponse {
    pub summary: UsageSummary,
    pub trend: Vec<UsageTrendPoint>,
    pub provider_stats: Vec<UsageGroupStats>,
    pub model_stats: Vec<UsageGroupStats>,
    pub operation_stats: Vec<UsageGroupStats>,
    pub filter_options: UsageFilterOptions,
    pub total_logs: usize,
    pub skipped_records: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecordsPage {
    pub records: Vec<UsageRecord>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub total_matching: usize,
    pub skipped_records: usize,
}

/** 兼容旧前端的一次性响应；新界面使用摘要和明细两个独立命令。 */
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatsResponse {
    pub summary: UsageSummary,
    pub trend: Vec<UsageTrendPoint>,
    pub logs: Vec<UsageRecord>,
    pub provider_stats: Vec<UsageGroupStats>,
    pub model_stats: Vec<UsageGroupStats>,
    pub operation_stats: Vec<UsageGroupStats>,
    pub filter_options: UsageFilterOptions,
    pub total_logs: usize,
    pub skipped_records: usize,
    pub next_cursor: Option<String>,
}
