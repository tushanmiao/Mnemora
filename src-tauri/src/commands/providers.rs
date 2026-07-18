use tauri::State;

use crate::{
    ai::{
        providers,
        types::{ConnectionTestResult, ProviderConnectionInput},
    },
    state::AppState,
};

async fn resolve_api_key(
    state: &State<'_, AppState>,
    provider: &mut ProviderConnectionInput,
) -> Result<(), String> {
    if provider
        .api_key
        .as_deref()
        .is_some_and(|api_key| !api_key.trim().is_empty())
    {
        return Ok(());
    }

    let provider_id = provider
        .provider_id
        .as_deref()
        .ok_or_else(|| "Provider ID is required when API Key is not supplied".to_string())?;
    let provider_exists = state
        .model_settings
        .read()
        .map_err(|_| "Model settings lock is unavailable".to_string())?
        .provider_exists(provider_id);
    if !provider_exists {
        return Err("Provider not found".to_string());
    }

    let secrets = state.secrets;
    let provider_id = provider_id.to_string();
    let api_key = tauri::async_runtime::spawn_blocking(move || secrets.get_api_key(&provider_id))
        .await
        .map_err(|error| format!("Secret background task failed: {error}"))??
        .ok_or_else(|| "API Key is not configured".to_string())?;
    provider.api_key = Some(api_key);
    Ok(())
}

/** 用户点击“获取模型”后调用；不会由启动、保存或页面打开自动触发。 */
#[tauri::command]
pub async fn fetch_provider_models(
    state: State<'_, AppState>,
    mut provider: ProviderConnectionInput,
) -> Result<Vec<String>, String> {
    resolve_api_key(&state, &mut provider).await?;
    providers::fetch_models(&state.http, &provider).await
}

/** 用户点击“测试连接”后调用；只发送一次请求，不重试，也不轮换 Key。 */
#[tauri::command]
pub async fn test_provider_connection(
    state: State<'_, AppState>,
    mut provider: ProviderConnectionInput,
) -> Result<ConnectionTestResult, String> {
    resolve_api_key(&state, &mut provider).await?;
    Ok(providers::test_connection(&state.http, &provider).await)
}
