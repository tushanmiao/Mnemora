//! 聊天附件的 Tauri 命令边界。
//!
//! 文件系统操作全部放入阻塞线程；导入会话目录时复用会话写锁，避免与删除和清空并发。

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    chat::{
        attachments::{
            discard_staged_attachment, discard_stored_attachments, import_attachments,
            inspect_attachment_paths, read_attachment_image, read_attachment_preview,
            save_pasted_attachment, PendingChatAttachment,
        },
        conversation_types::StoredChatAttachment,
    },
    state::AppState,
};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Attachment background task failed: {error}")
}

#[tauri::command]
pub async fn inspect_chat_attachments(
    paths: Vec<String>,
) -> Result<Vec<PendingChatAttachment>, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_attachment_paths(paths))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn save_pasted_chat_attachment(
    state: State<'_, AppState>,
    name: String,
    mime_type: String,
    data_base64: String,
) -> Result<PendingChatAttachment, String> {
    let attachment = tauri::async_runtime::spawn_blocking(move || {
        save_pasted_attachment(&name, &mime_type, &data_base64)
    })
    .await
    .map_err(join_error)??;
    state.register_staged_attachment(attachment.path.clone().into());
    Ok(attachment)
}

#[tauri::command]
pub async fn discard_staged_chat_attachment(
    state: State<'_, AppState>,
    path: String,
) -> Result<bool, String> {
    let tracked_path = path.clone().into();
    let removed = tauri::async_runtime::spawn_blocking(move || discard_staged_attachment(&path))
        .await
        .map_err(join_error)??;
    state.unregister_staged_attachment(&tracked_path);
    Ok(removed)
}

#[tauri::command]
pub async fn import_chat_attachments(
    state: State<'_, AppState>,
    request_id: String,
    conversation_id: String,
    paths: Vec<String>,
) -> Result<Vec<StoredChatAttachment>, String> {
    let cancellation = state.register_attachment_task(request_id.clone())?;
    let _write_guard = state.conversation_writes.lock().await;
    let repository = state.conversation_repository.clone();
    let tracked_paths = paths.iter().map(Into::into).collect::<Vec<_>>();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        import_attachments(&repository, &conversation_id, paths, Some(&cancellation))
    })
    .await;
    state.finish_attachment_task(&request_id);
    let attachments = joined.map_err(join_error)??;
    for path in tracked_paths {
        state.unregister_staged_attachment(&path);
    }
    Ok(attachments)
}

#[tauri::command]
pub async fn read_chat_attachment_preview(
    state: State<'_, AppState>,
    request_id: String,
    conversation_id: Option<String>,
    path: String,
    preview_path: Option<String>,
) -> Result<crate::chat::attachments::AttachmentDisplaySource, String> {
    let cancellation = state.register_attachment_task(request_id.clone())?;
    let permit = match state.attachment_preview_gate.acquire().await {
        Ok(permit) => permit,
        Err(_) => {
            state.finish_attachment_task(&request_id);
            return Err("附件预览队列已经关闭。".to_string());
        }
    };
    let repository = state.conversation_repository.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        read_attachment_preview(
            &repository,
            conversation_id.as_deref(),
            &path,
            preview_path.as_deref(),
            Some(&cancellation),
        )
    })
    .await;
    drop(permit);
    state.finish_attachment_task(&request_id);
    joined.map_err(join_error)?
}

#[tauri::command]
pub async fn read_chat_attachment_image(
    state: State<'_, AppState>,
    request_id: String,
    conversation_id: String,
    path: String,
) -> Result<crate::chat::attachments::AttachmentDisplaySource, String> {
    let cancellation = state.register_attachment_task(request_id.clone())?;
    let permit = match state.attachment_preview_gate.acquire().await {
        Ok(permit) => permit,
        Err(_) => {
            state.finish_attachment_task(&request_id);
            return Err("图片查看队列已经关闭。".to_string());
        }
    };
    let repository = state.conversation_repository.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        read_attachment_image(&repository, &conversation_id, &path, Some(&cancellation))
    })
    .await;
    drop(permit);
    state.finish_attachment_task(&request_id);
    joined.map_err(join_error)?
}

#[tauri::command]
pub fn cancel_chat_attachment_task(state: State<'_, AppState>, request_id: String) -> bool {
    state.cancel_attachment_task(&request_id)
}

#[tauri::command]
pub async fn discard_imported_chat_attachments(
    state: State<'_, AppState>,
    conversation_id: String,
    attachments: Vec<StoredChatAttachment>,
) -> Result<usize, String> {
    let _write_guard = state.conversation_writes.lock().await;
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        discard_stored_attachments(&repository, &conversation_id, &attachments)
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub fn open_chat_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
    path: String,
) -> Result<(), String> {
    let full_path = state
        .conversation_repository
        .resolve_attachment_path(&conversation_id, &path)?;
    if !full_path.is_file() {
        return Err("附件不存在。".to_string());
    }
    app.opener()
        .open_path(full_path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|error| format!("打开附件失败：{error}"))
}
