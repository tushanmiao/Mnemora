//! 基础设置 Tauri 命令。
//!
//! 普通设置读取不返回 API Key；用户主动导出的完整备份会包含系统凭据中的供应商 API Key。
//! 导入完成后只把不含密钥的设置结构返回前端。保存基础设置时同步 Windows 开机启动状态。

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{
    memory::MemoryLayer,
    settings::{app_types::AppSettings, types::ModelSettings},
    state::AppState,
};

const EXPORT_VERSION: u32 = 3;
const MAX_IMPORT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsBundle {
    pub version: u32,
    pub app_settings: AppSettings,
    pub model_settings: ModelSettings,
    #[serde(default)]
    pub memory_imported: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsBundleInspection {
    pub version: u32,
    pub contains_memory: bool,
    pub memory_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryBackup {
    pub l1: String,
    pub l2: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsBundleFile {
    pub version: u32,
    pub app_settings: AppSettings,
    pub model_settings: ModelSettings,
    #[serde(default)]
    pub provider_api_keys: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryBackup>,
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
    let launch_at_startup_changed = state
        .app_settings
        .read()
        .map_err(|_| "App settings lock is unavailable".to_string())?
        .launch_at_startup
        != settings.launch_at_startup;
    if launch_at_startup_changed {
        apply_autostart(&app, settings.launch_at_startup)?;
    }
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
    include_memory: bool,
) -> Result<(), String> {
    let path = validate_user_path(path)?;
    let app_settings = state
        .app_settings
        .read()
        .map_err(|_| "App settings lock is unavailable".to_string())?
        .clone();
    let model_settings = state
        .model_settings
        .read()
        .map_err(|_| "Model settings lock is unavailable".to_string())?
        .clone();
    let provider_ids = model_settings
        .providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<Vec<_>>();
    let secrets = state.secrets;
    let memory_repository = state.memory_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut provider_api_keys = BTreeMap::new();
        for provider_id in provider_ids {
            if let Some(api_key) = secrets.get_api_key(&provider_id)? {
                provider_api_keys.insert(provider_id, api_key);
            }
        }
        let memory = if include_memory {
            Some(MemoryBackup {
                l1: memory_repository.read(MemoryLayer::L1)?,
                l2: memory_repository.read(MemoryLayer::L2)?,
            })
        } else {
            None
        };
        let bundle = SettingsBundleFile {
            version: EXPORT_VERSION,
            app_settings,
            model_settings,
            provider_api_keys: Some(provider_api_keys),
            memory,
        };
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
pub async fn inspect_settings_bundle(path: String) -> Result<SettingsBundleInspection, String> {
    let path = validate_user_path(path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let bundle = read_bundle_file(&path)?;
        let memory_bytes = bundle
            .memory
            .as_ref()
            .map(|memory| memory.l1.len().saturating_add(memory.l2.len()))
            .unwrap_or(0);
        Ok(SettingsBundleInspection {
            version: bundle.version,
            contains_memory: bundle.memory.is_some(),
            memory_bytes,
        })
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub async fn import_settings_bundle(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    include_memory: bool,
) -> Result<SettingsBundle, String> {
    let path = validate_user_path(path)?;
    let previous_launch_at_startup = state
        .app_settings
        .read()
        .map_err(|_| "App settings lock is unavailable".to_string())?
        .launch_at_startup;
    let app_repository = state.app_settings_repository.clone();
    let model_repository = state.model_settings_repository.clone();
    let memory_repository = state.memory_repository.clone();
    let secrets = state.secrets;
    let bundle = tauri::async_runtime::spawn_blocking(move || {
        let mut file_bundle = read_bundle_file(&path)?;
        file_bundle.app_settings = file_bundle.app_settings.normalize_and_validate()?;
        file_bundle.model_settings = file_bundle.model_settings.normalize_and_validate()?;

        if let Some(provider_api_keys) = &file_bundle.provider_api_keys {
            let provider_ids = file_bundle
                .model_settings
                .providers
                .iter()
                .map(|provider| provider.id.as_str())
                .collect::<HashSet<_>>();
            for (provider_id, api_key) in provider_api_keys {
                if !provider_ids.contains(provider_id.as_str()) {
                    return Err(format!(
                        "Backup contains API Key for unknown provider '{provider_id}'"
                    ));
                }
                if api_key.trim().is_empty() || api_key.len() > 16_384 {
                    return Err(format!(
                        "Backup contains invalid API Key for provider '{provider_id}'"
                    ));
                }
            }
        }

        if include_memory {
            if let Some(memory) = &file_bundle.memory {
                validate_memory_backup(memory)?;
            }
        }

        app_repository.save(&file_bundle.app_settings)?;
        model_repository.save(&file_bundle.model_settings)?;
        if let Some(provider_api_keys) = file_bundle.provider_api_keys {
            for provider in &file_bundle.model_settings.providers {
                if let Some(api_key) = provider_api_keys.get(&provider.id) {
                    secrets.set_api_key(&provider.id, api_key)?;
                } else {
                    secrets.delete_api_key(&provider.id)?;
                }
            }
        }
        let memory_imported = include_memory && file_bundle.memory.is_some();
        if let Some(memory) = file_bundle.memory.as_ref().filter(|_| include_memory) {
            memory_repository.save(MemoryLayer::L1, &memory.l1)?;
            memory_repository.save(MemoryLayer::L2, &memory.l2)?;
        }
        secrets.refresh_api_key_statuses(&mut file_bundle.model_settings)?;
        Ok::<_, String>(SettingsBundle {
            version: EXPORT_VERSION,
            app_settings: file_bundle.app_settings,
            model_settings: file_bundle.model_settings,
            memory_imported,
        })
    })
    .await
    .map_err(join_error)??;

    if previous_launch_at_startup != bundle.app_settings.launch_at_startup {
        apply_autostart(&app, bundle.app_settings.launch_at_startup)?;
    }
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

fn read_bundle_file(path: &Path) -> Result<SettingsBundleFile, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Failed to inspect settings import: {error}"))?;
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err("Settings import file is too large".to_string());
    }
    let raw = fs::read(path).map_err(|error| format!("Failed to read settings import: {error}"))?;
    let bundle = serde_json::from_slice::<SettingsBundleFile>(&raw)
        .map_err(|error| format!("Failed to parse settings import: {error}"))?;
    if bundle.version > EXPORT_VERSION {
        return Err("Settings import version is newer than this app".to_string());
    }
    Ok(bundle)
}

fn validate_memory_backup(memory: &MemoryBackup) -> Result<(), String> {
    validate_memory_text(MemoryLayer::L1, &memory.l1)?;
    validate_memory_text(MemoryLayer::L2, &memory.l2)
}

fn validate_memory_text(layer: MemoryLayer, content: &str) -> Result<(), String> {
    if content.len() > layer.max_bytes() {
        return Err(format!(
            "{} cannot exceed {} bytes",
            layer.file_name(),
            layer.max_bytes()
        ));
    }
    let normalized = content.to_ascii_lowercase();
    let forbidden = [
        "api key",
        "api key:",
        "api_key:",
        "authorization:",
        "bearer ",
        "password",
        "密码",
        "私钥",
        "-----begin private key-----",
        "ignore previous instructions",
        "ignore all previous instructions",
        "忽略之前的指令",
        "<mnemora_memory_l1>",
    ];
    if forbidden.iter().any(|needle| normalized.contains(needle)) {
        return Err(format!(
            "{} contains credentials or prompt-injection text",
            layer.file_name()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{validate_memory_text, MemoryLayer, SettingsBundleFile};
    use crate::{settings::app_types::AppSettings, settings::types::ModelSettings};

    #[test]
    fn old_settings_bundle_without_memory_remains_importable() {
        let value = json!({
            "version": 2,
            "appSettings": AppSettings::default(),
            "modelSettings": ModelSettings::default()
        });
        let bundle: SettingsBundleFile = serde_json::from_value(value).unwrap();
        assert_eq!(bundle.version, 2);
        assert!(bundle.memory.is_none());
        assert!(bundle.provider_api_keys.is_none());
    }

    #[test]
    fn memory_backup_validation_checks_each_layer_and_sensitive_markers() {
        assert!(validate_memory_text(MemoryLayer::L1, &"x".repeat(5_000)).is_ok());
        assert!(validate_memory_text(MemoryLayer::L1, &"x".repeat(5_001)).is_err());
        assert!(validate_memory_text(MemoryLayer::L2, "Bearer secret").is_err());
        assert!(validate_memory_text(MemoryLayer::L2, "普通的长期记忆").is_ok());
    }
}
