//! 本地会话 JSON 的 Tauri 命令边界。
//!
//! 所有文件读写都放入阻塞线程；保存、删除和清空使用同一异步互斥锁串行化，避免索引竞争。

use std::{fs, path::PathBuf};

use tauri::{AppHandle, State};

use crate::{
    chat::conversation_types::{ConversationListItem, ConversationListPage, StoredConversation},
    chat::note_pipeline,
    library::types::{
        LibraryNote, LibraryNoteCreate, MAX_NOTE_CONTENT_CHARS, MAX_NOTE_TITLE_CHARS,
    },
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
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<bool, String> {
    // 删除来源会话前先遗弃其未完成深度笔记，避免后台任务继续读取已不存在的文件并反复恢复。
    let _ = note_pipeline::abandon_for_conversation(&app, &conversation_id).await?;
    let _write_guard = state.conversation_writes.lock().await;
    let _library_guard = state.library_operations.lock().await;
    let conversations = state.conversation_repository.clone();
    let library = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let removed = conversations.delete(&conversation_id)?;
        if removed {
            library.detach_note_sources_for_conversation(&conversation_id)?;
        }
        Ok(removed)
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub async fn clear_conversations(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    for conversation in state.conversation_repository.list()? {
        let _ = note_pipeline::abandon_for_conversation(&app, &conversation.id).await?;
    }
    let _write_guard = state.conversation_writes.lock().await;
    let _library_guard = state.library_operations.lock().await;
    let conversations = state.conversation_repository.clone();
    let library = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        conversations.clear()?;
        library.detach_all_note_conversation_sources()?;
        Ok(())
    })
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

/// 把整个对话转存为文献库笔记（`item_id` 为空的独立笔记）。
///
/// 复用导出 Markdown 的渲染逻辑，保证两条路径格式一致；对话可能未在前端加载，
/// 因此由 Rust 直接按 ID 读取，避免把完整消息在 IPC 上来回传输。
#[tauri::command]
pub async fn save_conversation_as_note(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<LibraryNote, String> {
    let _write_guard = state.library_operations.lock().await;
    let conversations = state.conversation_repository.clone();
    let library = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conversation = conversations.load(&conversation_id)?;
        let markdown = conversation_to_markdown(&conversation);
        library.create_note(LibraryNoteCreate {
            item_id: None,
            title: note_title_from_conversation(&conversation.title),
            content: clamp_note_content(markdown),
            group_name: None,
        })
    })
    .await
    .map_err(join_error)?
}

/// 笔记标题校验拒绝空值、控制字符和超长文本，这里统一净化对话标题。
fn note_title_from_conversation(title: &str) -> String {
    let cleaned = title
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return "未命名对话".to_string();
    }
    cleaned.chars().take(MAX_NOTE_TITLE_CHARS).collect()
}

/// 笔记正文超过上限会被存储层拒绝；超长对话按字符截断并注明。
fn clamp_note_content(content: String) -> String {
    const TRUNCATION_NOTICE: &str =
        "\n\n> 注：对话过长，笔记内容已截断。完整内容请使用「导出 Markdown」。";
    if content.chars().count() <= MAX_NOTE_CONTENT_CHARS {
        return content;
    }
    let keep = MAX_NOTE_CONTENT_CHARS.saturating_sub(TRUNCATION_NOTICE.chars().count());
    let mut truncated = content.chars().take(keep).collect::<String>();
    truncated.push_str(TRUNCATION_NOTICE);
    truncated
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
        if !message.note_references.is_empty() {
            output.push_str("笔记引用：\n");
            for reference in &message.note_references {
                output.push_str(&format!("- {}\n", reference.note_title));
                for line in reference.selected_text.lines() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_title_falls_back_and_strips_control_characters() {
        assert_eq!(note_title_from_conversation("   "), "未命名对话");
        assert_eq!(
            note_title_from_conversation("slain 的\t用法\n"),
            "slain 的 用法"
        );
        let long_title = "题".repeat(MAX_NOTE_TITLE_CHARS + 20);
        assert_eq!(
            note_title_from_conversation(&long_title).chars().count(),
            MAX_NOTE_TITLE_CHARS
        );
    }

    #[test]
    fn clamp_note_content_only_truncates_oversized_markdown() {
        let short = "# 标题\n\n正文".to_string();
        assert_eq!(clamp_note_content(short.clone()), short);

        let oversized = "长".repeat(MAX_NOTE_CONTENT_CHARS + 100);
        let clamped = clamp_note_content(oversized);
        assert!(clamped.chars().count() <= MAX_NOTE_CONTENT_CHARS);
        assert!(clamped.ends_with("完整内容请使用「导出 Markdown」。"));
    }
}
