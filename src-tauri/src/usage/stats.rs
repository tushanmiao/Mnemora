//! 用量记录的流式聚合和有界分页。

use std::collections::BTreeMap;
use std::path::Path;

use super::{
    storage,
    types::{
        UsageFilterOption, UsageFilterOptions, UsageGroupStats, UsageModelFilterOption,
        UsageRecord, UsageRecordsPage, UsageStatsQuery, UsageSummary, UsageSummaryResponse,
        UsageTrendPoint,
    },
};

const DEFAULT_LOG_LIMIT: usize = 50;
const MAX_LOG_LIMIT: usize = 200;
const MAX_BUCKETS: usize = 120;

#[derive(Default)]
struct GroupAccumulator {
    id: String,
    label: String,
    provider_id: String,
    provider_name: String,
    model_id: Option<String>,
    api_model: Option<String>,
    request_count: u64,
    success_count: u64,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    cost_usd: f64,
    duration_total: u64,
    last_used_at_ms: Option<u64>,
}

#[derive(Clone, Copy)]
enum GroupKind {
    Provider,
    Model,
    Operation,
}

pub fn build_summary(dir: &Path, query: UsageStatsQuery) -> UsageSummaryResponse {
    let mut accumulator = SummaryAccumulator::new(&query);
    let mut filter_options = FilterOptionAccumulator::default();
    let skipped_records = storage::visit_records(dir, query.since_ms, query.until_ms, |record| {
        // 筛选器目录只受时间范围影响，避免选择一个 Provider 后其他选项消失。
        filter_options.push(&record);
        if query.matches(&record) {
            accumulator.push(&record);
        }
    });
    accumulator.finish(skipped_records, filter_options.finish())
}

pub fn build_records_page(dir: &Path, query: UsageStatsQuery) -> UsageRecordsPage {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LOG_LIMIT)
        .clamp(1, MAX_LOG_LIMIT);
    let cursor = query.cursor.as_deref().and_then(decode_cursor);
    let mut records = Vec::with_capacity(limit + 1);
    let mut total_matching = 0usize;
    let skipped_records = storage::visit_records(dir, query.since_ms, query.until_ms, |record| {
        if !query.matches(&record) {
            return;
        }
        total_matching = total_matching.saturating_add(1);
        if !is_before_cursor(&record, cursor.as_ref()) {
            return;
        }
        let index = records
            .binary_search_by(|current: &UsageRecord| compare_records_desc(current, &record))
            .unwrap_or_else(|index| index);
        records.insert(index, record);
        if records.len() > limit + 1 {
            records.pop();
        }
    });
    let has_more = records.len() > limit;
    records.truncate(limit);
    let next_cursor = has_more
        .then(|| records.last().map(encode_cursor))
        .flatten();
    UsageRecordsPage {
        records,
        next_cursor,
        has_more,
        total_matching,
        skipped_records,
    }
}

struct SummaryAccumulator {
    summary: UsageSummary,
    duration_total: u64,
    ttft_total: u64,
    ttft_count: u64,
    speed_output_tokens: u64,
    speed_generation_ms: u64,
    fallback_speed_total: f64,
    fallback_speed_count: u64,
    cost_total: f64,
    has_cost: bool,
    trend: Vec<UsageTrendPoint>,
    since_ms: Option<u64>,
    bucket_ms: u64,
    provider_groups: BTreeMap<String, GroupAccumulator>,
    model_groups: BTreeMap<String, GroupAccumulator>,
    operation_groups: BTreeMap<String, GroupAccumulator>,
    total_logs: usize,
}

#[derive(Default)]
struct FilterOptionAccumulator {
    providers: BTreeMap<String, UsageFilterOption>,
    models: BTreeMap<String, UsageModelFilterOption>,
    operations: BTreeMap<String, UsageFilterOption>,
}

impl FilterOptionAccumulator {
    fn push(&mut self, record: &UsageRecord) {
        self.providers
            .entry(record.provider_id.clone())
            .or_insert_with(|| UsageFilterOption {
                id: record.provider_id.clone(),
                label: record.provider_name.clone(),
            });
        // Stable ID 不允许竖线，可安全地用作前端复合筛选键分隔符。
        let model_id = format!("{}|{}", record.provider_id, record.model_id);
        self.models
            .entry(model_id.clone())
            .or_insert_with(|| UsageModelFilterOption {
                id: model_id,
                provider_id: record.provider_id.clone(),
                provider_name: record.provider_name.clone(),
                model_id: record.model_id.clone(),
                api_model: record.api_model.clone(),
                label: record.display_name.clone(),
            });
        let logical_operation = record.logical_operation();
        let operation_id = format!("operation:{logical_operation}");
        self.operations
            .entry(operation_id.clone())
            .or_insert_with(|| UsageFilterOption {
                id: operation_id,
                label: logical_operation.to_string(),
            });
    }

    fn finish(self) -> UsageFilterOptions {
        let mut providers = self.providers.into_values().collect::<Vec<_>>();
        let mut models = self.models.into_values().collect::<Vec<_>>();
        let mut operations = self.operations.into_values().collect::<Vec<_>>();
        providers.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
        models.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
        operations.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
        UsageFilterOptions {
            providers,
            models,
            operations,
        }
    }
}

impl SummaryAccumulator {
    fn new(query: &UsageStatsQuery) -> Self {
        let bucket_ms = query.bucket_ms.unwrap_or(24 * 60 * 60 * 1_000).max(1);
        let bucket_count = query.bucket_count.unwrap_or(7).clamp(1, MAX_BUCKETS);
        let trend = query.since_ms.map_or_else(Vec::new, |since_ms| {
            (0..bucket_count)
                .map(|bucket_index| UsageTrendPoint {
                    bucket_index,
                    started_at_ms: since_ms
                        .saturating_add(bucket_ms.saturating_mul(bucket_index as u64)),
                    ..UsageTrendPoint::default()
                })
                .collect()
        });
        Self {
            trend,
            since_ms: query.since_ms,
            bucket_ms,
            ..Self::default()
        }
    }

    fn push(&mut self, record: &UsageRecord) {
        self.total_logs = self.total_logs.saturating_add(1);
        self.summary.total_requests = self.summary.total_requests.saturating_add(1);
        match record.status.as_str() {
            "success" => {
                self.summary.successful_requests =
                    self.summary.successful_requests.saturating_add(1)
            }
            "stopped" => {
                self.summary.stopped_requests = self.summary.stopped_requests.saturating_add(1)
            }
            _ => self.summary.failed_requests = self.summary.failed_requests.saturating_add(1),
        }
        match record.usage_source.as_str() {
            "providerReported" => {
                self.summary.provider_reported_requests =
                    self.summary.provider_reported_requests.saturating_add(1)
            }
            "gatewayNormalized" => {
                self.summary.gateway_normalized_requests =
                    self.summary.gateway_normalized_requests.saturating_add(1)
            }
            "estimated" => {
                self.summary.estimated_usage_requests =
                    self.summary.estimated_usage_requests.saturating_add(1)
            }
            "missing" => {
                self.summary.missing_usage_requests =
                    self.summary.missing_usage_requests.saturating_add(1)
            }
            _ => {
                self.summary.missing_usage_requests =
                    self.summary.missing_usage_requests.saturating_add(1)
            }
        }
        add_tokens_to_summary(&mut self.summary, record);
        self.duration_total = self.duration_total.saturating_add(record.duration_ms);
        if let Some(value) = record.time_to_first_token_ms {
            self.ttft_total = self.ttft_total.saturating_add(value);
            self.ttft_count = self.ttft_count.saturating_add(1);
        }
        if let (Some(output), Some(generation_ms)) = (
            record.output_tokens,
            record.generation_duration_ms.filter(|value| *value > 0),
        ) {
            self.speed_output_tokens = self.speed_output_tokens.saturating_add(output);
            self.speed_generation_ms = self.speed_generation_ms.saturating_add(generation_ms);
        } else if let Some(value) = record
            .output_tokens_per_second
            .filter(|value| value.is_finite() && *value >= 0.0)
        {
            self.fallback_speed_total += value;
            self.fallback_speed_count = self.fallback_speed_count.saturating_add(1);
        }
        let record_cost = valid_cost(record);
        if let Some(value) = record_cost {
            self.cost_total += value;
            self.has_cost = true;
        }
        match (record.cost_source.as_deref(), record_cost) {
            (Some("localCalculated") | Some("providerReported"), Some(_)) => {}
            (Some("localPartial") | Some("localEstimated"), Some(_)) => {
                self.summary.partial_cost_requests =
                    self.summary.partial_cost_requests.saturating_add(1);
            }
            _ => {
                self.summary.missing_cost_requests =
                    self.summary.missing_cost_requests.saturating_add(1);
            }
        }
        self.push_trend(record);
        push_group(&mut self.provider_groups, record, GroupKind::Provider);
        push_group(&mut self.model_groups, record, GroupKind::Model);
        push_group(&mut self.operation_groups, record, GroupKind::Operation);
    }

    fn push_trend(&mut self, record: &UsageRecord) {
        let Some(since_ms) = self.since_ms else {
            return;
        };
        let bucket_index = record.created_at_ms.saturating_sub(since_ms) / self.bucket_ms;
        let Some(point) = self.trend.get_mut(bucket_index as usize) else {
            return;
        };
        point.requests = point.requests.saturating_add(1);
        point.total_tokens = point
            .total_tokens
            .saturating_add(record_total_tokens(record));
        point.input_tokens = point
            .input_tokens
            .saturating_add(record_input_tokens(record));
        point.output_tokens = point
            .output_tokens
            .saturating_add(record.output_tokens.unwrap_or(0));
        point.cache_read_tokens = point
            .cache_read_tokens
            .saturating_add(record.cache_read_tokens.unwrap_or(0));
        point.cache_write_tokens = point
            .cache_write_tokens
            .saturating_add(record.cache_write_tokens.unwrap_or(0));
        point.cost_usd += valid_cost(record).unwrap_or(0.0);
    }

    fn finish(
        mut self,
        skipped_records: usize,
        filter_options: UsageFilterOptions,
    ) -> UsageSummaryResponse {
        if self.summary.total_requests > 0 {
            self.summary.average_duration_ms =
                Some(self.duration_total as f64 / self.summary.total_requests as f64);
        }
        if self.ttft_count > 0 {
            self.summary.average_time_to_first_token_ms =
                Some(self.ttft_total as f64 / self.ttft_count as f64);
        }
        if self.speed_generation_ms > 0 {
            self.summary.average_output_tokens_per_second =
                Some(self.speed_output_tokens as f64 * 1_000.0 / self.speed_generation_ms as f64);
        } else if self.fallback_speed_count > 0 {
            self.summary.average_output_tokens_per_second =
                Some(self.fallback_speed_total / self.fallback_speed_count as f64);
        }
        self.summary.total_cost_usd = self.has_cost.then_some(self.cost_total);
        self.summary.known_usage_requests = self
            .summary
            .total_requests
            .saturating_sub(self.summary.missing_usage_requests);
        UsageSummaryResponse {
            summary: self.summary,
            trend: self.trend,
            provider_stats: finish_groups(self.provider_groups),
            model_stats: finish_groups(self.model_groups),
            operation_stats: finish_groups(self.operation_groups),
            filter_options,
            total_logs: self.total_logs,
            skipped_records,
        }
    }
}

impl Default for SummaryAccumulator {
    fn default() -> Self {
        Self {
            summary: UsageSummary::default(),
            duration_total: 0,
            ttft_total: 0,
            ttft_count: 0,
            speed_output_tokens: 0,
            speed_generation_ms: 0,
            fallback_speed_total: 0.0,
            fallback_speed_count: 0,
            cost_total: 0.0,
            has_cost: false,
            trend: Vec::new(),
            since_ms: None,
            bucket_ms: 1,
            provider_groups: BTreeMap::new(),
            model_groups: BTreeMap::new(),
            operation_groups: BTreeMap::new(),
            total_logs: 0,
        }
    }
}

fn add_tokens_to_summary(summary: &mut UsageSummary, record: &UsageRecord) {
    summary.total_tokens = summary
        .total_tokens
        .saturating_add(record_total_tokens(record));
    summary.input_tokens = summary
        .input_tokens
        .saturating_add(record_input_tokens(record));
    summary.non_cached_input_tokens = summary
        .non_cached_input_tokens
        .saturating_add(record.non_cached_input_tokens.unwrap_or(0));
    summary.output_tokens = summary
        .output_tokens
        .saturating_add(record.output_tokens.unwrap_or(0));
    summary.reasoning_tokens = summary
        .reasoning_tokens
        .saturating_add(record.reasoning_tokens.unwrap_or(0));
    summary.cache_read_tokens = summary
        .cache_read_tokens
        .saturating_add(record.cache_read_tokens.unwrap_or(0));
    summary.cache_write_tokens = summary
        .cache_write_tokens
        .saturating_add(record.cache_write_tokens.unwrap_or(0));
}

fn record_total_tokens(record: &UsageRecord) -> u64 {
    record
        .total_tokens
        .or_else(|| {
            record
                .input_tokens
                .zip(record.output_tokens)
                .map(|(input, output)| input.saturating_add(output))
        })
        .unwrap_or(0)
}

fn record_input_tokens(record: &UsageRecord) -> u64 {
    record
        .input_tokens
        .or_else(|| {
            record
                .non_cached_input_tokens
                .zip(record.cache_read_tokens.or(Some(0)))
                .map(|(non_cached, cache_read)| {
                    non_cached
                        .saturating_add(cache_read)
                        .saturating_add(record.cache_write_tokens.unwrap_or(0))
                })
        })
        .unwrap_or(0)
}

fn valid_cost(record: &UsageRecord) -> Option<f64> {
    record
        .cost_usd
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn push_group(
    groups: &mut BTreeMap<String, GroupAccumulator>,
    record: &UsageRecord,
    kind: GroupKind,
) {
    let (key, label, model_id, api_model) = match kind {
        GroupKind::Provider => (
            record.provider_id.clone(),
            record.provider_name.clone(),
            None,
            None,
        ),
        GroupKind::Model => (
            format!("{}|{}", record.provider_id, record.model_id),
            record.display_name.clone(),
            Some(record.model_id.clone()),
            Some(record.api_model.clone()),
        ),
        GroupKind::Operation => (
            format!("operation:{}", record.logical_operation()),
            record.logical_operation().to_string(),
            None,
            None,
        ),
    };
    let group = groups
        .entry(key.clone())
        .or_insert_with(|| GroupAccumulator {
            id: key,
            label,
            provider_id: matches!(kind, GroupKind::Operation)
                .then(String::new)
                .unwrap_or_else(|| record.provider_id.clone()),
            provider_name: matches!(kind, GroupKind::Operation)
                .then(|| "操作".to_string())
                .unwrap_or_else(|| record.provider_name.clone()),
            model_id,
            api_model,
            ..GroupAccumulator::default()
        });
    group.request_count = group.request_count.saturating_add(1);
    if record.status == "success" {
        group.success_count = group.success_count.saturating_add(1);
    }
    group.total_tokens = group
        .total_tokens
        .saturating_add(record_total_tokens(record));
    group.input_tokens = group
        .input_tokens
        .saturating_add(record_input_tokens(record));
    group.output_tokens = group
        .output_tokens
        .saturating_add(record.output_tokens.unwrap_or(0));
    group.cache_read_tokens = group
        .cache_read_tokens
        .saturating_add(record.cache_read_tokens.unwrap_or(0));
    group.cache_write_tokens = group
        .cache_write_tokens
        .saturating_add(record.cache_write_tokens.unwrap_or(0));
    group.cost_usd += valid_cost(record).unwrap_or(0.0);
    group.duration_total = group.duration_total.saturating_add(record.duration_ms);
    group.last_used_at_ms = Some(group.last_used_at_ms.unwrap_or(0).max(record.created_at_ms));
}

fn finish_groups(groups: BTreeMap<String, GroupAccumulator>) -> Vec<UsageGroupStats> {
    let mut rows = groups
        .into_values()
        .map(|group| UsageGroupStats {
            id: group.id,
            label: group.label,
            provider_id: group.provider_id,
            provider_name: group.provider_name,
            model_id: group.model_id,
            api_model: group.api_model,
            request_count: group.request_count,
            success_count: group.success_count,
            total_tokens: group.total_tokens,
            input_tokens: group.input_tokens,
            output_tokens: group.output_tokens,
            cache_read_tokens: group.cache_read_tokens,
            cache_write_tokens: group.cache_write_tokens,
            cost_usd: group.cost_usd,
            average_duration_ms: (group.request_count > 0)
                .then_some(group.duration_total as f64 / group.request_count as f64),
            last_used_at_ms: group.last_used_at_ms,
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .total_tokens
            .cmp(&left.total_tokens)
            .then_with(|| right.request_count.cmp(&left.request_count))
            .then_with(|| left.id.cmp(&right.id))
    });
    rows
}

fn compare_records_desc(left: &UsageRecord, right: &UsageRecord) -> std::cmp::Ordering {
    right
        .created_at_ms
        .cmp(&left.created_at_ms)
        .then_with(|| right.id.cmp(&left.id))
}

fn encode_cursor(record: &UsageRecord) -> String {
    format!("{}:{}", record.created_at_ms, record.id)
}

fn decode_cursor(value: &str) -> Option<(u64, String)> {
    let (timestamp, id) = value.split_once(':')?;
    Some((timestamp.parse().ok()?, id.to_string()))
}

fn is_before_cursor(record: &UsageRecord, cursor: Option<&(u64, String)>) -> bool {
    cursor.is_none_or(|(timestamp, id)| {
        record.created_at_ms < *timestamp
            || (record.created_at_ms == *timestamp && record.id.as_str() < id.as_str())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use uuid::Uuid;

    use super::{decode_cursor, encode_cursor, is_before_cursor};
    use crate::usage::types::{UsageRecord, UsageStatsQuery};

    fn record(id: &str, created_at_ms: u64) -> UsageRecord {
        UsageRecord {
            id: id.into(),
            created_at_ms,
            duration_ms: 100,
            time_to_first_token_ms: Some(20),
            generation_duration_ms: Some(80),
            output_tokens_per_second: Some(50.0),
            source: "chat".into(),
            operation: "chatStream".into(),
            provider_id: "provider".into(),
            provider_name: "Provider".into(),
            model_id: "model".into(),
            api_model: "model-api".into(),
            display_name: "Model".into(),
            protocol: "openAiResponses".into(),
            status: "success".into(),
            status_code: Some(200),
            usage_source: "providerReported".into(),
            input_tokens: Some(10),
            non_cached_input_tokens: Some(6),
            context_input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            reasoning_tokens: Some(2),
            cache_read_tokens: Some(4),
            cache_write_tokens: None,
            cost_usd: Some(0.01),
            cost_source: Some("localCalculated".into()),
            pricing_snapshot: None,
            conversation_id: None,
            message_id: None,
            run_id: None,
            round_index: None,
            call_index: None,
            parent_operation: None,
            activated_skill_ids: Vec::new(),
            tool_definition_count: 0,
            tool_call_count: 0,
            error_kind: None,
        }
    }

    #[test]
    fn cursor_round_trips_and_excludes_newer_records() {
        let anchor = record("usage-2", 200);
        let cursor = decode_cursor(&encode_cursor(&anchor)).unwrap();
        assert!(is_before_cursor(&record("usage-1", 100), Some(&cursor)));
        assert!(!is_before_cursor(&record("usage-3", 300), Some(&cursor)));
        assert!(!is_before_cursor(&anchor, Some(&cursor)));
    }

    #[test]
    fn summary_uses_weighted_speed_and_unfiltered_filter_options() {
        let root = temp_dir();
        let mut first = record("usage-1", 1_783_440_000_000);
        first.output_tokens = Some(10);
        first.total_tokens = Some(20);
        first.generation_duration_ms = Some(1_000);
        first.output_tokens_per_second = Some(10.0);
        let mut second = record("usage-2", 1_783_440_001_000);
        second.provider_id = "provider-2".into();
        second.provider_name = "Provider 2".into();
        second.model_id = "model".into();
        second.display_name = "Second model".into();
        second.input_tokens = Some(90);
        second.output_tokens = Some(90);
        second.total_tokens = Some(180);
        second.generation_duration_ms = Some(3_000);
        second.output_tokens_per_second = Some(30.0);

        write_records(&root, &[first.clone(), second]);
        let summary = super::build_summary(
            &root,
            UsageStatsQuery {
                since_ms: Some(1_783_440_000_000),
                until_ms: Some(1_783_440_002_000),
                ..UsageStatsQuery::default()
            },
        );

        assert_eq!(summary.summary.total_requests, 2);
        assert_eq!(summary.summary.total_tokens, 200);
        assert_eq!(summary.summary.average_output_tokens_per_second, Some(25.0));
        assert_eq!(summary.provider_stats.len(), 2);
        assert_eq!(summary.filter_options.providers.len(), 2);
        assert_eq!(summary.filter_options.models.len(), 2);

        let filtered = super::build_summary(
            &root,
            UsageStatsQuery {
                since_ms: Some(1_783_440_000_000),
                until_ms: Some(1_783_440_002_000),
                provider_id: Some("provider".into()),
                ..UsageStatsQuery::default()
            },
        );
        assert_eq!(filtered.summary.total_requests, 1);
        assert_eq!(filtered.provider_stats.len(), 1);
        assert_eq!(filtered.filter_options.providers.len(), 2);
        assert_eq!(filtered.filter_options.models.len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn summary_falls_back_to_input_plus_output_when_total_is_missing() {
        let root = temp_dir();
        let mut value = record("usage-fallback", 1_783_440_000_000);
        value.total_tokens = None;
        write_records(&root, &[value]);
        let summary = super::build_summary(
            &root,
            UsageStatsQuery {
                since_ms: Some(1_783_440_000_000),
                until_ms: Some(1_783_440_001_000),
                ..UsageStatsQuery::default()
            },
        );
        assert_eq!(summary.summary.total_tokens, 15);
        assert_eq!(summary.trend[0].total_tokens, 15);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn operation_filter_uses_the_agent_parent_operation() {
        let root = temp_dir();
        let mut value = record("usage-operation", 1_783_440_000_000);
        value.operation = "agentModelCall".into();
        value.parent_operation = Some("contextCompression".into());
        write_records(&root, &[value]);
        let summary = super::build_summary(
            &root,
            UsageStatsQuery {
                since_ms: Some(1_783_440_000_000),
                until_ms: Some(1_783_440_001_000),
                operation: Some("contextCompression".into()),
                ..UsageStatsQuery::default()
            },
        );
        assert_eq!(summary.summary.total_requests, 1);
        assert_eq!(summary.operation_stats[0].label, "contextCompression");
        assert_eq!(
            summary.filter_options.operations[0].label,
            "contextCompression"
        );
        let _ = fs::remove_dir_all(root);
    }

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("mnemora-usage-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_records(root: &Path, records: &[UsageRecord]) {
        let content = records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(root.join("usage-2026-07.jsonl"), format!("{content}\n")).unwrap();
    }
}
