//! 模型设置领域类型。
//!
//! 层次关系：`ModelSettings -> ProviderConfig -> ProviderModelConfig`。
//! `normalize_and_validate` 负责版本升级入口、字符串规范化、ID/URL/重复项校验，
//! 并在默认模型失效时选择第一个仍然启用的模型。
//! `has_api_key` 只表示系统凭据状态，反序列化时不会信任前端或 JSON 中的旧值。

use std::collections::HashSet;

use reqwest::Url;
use serde::{Deserialize, Serialize};

pub const CURRENT_MODEL_SETTINGS_VERSION: u32 = 3;
const MAX_PROVIDERS: usize = 100;
const MAX_MODELS_PER_PROVIDER: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Openai,
    Anthropic,
    Gemini,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiProtocol {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
    GeminiGenerateContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthScheme {
    ProtocolDefault,
    Bearer,
    XApiKey,
    XGoogApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConfig {
    pub id: String,
    pub api_model: String,
    pub display_name: String,
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub protocol: ApiProtocol,
    pub auth_scheme: AuthScheme,
    pub base_url: String,
    #[serde(default, skip_deserializing)]
    pub has_api_key: bool,
    pub enabled: bool,
    #[serde(default)]
    pub models: Vec<ProviderModelConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSettings {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub default_provider_id: Option<String>,
    #[serde(default)]
    pub default_model_id: Option<String>,
}

fn current_version() -> u32 {
    CURRENT_MODEL_SETTINGS_VERSION
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            version: CURRENT_MODEL_SETTINGS_VERSION,
            providers: vec![
                ProviderConfig {
                    id: "official-openai".to_string(),
                    name: "OpenAI".to_string(),
                    kind: ProviderKind::Openai,
                    protocol: ApiProtocol::OpenAiResponses,
                    auth_scheme: AuthScheme::ProtocolDefault,
                    base_url: "https://api.openai.com/v1".to_string(),
                    has_api_key: false,
                    enabled: true,
                    models: Vec::new(),
                },
                ProviderConfig {
                    id: "official-anthropic".to_string(),
                    name: "Anthropic".to_string(),
                    kind: ProviderKind::Anthropic,
                    protocol: ApiProtocol::AnthropicMessages,
                    auth_scheme: AuthScheme::ProtocolDefault,
                    base_url: "https://api.anthropic.com/v1".to_string(),
                    has_api_key: false,
                    enabled: true,
                    models: Vec::new(),
                },
                ProviderConfig {
                    id: "official-gemini".to_string(),
                    name: "Gemini".to_string(),
                    kind: ProviderKind::Gemini,
                    protocol: ApiProtocol::GeminiGenerateContent,
                    auth_scheme: AuthScheme::ProtocolDefault,
                    base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
                    has_api_key: false,
                    enabled: true,
                    models: Vec::new(),
                },
            ],
            default_provider_id: None,
            default_model_id: None,
        }
    }
}

impl ModelSettings {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        let source_version = self.version;
        if self.version > CURRENT_MODEL_SETTINGS_VERSION {
            return Err(format!(
                "Model settings version {} is newer than supported version {}",
                self.version, CURRENT_MODEL_SETTINGS_VERSION
            ));
        }
        self.version = CURRENT_MODEL_SETTINGS_VERSION;

        if self.providers.len() > MAX_PROVIDERS {
            return Err(format!("Too many providers; maximum is {MAX_PROVIDERS}"));
        }

        let mut provider_ids = HashSet::new();
        let mut all_model_ids = HashSet::new();
        for provider in &mut self.providers {
            provider.id = provider.id.trim().to_string();
            validate_stable_id("Provider ID", &provider.id)?;
            if !provider_ids.insert(provider.id.clone()) {
                return Err(format!("Duplicate provider ID: {}", provider.id));
            }

            provider.name = provider.name.trim().to_string();
            if provider.name.is_empty() {
                return Err(format!("Provider '{}' must have a name", provider.id));
            }
            if provider.name.chars().count() > 100 {
                return Err(format!("Provider '{}' name is too long", provider.id));
            }

            provider.base_url = normalize_base_url(&provider.base_url)?;
            if provider.models.len() > MAX_MODELS_PER_PROVIDER {
                return Err(format!(
                    "Provider '{}' has too many models; maximum is {MAX_MODELS_PER_PROVIDER}",
                    provider.id
                ));
            }

            let mut api_models = HashSet::new();
            for model in &mut provider.models {
                if source_version < 3 && model.context_window_tokens.is_none() {
                    model.context_window_tokens = Some(128_000);
                }
                model.id = model.id.trim().to_string();
                validate_stable_id("Model ID", &model.id)?;
                if !all_model_ids.insert(model.id.clone()) {
                    return Err(format!("Duplicate model ID: {}", model.id));
                }

                model.api_model = model.api_model.trim().to_string();
                if model.api_model.is_empty() {
                    return Err(format!("Model '{}' must have an API model name", model.id));
                }
                if model.api_model.chars().count() > 300 {
                    return Err(format!("Model '{}' API model name is too long", model.id));
                }
                if !api_models.insert(model.api_model.clone()) {
                    return Err(format!(
                        "Provider '{}' contains duplicate API model '{}'",
                        provider.id, model.api_model
                    ));
                }

                model.display_name = model.display_name.trim().to_string();
                if model.display_name.is_empty() {
                    model.display_name = model.api_model.clone();
                }
                if model.display_name.chars().count() > 200 {
                    return Err(format!("Model '{}' display name is too long", model.id));
                }
                if model
                    .context_window_tokens
                    .is_some_and(|tokens| !(1_024..=10_000_000).contains(&tokens))
                {
                    return Err(format!(
                        "Model '{}' context window must be between 1024 and 10000000 tokens",
                        model.id
                    ));
                }
            }
        }

        self.reconcile_default_model();
        Ok(self)
    }

    pub fn provider_exists(&self, provider_id: &str) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.id == provider_id)
    }

    fn reconcile_default_model(&mut self) {
        let current_is_valid = self.providers.iter().any(|provider| {
            provider.enabled
                && self.default_provider_id.as_deref() == Some(provider.id.as_str())
                && provider.models.iter().any(|model| {
                    model.enabled && self.default_model_id.as_deref() == Some(model.id.as_str())
                })
        });

        if current_is_valid {
            return;
        }

        let fallback = self.providers.iter().find_map(|provider| {
            if !provider.enabled {
                return None;
            }
            provider
                .models
                .iter()
                .find(|model| model.enabled)
                .map(|model| (provider.id.clone(), model.id.clone()))
        });

        self.default_provider_id = fallback
            .as_ref()
            .map(|(provider_id, _)| provider_id.clone());
        self.default_model_id = fallback.map(|(_, model_id)| model_id);
    }
}

pub fn validate_stable_id(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    if value.len() > 160 {
        return Err(format!("{label} is too long"));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(format!(
            "{label} may only contain letters, numbers, '-', '_', '.' or ':'"
        ));
    }
    Ok(())
}

fn normalize_base_url(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return Err("API Base URL is required".to_string());
    }
    if value.len() > 2_048 {
        return Err("API Base URL is too long".to_string());
    }

    let url = Url::parse(value).map_err(|_| "API Base URL is invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("API Base URL must use http or https".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("API Base URL cannot contain credentials".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("API Base URL cannot contain a query or fragment".to_string());
    }

    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ModelSettings, ProviderModelConfig, CURRENT_MODEL_SETTINGS_VERSION};

    #[test]
    fn defaults_include_three_official_providers() {
        let settings = ModelSettings::default();
        assert_eq!(settings.version, CURRENT_MODEL_SETTINGS_VERSION);
        assert_eq!(settings.providers.len(), 3);
        assert_eq!(settings.providers[0].id, "official-openai");
    }

    #[test]
    fn normalizes_model_display_name_and_default_selection() {
        let mut settings = ModelSettings::default();
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-1".to_string(),
            api_model: "  gpt-test  ".to_string(),
            display_name: "  ".to_string(),
            context_window_tokens: Some(128_000),
            enabled: true,
        });

        let settings = settings.normalize_and_validate().unwrap();
        assert_eq!(settings.providers[0].models[0].api_model, "gpt-test");
        assert_eq!(settings.providers[0].models[0].display_name, "gpt-test");
        assert_eq!(
            settings.default_provider_id.as_deref(),
            Some("official-openai")
        );
        assert_eq!(settings.default_model_id.as_deref(), Some("model-1"));
    }

    #[test]
    fn rejects_duplicate_api_models_within_provider() {
        let mut settings = ModelSettings::default();
        settings.providers[0].models = vec![
            ProviderModelConfig {
                id: "model-1".to_string(),
                api_model: "gpt-test".to_string(),
                display_name: "One".to_string(),
                context_window_tokens: None,
                enabled: true,
            },
            ProviderModelConfig {
                id: "model-2".to_string(),
                api_model: "gpt-test".to_string(),
                display_name: "Two".to_string(),
                context_window_tokens: None,
                enabled: true,
            },
        ];

        let error = settings.normalize_and_validate().unwrap_err();
        assert!(error.contains("duplicate API model"));
    }

    #[test]
    fn migrates_missing_context_window_to_lightweight_default() {
        let mut settings = ModelSettings::default();
        settings.version = 2;
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-context".to_string(),
            api_model: "model-context".to_string(),
            display_name: "Model Context".to_string(),
            context_window_tokens: None,
            enabled: true,
        });
        let settings = settings.normalize_and_validate().unwrap();
        assert_eq!(
            settings.providers[0].models[0].context_window_tokens,
            Some(128_000)
        );
    }
}
