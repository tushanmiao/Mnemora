//! 深度笔记与增量笔记编辑的 Tauri 命令边界。

use tauri::{ipc::Channel, AppHandle, State};

use crate::{
    chat::note_pipeline::{
        self, DeepNoteRunDetail, NoteEditPrepareRequest, NoteEditPrepareResult,
        NotePipelineAdjustRequest, NotePipelineConfirmRequest, NotePipelineProgress,
        NotePipelineStartRequest,
    },
    library::types::{LibraryNote, NotePipelineRun},
    state::AppState,
};

#[tauri::command]
pub async fn note_pipeline_start(
    app: AppHandle,
    request: NotePipelineStartRequest,
    on_event: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    note_pipeline::start(&app, request, on_event).await
}

#[tauri::command]
pub async fn note_pipeline_adjust(
    app: AppHandle,
    request: NotePipelineAdjustRequest,
    on_event: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    note_pipeline::adjust(&app, request, on_event).await
}

#[tauri::command]
pub async fn note_pipeline_confirm(
    app: AppHandle,
    request: NotePipelineConfirmRequest,
    on_event: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    note_pipeline::confirm(&app, request, on_event).await
}

#[tauri::command]
pub async fn note_pipeline_resume(
    app: AppHandle,
    run_id: String,
    on_event: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    note_pipeline::resume(&app, run_id, on_event).await
}

#[tauri::command]
pub async fn note_pipeline_cancel(app: AppHandle, run_id: String) -> Result<bool, String> {
    note_pipeline::cancel(&app, &run_id).await
}

#[tauri::command]
pub async fn note_pipeline_pause(
    app: AppHandle,
    run_id: String,
) -> Result<NotePipelineRun, String> {
    note_pipeline::pause(&app, &run_id).await
}

#[tauri::command]
pub async fn note_pipeline_list_resumable(
    state: State<'_, AppState>,
) -> Result<Vec<NotePipelineRun>, String> {
    note_pipeline::list_resumable(&state)
}

#[tauri::command]
pub async fn note_pipeline_get(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<NotePipelineRun, String> {
    note_pipeline::get_run(&state, &run_id)
}

#[tauri::command]
pub async fn note_pipeline_get_detail(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<DeepNoteRunDetail, String> {
    note_pipeline::get_detail(&state, &run_id)
}

#[tauri::command]
pub async fn note_edit_prepare(
    state: State<'_, AppState>,
    request: NoteEditPrepareRequest,
) -> Result<NoteEditPrepareResult, String> {
    note_pipeline::prepare_note_edit(&state, request).await
}

#[tauri::command]
pub async fn note_edit_resolve(
    state: State<'_, AppState>,
    proposal_id: String,
    accepted: bool,
) -> Result<Option<LibraryNote>, String> {
    let _guard = state.library_operations.lock().await;
    state
        .library_repository
        .resolve_note_edit_proposal(&proposal_id, accepted)
}
