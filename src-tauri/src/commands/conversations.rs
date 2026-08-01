//! 本地会话 JSON 的 Tauri 命令边界。
//!
//! 所有文件读写都放入阻塞线程；保存、删除和清空使用同一异步互斥锁串行化，避免索引竞争。

use std::{fs, path::PathBuf};

use tauri::State;

use crate::{
    chat::conversation_types::{ConversationListItem, ConversationListPage, StoredConversation},
    state::AppState,
};

const DEFAULT_CONVERSATION_PAGE_SIZE: usize = 50;
const MAX_CONVERSATION_PAGE_SIZE: usize = 100;
const MAX_EXPORT_PATH_CHARS: usize = 32_768;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationExportFormat {
    Markdown,
    Json,
}

fn join_error(error: impl std::fmt::Display) -> String {
    format!("Conversation background task failed: {error}")
}

#[tauri::command]
pub async fn list_conversations(
    state: State<'_, AppState>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<ConversationListPage, String> {
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(DEFAULT_CONVERSATION_PAGE_SIZE);
    if !(1..=MAX_CONVERSATION_PAGE_SIZE).contains(&limit) {
        return Err(format!(
            "Conversation page size must be between 1 and {MAX_CONVERSATION_PAGE_SIZE}"
        ));
    }
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list_page(offset, limit))
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

#[tauri::command]
pub async fn export_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
    path: String,
    format: ConversationExportFormat,
) -> Result<(), String> {
    if path.trim().is_empty() || path.chars().count() > MAX_EXPORT_PATH_CHARS {
        return Err("导出路径无效。".to_string());
    }
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conversation = repository.load(&conversation_id)?;
        let content = match format {
            ConversationExportFormat::Markdown => conversation_to_markdown(&conversation),
            ConversationExportFormat::Json => serde_json::to_string_pretty(&conversation)
                .map_err(|error| format!("序列化会话失败：{error}"))?,
        };
        write_export_file(PathBuf::from(path), content.as_bytes())
    })
    .await
    .map_err(join_error)?
}

fn conversation_to_markdown(conversation: &StoredConversation) -> String {
    let mut output = format!("# {}\n\n", conversation.title.trim());
    for message in &conversation.messages {
        let role = match message.role {
            crate::ai::types::ModelRole::User => "用户",
            crate::ai::types::ModelRole::Assistant => "助手",
            crate::ai::types::ModelRole::Tool => "工具",
        };
        output.push_str(&format!("## {role}\n\n"));
        if !message.content.trim().is_empty() {
            output.push_str(message.content.trim());
            output.push_str("\n\n");
        }
        if !message.attachments.is_empty() {
            output.push_str("附件：\n");
            for attachment in &message.attachments {
                output.push_str(&format!(
                    "- {}（{}，{} bytes）\n",
                    attachment.name, attachment.mime_type, attachment.size_bytes
                ));
            }
            output.push('\n');
        }
        if !message.literature_references.is_empty() {
            output.push_str("文献引用：\n");
            for reference in &message.literature_references {
                output.push_str(&format!(
                    "- {}，第 {} 页\n",
                    reference.title,
                    reference.page_index + 1
                ));
                for line in reference.text.lines() {
                    output.push_str("> ");
                    output.push_str(line);
                    output.push('\n');
                }
            }
            output.push('\n');
        }
        if let Some(reasoning) = message
            .reasoning
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            output.push_str("<details>\n<summary>思考过程</summary>\n\n");
            output.push_str(reasoning.trim());
            output.push_str("\n\n</details>\n\n");
        }
        if let Some(error) = message
            .error_message
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            output.push_str(&format!("> 错误：{}\n\n", error.trim()));
        }
    }
    output
}

fn write_export_file(path: PathBuf, content: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建导出目录失败：{error}"))?;
    }
    fs::write(path, content).map_err(|error| format!("写入会话导出文件失败：{error}"))
}
