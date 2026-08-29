use std::path::PathBuf;

use tauri::{async_runtime, State};

use crate::{
    plugins::{PluginInstallRequest, PluginOverview, PluginSummary},
    state::AppState,
};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Plugin background task failed: {error}")
}

#[tauri::command]
pub async fn plugins_list(state: State<'_, AppState>) -> Result<PluginOverview, String> {
    let _guard = state.plugin_operations.lock().await;
    let manager = state.plugin_manager.clone();
    async_runtime::spawn_blocking(move || manager.list())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugins_install(
    state: State<'_, AppState>,
    path: String,
    request: PluginInstallRequest,
) -> Result<PluginSummary, String> {
    let _guard = state.plugin_operations.lock().await;
    let manager = state.plugin_manager.clone();
    async_runtime::spawn_blocking(move || manager.install(&PathBuf::from(path), request))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugins_set_enabled(
    state: State<'_, AppState>,
    plugin_id: String,
    enabled: bool,
) -> Result<PluginSummary, String> {
    let _guard = state.plugin_operations.lock().await;
    let manager = state.plugin_manager.clone();
    async_runtime::spawn_blocking(move || manager.set_enabled(plugin_id.trim(), enabled))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugins_rollback(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<PluginSummary, String> {
    let _guard = state.plugin_operations.lock().await;
    let manager = state.plugin_manager.clone();
    async_runtime::spawn_blocking(move || manager.rollback(plugin_id.trim()))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn plugins_uninstall(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<bool, String> {
    let _guard = state.plugin_operations.lock().await;
    let manager = state.plugin_manager.clone();
    async_runtime::spawn_blocking(move || manager.uninstall(plugin_id.trim()))
        .await
        .map_err(join_error)?
}
