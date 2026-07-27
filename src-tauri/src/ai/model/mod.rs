//! 内置模型元数据库（参考 Kivio 的 model_metadata 设计）。
//!
//! 数据源是 `src/data/modelDatabase.json`，前端与 Rust 通过 `include_str!` 共享同一份。
//! 按模型名提供能力（视觉）、上下文窗口、最大输出与默认定价查询；解析优先级由调用方
//! 决定（用户在设置里的显式配置优先，本库只提供"数据库默认值"）。
//!
//! 匹配策略（对齐 Kivio）：
//! 1. 精确匹配（含去掉 `provider/` 前缀后的精确匹配）；
//! 2. 分隔符归一化后的精确匹配（`claude-sonnet-4-6` ↔ `claude-sonnet-4.6`）；
//! 3. 前缀匹配（数据库 key 是模型名前缀，最长 key 优先）；
//! 4. 包含匹配（模型名包含数据库 key，最长 key 优先）。
//! 中转站大多沿用官方模型名，因此绝大多数场景能命中；匹配不到时返回 `None`，
//! 由调用方保持宽松默认（不拦截），避免误伤未收录的新模型。

use std::sync::OnceLock;

use serde_json::Value;

use crate::settings::types::ModelPricing;

const MODEL_DATABASE_JSON: &str = include_str!("../../../../src/data/modelDatabase.json");

fn database_entries() -> Option<&'static serde_json::Map<String, Value>> {
    static DATABASE: OnceLock<Value> = OnceLock::new();
    DATABASE
        .get_or_init(|| serde_json::from_str(MODEL_DATABASE_JSON).unwrap_or(Value::Null))
        .as_object()
}

/// 版本分隔符归一化：数据库 key 用点号（`gpt-5.5`），部分中转站返回连字符
/// 变体（`gpt-5-5`）。统一把 `.` 视作 `-` 后再比对。
fn normalize_separators(value: &str) -> String {
    value.replace('.', "-")
}

fn database_entry(api_model: &str) -> Option<&'static Value> {
    let model = api_model.trim();
    if model.is_empty() {
        return None;
    }

    let entries = database_entries()?;
    let name = model.to_ascii_lowercase();
    // OpenRouter 风格 `provider/model` → 去掉前缀再匹配。
    let stripped = name.rsplit('/').next().unwrap_or(&name);

    // 1. 精确匹配。
    if let Some(entry) = entries.get(name.as_str()) {
        return Some(entry);
    }
    if let Some(entry) = entries.get(stripped) {
        return Some(entry);
    }

    // 2. 分隔符归一化后的精确匹配。
    let norm_name = normalize_separators(&name);
    let norm_stripped = normalize_separators(stripped);
    if let Some(entry) = entries.iter().find_map(|(key, entry)| {
        let norm_key = normalize_separators(key);
        (key != "_meta" && (norm_key == norm_name || norm_key == norm_stripped)).then_some(entry)
    }) {
        return Some(entry);
    }

    let candidates: Vec<&str> = if norm_name == norm_stripped {
        vec![norm_stripped.as_str()]
    } else {
        vec![norm_name.as_str(), norm_stripped.as_str()]
    };

    // 3. 前缀匹配（最长 key 优先）。
    entries
        .iter()
        .filter_map(|(key, entry)| {
            if key == "_meta" {
                return None;
            }
            let norm_key = normalize_separators(key);
            candidates
                .iter()
                .any(|candidate| {
                    candidate.starts_with(norm_key.as_str()) && norm_key.len() < candidate.len()
                })
                .then_some((norm_key.len(), entry))
        })
        .max_by_key(|(key_len, _)| *key_len)
        .map(|(_, entry)| entry)
        // 4. 包含匹配（最长 key 优先）。
        .or_else(|| {
            entries
                .iter()
                .filter_map(|(key, entry)| {
                    if key == "_meta" {
                        return None;
                    }
                    let norm_key = normalize_separators(key);
                    candidates
                        .iter()
                        .any(|candidate| {
                            norm_key.as_str() != *candidate && candidate.contains(norm_key.as_str())
                        })
                        .then_some((norm_key.len(), entry))
                })
                .max_by_key(|(key_len, _)| *key_len)
                .map(|(_, entry)| entry)
        })
}

/// 数据库中该模型是否支持图片输入。`None` 表示未收录（调用方应保持宽松默认）。
pub fn database_supports_vision(api_model: &str) -> Option<bool> {
    database_entry(api_model)?
        .get("capabilities")
        .and_then(|capabilities| capabilities.get("vision"))
        .and_then(Value::as_bool)
}

/// 名称家族启发式（参考 cherry-studio 的 vision 白/黑名单设计）：数据库未命中时
/// 按模型名家族兜底判定。只收录高置信家族，拿不准时返回 `None` 交给上层放行，
/// 避免误伤中转站上的新模型。
///
/// 顺序敏感：视觉标记先于 DeepSeek 判定，保证 `deepseek-vl` 这类多模态变体不被
/// 家族黑名单误杀。
pub fn heuristic_supports_vision(api_model: &str) -> Option<bool> {
    let lower = api_model.trim().to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(lower.as_str());
    if name.is_empty() {
        return None;
    }

    // 非对话用途的模型家族：embedding / rerank / 语音，不接受图片。
    const NON_CHAT_MARKERS: [&str; 4] = ["embed", "rerank", "tts", "whisper"];
    if NON_CHAT_MARKERS.iter().any(|marker| name.contains(marker)) {
        return Some(false);
    }

    // 高置信视觉家族（名称即宣告多模态）。
    const VISION_MARKERS: [&str; 10] = [
        "vision",
        "-vl",
        "vl-",
        "llava",
        "pixtral",
        "internvl",
        "qvq",
        "moondream",
        "minicpm-v",
        "gpt-4o",
    ];
    if VISION_MARKERS.iter().any(|marker| name.contains(marker)) {
        return Some(true);
    }

    // DeepSeek 对话家族（非 VL 变体）至今不支持图片输入——用户反馈的重灾区。
    if name.contains("deepseek") {
        return Some(false);
    }

    None
}

/// 组合判定：数据库优先，其次名称家族启发式。`None` 表示两层都无法判断。
/// 用户在设置里的显式覆盖仍由调用方置于最前。
pub fn resolve_supports_vision(api_model: &str) -> Option<bool> {
    database_supports_vision(api_model).or_else(|| heuristic_supports_vision(api_model))
}

/// 数据库中该模型的上下文窗口默认值。
pub fn database_context_window_tokens(api_model: &str) -> Option<u64> {
    database_entry(api_model)?
        .get("contextWindow")
        .and_then(Value::as_u64)
        .filter(|tokens| *tokens > 0)
}

/// 数据库中该模型的默认定价（USD / 百万 Token）。
/// 数据库的 `cachedInput` 映射为 Mnemora 的 `cache_read_per_million`；
/// 数据库没有缓存写入价，保持 `None`。
pub fn database_pricing(api_model: &str) -> Option<ModelPricing> {
    let pricing = database_entry(api_model)?.get("pricing")?;
    let input = pricing.get("input").and_then(Value::as_f64);
    let output = pricing.get("output").and_then(Value::as_f64);
    let cached_input = pricing.get("cachedInput").and_then(Value::as_f64);
    if input.is_none() && output.is_none() && cached_input.is_none() {
        return None;
    }
    Some(ModelPricing {
        input_per_million: input,
        output_per_million: output,
        cache_read_per_million: cached_input,
        cache_write_per_million: None,
        currency: "USD".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_loads_and_contains_entries() {
        let entries = database_entries().expect("model database must parse");
        assert!(entries.len() > 50, "database looks truncated");
    }

    #[test]
    fn exact_match_resolves_vision() {
        assert_eq!(database_supports_vision("gpt-5.5"), Some(true));
    }

    #[test]
    fn openrouter_prefix_is_stripped() {
        assert_eq!(database_supports_vision("openai/gpt-5.5"), Some(true));
    }

    #[test]
    fn separator_variant_matches() {
        // 中转站可能返回连字符版本号。
        assert_eq!(
            database_supports_vision("gpt-5-5"),
            database_supports_vision("gpt-5.5"),
        );
    }

    #[test]
    fn prefix_match_prefers_longest_key() {
        // 带日期/后缀的模型名应命中最长的已知前缀，而不是更短的大版本。
        let dated = database_supports_vision("gpt-5.5-2026-01-01");
        assert_eq!(dated, Some(true));
    }

    #[test]
    fn text_only_model_reports_no_vision() {
        // 数据库里应存在明确标记不支持视觉的纯文本模型。
        let entries = database_entries().unwrap();
        let text_only = entries.iter().find_map(|(key, entry)| {
            (key != "_meta"
                && entry
                    .get("capabilities")
                    .and_then(|c| c.get("vision"))
                    .and_then(Value::as_bool)
                    == Some(false))
            .then_some(key.clone())
        });
        let key = text_only.expect("database should contain at least one text-only model");
        assert_eq!(database_supports_vision(&key), Some(false));
    }

    #[test]
    fn unknown_model_returns_none() {
        assert_eq!(database_supports_vision("totally-unknown-model-xyz"), None);
        assert_eq!(database_pricing("totally-unknown-model-xyz"), None);
        assert_eq!(
            database_context_window_tokens("totally-unknown-model-xyz"),
            None
        );
    }

    #[test]
    fn heuristic_blocks_deepseek_family_variants() {
        // 数据库未收录的 DeepSeek 变体名也要判为不支持（cherry-studio 行为）。
        assert_eq!(
            heuristic_supports_vision("deepseek-v3.2-terminus"),
            Some(false)
        );
        assert_eq!(heuristic_supports_vision("DeepSeek-Coder-X"), Some(false));
    }

    #[test]
    fn heuristic_keeps_deepseek_vl_as_vision() {
        // 视觉标记优先于 DeepSeek 黑名单：VL 变体是多模态。
        assert_eq!(heuristic_supports_vision("deepseek-vl2"), Some(true));
    }

    #[test]
    fn heuristic_recognizes_vision_families() {
        assert_eq!(heuristic_supports_vision("qwen9-vl-plus"), Some(true));
        assert_eq!(heuristic_supports_vision("llava-next-13b"), Some(true));
        assert_eq!(heuristic_supports_vision("some-vision-preview"), Some(true));
    }

    #[test]
    fn heuristic_blocks_non_chat_models() {
        assert_eq!(heuristic_supports_vision("text-embedding-9"), Some(false));
        assert_eq!(heuristic_supports_vision("whisper-turbo"), Some(false));
    }

    #[test]
    fn heuristic_stays_silent_for_unknown_families() {
        assert_eq!(heuristic_supports_vision("mystery-chat-model"), None);
    }

    #[test]
    fn resolve_prefers_database_over_heuristic() {
        // gpt-5.5 在库里（true）；库优先于启发式。
        assert_eq!(resolve_supports_vision("gpt-5.5"), Some(true));
        // 库未收录 → 启发式接手。
        assert_eq!(
            resolve_supports_vision("deepseek-super-new-chat"),
            Some(false)
        );
        // 两层都未知 → None。
        assert_eq!(resolve_supports_vision("mystery-chat-model"), None);
    }

    #[test]
    fn pricing_maps_cached_input_to_cache_read() {
        let pricing = database_pricing("gpt-5.5").expect("gpt-5.5 should have pricing");
        assert!(pricing.input_per_million.is_some());
        assert!(pricing.output_per_million.is_some());
        assert_eq!(pricing.currency, "USD");
        assert_eq!(pricing.cache_write_per_million, None);
    }

    #[test]
    fn context_window_resolves_from_database() {
        assert_eq!(database_context_window_tokens("gpt-5.5"), Some(256_000));
    }
}
