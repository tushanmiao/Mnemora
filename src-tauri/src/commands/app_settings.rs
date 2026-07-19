//! 基础设置 Tauri 命令。
//!
//! 普通读取和导出只包含非敏感设置；API Key 不进入备份文件。保存时同步 Windows 开机启动状态。

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{
    settings::{app_types::AppSettings, types::ModelSettings},
    state::AppState,
};

const EXPORT_VERSION: u32 = 1;
const MAX_IMPORT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsBundle {
    pub version: u32,
    pub app_settings: AppSettings,
    pub model_settings: ModelSettings,
}

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Settings background task failed: {error}")
}

fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|error| format!("Failed to update launch-at-startup setting: {error}"))
}

fn validate_user_path(path: String) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("File path cannot be empty".to_string());
    }
    if path.len() > 32_768 {
        return Err("File path is too long".to_string());
    }
    Ok(PathBuf::from(path))
}

#[tauri::command]
pub async fn load_application_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    state
        .app_settings
        .read()
        .map(|settings| settings.clone())
        .map_err(|_| "App settings lock is unavailable".to_string())
}

#[tauri::command]
pub async fn save_application_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let settings = settings.normalize_and_validate()?;
    apply_autostart(&app, settings.launch_at_startup)?;
    let repository = state.app_settings_repository.clone();
    let settings_for_save = settings.clone();
    tauri::async_runtime::spawn_blocking(move || repository.save(&settings_for_save))
        .await
        .map_err(join_error)??;
    *state
        .app_settings
        .write()
        .map_err(|_| "App settings lock is unavailable".to_string())? = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub async fn export_settings_bundle(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let path = validate_user_path(path)?;
    let bundle = SettingsBundle {
        version: EXPORT_VERSION,
        app_settings: state
            .app_settings
            .read()
            .map_err(|_| "App settings lock is unavailable".to_string())?
            .clone(),
        model_settings: state
            .model_settings
            .read()
            .map_err(|_| "Model settings lock is unavailable".to_string())?
            .clone(),
    };
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create export directory: {error}"))?;
        }
        let json = serde_json::to_vec_pretty(&bundle)
            .map_err(|error| format!("Failed to serialize settings export: {error}"))?;
        fs::write(path, json).map_err(|error| format!("Failed to export settings: {error}"))
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub async fn import_settings_bundle(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<SettingsBundle, String> {
    let path = validate_user_path(path)?;
    let app_repository = state.app_settings_repository.clone();
    let model_repository = state.model_settings_repository.clone();
    let secrets = state.secrets;
    let bundle = tauri::async_runtime::spawn_blocking(move || {
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Failed to inspect settings import: {error}"))?;
        if metadata.len() > MAX_IMPORT_BYTES {
            return Err("Settings import file is too large".to_string());
        }
        let raw =
            fs::read(&path).map_err(|error| format!("Failed to read settings import: {error}"))?;
        let mut bundle: SettingsBundle = serde_json::from_slice(&raw)
            .map_err(|error| format!("Failed to parse settings import: {error}"))?;
        if bundle.version > EXPORT_VERSION {
            return Err("Settings import version is newer than this app".to_string());
        }
        bundle.version = EXPORT_VERSION;
        bundle.app_settings = bundle.app_settings.normalize_and_validate()?;
        bundle.model_settings = bundle.model_settings.normalize_and_validate()?;
        secrets.refresh_api_key_statuses(&mut bundle.model_settings)?;
        app_repository.save(&bundle.app_settings)?;
        model_repository.save(&bundle.model_settings)?;
        Ok::<_, String>(bundle)
    })
    .await
    .map_err(join_error)??;

    apply_autostart(&app, bundle.app_settings.launch_at_startup)?;
    *state
        .app_settings
        .write()
        .map_err(|_| "App settings lock is unavailable".to_string())? = bundle.app_settings.clone();
    *state
        .model_settings
        .write()
        .map_err(|_| "Model settings lock is unavailable".to_string())? =
        bundle.model_settings.clone();
    Ok(bundle)
}
