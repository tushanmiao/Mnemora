//! 模型设置 Tauri 命令边界。
//!
//! 调用关系：React `api/settings.ts` -> 本模块 -> 配置仓库或系统凭据存储。
//! `load_model_settings` / `save_model_settings` 只传递非敏感配置；
//! `set_provider_api_key` / `delete_provider_api_key` 是密钥的单向写入和删除入口。

use std::collections::HashSet;

use tauri::State;
use zeroize::Zeroizing;

use crate::{settings::types::ModelSettings, state::AppState};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Settings background task failed: {error}")
}

/** 读取 Rust 内存中的非敏感模型设置；返回值永远不包含完整 API Key。 */
#[tauri::command]
pub async fn load_model_settings(state: State<'_, AppState>) -> Result<ModelSettings, String> {
    state
        .model_settings
        .read()
        .map(|settings| settings.clone())
        .map_err(|_| "Model settings lock is unavailable".to_string())
}

/** 保存版本化非敏感配置，并清理已经被删除供应商的系统凭据。 */
#[tauri::command]
pub async fn save_model_settings(
    state: State<'_, AppState>,
    settings: ModelSettings,
) -> Result<ModelSettings, String> {
    let settings = settings.normalize_and_validate()?;
    let previous_provider_ids = state
        .model_settings
        .read()
        .map_err(|_| "Model settings lock is unavailable".to_string())?
        .providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<HashSet<_>>();
    let next_provider_ids = settings
        .providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<HashSet<_>>();
    let removed_provider_ids = previous_provider_ids
        .difference(&next_provider_ids)
        .cloned()
        .collect::<Vec<_>>();

    let repository = state.model_settings_repository.clone();
    let secrets = state.secrets;
    let saved = tauri::async_runtime::spawn_blocking(move || {
        for provider_id in removed_provider_ids {
            secrets.delete_api_key(&provider_id)?;
        }

        let mut settings = settings;
        secrets.refresh_api_key_statuses(&mut settings)?;
        repository.save(&settings)?;
        Ok::<_, String>(settings)
    })
    .await
    .map_err(join_error)??;

    *state
        .model_settings
        .write()
        .map_err(|_| "Model settings lock is unavailable".to_string())? = saved.clone();
    Ok(saved)
}

/** 把一个供应商的 API Key 写入操作系统凭据存储，不写入 JSON 设置。 */
#[tauri::command]
pub async fn set_provider_api_key(
    state: State<'_, AppState>,
    provider_id: String,
    api_key: String,
) -> Result<bool, String> {
    let provider_exists = state
        .model_settings
        .read()
        .map_err(|_| "Model settings lock is unavailable".to_string())?
        .provider_exists(&provider_id);
    if !provider_exists {
        return Err("Provider not found".to_string());
    }

    let secrets = state.secrets;
    let provider_id_for_store = provider_id.clone();
    let api_key = Zeroizing::new(api_key);
    tauri::async_runtime::spawn_blocking(move || {
        secrets.set_api_key(&provider_id_for_store, api_key.as_str())
    })
    .await
    .map_err(join_error)??;

    if let Some(provider) = state
        .model_settings
        .write()
        .map_err(|_| "Model settings lock is unavailable".to_string())?
        .providers
        .iter_mut()
        .find(|provider| provider.id == provider_id)
    {
        provider.has_api_key = true;
    }
    Ok(true)
}

/** 删除一个供应商的系统凭据；凭据不存在时也视为成功。 */
#[tauri::command]
pub async fn delete_provider_api_key(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<bool, String> {
    let provider_exists = state
        .model_settings
        .read()
        .map_err(|_| "Model settings lock is unavailable".to_string())?
        .provider_exists(&provider_id);
    if !provider_exists {
        return Err("Provider not found".to_string());
    }

    let secrets = state.secrets;
    let provider_id_for_store = provider_id.clone();
    tauri::async_runtime::spawn_blocking(move || secrets.delete_api_key(&provider_id_for_store))
        .await
        .map_err(join_error)??;

    if let Some(provider) = state
        .model_settings
        .write()
        .map_err(|_| "Model settings lock is unavailable".to_string())?
        .providers
        .iter_mut()
        .find(|provider| provider.id == provider_id)
    {
        provider.has_api_key = false;
    }
    Ok(true)
}
