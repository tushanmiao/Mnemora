//! Chat Tauri 命令边界。
//!
//! React 调用 `chat_complete` 提交 Mnemora 内部模型 ID 和标准消息；本文件只转交 Chat 服务，
//! 不读取 API Key、不拼接供应商 URL，也不解析任何供应商 JSON。

use tauri::State;

use crate::{
    ai::{error::ModelError, types::ModelResponse},
    chat::{service, types::ChatCompletionRequest},
    state::AppState,
};

#[tauri::command]
pub async fn chat_complete(
    state: State<'_, AppState>,
    request: ChatCompletionRequest,
) -> Result<ModelResponse, ModelError> {
    service::complete(&state, request).await
}
