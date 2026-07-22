//! 不同供应商 Token 口径的统一入口。
//!
//! OpenAI/Gemini 的缓存读取属于输入子集；Anthropic 的普通输入、缓存读取和缓存创建
//! 互不重叠。业务层只使用归一化后的 `input_tokens` 和 `context_input_tokens`。

use crate::{
    ai::types::{ModelUsage, PricingSnapshot, UsageSource},
    settings::types::{ApiProtocol, ModelPricing, CURRENT_MODEL_SETTINGS_VERSION},
};

pub fn normalize_usage(protocol: ApiProtocol, mut usage: ModelUsage) -> ModelUsage {
    // 先补齐供应商偶尔省略的 input/output，避免统计页只显示一半 Token。
    infer_missing_token_parts(&mut usage, protocol);
    let has_tokens = has_token_data(&usage);
    if usage.usage_source == UsageSource::Missing && has_tokens {
        usage.usage_source = UsageSource::ProviderReported;
    }
    if has_tokens && usage.call_count == 0 {
        usage.call_count = 1;
    }

    match protocol {
        ApiProtocol::AnthropicMessages => {
            // Anthropic 的 input_tokens 不包含 cache 两部分；第二次归一化时
            // 使用 non_cached_input_tokens，保证函数幂等，不会重复累加缓存。
            let ordinary_input = usage
                .non_cached_input_tokens
                .or(usage.input_tokens)
                .unwrap_or(0);
            let effective_input = if usage.non_cached_input_tokens.is_some() {
                usage.input_tokens.unwrap_or(ordinary_input)
            } else {
                ordinary_input
                    .saturating_add(usage.cache_read_tokens.unwrap_or(0))
                    .saturating_add(usage.cache_write_tokens.unwrap_or(0))
            };
            usage.non_cached_input_tokens = has_tokens.then_some(ordinary_input);
            usage.input_tokens = has_tokens.then_some(effective_input);
            usage.context_input_tokens = has_tokens.then_some(effective_input);
            // Anthropic 原始 total（部分中转会补）常只等于普通 input + output；
            // 归一化后的 total 必须包含 cache read/write，供跨协议统计使用。
            usage.total_tokens = usage
                .output_tokens
                .map(|output| effective_input.saturating_add(output))
                .or(usage.total_tokens);
        }
        ApiProtocol::OpenAiChatCompletions
        | ApiProtocol::OpenAiResponses
        | ApiProtocol::GeminiGenerateContent => {
            if let Some(input) = usage.input_tokens {
                usage.non_cached_input_tokens =
                    Some(input.saturating_sub(usage.cache_read_tokens.unwrap_or(0)));
                usage.context_input_tokens = Some(input);
                if usage.total_tokens.is_none() {
                    usage.total_tokens = usage
                        .output_tokens
                        .map(|output| input.saturating_add(output));
                }
            }
        }
    }
    usage
}

pub fn merge_run_usage(target: &mut ModelUsage, call: &ModelUsage) {
    target.input_tokens = sum_options(target.input_tokens, effective_input_tokens(call));
    target.output_tokens = sum_options(target.output_tokens, call.output_tokens);
    target.total_tokens = sum_options(target.total_tokens, effective_total_tokens(call));
    target.reasoning_tokens = sum_options(target.reasoning_tokens, call.reasoning_tokens);
    target.cache_read_tokens = sum_options(target.cache_read_tokens, call.cache_read_tokens);
    target.cache_write_tokens = sum_options(target.cache_write_tokens, call.cache_write_tokens);
    target.non_cached_input_tokens =
        sum_options(target.non_cached_input_tokens, call.non_cached_input_tokens);
    target.total_duration_ms = sum_options(target.total_duration_ms, call.total_duration_ms);
    target.generation_duration_ms =
        sum_options(target.generation_duration_ms, call.generation_duration_ms);
    target.cost_usd = sum_f64_options(target.cost_usd, call.cost_usd);
    target.context_input_tokens = call.context_input_tokens.or(target.context_input_tokens);
    target.time_to_first_token_ms = target
        .time_to_first_token_ms
        .or(call.time_to_first_token_ms);
    target.call_count = target.call_count.saturating_add(call.call_count.max(1));
    if let (Some(output), Some(generation_ms)) =
        (target.output_tokens, target.generation_duration_ms)
    {
        target.output_tokens_per_second =
            (generation_ms > 0).then_some(output as f64 * 1_000.0 / generation_ms as f64);
    } else {
        target.output_tokens_per_second = target
            .output_tokens_per_second
            .or(call.output_tokens_per_second);
    }
    target.usage_source = merge_usage_source(target.usage_source, call.usage_source);
    target.cost_source =
        merge_cost_source(target.cost_source.as_deref(), call.cost_source.as_deref());
    target.pricing_snapshot = call
        .pricing_snapshot
        .clone()
        .or_else(|| target.pricing_snapshot.clone());
    refresh_total_tokens(target);
}

/** 在请求结束后统一计算成本并保存价格快照。 */
pub fn apply_pricing(usage: &mut ModelUsage, pricing: Option<&ModelPricing>, captured_at_ms: u64) {
    if usage.cost_usd.is_some() && usage.cost_source.as_deref() == Some("providerReported") {
        return;
    }
    let Some(pricing) = pricing else {
        usage
            .cost_source
            .get_or_insert_with(|| "missing".to_string());
        return;
    };
    let input_known = usage.non_cached_input_tokens.is_some() || usage.input_tokens.is_some();
    let output_known = usage.output_tokens.is_some();
    let non_cached = usage
        .non_cached_input_tokens
        .or(usage.input_tokens)
        .unwrap_or(0);
    let cache_read = usage.cache_read_tokens.unwrap_or(0);
    let cache_write = usage.cache_write_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    let mut cost = 0.0;
    let mut has_tokens = false;
    let mut incomplete = !input_known || !output_known;
    let mut used_fallback_rate = false;
    for (tokens, rate, fallback) in [
        (non_cached, pricing.input_per_million, false),
        (
            cache_read,
            pricing.cache_read_per_million.or(pricing.input_per_million),
            pricing.cache_read_per_million.is_none() && cache_read > 0,
        ),
        (
            cache_write,
            pricing
                .cache_write_per_million
                .or(pricing.input_per_million),
            pricing.cache_write_per_million.is_none() && cache_write > 0,
        ),
        (output, pricing.output_per_million, false),
    ] {
        if tokens == 0 {
            continue;
        }
        has_tokens = true;
        if let Some(rate) = rate.filter(|rate| rate.is_finite() && *rate >= 0.0) {
            cost += tokens as f64 * rate / 1_000_000.0;
            used_fallback_rate |= fallback;
        } else {
            incomplete = true;
        }
    }
    let has_any_rate = pricing.input_per_million.is_some()
        || pricing.output_per_million.is_some()
        || pricing.cache_read_per_million.is_some()
        || pricing.cache_write_per_million.is_some();
    if has_tokens && has_any_rate {
        usage.cost_usd = Some(cost);
        usage.cost_source = Some(
            if incomplete {
                "localPartial"
            } else if used_fallback_rate {
                "localEstimated"
            } else {
                "localCalculated"
            }
            .to_string(),
        );
        usage.pricing_snapshot = Some(PricingSnapshot {
            input_per_million: pricing.input_per_million,
            output_per_million: pricing.output_per_million,
            cache_read_per_million: pricing.cache_read_per_million,
            cache_write_per_million: pricing.cache_write_per_million,
            currency: pricing.currency.clone(),
            captured_at_ms,
            settings_version: CURRENT_MODEL_SETTINGS_VERSION,
        });
    } else {
        usage.cost_usd = None;
        usage.cost_source = Some("missing".to_string());
    }
}

/** 根据首个有效增量和完整耗时生成 TTFT 与输出速度。 */
pub fn apply_stream_metrics(usage: &mut ModelUsage, duration_ms: u64, ttft_ms: Option<u64>) {
    usage.total_duration_ms = Some(duration_ms);
    usage.time_to_first_token_ms = ttft_ms;
    usage.generation_duration_ms = ttft_ms.map(|value| duration_ms.saturating_sub(value));
    usage.output_tokens_per_second = usage
        .output_tokens
        .zip(usage.generation_duration_ms)
        .filter(|(_, generation_ms)| *generation_ms > 0)
        .map(|(tokens, generation_ms)| tokens as f64 * 1_000.0 / generation_ms as f64);
}

fn sum_options(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0).saturating_add(right.unwrap_or(0))),
    }
}

fn sum_f64_options(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (None, None) => None,
        (left, right) => Some(left.unwrap_or(0.0) + right.unwrap_or(0.0)),
    }
}

fn has_token_data(usage: &ModelUsage) -> bool {
    usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.total_tokens.is_some()
        || usage.reasoning_tokens.is_some()
        || usage.cache_read_tokens.is_some()
        || usage.cache_write_tokens.is_some()
}

fn infer_missing_token_parts(usage: &mut ModelUsage, protocol: ApiProtocol) {
    match protocol {
        ApiProtocol::AnthropicMessages => {
            // Anthropic 的 total 可能包含缓存 Token，因此只有在所有组成部分已知时才反推。
            if usage.input_tokens.is_none() {
                if let (Some(total), Some(output)) = (usage.total_tokens, usage.output_tokens) {
                    let cached = usage
                        .cache_read_tokens
                        .unwrap_or(0)
                        .saturating_add(usage.cache_write_tokens.unwrap_or(0));
                    usage.input_tokens = Some(total.saturating_sub(output).saturating_sub(cached));
                }
            }
        }
        ApiProtocol::OpenAiChatCompletions
        | ApiProtocol::OpenAiResponses
        | ApiProtocol::GeminiGenerateContent => {
            if usage.input_tokens.is_none() {
                if let (Some(total), Some(output)) = (usage.total_tokens, usage.output_tokens) {
                    usage.input_tokens = Some(total.saturating_sub(output));
                }
            }
            if usage.output_tokens.is_none() {
                if let (Some(total), Some(input)) = (usage.total_tokens, usage.input_tokens) {
                    usage.output_tokens = Some(total.saturating_sub(input));
                }
            }
        }
    }
}

fn refresh_total_tokens(usage: &mut ModelUsage) {
    if usage.total_tokens.is_none() {
        usage.total_tokens = usage
            .input_tokens
            .zip(usage.output_tokens)
            .map(|(input, output)| input.saturating_add(output));
    }
}

fn effective_input_tokens(usage: &ModelUsage) -> Option<u64> {
    usage.input_tokens.or_else(|| {
        usage.non_cached_input_tokens.map(|value| {
            value
                .saturating_add(usage.cache_read_tokens.unwrap_or(0))
                .saturating_add(usage.cache_write_tokens.unwrap_or(0))
        })
    })
}

fn effective_total_tokens(usage: &ModelUsage) -> Option<u64> {
    usage.total_tokens.or_else(|| {
        effective_input_tokens(usage)
            .zip(usage.output_tokens)
            .map(|(input, output)| input.saturating_add(output))
    })
}

fn merge_usage_source(left: UsageSource, right: UsageSource) -> UsageSource {
    if usage_source_rank(right) > usage_source_rank(left) {
        right
    } else {
        left
    }
}

fn usage_source_rank(source: UsageSource) -> u8 {
    match source {
        UsageSource::Missing => 0,
        UsageSource::Estimated => 1,
        UsageSource::GatewayNormalized => 2,
        UsageSource::ProviderReported => 3,
    }
}

fn merge_cost_source(left: Option<&str>, right: Option<&str>) -> Option<String> {
    match (left, right) {
        (None, None) => None,
        (Some(value), None) | (None, Some(value)) => Some(value.to_string()),
        (Some(left), Some(right)) if left == right => Some(left.to_string()),
        (Some("localPartial"), _) | (_, Some("localPartial")) => Some("localPartial".to_string()),
        (Some("localEstimated"), Some("localCalculated"))
        | (Some("localCalculated"), Some("localEstimated")) => Some("localEstimated".to_string()),
        (Some("providerReported"), Some("localCalculated"))
        | (Some("localCalculated"), Some("providerReported")) => {
            Some("localCalculated".to_string())
        }
        _ => Some("localPartial".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ai::types::{ModelUsage, UsageSource},
        settings::types::{ApiProtocol, ModelPricing},
    };

    #[test]
    fn openai_and_gemini_cache_is_not_added_twice() {
        for protocol in [
            ApiProtocol::OpenAiChatCompletions,
            ApiProtocol::OpenAiResponses,
            ApiProtocol::GeminiGenerateContent,
        ] {
            let usage = super::normalize_usage(
                protocol,
                ModelUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(20),
                    total_tokens: Some(120),
                    cache_read_tokens: Some(40),
                    ..ModelUsage::default()
                },
            );
            assert_eq!(usage.input_tokens, Some(100));
            assert_eq!(usage.non_cached_input_tokens, Some(60));
            assert_eq!(usage.context_input_tokens, Some(100));
            assert_eq!(usage.total_tokens, Some(120));
        }
    }

    #[test]
    fn anthropic_cache_parts_are_added_to_effective_input() {
        let usage = super::normalize_usage(
            ApiProtocol::AnthropicMessages,
            ModelUsage {
                input_tokens: Some(60),
                output_tokens: Some(20),
                total_tokens: Some(80),
                cache_read_tokens: Some(30),
                cache_write_tokens: Some(10),
                ..ModelUsage::default()
            },
        );
        assert_eq!(usage.non_cached_input_tokens, Some(60));
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.context_input_tokens, Some(100));
        assert_eq!(usage.total_tokens, Some(120));
    }

    #[test]
    fn run_usage_accumulates_calls_but_keeps_latest_context_anchor() {
        let mut run = ModelUsage::default();
        let first = ModelUsage {
            input_tokens: Some(100),
            output_tokens: Some(20),
            total_tokens: Some(120),
            context_input_tokens: Some(100),
            call_count: 1,
            ..ModelUsage::default()
        };
        let second = ModelUsage {
            input_tokens: Some(180),
            output_tokens: Some(30),
            total_tokens: Some(210),
            context_input_tokens: Some(180),
            call_count: 1,
            ..ModelUsage::default()
        };

        super::merge_run_usage(&mut run, &first);
        super::merge_run_usage(&mut run, &second);

        assert_eq!(run.input_tokens, Some(280));
        assert_eq!(run.total_tokens, Some(330));
        assert_eq!(run.context_input_tokens, Some(180));
        assert_eq!(run.call_count, 2);
    }

    #[test]
    fn infers_missing_openai_parts_from_total() {
        let usage = super::normalize_usage(
            ApiProtocol::OpenAiResponses,
            ModelUsage {
                total_tokens: Some(150),
                output_tokens: Some(50),
                ..ModelUsage::default()
            },
        );
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.non_cached_input_tokens, Some(100));
        assert_eq!(usage.context_input_tokens, Some(100));
    }

    #[test]
    fn anthropic_normalization_is_idempotent() {
        let first = super::normalize_usage(
            ApiProtocol::AnthropicMessages,
            ModelUsage {
                input_tokens: Some(60),
                output_tokens: Some(20),
                cache_read_tokens: Some(30),
                cache_write_tokens: Some(10),
                ..ModelUsage::default()
            },
        );
        let second = super::normalize_usage(ApiProtocol::AnthropicMessages, first.clone());
        assert_eq!(first, second);
    }

    #[test]
    fn later_provider_reported_usage_upgrades_an_estimate() {
        let mut run = ModelUsage {
            usage_source: UsageSource::Estimated,
            ..ModelUsage::default()
        };
        let call = ModelUsage {
            usage_source: UsageSource::ProviderReported,
            input_tokens: Some(1),
            output_tokens: Some(1),
            ..ModelUsage::default()
        };
        super::merge_run_usage(&mut run, &call);
        assert_eq!(run.usage_source, UsageSource::ProviderReported);
    }

    #[test]
    fn weighted_speed_is_used_for_merged_run() {
        let mut run = ModelUsage::default();
        super::merge_run_usage(
            &mut run,
            &ModelUsage {
                output_tokens: Some(10),
                generation_duration_ms: Some(1_000),
                output_tokens_per_second: Some(10.0),
                call_count: 1,
                ..ModelUsage::default()
            },
        );
        super::merge_run_usage(
            &mut run,
            &ModelUsage {
                output_tokens: Some(90),
                generation_duration_ms: Some(9_000),
                output_tokens_per_second: Some(10.0),
                call_count: 1,
                ..ModelUsage::default()
            },
        );
        assert_eq!(run.output_tokens_per_second, Some(10.0));
    }

    #[test]
    fn pricing_marks_missing_output_usage_as_partial() {
        let mut usage = ModelUsage {
            input_tokens: Some(1_000),
            non_cached_input_tokens: Some(1_000),
            ..ModelUsage::default()
        };
        super::apply_pricing(
            &mut usage,
            Some(&ModelPricing {
                input_per_million: Some(1.0),
                output_per_million: Some(2.0),
                cache_read_per_million: None,
                cache_write_per_million: None,
                currency: "USD".into(),
            }),
            1,
        );
        assert_eq!(usage.cost_usd, Some(0.001));
        assert_eq!(usage.cost_source.as_deref(), Some("localPartial"));
    }

    #[test]
    fn merged_cost_source_keeps_partial_semantics() {
        let mut run = ModelUsage {
            cost_usd: Some(0.01),
            cost_source: Some("localCalculated".into()),
            call_count: 1,
            ..ModelUsage::default()
        };
        super::merge_run_usage(
            &mut run,
            &ModelUsage {
                cost_usd: Some(0.02),
                cost_source: Some("missing".into()),
                call_count: 1,
                ..ModelUsage::default()
            },
        );
        assert_eq!(run.cost_usd, Some(0.03));
        assert_eq!(run.cost_source.as_deref(), Some("localPartial"));
    }
}
