//! 笔记同步的 Tauri 命令入口。Token 只写入系统凭据，不返回给前端。

use tauri::State;

use crate::{
    state::AppState,
    sync::{
        service,
        types::{SyncRequest, SyncResult, SyncSettings},
    },
};

#[tauri::command]
pub async fn sync_load_settings(state: State<'_, AppState>) -> Result<SyncSettings, String> {
    let mut settings = state
        .sync_settings
        .read()
        .map_err(|_| "同步设置暂时不可用。".to_string())?
        .clone();
    let secret_store = state.sync_secrets;
    settings.notion.has_token =
        tokio::task::spawn_blocking(move || secret_store.has_notion_token())
            .await
            .map_err(join_error)??;
    let secret_store = state.sync_secrets;
    settings.feishu.has_app_secret =
        tokio::task::spawn_blocking(move || secret_store.has_feishu_app_secret())
            .await
            .map_err(join_error)??;
    Ok(settings)
}

#[tauri::command]
pub async fn sync_save_settings(
    state: State<'_, AppState>,
    settings: SyncSettings,
) -> Result<SyncSettings, String> {
    let mut settings = settings.normalize_and_validate()?;
    // 第一版只支持手动同步，避免后台定时器和 Vault 监听常驻内存。
    settings.auto_sync = false;
    let secret_store = state.sync_secrets;
    settings.notion.has_token =
        tokio::task::spawn_blocking(move || secret_store.has_notion_token())
            .await
            .map_err(join_error)??;
    let secret_store = state.sync_secrets;
    settings.feishu.has_app_secret =
        tokio::task::spawn_blocking(move || secret_store.has_feishu_app_secret())
            .await
            .map_err(join_error)??;
    let repository = state.sync_settings_repository.clone();
    let saved = settings.clone();
    tokio::task::spawn_blocking(move || repository.save(&saved))
        .await
        .map_err(join_error)??;
    *state
        .sync_settings
        .write()
        .map_err(|_| "同步设置暂时不可用。".to_string())? = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub async fn sync_set_notion_token(
    state: State<'_, AppState>,
    token: String,
) -> Result<bool, String> {
    let secret_store = state.sync_secrets;
    tokio::task::spawn_blocking(move || secret_store.set_notion_token(&token))
        .await
        .map_err(join_error)??;
    refresh_token_status(&state, true)?;
    persist_sync_settings(&state).await?;
    Ok(true)
}

#[tauri::command]
pub async fn sync_delete_notion_token(state: State<'_, AppState>) -> Result<bool, String> {
    let secret_store = state.sync_secrets;
    let deleted = tokio::task::spawn_blocking(move || secret_store.delete_notion_token())
        .await
        .map_err(join_error)??;
    refresh_token_status(&state, false)?;
    persist_sync_settings(&state).await?;
    Ok(deleted)
}

#[tauri::command]
pub async fn sync_set_feishu_app_secret(
    state: State<'_, AppState>,
    secret: String,
) -> Result<bool, String> {
    let secret_store = state.sync_secrets;
    tokio::task::spawn_blocking(move || secret_store.set_feishu_app_secret(&secret))
        .await
        .map_err(join_error)??;
    refresh_feishu_secret_status(&state, true)?;
    persist_sync_settings(&state).await?;
    Ok(true)
}

#[tauri::command]
pub async fn sync_delete_feishu_app_secret(state: State<'_, AppState>) -> Result<bool, String> {
    let secret_store = state.sync_secrets;
    let deleted = tokio::task::spawn_blocking(move || secret_store.delete_feishu_app_secret())
        .await
        .map_err(join_error)??;
    refresh_feishu_secret_status(&state, false)?;
    persist_sync_settings(&state).await?;
    Ok(deleted)
}

#[tauri::command]
pub async fn sync_run(
    state: State<'_, AppState>,
    request: SyncRequest,
) -> Result<SyncResult, String> {
    let _operation_guard = state.sync_operations.lock().await;
    let cancellation = state.start_sync_run().await;
    let settings = state
        .sync_settings
        .read()
        .map_err(|_| "同步设置暂时不可用。".to_string())?
        .clone();
    let sync = service::run(
        state.http.clone(),
        state.library_repository.clone(),
        state.sync_mapping_repository.clone(),
        state.sync_secrets,
        settings,
        request,
    );
    let result = tokio::select! {
        result = sync => result,
        _ = cancellation.cancelled() => Err("同步已取消。".to_string()),
    };
    state.finish_sync_run().await;
    result
}

fn refresh_token_status(state: &AppState, has_token: bool) -> Result<(), String> {
    state
        .sync_settings
        .write()
        .map_err(|_| "同步设置暂时不可用。".to_string())?
        .notion
        .has_token = has_token;
    Ok(())
}

fn refresh_feishu_secret_status(state: &AppState, has_secret: bool) -> Result<(), String> {
    state
        .sync_settings
        .write()
        .map_err(|_| "同步设置暂时不可用。".to_string())?
        .feishu
        .has_app_secret = has_secret;
    Ok(())
}

async fn persist_sync_settings(state: &AppState) -> Result<(), String> {
    let settings = state
        .sync_settings
        .read()
        .map_err(|_| "同步设置暂时不可用。".to_string())?
        .clone();
    let repository = state.sync_settings_repository.clone();
    tokio::task::spawn_blocking(move || repository.save(&settings))
        .await
        .map_err(join_error)??;
    Ok(())
}

fn join_error(error: tokio::task::JoinError) -> String {
    format!("同步后台任务失败：{error}")
}
