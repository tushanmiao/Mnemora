//! Chat Tauri 命令边界。
//!
//! React 调用 `chat_complete` 或 `chat_stream_start` 提交内部模型 ID 和标准消息；
//! `chat_stream_cancel` 只按 Run ID 触发取消。本文件不读取 Key、不拼 URL、不解析供应商 JSON。

use tauri::{ipc::Channel, State};

use crate::{
    ai::error::ModelError,
    chat::{
        agent::types::{AgentRunSnapshot, ToolQuestionAnswer},
        service,
        types::{
            ChatCompletionRequest, ChatCompletionResponse, ChatStreamRequest, ModelStreamEvent,
        },
    },
    state::AppState,
};

#[tauri::command]
pub fn chat_agent_run_get(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Option<AgentRunSnapshot>, String> {
    state.library_repository.get_agent_run_snapshot(&run_id)
}

#[tauri::command]
pub async fn chat_complete(
    state: State<'_, AppState>,
    request: ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ModelError> {
    service::complete(&state, request).await
}

#[tauri::command]
pub async fn chat_stream_start(
    state: State<'_, AppState>,
    request: ChatStreamRequest,
    on_event: Channel<ModelStreamEvent>,
) -> Result<(), ModelError> {
    service::stream(&state, request, on_event).await
}

#[tauri::command]
pub async fn chat_stream_cancel(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<bool, ModelError> {
    service::cancel(&state, &run_id).await
}

#[tauri::command]
pub async fn chat_tool_approval_resolve(
    state: State<'_, AppState>,
    approval_id: String,
    approved: bool,
) -> Result<bool, ModelError> {
    service::resolve_tool_approval(&state, &approval_id, approved).await
}

/// 提问工具的回答通道。
///
/// 与审批分开是因为载荷不同：审批只有一个布尔值，回答要带每问的选项。共用一个
/// 命令就得让前端传一个「要么 approved 要么 answers」的联合体，签名会更难用。
#[tauri::command]
pub async fn chat_tool_question_resolve(
    state: State<'_, AppState>,
    approval_id: String,
    answers: Vec<ToolQuestionAnswer>,
) -> Result<bool, ModelError> {
    service::resolve_tool_question(&state, &approval_id, answers).await
}
