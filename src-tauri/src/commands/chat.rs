//! Chat Tauri 命令边界。
//!
//! React 调用 `chat_complete` 或 `chat_stream_start` 提交内部模型 ID 和标准消息；
//! `chat_stream_cancel` 只按 Run ID 触发取消。本文件不读取 Key、不拼 URL、不解析供应商 JSON。

use tauri::{ipc::Channel, State};

use crate::{
    ai::{error::ModelError, types::ModelResponse},
    chat::{
        service,
        types::{ChatCompletionRequest, ChatStreamRequest, ModelStreamEvent},
    },
    state::AppState,
};

#[tauri::command]
pub async fn chat_complete(
    state: State<'_, AppState>,
    request: ChatCompletionRequest,
) -> Result<ModelResponse, ModelError> {
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
