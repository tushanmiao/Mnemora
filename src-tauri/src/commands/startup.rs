//! 前端启动和渲染异常的持久化入口。

use tauri::{State, Window};

use crate::{startup_log::StartupDiagnosticPayload, state::AppState};

#[tauri::command]
pub async fn record_startup_error(
    window: Window,
    state: State<'_, AppState>,
    diagnostic: StartupDiagnosticPayload,
) -> Result<(), String> {
    let logger = state.startup_error_log.clone();
    let window_label = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || logger.record(&window_label, diagnostic))
        .await
        .map_err(|error| format!("Startup error log task failed: {error}"))?
}
