//! 单次真实模型调用的持久化记录器。

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{ai::types::UsageSource, state::AppState};

use super::{
    storage,
    types::{UsageRecord, UsageRecordInput},
};

static RECORD_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub async fn record_model_call(state: &AppState, input: UsageRecordInput) {
    let usage = input.usage.unwrap_or_default();
    let record = UsageRecord {
        id: format!(
            "usage_{}_{}",
            input.created_at_ms,
            RECORD_COUNTER.fetch_add(1, Ordering::Relaxed)
        ),
        created_at_ms: input.created_at_ms,
        duration_ms: input.duration_ms,
        time_to_first_token_ms: usage.time_to_first_token_ms,
        generation_duration_ms: usage.generation_duration_ms,
        output_tokens_per_second: usage.output_tokens_per_second,
        source: input.source,
        operation: input.operation,
        provider_id: input.provider_id,
        provider_name: input.provider_name,
        model_id: input.model_id,
        api_model: input.api_model,
        display_name: input.display_name,
        protocol: input.protocol,
        status: input.status,
        status_code: input.status_code,
        usage_source: usage_source_name(usage.usage_source).to_string(),
        input_tokens: usage.input_tokens,
        non_cached_input_tokens: usage.non_cached_input_tokens,
        context_input_tokens: usage.context_input_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        cost_usd: usage.cost_usd,
        cost_source: usage.cost_source,
        pricing_snapshot: usage.pricing_snapshot,
        conversation_id: input.conversation_id,
        message_id: input.message_id,
        run_id: input.run_id,
        round_index: input.round_index,
        call_index: input.call_index,
        parent_operation: input.parent_operation,
        activated_skill_ids: input.activated_skill_ids,
        tool_definition_count: input.tool_definition_count,
        tool_call_count: input.tool_call_count,
        error_kind: input.error_kind,
    };

    let _guard = state.usage_operations.lock().await;
    let usage_dir = state.usage_dir.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || storage::append_record(&usage_dir, &record))
            .await
            .map_err(|error| format!("用量记录后台任务失败：{error}"))
            .and_then(|result| result);
    if let Err(error) = result {
        eprintln!("Failed to record model usage: {error}");
    }
}

fn usage_source_name(source: UsageSource) -> &'static str {
    match source {
        UsageSource::ProviderReported => "providerReported",
        UsageSource::GatewayNormalized => "gatewayNormalized",
        UsageSource::Estimated => "estimated",
        UsageSource::Missing => "missing",
    }
}

#[cfg(test)]
mod tests {
    use crate::ai::types::UsageSource;

    #[test]
    fn exposes_stable_usage_source_names() {
        assert_eq!(
            super::usage_source_name(UsageSource::ProviderReported),
            "providerReported"
        );
        assert_eq!(super::usage_source_name(UsageSource::Missing), "missing");
    }
}
