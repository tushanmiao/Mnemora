//! Skill 管理的 Tauri 命令边界。

use tauri::{async_runtime, State};

use crate::{
    skills::types::{
        SkillDetail, SkillImportRequest, SkillImportResult, SkillListResult, SkillSummary,
    },
    state::AppState,
};

#[tauri::command]
pub async fn skills_list(state: State<'_, AppState>) -> Result<SkillListResult, String> {
    let _guard = state.skill_operations.lock().await;
    let repository = state.skill_repository.clone();
    async_runtime::spawn_blocking(move || repository.list())
        .await
        .map_err(|error| format!("技能列表后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn skills_get_detail(
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<SkillDetail, String> {
    let repository = state.skill_repository.clone();
    async_runtime::spawn_blocking(move || repository.get_detail(skill_id.trim()))
        .await
        .map_err(|error| format!("技能详情后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn skills_import(
    state: State<'_, AppState>,
    request: SkillImportRequest,
) -> Result<SkillImportResult, String> {
    let _guard = state.skill_operations.lock().await;
    let repository = state.skill_repository.clone();
    async_runtime::spawn_blocking(move || repository.import(request))
        .await
        .map_err(|error| format!("技能安装后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn skills_set_enabled(
    state: State<'_, AppState>,
    skill_id: String,
    enabled: bool,
) -> Result<SkillSummary, String> {
    let _guard = state.skill_operations.lock().await;
    let repository = state.skill_repository.clone();
    async_runtime::spawn_blocking(move || repository.set_enabled(skill_id.trim(), enabled))
        .await
        .map_err(|error| format!("技能状态后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn skills_uninstall(state: State<'_, AppState>, skill_id: String) -> Result<(), String> {
    let _guard = state.skill_operations.lock().await;
    let repository = state.skill_repository.clone();
    async_runtime::spawn_blocking(move || repository.uninstall(skill_id.trim()))
        .await
        .map_err(|error| format!("技能删除后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn skills_restore_builtin(
    state: State<'_, AppState>,
    skill_id: String,
) -> Result<SkillSummary, String> {
    let _guard = state.skill_operations.lock().await;
    let repository = state.skill_repository.clone();
    async_runtime::spawn_blocking(move || repository.restore_builtin(skill_id.trim()))
        .await
        .map_err(|error| format!("恢复内置技能后台任务失败：{error}"))?
}
