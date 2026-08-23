//! 本地会话 JSON 的 Tauri 命令边界。
//!
//! 所有文件读写都放入阻塞线程；保存、删除和清空使用同一异步互斥锁串行化，避免索引竞争。

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::{AppHandle, State};

use crate::{
    ai::types::ModelRole,
    chat::note_pipeline,
    chat::{
        attachments::import_note_source_files,
        conversation_types::{
            AiPermissionMode, ConversationListItem, ConversationListPage, MessageStatus,
            StoredChatMessage, StoredConversation,
        },
    },
    library::types::{
        LibraryNote, LibraryNoteCreate, MAX_NOTE_CONTENT_CHARS, MAX_NOTE_TITLE_CHARS,
    },
    state::AppState,
};
use uuid::Uuid;

const DEFAULT_CONVERSATION_PAGE_SIZE: usize = 50;
const MAX_CONVERSATION_PAGE_SIZE: usize = 100;
const MAX_EXPORT_PATH_CHARS: usize = 32_768;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalNoteSourceResult {
    pub conversation_id: String,
    pub file_names: Vec<String>,
    pub attachment_count: usize,
}

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
) -> Result<String, String> {
    if path.trim().is_empty() || path.chars().count() > MAX_EXPORT_PATH_CHARS {
        return Err("导出路径无效。".to_string());
    }
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conversation = repository.load(&conversation_id)?;
        match format {
            ConversationExportFormat::Markdown => {
                let exported =
                    export_markdown_bundle(&repository, &conversation, PathBuf::from(path))?;
                Ok(exported.to_string_lossy().into_owned())
            }
            ConversationExportFormat::Json => {
                let path = PathBuf::from(path);
                let content = serde_json::to_string_pretty(&conversation)
                    .map_err(|error| format!("序列化会话失败：{error}"))?;
                write_export_file(path.clone(), content.as_bytes())?;
                Ok(path.to_string_lossy().into_owned())
            }
        }
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

/// 把本地文件复制到一个隐藏的来源会话，再交给既有深度笔记管线处理。
/// 每个文件独占一条完成态消息，避免单消息 10 附件上限限制批量导入，同时保留文件级来源边界。
#[tauri::command]
pub async fn prepare_local_note_source(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<LocalNoteSourceResult, String> {
    if paths.is_empty() || paths.len() > 100 {
        return Err("一次请选择 1 到 100 个本地文件。".to_string());
    }
    let conversation_id = Uuid::new_v4().to_string();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let repository = state.conversation_repository.clone();
    let _write_guard = state.conversation_writes.lock().await;
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let mut messages = Vec::with_capacity(paths.len());
            let mut file_names = Vec::with_capacity(paths.len());
            for path in paths {
                let attachments =
                    import_note_source_files(&repository, &conversation_id, vec![path])?;
                let Some(attachment) = attachments.into_iter().next() else {
                    return Err("本地文件没有生成有效附件。".to_string());
                };
                file_names.push(attachment.name.clone());
                messages.push(StoredChatMessage {
                    id: Uuid::new_v4().to_string(),
                    conversation_id: conversation_id.clone(),
                    role: ModelRole::User,
                    content: format!(
                        "本地文件来源：{}\n请把该文件作为主要证据，生成可追溯的深度笔记。",
                        attachment.name
                    ),
                    attachments: vec![attachment],
                    literature_references: Vec::new(),
                    note_references: Vec::new(),
                    reasoning: None,
                    status: MessageStatus::Completed,
                    created_at: now,
                    updated_at: now,
                    model_id: None,
                    model_snapshot: None,
                    usage: None,
                    activated_skills: Vec::new(),
                    tool_traces: Vec::new(),
                    agent_events: Some(Vec::new()),
                    agent_run_id: None,
                    workflow_summary: None,
                    error_message: None,
                });
            }
            let title = if file_names.len() == 1 {
                format!("本地文件：{}", file_names[0])
            } else {
                format!("本地文件笔记（{} 个文件）", file_names.len())
            };
            repository.save(&StoredConversation {
                id: conversation_id.clone(),
                title,
                messages,
                assistant_id: None,
                provider_id: None,
                model_id: None,
                thinking_enabled: None,
                reasoning_effort: None,
                system_prompt: String::new(),
                context_summary: String::new(),
                compressed_until_message_id: None,
                context_compression_count: 0,
                enabled_skill_ids: Vec::new(),
                linked_library_item_ids: Vec::new(),
                permission_mode: AiPermissionMode::AskSensitive,
                project_id: None,
                collection_id: None,
                source_kind: Some("localFiles".to_string()),
                pinned: false,
                created_at: now,
                updated_at: now,
            })?;
            Ok(LocalNoteSourceResult {
                conversation_id: conversation_id.clone(),
                attachment_count: file_names.len(),
                file_names,
            })
        })();
        if result.is_err() {
            let _ = repository.delete(&conversation_id);
        }
        result
    })
    .await
    .map_err(join_error)?
}

#[tauri::command]
pub async fn discard_local_note_source(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<bool, String> {
    let _write_guard = state.conversation_writes.lock().await;
    let repository = state.conversation_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conversation = repository.load(&conversation_id)?;
        if conversation.source_kind.as_deref() != Some("localFiles") {
            return Err("只能清理本地文件笔记的隐藏来源。".to_string());
        }
        repository.delete(&conversation_id)
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
    conversation_to_markdown_with_attachments(conversation, None)
}

fn conversation_to_markdown_with_attachments(
    conversation: &StoredConversation,
    exported_attachments: Option<&HashMap<String, String>>,
) -> String {
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
                if let Some(relative) =
                    exported_attachments.and_then(|attachments| attachments.get(&attachment.path))
                {
                    let label = escape_markdown_label(&attachment.name);
                    let destination = relative.replace('\\', "/");
                    let link = if attachment.kind == "image" {
                        format!("![{label}](<{destination}>)")
                    } else {
                        format!("[{label}](<{destination}>)")
                    };
                    output.push_str(&format!(
                        "- {link}（{}，{} bytes）\n",
                        attachment.mime_type, attachment.size_bytes
                    ));
                } else {
                    output.push_str(&format!(
                        "- {}（{}，{} bytes）\n",
                        attachment.name, attachment.mime_type, attachment.size_bytes
                    ));
                }
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

fn export_markdown_bundle(
    repository: &crate::chat::storage::ConversationRepository,
    conversation: &StoredConversation,
    parent: PathBuf,
) -> Result<PathBuf, String> {
    fs::create_dir_all(&parent).map_err(|error| format!("创建导出位置失败：{error}"))?;
    let metadata = fs::metadata(&parent).map_err(|error| format!("读取导出位置失败：{error}"))?;
    if !metadata.is_dir() {
        return Err("Markdown 会话包必须导出到一个目录中。".to_string());
    }

    let folder_name = sanitize_export_name(&conversation.title, "mnemora-conversation");
    let final_directory = unique_export_directory(&parent, &folder_name)?;
    let staging_directory = parent.join(format!(".mnemora-export-{}", Uuid::new_v4()));
    fs::create_dir(&staging_directory).map_err(|error| format!("创建导出临时目录失败：{error}"))?;

    let result = (|| {
        let attachment_directory = staging_directory.join("attachments");
        let mut exported = HashMap::new();
        let mut used_names = HashSet::new();
        let has_attachments = conversation
            .messages
            .iter()
            .any(|message| !message.attachments.is_empty());
        if has_attachments {
            fs::create_dir(&attachment_directory)
                .map_err(|error| format!("创建导出附件目录失败：{error}"))?;
        }

        for message in &conversation.messages {
            for attachment in &message.attachments {
                if exported.contains_key(&attachment.path) {
                    continue;
                }
                let source =
                    repository.resolve_attachment_path(&conversation.id, &attachment.path)?;
                let source_metadata = fs::metadata(&source)
                    .map_err(|error| format!("读取附件“{}”失败：{error}", attachment.name))?;
                if !source_metadata.is_file() || source_metadata.len() != attachment.size_bytes {
                    return Err(format!("附件“{}”缺失或大小与记录不一致。", attachment.name));
                }
                let safe_name = sanitize_export_name(&attachment.name, "attachment");
                let export_name = unique_export_file_name(&safe_name, &mut used_names);
                let destination = attachment_directory.join(&export_name);
                fs::copy(&source, &destination)
                    .map_err(|error| format!("复制附件“{}”失败：{error}", attachment.name))?;
                exported.insert(
                    attachment.path.clone(),
                    format!("attachments/{export_name}"),
                );
            }
        }

        let markdown = conversation_to_markdown_with_attachments(conversation, Some(&exported));
        let markdown_path = staging_directory.join(format!("{folder_name}.md"));
        fs::write(&markdown_path, markdown.as_bytes())
            .map_err(|error| format!("写入 Markdown 会话包失败：{error}"))?;
        fs::rename(&staging_directory, &final_directory)
            .map_err(|error| format!("完成 Markdown 会话包导出失败：{error}"))?;
        Ok(final_directory.clone())
    })();

    if result.is_err() && staging_directory.exists() {
        let _ = fs::remove_dir_all(&staging_directory);
    }
    result
}

fn sanitize_export_name(value: &str, fallback: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let cleaned = cleaned.trim().trim_matches(['.', ' ']);
    let bounded = cleaned.chars().take(120).collect::<String>();
    let bounded = bounded.trim_end_matches(['.', ' ']);
    if bounded.is_empty() {
        fallback.to_string()
    } else {
        avoid_windows_reserved_name(&bounded)
    }
}

fn avoid_windows_reserved_name(value: &str) -> String {
    let device = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || device
            .strip_prefix("COM")
            .or_else(|| device.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if !reserved {
        return value.to_string();
    }
    value
        .find('.')
        .map(|dot| format!("{}_{}", &value[..dot], &value[dot..]))
        .unwrap_or_else(|| format!("{value}_"))
}

fn unique_export_directory(parent: &Path, base_name: &str) -> Result<PathBuf, String> {
    for index in 0..1_000usize {
        let name = if index == 0 {
            base_name.to_string()
        } else {
            format!("{base_name} ({index})")
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("导出位置中同名会话包过多，请更换目录后重试。".to_string())
}

fn unique_export_file_name(base_name: &str, used: &mut HashSet<String>) -> String {
    let path = Path::new(base_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 0..10_000usize {
        let name = if index == 0 {
            base_name.to_string()
        } else if let Some(extension) = extension {
            format!("{stem} ({index}).{extension}")
        } else {
            format!("{stem} ({index})")
        };
        if used.insert(name.to_lowercase()) {
            return name;
        }
    }
    format!("attachment-{}", Uuid::new_v4())
}

fn escape_markdown_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
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
    use crate::chat::conversation_types::StoredChatAttachment;

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

    #[test]
    fn markdown_bundle_copies_attachments_and_uses_relative_links() {
        let root =
            std::env::temp_dir().join(format!("mnemora-conversation-export-{}", Uuid::new_v4()));
        let repository = crate::chat::storage::ConversationRepository::new(root.clone());
        let conversation_id = "conversation-1".to_string();
        let attachment_directory = repository.attachments_directory(&conversation_id).unwrap();
        fs::create_dir_all(&attachment_directory).unwrap();
        fs::write(attachment_directory.join("stored-one.txt"), b"first").unwrap();
        fs::write(attachment_directory.join("stored-two.txt"), b"second").unwrap();
        let attachments = vec![
            StoredChatAttachment {
                id: "attachment-1".to_string(),
                kind: "file".to_string(),
                name: "source file.txt".to_string(),
                mime_type: "text/plain".to_string(),
                size_bytes: 5,
                path: "stored-one.txt".to_string(),
                preview_path: None,
                width: None,
                height: None,
            },
            StoredChatAttachment {
                id: "attachment-2".to_string(),
                kind: "file".to_string(),
                name: "source file.txt".to_string(),
                mime_type: "text/plain".to_string(),
                size_bytes: 6,
                path: "stored-two.txt".to_string(),
                preview_path: None,
                width: None,
                height: None,
            },
        ];
        let conversation = StoredConversation {
            id: conversation_id,
            title: "附件导出测试".to_string(),
            messages: vec![StoredChatMessage {
                id: "message-1".to_string(),
                conversation_id: "conversation-1".to_string(),
                role: ModelRole::User,
                content: "请整理附件。".to_string(),
                attachments,
                literature_references: Vec::new(),
                note_references: Vec::new(),
                reasoning: None,
                status: MessageStatus::Completed,
                created_at: 1,
                updated_at: 1,
                model_id: None,
                model_snapshot: None,
                usage: None,
                activated_skills: Vec::new(),
                tool_traces: Vec::new(),
                agent_events: Some(Vec::new()),
                agent_run_id: None,
                workflow_summary: None,
                error_message: None,
            }],
            assistant_id: None,
            provider_id: None,
            model_id: None,
            thinking_enabled: None,
            reasoning_effort: None,
            system_prompt: String::new(),
            context_summary: String::new(),
            compressed_until_message_id: None,
            context_compression_count: 0,
            enabled_skill_ids: Vec::new(),
            linked_library_item_ids: Vec::new(),
            permission_mode: AiPermissionMode::AskSensitive,
            project_id: None,
            collection_id: None,
            source_kind: None,
            pinned: false,
            created_at: 1,
            updated_at: 1,
        };
        let export_parent = root.join("exports");
        let exported = export_markdown_bundle(&repository, &conversation, export_parent).unwrap();
        assert!(exported.join("附件导出测试.md").is_file());
        assert_eq!(
            fs::read(exported.join("attachments").join("source file.txt")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(exported.join("attachments").join("source file (1).txt")).unwrap(),
            b"second"
        );
        let markdown = fs::read_to_string(exported.join("附件导出测试.md")).unwrap();
        assert!(markdown.contains("[source file.txt](<attachments/source file.txt>)"));
        assert!(markdown.contains("[source file.txt](<attachments/source file (1).txt>)"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_names_avoid_windows_devices() {
        assert_eq!(sanitize_export_name("CON", "fallback"), "CON_");
        assert_eq!(sanitize_export_name("lpt1.txt", "fallback"), "lpt1_.txt");
        assert_eq!(sanitize_export_name("normal.txt", "fallback"), "normal.txt");
    }
}
