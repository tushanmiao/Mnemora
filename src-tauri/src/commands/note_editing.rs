use crate::{
    library::note_editing::{
        NoteDraft, NoteEditingSnapshot, NoteImageAsset, NoteVersionEntry, SaveNoteReceipt,
        SaveNoteRequest,
    },
    state::AppState,
};
use tauri::Manager;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn note_editor_open_attachment(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    note_id: String,
    relative_path: String,
) -> Result<(), String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    let path = tauri::async_runtime::spawn_blocking(move || {
        repository.note_asset_bytes(&note_id, &relative_path)?;
        let relative = percent_encoding::percent_decode_str(&relative_path)
            .decode_utf8()
            .map_err(|_| "NOTE_ASSET_INVALID")?;
        let note = repository.get_note(&note_id)?;
        let directory = std::path::PathBuf::from(note.directory_path.ok_or("NOTE_SOURCE_MISSING")?)
            .canonicalize()
            .map_err(|_| "NOTE_SOURCE_MISSING")?;
        let path = directory
            .join(relative.as_ref())
            .canonicalize()
            .map_err(|_| "NOTE_ASSET_INVALID")?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !path.starts_with(directory)
            || ![
                "pdf", "png", "jpg", "jpeg", "gif", "webp", "txt", "md", "csv",
            ]
            .contains(&extension.as_str())
        {
            return Err("NOTE_ASSET_INVALID: 此附件类型不支持直接打开，请导出后查看。".to_string());
        }
        Ok(path)
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE".to_string())??;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|_| "NOTE_ASSET_INVALID: 无法打开附件。".to_string())
}

#[tauri::command]
pub async fn note_editor_validate_selection(
    state: State<'_, AppState>,
    note_id: String,
    note_version: String,
    content_hash: String,
    byte_start: u32,
    byte_end: u32,
    selected_text: String,
) -> Result<(), String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.validate_note_selection(
            &note_id,
            &note_version,
            &content_hash,
            byte_start,
            byte_end,
            &selected_text,
        )
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 选区校验失败。".to_string())?
}

#[tauri::command]
pub fn note_editor_register_close(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("NOTE_READ_ONLY".into());
    }
    crate::window_lifecycle::NOTE_EDITOR_CLOSE_GUARD
        .store(true, std::sync::atomic::Ordering::Release);
    Ok(())
}

#[tauri::command]
pub fn note_editor_finish_close(window: tauri::WebviewWindow, exit: bool) -> Result<(), String> {
    if window.label() != "main" {
        return Err("NOTE_READ_ONLY".into());
    }
    crate::window_lifecycle::NOTE_EDITOR_CLOSE_GUARD
        .store(false, std::sync::atomic::Ordering::Release);
    if exit {
        window.app_handle().exit(0);
    } else {
        crate::window_lifecycle::cleanup_before_main_window_close(window.app_handle());
        window.destroy().map_err(|_| "关闭窗口失败。".to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn note_editor_load(
    state: State<'_, AppState>,
    note_id: String,
) -> Result<NoteEditingSnapshot, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.recover_note_saves()?;
        repository.prune_note_editing_artifacts(&note_id)?;
        repository.note_editing_snapshot(&note_id)
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 加载任务失败。".to_string())?
}

#[tauri::command]
pub async fn note_editor_save(
    state: State<'_, AppState>,
    request: SaveNoteRequest,
) -> Result<SaveNoteReceipt, String> {
    let receipt = {
        let _guard = state.library_operations.lock().await;
        let repository = state.library_repository.clone();
        tauri::async_runtime::spawn_blocking(move || repository.save_note_checked(request))
            .await
            .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 保存任务失败。".to_string())??
    };
    super::knowledge::schedule_note_sync(state.inner(), receipt.note_id.clone());
    Ok(receipt)
}

#[tauri::command]
pub async fn note_editor_checkpoint(
    state: State<'_, AppState>,
    draft: NoteDraft,
) -> Result<(), String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.checkpoint_note_draft(draft))
        .await
        .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 草稿任务失败。".to_string())?
}

#[tauri::command]
pub async fn note_editor_discard_draft(
    state: State<'_, AppState>,
    note_id: String,
    session_id: String,
    generation: u32,
) -> Result<(), String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.discard_note_draft(&note_id, &session_id, generation)
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 草稿任务失败。".to_string())?
}

#[tauri::command]
pub async fn note_editor_stage_image(
    state: State<'_, AppState>,
    note_id: String,
    session_id: String,
    name: String,
    data_base64: String,
) -> Result<NoteImageAsset, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.stage_note_image(&note_id, &session_id, &name, &data_base64)
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 图片任务失败。".to_string())?
}

#[tauri::command]
pub async fn note_editor_versions(
    state: State<'_, AppState>,
    note_id: String,
) -> Result<Vec<NoteVersionEntry>, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.note_versions(&note_id))
        .await
        .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 版本读取失败。".to_string())?
}

#[tauri::command]
pub async fn note_editor_pin_version(
    state: State<'_, AppState>,
    note_id: String,
    version_id: String,
    pinned: bool,
) -> Result<(), String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.pin_note_version(&note_id, &version_id, pinned)
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 版本更新失败。".to_string())?
}

#[tauri::command]
pub async fn note_editor_copy_version(
    state: State<'_, AppState>,
    note_id: String,
    version_id: String,
) -> Result<crate::library::types::LibraryNote, String> {
    let note = {
        let _guard = state.library_operations.lock().await;
        let repository = state.library_repository.clone();
        tauri::async_runtime::spawn_blocking(move || {
            repository.copy_note_version(&note_id, &version_id)
        })
        .await
        .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 复制版本失败。".to_string())??
    };
    super::knowledge::schedule_note_sync(state.inner(), note.id.clone());
    Ok(note)
}

#[tauri::command]
pub async fn note_editor_read_asset(
    state: State<'_, AppState>,
    note_id: String,
    relative_path: String,
) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (bytes, mime) = repository.note_asset_bytes(&note_id, &relative_path)?;
        if !["image/png", "image/jpeg", "image/webp", "image/gif"].contains(&mime.as_str()) {
            return Err("NOTE_ASSET_INVALID: 非图片附件。".into());
        }
        Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 图片读取失败。".to_string())?
}

#[tauri::command]
pub async fn note_editor_export_bundle(
    state: State<'_, AppState>,
    note_id: String,
    title: String,
    markdown: String,
    destination: String,
) -> Result<String, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.export_note_snapshot(&note_id, &title, &markdown, &destination)
    })
    .await
    .map_err(|_| "NOTE_STORAGE_UNAVAILABLE: 导出失败。".to_string())?
}
