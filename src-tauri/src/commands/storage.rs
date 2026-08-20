use std::path::PathBuf;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::state::AppState;
use crate::storage::StorageStatus;

#[tauri::command]
pub async fn storage_get_status(state: State<'_, AppState>) -> Result<StorageStatus, String> {
    let storage = state.storage.clone();
    tauri::async_runtime::spawn_blocking(move || storage.status())
        .await
        .map_err(|error| format!("读取存储状态失败：{error}"))?
}

#[tauri::command]
pub async fn storage_open_directory(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = state.storage.current_data_dir().to_path_buf();
    if !state.storage.is_available() {
        return Err("当前数据目录不可用，无法打开。".to_string());
    }
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|error| format!("打开数据目录失败：{error}"))
}

#[tauri::command]
pub async fn storage_migrate_data(
    app: AppHandle,
    state: State<'_, AppState>,
    destination: String,
) -> Result<(), String> {
    let destination = PathBuf::from(destination.trim());
    if destination.as_os_str().is_empty() {
        return Err("请选择数据目录。".to_string());
    }

    // 数据迁移发生在下一次启动、所有 Repository 重新绑定路径之前。
    // 有活动任务时先只发出取消信号，让用户稍后重试；没有活动任务的第二次操作
    // 才会写入迁移日志并立刻重启，避免 Repository 在准备完成后继续写旧目录。
    let _storage_guard = state.storage_operations.lock().await;
    let cancelled_chat = state.cancel_all_chat_runs().await;
    let cancelled_notes = state.cancel_all_note_pipeline_runs().await;
    let cancelled_approvals = state.cancel_all_tool_approvals().await;
    let cancelled_attachments = state.cancel_all_attachment_tasks();
    let cancelled_sync = state.cancel_sync_run().await;
    let cancelled_update = state.cancel_update_check().await;
    state.cleanup_current_staged_attachments();

    if cancelled_chat > 0
        || cancelled_notes > 0
        || cancelled_approvals > 0
        || cancelled_attachments > 0
        || cancelled_sync
        || cancelled_update
    {
        return Err("仍有后台任务正在停止，请等待几秒后重新更改数据位置。".to_string());
    }

    let _conversation_guard = state.conversation_writes.lock().await;
    let _library_guard = state.library_operations.lock().await;
    let _english_guard = state.english_operations.lock().await;
    let _english_learning_guard = state.english_learning_operations.lock().await;
    let _english_audio_guard = state.english_audio_operations.lock().await;
    let _sync_guard = state.sync_operations.lock().await;
    let _skill_guard = state.skill_operations.lock().await;
    let _usage_guard = state.usage_operations.lock().await;

    let storage = state.storage.clone();
    tauri::async_runtime::spawn_blocking(move || storage.prepare_migration(destination))
        .await
        .map_err(|error| format!("准备数据迁移失败：{error}"))??;

    eprintln!(
        "Prepared data migration; cancelled chat={cancelled_chat}, notes={cancelled_notes}, approvals={cancelled_approvals}, attachments={cancelled_attachments}, sync={cancelled_sync}, update={cancelled_update}."
    );
    app.restart();
}
