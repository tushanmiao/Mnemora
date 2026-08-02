//! Work 文献库 Tauri 命令边界。
//!
//! SQLite、哈希和文件复制都放入阻塞线程；所有写操作通过同一异步锁串行化。

use tauri::ipc::Response;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    library::types::{
        LibraryAnnotation, LibraryAnnotationCreate, LibraryAnnotationUpdate, LibraryCollection,
        LibraryImportResult, LibraryItem, LibraryItemUpdate, LibraryListPage, LibraryListRequest,
        LibraryNote, LibraryNoteCreate, LibraryNoteImportResult, LibraryNoteSummary, LibraryNoteUpdate, LibraryReadingState,
        LibraryReadingStateUpdate,
    },
    state::AppState,
};

fn join_error(error: impl std::fmt::Display) -> String {
    format!("文献库后台任务失败：{error}")
}

#[tauri::command]
pub async fn library_list_items(
    state: State<'_, AppState>,
    request: LibraryListRequest,
) -> Result<LibraryListPage, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list_items(request))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_get_item(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<LibraryItem, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.get_item(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_import_pdfs(
    state: State<'_, AppState>,
    paths: Vec<String>,
    collection_id: Option<String>,
) -> Result<LibraryImportResult, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.import_pdfs(paths, collection_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_update_item(
    state: State<'_, AppState>,
    update: LibraryItemUpdate,
) -> Result<LibraryItem, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.update_item(update))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_set_favorite(
    state: State<'_, AppState>,
    item_id: String,
    favorite: bool,
) -> Result<LibraryItem, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.set_favorite(&item_id, favorite))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_move_to_trash(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<LibraryItem, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.move_to_trash(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_restore_item(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<LibraryItem, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.restore_from_trash(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_delete_permanently(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<bool, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.delete_permanently(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_mark_opened(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<LibraryItem, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.mark_opened(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_open_item(
    app: AppHandle,
    state: State<'_, AppState>,
    item_id: String,
) -> Result<LibraryItem, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    let (item, path) = tauri::async_runtime::spawn_blocking(move || {
        let item = repository.mark_opened(&item_id)?;
        let path = repository.primary_file_path(&item_id)?;
        Ok::<_, String>((item, path))
    })
    .await
    .map_err(join_error)??;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<String>)
        .map_err(|error| format!("使用系统阅读器打开 PDF 失败：{error}"))?;
    Ok(item)
}

#[tauri::command]
pub async fn library_read_pdf_range(
    state: State<'_, AppState>,
    item_id: String,
    start: u64,
    end: u64,
) -> Result<Response, String> {
    let repository = state.library_repository.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        repository.read_pdf_range(&item_id, start, end)
    })
    .await
    .map_err(join_error)??;
    Ok(Response::new(bytes))
}

#[tauri::command]
pub async fn library_get_reading_state(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<LibraryReadingState, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.get_reading_state(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_save_reading_state(
    state: State<'_, AppState>,
    update: LibraryReadingStateUpdate,
) -> Result<LibraryReadingState, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.save_reading_state(update))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_list_annotations(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Vec<LibraryAnnotation>, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list_annotations(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_create_annotation(
    state: State<'_, AppState>,
    create: LibraryAnnotationCreate,
) -> Result<LibraryAnnotation, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.create_annotation(create))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_update_annotation(
    state: State<'_, AppState>,
    update: LibraryAnnotationUpdate,
) -> Result<LibraryAnnotation, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.update_annotation(update))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_delete_annotation(
    state: State<'_, AppState>,
    annotation_id: String,
) -> Result<bool, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.delete_annotation(&annotation_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_list_notes(
    state: State<'_, AppState>,
    item_id: Option<String>,
) -> Result<Vec<LibraryNoteSummary>, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list_notes(item_id.as_deref()))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_get_note(
    state: State<'_, AppState>,
    note_id: String,
) -> Result<LibraryNote, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.get_note(&note_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_create_note(
    state: State<'_, AppState>,
    create: LibraryNoteCreate,
) -> Result<LibraryNote, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.create_note(create))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_import_markdown_notes(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<LibraryNoteImportResult, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.import_markdown_notes(paths))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_update_note(
    state: State<'_, AppState>,
    update: LibraryNoteUpdate,
) -> Result<LibraryNote, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.update_note(update))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_delete_note(
    state: State<'_, AppState>,
    note_id: String,
) -> Result<bool, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.delete_note(&note_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_list_collections(
    state: State<'_, AppState>,
) -> Result<Vec<LibraryCollection>, String> {
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list_collections())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_create_collection(
    state: State<'_, AppState>,
    name: String,
) -> Result<LibraryCollection, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.create_collection(&name))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn library_rename_collection(
    state: State<'_, AppState>,
    collection_id: String,
    name: String,
) -> Result<(), String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.rename_collection(&collection_id, &name)
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub async fn library_delete_collection(
    state: State<'_, AppState>,
    collection_id: String,
) -> Result<bool, String> {
    let _write_guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.delete_collection(&collection_id))
        .await
        .map_err(join_error)?
}
