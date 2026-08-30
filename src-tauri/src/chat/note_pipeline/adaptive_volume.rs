//! DeepNote 路由身份、短期可用性与单请求体积控制。
//!
//! 中转站目录、健康和稳定载荷的变化尺度不同，因此这里把它们拆开：
//! `RouteAvailability` 只决定当前是否允许探测/调用；`learned_target_tokens`
//! 只从真正与载荷有关的 Chunk 结果学习；短期超时先落到有期限的安全档，避免一次
//! 网关抖动永久污染已经学到的包线。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ai::error::ModelErrorKind,
    settings::types::{ApiProtocol, AuthScheme, ModelSettings, ProviderConfig},
};

pub const MIN_ADAPTIVE_CHUNK_TOKENS: u64 = 2_048;
pub const INITIAL_ADAPTIVE_CHUNK_TOKENS: u64 = 8_000;
pub const MAX_ADAPTIVE_CHUNK_TOKENS: u64 = 16_000;
pub const DEFAULT_ADDITIVE_STEP_TOKENS: u64 = 1_024;
const SUCCESSES_PER_INCREASE: u32 = 3;
const CAPACITY_EXERCISE_PERCENT: u64 = 75;
const TRANSIENT_TIMEOUT_PENALTY_MS: u64 = 10 * 60 * 1_000;
const MODEL_RECHECK_MS: u64 = 15 * 60 * 1_000;
const CONFIG_RECHECK_MS: u64 = 5 * 60 * 1_000;
const HEALTH_CIRCUIT_MS: u64 = 60 * 1_000;
const HEALTH_FAILURES_TO_OPEN: u32 = 3;
const PROFILE_STALE_MS: u64 = 24 * 60 * 60 * 1_000;

fn stable_hash(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}

fn protocol_key(protocol: ApiProtocol) -> &'static str {
    match protocol {
        ApiProtocol::OpenAiChatCompletions => "openAiChatCompletions",
        ApiProtocol::OpenAiResponses => "openAiResponses",
        ApiProtocol::AnthropicMessages => "anthropicMessages",
        ApiProtocol::GeminiGenerateContent => "geminiGenerateContent",
    }
}

fn auth_key(auth: AuthScheme) -> &'static str {
    match auth {
        AuthScheme::ProtocolDefault => "protocolDefault",
        AuthScheme::Bearer => "bearer",
        AuthScheme::XApiKey => "xApiKey",
        AuthScheme::XGoogApiKey => "xGoogApiKey",
    }
}

pub fn provider_config_epoch(provider: &ProviderConfig) -> String {
    stable_hash(format!(
        "deep-note-provider-v1\0{}\0{}\0{}\0{}\0{}",
        provider.id,
        provider.base_url,
        protocol_key(provider.protocol),
        auth_key(provider.auth_scheme),
        provider.credential_revision,
    ))
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteRouteIdentity {
    pub route_key: String,
    pub provider_id: String,
    pub provider_config_epoch: String,
    pub model_id: String,
    pub api_model: String,
    pub protocol: String,
    pub transport_mode: String,
}

impl DeepNoteRouteIdentity {
    pub fn resolve(
        settings: &ModelSettings,
        provider_id: &str,
        model_id: &str,
        streaming_preferred: bool,
    ) -> Result<Self, String> {
        let provider = settings
            .providers
            .iter()
            .find(|provider| provider.enabled && provider.id == provider_id)
            .ok_or_else(|| "没有找到启用的深度笔记模型供应商。".to_string())?;
        let model = provider
            .models
            .iter()
            .find(|model| model.enabled && model.id == model_id)
            .ok_or_else(|| "没有找到启用的深度笔记模型。".to_string())?;
        let provider_config_epoch = provider_config_epoch(provider);
        let protocol = protocol_key(provider.protocol).to_string();
        let transport_mode = if streaming_preferred {
            "streamingPreferred"
        } else {
            "nonStreaming"
        }
        .to_string();
        let route_key = stable_hash(format!(
            "deep-note-route-v1\0{}\0{}\0{}\0{}\0{}",
            provider.id, provider_config_epoch, model.id, model.api_model, transport_mode,
        ));
        Ok(Self {
            route_key,
            provider_id: provider.id.clone(),
            provider_config_epoch,
            model_id: model.id.clone(),
            api_model: model.api_model.clone(),
            protocol,
            transport_mode,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RouteAvailability {
    #[default]
    Unknown,
    Available,
    Degraded,
    CircuitOpen,
    Unsupported,
    Disabled,
    Tombstoned,
}

impl RouteAvailability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Available => "available",
            Self::Degraded => "degraded",
            Self::CircuitOpen => "circuitOpen",
            Self::Unsupported => "unsupported",
            Self::Disabled => "disabled",
            Self::Tombstoned => "tombstoned",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveVolumeProfile {
    pub identity: DeepNoteRouteIdentity,
    #[serde(default)]
    pub availability: RouteAvailability,
    pub learned_target_tokens: u64,
    pub additive_step_tokens: u64,
    #[serde(default)]
    pub temporary_limit_tokens: Option<u64>,
    #[serde(default)]
    pub temporary_limit_until_ms: Option<u64>,
    #[serde(default)]
    pub retry_after_until_ms: Option<u64>,
    #[serde(default)]
    pub consecutive_successes: u32,
    #[serde(default)]
    pub consecutive_health_failures: u32,
    #[serde(default)]
    pub timeout_streak: u32,
    #[serde(default)]
    pub sample_count: u64,
    #[serde(default)]
    pub success_count: u64,
    #[serde(default)]
    pub failure_count: u64,
    #[serde(default)]
    pub last_error_kind: Option<String>,
    #[serde(default)]
    pub last_seen_at_ms: u64,
    #[serde(default)]
    pub last_success_at_ms: Option<u64>,
    #[serde(default)]
    pub last_failure_at_ms: Option<u64>,
    #[serde(default)]
    pub last_request_bytes: usize,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl AdaptiveVolumeProfile {
    pub fn new(identity: DeepNoteRouteIdentity, prior_target_tokens: u64, now_ms: u64) -> Self {
        Self {
            identity,
            availability: RouteAvailability::Unknown,
            learned_target_tokens: prior_target_tokens
                .clamp(MIN_ADAPTIVE_CHUNK_TOKENS, MAX_ADAPTIVE_CHUNK_TOKENS),
            additive_step_tokens: DEFAULT_ADDITIVE_STEP_TOKENS,
            temporary_limit_tokens: None,
            temporary_limit_until_ms: None,
            retry_after_until_ms: None,
            consecutive_successes: 0,
            consecutive_health_failures: 0,
            timeout_streak: 0,
            sample_count: 0,
            success_count: 0,
            failure_count: 0,
            last_error_kind: None,
            last_seen_at_ms: now_ms,
            last_success_at_ms: None,
            last_failure_at_ms: None,
            last_request_bytes: 0,
            updated_at_ms: now_ms,
        }
    }

    pub fn effective_target_tokens(&self, now_ms: u64) -> u64 {
        let learned = if now_ms.saturating_sub(self.last_seen_at_ms) >= PROFILE_STALE_MS {
            self.learned_target_tokens
                .min(INITIAL_ADAPTIVE_CHUNK_TOKENS)
        } else {
            self.learned_target_tokens
        };
        let temporary = self
            .temporary_limit_until_ms
            .filter(|until| *until > now_ms)
            .and(self.temporary_limit_tokens);
        temporary
            .map(|limit| learned.min(limit))
            .unwrap_or(learned)
            .clamp(MIN_ADAPTIVE_CHUNK_TOKENS, MAX_ADAPTIVE_CHUNK_TOKENS)
    }

    pub fn blocked_reason(&self, now_ms: u64) -> Option<String> {
        match self.availability {
            RouteAvailability::CircuitOpen | RouteAvailability::Unsupported
                if self
                    .retry_after_until_ms
                    .is_some_and(|until| until > now_ms) =>
            {
                Some(format!(
                    "路由 {} 暂不可用，将在 {} 毫秒后允许半开探测。",
                    self.identity.route_key,
                    self.retry_after_until_ms
                        .unwrap_or(now_ms)
                        .saturating_sub(now_ms)
                ))
            }
            RouteAvailability::Disabled => Some("深度笔记路由已被禁用。".to_string()),
            RouteAvailability::Tombstoned => Some("深度笔记路由对应的配置已经移除。".to_string()),
            _ => None,
        }
    }

    pub fn apply_outcome(&mut self, outcome: &AdaptiveVolumeOutcome, now_ms: u64) {
        if now_ms.saturating_sub(self.last_seen_at_ms) >= PROFILE_STALE_MS {
            self.learned_target_tokens = self
                .learned_target_tokens
                .min(INITIAL_ADAPTIVE_CHUNK_TOKENS);
            self.consecutive_successes = 0;
            self.consecutive_health_failures = 0;
            self.timeout_streak = 0;
            if matches!(
                self.availability,
                RouteAvailability::Available
                    | RouteAvailability::Degraded
                    | RouteAvailability::CircuitOpen
            ) {
                self.availability = RouteAvailability::Unknown;
            }
        }
        self.sample_count = self.sample_count.saturating_add(1);
        self.last_seen_at_ms = now_ms;
        self.last_request_bytes = outcome.request_bytes;
        self.updated_at_ms = now_ms;

        if outcome.error_kind.is_none() {
            self.success_count = self.success_count.saturating_add(1);
            self.last_success_at_ms = Some(now_ms);
            self.last_error_kind = None;
            self.availability = RouteAvailability::Available;
            self.consecutive_health_failures = 0;
            self.timeout_streak = 0;
            self.retry_after_until_ms = None;
            if outcome.capacity_relevant {
                let exercised_threshold = self
                    .effective_target_tokens(now_ms)
                    .saturating_mul(CAPACITY_EXERCISE_PERCENT)
                    / 100;
                if outcome.estimated_input_tokens >= exercised_threshold {
                    self.consecutive_successes = self.consecutive_successes.saturating_add(1);
                    if self.consecutive_successes >= SUCCESSES_PER_INCREASE {
                        self.learned_target_tokens = self
                            .learned_target_tokens
                            .saturating_add(self.additive_step_tokens)
                            .min(MAX_ADAPTIVE_CHUNK_TOKENS);
                        self.consecutive_successes = 0;
                    }
                }
            }
            return;
        }

        let kind = outcome.error_kind.expect("error kind checked above");
        self.failure_count = self.failure_count.saturating_add(1);
        self.last_failure_at_ms = Some(now_ms);
        self.last_error_kind = Some(format!("{kind:?}"));
        self.consecutive_successes = 0;
        match kind {
            ModelErrorKind::ContextLengthExceeded if outcome.capacity_relevant => {
                self.learned_target_tokens =
                    (self.learned_target_tokens / 2).max(MIN_ADAPTIVE_CHUNK_TOKENS);
                self.temporary_limit_tokens = None;
                self.temporary_limit_until_ms = None;
                self.timeout_streak = 0;
                self.availability = RouteAvailability::Available;
            }
            ModelErrorKind::ClientTimeout | ModelErrorKind::UpstreamTimeout
                if outcome.capacity_relevant =>
            {
                let reduced = (self.learned_target_tokens / 2).max(MIN_ADAPTIVE_CHUNK_TOKENS);
                self.temporary_limit_tokens = Some(reduced);
                self.temporary_limit_until_ms = Some(now_ms + TRANSIENT_TIMEOUT_PENALTY_MS);
                self.timeout_streak = self.timeout_streak.saturating_add(1);
                if self.timeout_streak >= 2 {
                    self.learned_target_tokens = reduced;
                    self.timeout_streak = 0;
                }
                self.record_health_failure(now_ms, outcome.retry_after_ms);
            }
            ModelErrorKind::ModelNotFound => {
                self.availability = RouteAvailability::Unsupported;
                self.retry_after_until_ms = Some(now_ms + MODEL_RECHECK_MS);
                self.consecutive_health_failures = 0;
                self.timeout_streak = 0;
            }
            ModelErrorKind::MissingApiKey
            | ModelErrorKind::Authentication
            | ModelErrorKind::PermissionDenied
            | ModelErrorKind::QuotaExceeded
            | ModelErrorKind::InvalidConfiguration => {
                self.availability = RouteAvailability::CircuitOpen;
                self.retry_after_until_ms = Some(now_ms + CONFIG_RECHECK_MS);
                self.consecutive_health_failures = 0;
                self.timeout_streak = 0;
            }
            ModelErrorKind::RateLimited | ModelErrorKind::ConcurrencyLimited => {
                self.availability = RouteAvailability::CircuitOpen;
                self.retry_after_until_ms = Some(
                    now_ms
                        + outcome
                            .retry_after_ms
                            .unwrap_or(HEALTH_CIRCUIT_MS)
                            .max(1_000),
                );
                self.consecutive_health_failures = 0;
            }
            ModelErrorKind::ProviderUnavailable => {
                self.availability = RouteAvailability::CircuitOpen;
                self.retry_after_until_ms = Some(
                    now_ms
                        + outcome
                            .retry_after_ms
                            .unwrap_or(HEALTH_CIRCUIT_MS)
                            .max(1_000),
                );
                self.consecutive_health_failures = 0;
            }
            ModelErrorKind::Connection
            | ModelErrorKind::Provider
            | ModelErrorKind::InvalidResponse => {
                self.record_health_failure(now_ms, outcome.retry_after_ms);
            }
            ModelErrorKind::ContextLengthExceeded
            | ModelErrorKind::ContentFiltered
            | ModelErrorKind::Cancelled
            | ModelErrorKind::ClientTimeout
            | ModelErrorKind::UpstreamTimeout => {
                // 非 Chunk 超时不具备体积学习意义，但仍然是实时健康信号。
                if matches!(
                    kind,
                    ModelErrorKind::ClientTimeout | ModelErrorKind::UpstreamTimeout
                ) {
                    self.record_health_failure(now_ms, outcome.retry_after_ms);
                }
                self.timeout_streak = 0;
            }
        }
    }

    fn record_health_failure(&mut self, now_ms: u64, retry_after_ms: Option<u64>) {
        self.consecutive_health_failures = self.consecutive_health_failures.saturating_add(1);
        if self.consecutive_health_failures >= HEALTH_FAILURES_TO_OPEN {
            self.availability = RouteAvailability::CircuitOpen;
            self.retry_after_until_ms =
                Some(now_ms + retry_after_ms.unwrap_or(HEALTH_CIRCUIT_MS).max(1_000));
            self.consecutive_health_failures = 0;
        } else {
            self.availability = RouteAvailability::Degraded;
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AdaptiveVolumeOutcome {
    pub error_kind: Option<ModelErrorKind>,
    pub capacity_relevant: bool,
    pub estimated_input_tokens: u64,
    pub request_bytes: usize,
    pub retry_after_ms: Option<u64>,
}

impl AdaptiveVolumeOutcome {
    pub fn success(
        capacity_relevant: bool,
        estimated_input_tokens: u64,
        request_bytes: usize,
    ) -> Self {
        Self {
            error_kind: None,
            capacity_relevant,
            estimated_input_tokens,
            request_bytes,
            retry_after_ms: None,
        }
    }

    pub fn failure(
        kind: ModelErrorKind,
        capacity_relevant: bool,
        estimated_input_tokens: u64,
        request_bytes: usize,
        retry_after_ms: Option<u64>,
    ) -> Self {
        Self {
            error_kind: Some(kind),
            capacity_relevant,
            estimated_input_tokens,
            request_bytes,
            retry_after_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::types::{ProviderKind, ProviderModelConfig};

    fn settings(credential_revision: u64, base_url: &str) -> ModelSettings {
        ModelSettings {
            version: crate::settings::types::CURRENT_MODEL_SETTINGS_VERSION,
            providers: vec![ProviderConfig {
                id: "relay".to_string(),
                name: "Relay".to_string(),
                kind: ProviderKind::Custom,
                protocol: ApiProtocol::OpenAiChatCompletions,
                auth_scheme: AuthScheme::Bearer,
                base_url: base_url.to_string(),
                credential_revision,
                has_api_key: true,
                enabled: true,
                models: vec![ProviderModelConfig {
                    id: "model".to_string(),
                    api_model: "upstream-model".to_string(),
                    display_name: "Model".to_string(),
                    context_window_tokens: Some(128_000),
                    pricing: None,
                    capabilities: None,
                    enabled: true,
                }],
            }],
            default_provider_id: None,
            default_model_id: None,
            note_provider_id: None,
            note_model_id: None,
        }
    }

    #[test]
    fn route_identity_changes_with_endpoint_credential_and_transport() {
        let first = DeepNoteRouteIdentity::resolve(
            &settings(1, "https://one.test/v1"),
            "relay",
            "model",
            true,
        )
        .unwrap();
        let endpoint_changed = DeepNoteRouteIdentity::resolve(
            &settings(1, "https://two.test/v1"),
            "relay",
            "model",
            true,
        )
        .unwrap();
        let credential_changed = DeepNoteRouteIdentity::resolve(
            &settings(2, "https://one.test/v1"),
            "relay",
            "model",
            true,
        )
        .unwrap();
        let transport_changed = DeepNoteRouteIdentity::resolve(
            &settings(1, "https://one.test/v1"),
            "relay",
            "model",
            false,
        )
        .unwrap();
        assert_ne!(first.route_key, endpoint_changed.route_key);
        assert_ne!(first.route_key, credential_changed.route_key);
        assert_ne!(first.route_key, transport_changed.route_key);
    }

    #[test]
    fn aimd_increases_only_after_capacity_exercising_successes() {
        let identity = DeepNoteRouteIdentity::resolve(
            &settings(1, "https://one.test/v1"),
            "relay",
            "model",
            true,
        )
        .unwrap();
        let mut profile = AdaptiveVolumeProfile::new(identity, 8_000, 1);
        for now in 2..=4 {
            profile.apply_outcome(&AdaptiveVolumeOutcome::success(true, 1_000, 4_000), now);
        }
        assert_eq!(profile.learned_target_tokens, 8_000);
        for now in 5..=7 {
            profile.apply_outcome(&AdaptiveVolumeOutcome::success(true, 7_000, 28_000), now);
        }
        assert_eq!(profile.learned_target_tokens, 9_024);
    }

    #[test]
    fn first_timeout_is_temporary_and_second_timeout_lowers_learned_target() {
        let identity = DeepNoteRouteIdentity::resolve(
            &settings(1, "https://one.test/v1"),
            "relay",
            "model",
            true,
        )
        .unwrap();
        let mut profile = AdaptiveVolumeProfile::new(identity, 8_000, 1);
        let timeout = AdaptiveVolumeOutcome::failure(
            ModelErrorKind::UpstreamTimeout,
            true,
            8_000,
            32_000,
            None,
        );
        profile.apply_outcome(&timeout, 2);
        assert_eq!(profile.learned_target_tokens, 8_000);
        assert_eq!(profile.effective_target_tokens(3), 4_000);
        profile.apply_outcome(&timeout, 4);
        assert_eq!(profile.learned_target_tokens, 4_000);
        assert_eq!(profile.effective_target_tokens(5), 4_000);
    }

    #[test]
    fn availability_failures_do_not_shrink_payload_capacity() {
        let identity = DeepNoteRouteIdentity::resolve(
            &settings(1, "https://one.test/v1"),
            "relay",
            "model",
            true,
        )
        .unwrap();
        let mut profile = AdaptiveVolumeProfile::new(identity, 8_000, 1);
        profile.apply_outcome(
            &AdaptiveVolumeOutcome::failure(
                ModelErrorKind::RateLimited,
                true,
                8_000,
                32_000,
                Some(5_000),
            ),
            2,
        );
        assert_eq!(profile.learned_target_tokens, 8_000);
        assert_eq!(profile.availability, RouteAvailability::CircuitOpen);
        assert!(profile.blocked_reason(3).is_some());
        assert!(profile.blocked_reason(5_003).is_none());
    }

    #[test]
    fn stale_high_capacity_profile_returns_to_conservative_prior() {
        let identity = DeepNoteRouteIdentity::resolve(
            &settings(1, "https://one.test/v1"),
            "relay",
            "model",
            true,
        )
        .unwrap();
        let mut profile = AdaptiveVolumeProfile::new(identity, 16_000, 1);
        profile.availability = RouteAvailability::Available;
        let stale_at = PROFILE_STALE_MS + 2;
        assert_eq!(
            profile.effective_target_tokens(stale_at),
            INITIAL_ADAPTIVE_CHUNK_TOKENS
        );
        profile.apply_outcome(
            &AdaptiveVolumeOutcome::success(true, 8_000, 32_000),
            stale_at,
        );
        assert_eq!(profile.learned_target_tokens, INITIAL_ADAPTIVE_CHUNK_TOKENS);
        assert_eq!(profile.availability, RouteAvailability::Available);
    }
}
