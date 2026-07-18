//! 非流式 Chat 服务。
//!
//! 调用顺序：校验命令输入 -> 解析启用的 Provider/Model -> 从系统凭据读取 Key ->
//! 转为短生命周期 `ProviderRequestContext` -> 调用 AI dispatcher。设置锁不会跨网络请求持有。

use zeroize::Zeroizing;

use crate::{
    ai::{
        dispatcher,
        error::ModelError,
        types::{ModelResponse, ProviderRequestContext},
    },
    settings::types::{ApiProtocol, AuthScheme, ModelSettings},
    state::AppState,
};

use super::types::ChatCompletionRequest;

struct ResolvedTarget {
    protocol: ApiProtocol,
    auth_scheme: AuthScheme,
    base_url: String,
    api_model: String,
}

pub async fn complete(
    state: &AppState,
    request: ChatCompletionRequest,
) -> Result<ModelResponse, ModelError> {
    request.validate()?;
    let provider_id = request.provider_id.trim().to_string();
    let model_id = request.model_id.trim().to_string();
    let target = {
        let settings = state
            .model_settings
            .read()
            .map_err(|_| ModelError::provider("模型设置暂时不可用，请重新启动应用后再试。"))?;
        resolve_target(&settings, &provider_id, &model_id)?
    };

    let secrets = state.secrets;
    let provider_id_for_store = provider_id.clone();
    let api_key =
        tauri::async_runtime::spawn_blocking(move || secrets.get_api_key(&provider_id_for_store))
            .await
            .map_err(|_| ModelError::provider("读取系统凭据的后台任务失败。"))?
            .map_err(|_| ModelError::provider("无法从系统凭据读取 API Key。"))?
            .ok_or_else(ModelError::missing_api_key)?;
    let api_key = Zeroizing::new(api_key);
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(ModelError::missing_api_key());
    }

    let model_request = request.into_model_request(target.api_model);
    let context = ProviderRequestContext {
        protocol: target.protocol,
        auth_scheme: target.auth_scheme,
        base_url: &target.base_url,
        api_key,
    };
    dispatcher::complete(&state.http, &context, &model_request).await
}

fn resolve_target(
    settings: &ModelSettings,
    provider_id: &str,
    model_id: &str,
) -> Result<ResolvedTarget, ModelError> {
    let provider = settings
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| ModelError::invalid_configuration("没有找到指定的模型供应商。"))?;
    if !provider.enabled {
        return Err(ModelError::invalid_configuration(
            "当前模型供应商已经停用。",
        ));
    }

    let model = provider
        .models
        .iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| ModelError::invalid_configuration("指定模型不属于当前供应商。"))?;
    if !model.enabled {
        return Err(ModelError::invalid_configuration("当前模型已经停用。"));
    }

    Ok(ResolvedTarget {
        protocol: provider.protocol,
        auth_scheme: provider.auth_scheme,
        base_url: provider.base_url.clone(),
        api_model: model.api_model.clone(),
    })
}

#[cfg(test)]
mod tests {
    use crate::settings::types::{ModelSettings, ProviderModelConfig};

    #[test]
    fn resolves_model_only_within_requested_provider() {
        let mut settings = ModelSettings::default();
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-1".to_string(),
            api_model: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            enabled: true,
        });

        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert_eq!(target.api_model, "gpt-test");
        assert!(super::resolve_target(&settings, "official-anthropic", "model-1").is_err());
    }

    #[test]
    fn rejects_disabled_provider_or_model() {
        let mut settings = ModelSettings::default();
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-1".to_string(),
            api_model: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            enabled: false,
        });
        assert!(super::resolve_target(&settings, "official-openai", "model-1").is_err());

        settings.providers[0].models[0].enabled = true;
        settings.providers[0].enabled = false;
        assert!(super::resolve_target(&settings, "official-openai", "model-1").is_err());
    }
}
