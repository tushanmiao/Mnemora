//! 单次模型调用记录器。

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{ai::types::ModelUsage, state::AppState};

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
    let usage = input.usage;
    let has_provider_usage = usage.as_ref().is_some_and(has_provider_token_usage);
    let input_tokens = usage.as_ref().and_then(|value| value.input_tokens);
    let output_tokens = usage.as_ref().and_then(|value| value.output_tokens);
    let cache_read_tokens = usage.as_ref().and_then(|value| value.cache_read_tokens);
    let cache_write_tokens = usage.as_ref().and_then(|value| value.cache_write_tokens);
    let effective_input = if input.protocol == "anthropicMessages" {
        input_tokens
            .unwrap_or(0)
            .saturating_add(cache_read_tokens.unwrap_or(0))
            .saturating_add(cache_write_tokens.unwrap_or(0))
    } else {
        input_tokens.unwrap_or(0)
    };
    let total_tokens = has_provider_usage.then(|| {
        if input.protocol == "anthropicMessages" {
            effective_input.saturating_add(output_tokens.unwrap_or(0))
        } else {
            usage
                .as_ref()
                .and_then(|value| value.total_tokens)
                .unwrap_or_else(|| effective_input.saturating_add(output_tokens.unwrap_or(0)))
        }
    });
    let record = UsageRecord {
        id: format!(
            "usage_{}_{}",
            input.created_at_ms,
            RECORD_COUNTER.fetch_add(1, Ordering::Relaxed)
        ),
        created_at_ms: input.created_at_ms,
        duration_ms: input.duration_ms,
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
        usage_source: if has_provider_usage {
            "providerReported"
        } else {
            "missing"
        }
        .to_string(),
        input_tokens,
        output_tokens,
        total_tokens,
        reasoning_tokens: usage.as_ref().and_then(|value| value.reasoning_tokens),
        cache_read_tokens,
        cache_write_tokens,
        cost_usd: None,
        conversation_id: input.conversation_id,
        message_id: input.message_id,
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

fn has_provider_token_usage(usage: &ModelUsage) -> bool {
    usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.total_tokens.is_some()
        || usage.reasoning_tokens.is_some()
        || usage.cache_read_tokens.is_some()
        || usage.cache_write_tokens.is_some()
}

#[cfg(test)]
mod tests {
    use crate::ai::types::ModelUsage;

    #[test]
    fn duration_only_is_not_provider_usage() {
        let usage = ModelUsage {
            total_duration_ms: Some(10),
            ..ModelUsage::default()
        };
        assert!(!super::has_provider_token_usage(&usage));
    }
}
