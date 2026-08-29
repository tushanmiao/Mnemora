use tauri::{async_runtime, State};

use crate::{
    mcp::{McpOverview, McpServerConfig, McpServerView},
    state::AppState,
};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("MCP background task failed: {error}")
}

#[tauri::command]
pub async fn mcp_list_servers(state: State<'_, AppState>) -> Result<McpOverview, String> {
    state.mcp_manager.overview()
}

#[tauri::command]
pub async fn mcp_upsert_server(
    state: State<'_, AppState>,
    config: McpServerConfig,
    bearer_token: Option<String>,
) -> Result<McpServerView, String> {
    let manager = state.mcp_manager.clone();
    async_runtime::spawn_blocking(move || manager.upsert_server(config, bearer_token))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn mcp_set_server_enabled(
    state: State<'_, AppState>,
    server_id: String,
    enabled: bool,
) -> Result<McpServerView, String> {
    let manager = state.mcp_manager.clone();
    async_runtime::spawn_blocking(move || manager.set_enabled(server_id.trim(), enabled))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn mcp_refresh_server(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<McpServerView, String> {
    state
        .mcp_manager
        .refresh_server(server_id.trim(), true)
        .await
}

#[tauri::command]
pub async fn mcp_remove_server(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<bool, String> {
    let manager = state.mcp_manager.clone();
    async_runtime::spawn_blocking(move || manager.remove_server(server_id.trim()))
        .await
        .map_err(join_error)?
}
