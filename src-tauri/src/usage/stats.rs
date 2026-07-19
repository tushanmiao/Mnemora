//! 用量记录的内存聚合。

use std::collections::BTreeMap;

use super::types::{
    UsageGroupStats, UsageRecord, UsageStatsQuery, UsageStatsResponse, UsageSummary,
    UsageTrendPoint,
};

const DEFAULT_LOG_LIMIT: usize = 100;
const MAX_LOG_LIMIT: usize = 500;
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
    duration_total: u64,
    last_used_at_ms: Option<u64>,
}

pub fn build_response(
    mut records: Vec<UsageRecord>,
    skipped_records: usize,
    query: UsageStatsQuery,
) -> UsageStatsResponse {
    if let Some(since_ms) = query.since_ms {
        records.retain(|record| record.created_at_ms >= since_ms);
    }
    records.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
    let total_logs = records.len();
    let summary = summarize(&records);
    let trend = build_trend(&records, &query);
    let provider_stats = group_records(&records, false);
    let model_stats = group_records(&records, true);
    records.truncate(query.limit.unwrap_or(DEFAULT_LOG_LIMIT).min(MAX_LOG_LIMIT));
    UsageStatsResponse {
        summary,
        trend,
        logs: records,
        provider_stats,
        model_stats,
        total_logs,
        skipped_records,
    }
}

fn summarize(records: &[UsageRecord]) -> UsageSummary {
    let mut summary = UsageSummary::default();
    let mut duration_total = 0u64;
    let mut cost_total = 0.0;
    let mut has_cost = false;
    for record in records {
        summary.total_requests = summary.total_requests.saturating_add(1);
        match record.status.as_str() {
            "success" => {
                summary.successful_requests = summary.successful_requests.saturating_add(1)
            }
            "stopped" => summary.stopped_requests = summary.stopped_requests.saturating_add(1),
            _ => summary.failed_requests = summary.failed_requests.saturating_add(1),
        }
        if record.usage_source == "providerReported" {
            summary.provider_reported_requests =
                summary.provider_reported_requests.saturating_add(1);
        } else {
            summary.missing_usage_requests = summary.missing_usage_requests.saturating_add(1);
        }
        summary.total_tokens = summary
            .total_tokens
            .saturating_add(record.total_tokens.unwrap_or(0));
        summary.input_tokens = summary
            .input_tokens
            .saturating_add(effective_input_tokens(record));
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
        duration_total = duration_total.saturating_add(record.duration_ms);
        if let Some(cost) = record.cost_usd {
            cost_total += cost;
            has_cost = true;
        }
    }
    if summary.total_requests > 0 {
        summary.average_duration_ms = Some(duration_total as f64 / summary.total_requests as f64);
    }
    summary.total_cost_usd = has_cost.then_some(cost_total);
    summary
}

fn effective_input_tokens(record: &UsageRecord) -> u64 {
    if record.protocol == "anthropicMessages" {
        record
            .input_tokens
            .unwrap_or(0)
            .saturating_add(record.cache_read_tokens.unwrap_or(0))
            .saturating_add(record.cache_write_tokens.unwrap_or(0))
    } else {
        record.input_tokens.unwrap_or(0)
    }
}

fn build_trend(records: &[UsageRecord], query: &UsageStatsQuery) -> Vec<UsageTrendPoint> {
    let Some(since_ms) = query.since_ms else {
        return Vec::new();
    };
    let bucket_ms = query.bucket_ms.unwrap_or(24 * 60 * 60 * 1_000).max(1);
    let bucket_count = query.bucket_count.unwrap_or(7).clamp(1, MAX_BUCKETS);
    let mut points = (0..bucket_count)
        .map(|bucket_index| UsageTrendPoint {
            bucket_index,
            started_at_ms: since_ms.saturating_add(bucket_ms.saturating_mul(bucket_index as u64)),
            ..UsageTrendPoint::default()
        })
        .collect::<Vec<_>>();
    for record in records {
        let bucket_index = record.created_at_ms.saturating_sub(since_ms) / bucket_ms;
        let Some(point) = points.get_mut(bucket_index as usize) else {
            continue;
        };
        point.requests = point.requests.saturating_add(1);
        point.total_tokens = point
            .total_tokens
            .saturating_add(record.total_tokens.unwrap_or(0));
        point.input_tokens = point
            .input_tokens
            .saturating_add(effective_input_tokens(record));
        point.output_tokens = point
            .output_tokens
            .saturating_add(record.output_tokens.unwrap_or(0));
        point.cache_read_tokens = point
            .cache_read_tokens
            .saturating_add(record.cache_read_tokens.unwrap_or(0));
        point.cache_write_tokens = point
            .cache_write_tokens
            .saturating_add(record.cache_write_tokens.unwrap_or(0));
    }
    points
}

fn group_records(records: &[UsageRecord], by_model: bool) -> Vec<UsageGroupStats> {
    let mut groups = BTreeMap::<String, GroupAccumulator>::new();
    for record in records {
        let key = if by_model {
            format!("{}:{}", record.provider_id, record.model_id)
        } else {
            record.provider_id.clone()
        };
        let group = groups
            .entry(key.clone())
            .or_insert_with(|| GroupAccumulator {
                id: key,
                label: if by_model {
                    record.display_name.clone()
                } else {
                    record.provider_name.clone()
                },
                provider_id: record.provider_id.clone(),
                provider_name: record.provider_name.clone(),
                model_id: by_model.then(|| record.model_id.clone()),
                api_model: by_model.then(|| record.api_model.clone()),
                ..GroupAccumulator::default()
            });
        group.request_count = group.request_count.saturating_add(1);
        if record.status == "success" {
            group.success_count = group.success_count.saturating_add(1);
        }
        group.total_tokens = group
            .total_tokens
            .saturating_add(record.total_tokens.unwrap_or(0));
        group.input_tokens = group
            .input_tokens
            .saturating_add(effective_input_tokens(record));
        group.output_tokens = group
            .output_tokens
            .saturating_add(record.output_tokens.unwrap_or(0));
        group.cache_read_tokens = group
            .cache_read_tokens
            .saturating_add(record.cache_read_tokens.unwrap_or(0));
        group.cache_write_tokens = group
            .cache_write_tokens
            .saturating_add(record.cache_write_tokens.unwrap_or(0));
        group.duration_total = group.duration_total.saturating_add(record.duration_ms);
        group.last_used_at_ms = Some(group.last_used_at_ms.unwrap_or(0).max(record.created_at_ms));
    }
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
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::{effective_input_tokens, summarize};
    use crate::usage::types::UsageRecord;

    fn record(protocol: &str) -> UsageRecord {
        UsageRecord {
            id: "usage-1".into(),
            created_at_ms: 1,
            duration_ms: 100,
            source: "chat".into(),
            operation: "chatStream".into(),
            provider_id: "provider".into(),
            provider_name: "Provider".into(),
            model_id: "model".into(),
            api_model: "model-api".into(),
            display_name: "Model".into(),
            protocol: protocol.into(),
            status: "success".into(),
            status_code: Some(200),
            usage_source: "providerReported".into(),
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            reasoning_tokens: Some(2),
            cache_read_tokens: Some(4),
            cache_write_tokens: Some(3),
            cost_usd: None,
            conversation_id: None,
            message_id: None,
            error_kind: None,
        }
    }

    #[test]
    fn anthropic_effective_input_includes_cache_tokens() {
        assert_eq!(effective_input_tokens(&record("anthropicMessages")), 17);
    }

    #[test]
    fn summary_counts_tokens_and_coverage() {
        let summary = summarize(&[record("openAiResponses")]);
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.total_tokens, 15);
        assert_eq!(summary.provider_reported_requests, 1);
        assert_eq!(summary.average_duration_ms, Some(100.0));
    }
}
