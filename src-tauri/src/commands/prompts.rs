use tauri::{async_runtime, State};

use crate::{
    prompts::types::{PromptTemplate, PromptTemplateInput},
    state::AppState,
};

#[tauri::command]
pub async fn prompt_templates_list(
    state: State<'_, AppState>,
) -> Result<Vec<PromptTemplate>, String> {
    let _guard = state.prompt_template_operations.lock().await;
    let repository = state.prompt_template_repository.clone();
    async_runtime::spawn_blocking(move || repository.list())
        .await
        .map_err(|error| format!("提示词列表后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn prompt_templates_upsert(
    state: State<'_, AppState>,
    input: PromptTemplateInput,
) -> Result<PromptTemplate, String> {
    let _guard = state.prompt_template_operations.lock().await;
    let repository = state.prompt_template_repository.clone();
    async_runtime::spawn_blocking(move || repository.upsert(input))
        .await
        .map_err(|error| format!("保存提示词后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn prompt_templates_delete(
    state: State<'_, AppState>,
    prompt_id: String,
) -> Result<bool, String> {
    let _guard = state.prompt_template_operations.lock().await;
    let repository = state.prompt_template_repository.clone();
    async_runtime::spawn_blocking(move || repository.delete(&prompt_id))
        .await
        .map_err(|error| format!("删除提示词后台任务失败：{error}"))?
}
