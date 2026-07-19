//! 本地会话 JSON 的 Tauri 命令边界。
//!
//! 所有文件读写都放入阻塞线程；保存、删除和清空使用同一异步互斥锁串行化，避免索引竞争。

use tauri::State;

use crate::{
    chat::conversation_types::{ConversationListItem, StoredConversation},
    state::AppState,
};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Conversation background task failed: {error}")
}

#[tauri::command]
pub async fn list_conversations(
    state: State<'_, AppState>,
) -> Result<Vec<ConversationListItem>, String> {
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn load_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<StoredConversation, String> {
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.load(&conversation_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn save_conversation(
    state: State<'_, AppState>,
    conversation: StoredConversation,
) -> Result<ConversationListItem, String> {
    let _write_guard = state.conversation_writes.lock().await;
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.save(&conversation))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn delete_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<bool, String> {
    let _write_guard = state.conversation_writes.lock().await;
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.delete(&conversation_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn clear_conversations(state: State<'_, AppState>) -> Result<(), String> {
    let _write_guard = state.conversation_writes.lock().await;
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.clear())
        .await
        .map_err(join_error)?
}
