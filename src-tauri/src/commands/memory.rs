//! 记忆设置页使用的按需文件命令。

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{memory::MemoryLayer, state::AppState};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Memory background task failed: {error}")
}

#[tauri::command]
pub async fn memory_load(state: State<'_, AppState>, layer: MemoryLayer) -> Result<String, String> {
    let repository = state.memory_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.read(layer))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn memory_save(
    state: State<'_, AppState>,
    layer: MemoryLayer,
    content: String,
) -> Result<(), String> {
    let repository = state.memory_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.save(layer, &content))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn memory_clear(state: State<'_, AppState>, layer: MemoryLayer) -> Result<(), String> {
    let repository = state.memory_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.clear(layer))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn memory_get_directory(state: State<'_, AppState>) -> Result<String, String> {
    let repository = state.memory_repository.clone();
    let directory = tauri::async_runtime::spawn_blocking(move || repository.directory())
        .await
        .map_err(join_error)??;
    Ok(directory.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn memory_open_directory(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let repository = state.memory_repository.clone();
    let directory = tauri::async_runtime::spawn_blocking(move || repository.directory())
        .await
        .map_err(join_error)??;
    let display_path = directory.to_string_lossy().into_owned();
    app.opener()
        .open_path(display_path.clone(), None::<String>)
        .map_err(|error| format!("Failed to open memory directory: {error}"))?;
    Ok(display_path)
}
