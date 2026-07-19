//! Chat 请求服务。
//!
//! 非流式和流式调用共享目标解析与系统凭据读取。流式调用额外负责运行注册、Channel 事件
//! 和真实网络取消；设置锁不会跨网络请求持有，活动运行结束后一定从注册表移除。

use std::time::Duration;

use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::{
    ai::{
        dispatcher,
        error::{ModelError, ModelErrorKind},
        stream,
        types::{ModelResponse, ModelStreamChunk, ModelStreamOutcome, ProviderRequestContext},
    },
    settings::types::{ApiProtocol, AuthScheme, ModelSettings},
    state::AppState,
};

use super::types::{ChatCompletionRequest, ChatStreamRequest, ModelStreamEvent};

struct ResolvedTarget {
    protocol: ApiProtocol,
    auth_scheme: AuthScheme,
    base_url: String,
    api_model: String,
}

#[derive(Clone, Copy)]
struct RetryPolicy {
    max_retries: u8,
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
    let retry_policy = retry_policy(state);
    let mut retry_index = 0;
    loop {
        match dispatcher::complete(&state.http, &context, &model_request).await {
            Ok(response) => return Ok(response),
            Err(error)
                if retry_index < retry_policy.max_retries && should_retry(&error) =>
            {
                tokio::time::sleep(retry_delay(&error, retry_index)).await;
                retry_index += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

pub async fn stream(
    state: &AppState,
    request: ChatStreamRequest,
    on_event: Channel<ModelStreamEvent>,
) -> Result<(), ModelError> {
    request.validate()?;
    let run_id = request.run_id.trim().to_string();
    let conversation_id = request.conversation_id.trim().to_string();
    let message_id = request.message_id.trim().to_string();
    let cancellation = CancellationToken::new();

    {
        let mut active_runs = state.active_chat_runs.lock().await;
        if active_runs.contains_key(&run_id) {
            return Err(ModelError::invalid_configuration(
                "相同 Run ID 的流式请求已经存在。",
            ));
        }
        active_runs.insert(run_id.clone(), cancellation.clone());
    }

    if let Err(error) = on_event.send(ModelStreamEvent::Started {
        run_id: run_id.clone(),
        conversation_id: conversation_id.clone(),
        message_id: message_id.clone(),
    }) {
        state.active_chat_runs.lock().await.remove(&run_id);
        return Err(ModelError::provider(format!("无法发送流式开始事件：{error}")));
    }

    let result = stream_inner(
        state,
        request.completion,
        &cancellation,
        &on_event,
        &run_id,
        &conversation_id,
        &message_id,
    )
    .await;
    state.active_chat_runs.lock().await.remove(&run_id);

    let terminal_event = match result {
        Ok(ModelStreamOutcome::Completed(summary)) => ModelStreamEvent::Completed {
            run_id,
            conversation_id,
            message_id,
            finish_reason: summary.finish_reason,
            usage: summary.usage,
        },
        Ok(ModelStreamOutcome::Cancelled) => ModelStreamEvent::Stopped {
            run_id,
            conversation_id,
            message_id,
        },
        Err(error) => ModelStreamEvent::Error {
            run_id,
            conversation_id,
            message_id,
            error,
        },
    };
    on_event
        .send(terminal_event)
        .map_err(|error| ModelError::provider(format!("无法发送流式结束事件：{error}")))
}

async fn stream_inner(
    state: &AppState,
    request: ChatCompletionRequest,
    cancellation: &CancellationToken,
    on_event: &Channel<ModelStreamEvent>,
    run_id: &str,
    conversation_id: &str,
    message_id: &str,
) -> Result<ModelStreamOutcome, ModelError> {
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
    let retry_policy = retry_policy(state);
    let mut retry_index = 0;
    let mut emitted_text = false;
    loop {
        let mut emit = |chunk: ModelStreamChunk| match chunk {
            ModelStreamChunk::TextDelta(delta) => {
                on_event
                    .send(ModelStreamEvent::TextDelta {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.to_string(),
                        message_id: message_id.to_string(),
                        delta,
                    })
                    .map_err(|error| {
                        ModelError::provider(format!("无法发送文本增量：{error}"))
                    })?;
                emitted_text = true;
                Ok(())
            }
        };
        match stream::stream(
            &state.http,
            &context,
            &model_request,
            cancellation,
            &mut emit,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(error)
                if !emitted_text
                    && retry_index < retry_policy.max_retries
                    && should_retry(&error) =>
            {
                let delay = retry_delay(&error, retry_index);
                tokio::select! {
                    _ = cancellation.cancelled() => return Ok(ModelStreamOutcome::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
                retry_index += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

pub async fn cancel(state: &AppState, run_id: &str) -> Result<bool, ModelError> {
    crate::settings::types::validate_stable_id("Run ID", run_id.trim())
        .map_err(ModelError::invalid_configuration)?;
    let active_runs = state.active_chat_runs.lock().await;
    let Some(cancellation) = active_runs.get(run_id.trim()) else {
        return Ok(false);
    };
    cancellation.cancel();
    Ok(true)
}

fn retry_policy(state: &AppState) -> RetryPolicy {
    state
        .app_settings
        .read()
        .map(|settings| RetryPolicy {
            max_retries: if settings.retry_enabled {
                settings.retry_attempts
            } else {
                0
            },
        })
        .unwrap_or(RetryPolicy { max_retries: 0 })
}

fn should_retry(error: &ModelError) -> bool {
    matches!(
        error.kind,
        ModelErrorKind::RateLimited
            | ModelErrorKind::Timeout
            | ModelErrorKind::Connection
            | ModelErrorKind::Provider
    )
}

fn retry_delay(error: &ModelError, retry_index: u8) -> Duration {
    let exponential_ms = 300u64.saturating_mul(1u64 << retry_index.min(4));
    Duration::from_millis(
        error
            .retry_after_ms
            .unwrap_or(exponential_ms)
            .clamp(100, 5_000),
    )
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
    use crate::ai::error::{ModelError, ModelErrorKind};
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

    #[test]
    fn retries_only_transient_model_errors() {
        let transient = ModelError {
            kind: ModelErrorKind::RateLimited,
            message: "retry".to_string(),
            status_code: Some(429),
            provider_code: None,
            retry_after_ms: None,
        };
        let permanent = ModelError {
            kind: ModelErrorKind::Authentication,
            message: "stop".to_string(),
            status_code: Some(401),
            provider_code: None,
            retry_after_ms: None,
        };
        assert!(super::should_retry(&transient));
        assert!(!super::should_retry(&permanent));
    }
}
