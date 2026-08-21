//! Bounded note tools backed by Mnemora's existing durable library store.

use serde_json::{json, Value};

use crate::{
    ai::error::ModelError,
    library::{
        types::{LibraryNoteCreate, LibraryNoteUpdate},
        LibraryRepository,
    },
};

use super::types::ToolExecution;

const MAX_OUTPUT_BYTES: usize = 32_000;
const MAX_NOTE_LINES: usize = 2_000;
const MAX_PREVIEW_CHARS: usize = 2_000;

pub(super) fn note_list(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let cursor = arguments.get("cursor").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(1, 100) as usize;
    let (notes, total) = repository
        .list_notes_page_for_agent(query, cursor, limit)
        .map_err(ModelError::invalid_configuration)?;
    let entries = notes
        .into_iter()
        .map(|note| {
            json!({
                "id": note.id,
                "title": note.title,
                "preview": note.content_preview,
                "contentChars": note.content_chars,
                "groupName": note.group_name,
                "updatedAt": note.updated_at,
                "reference": format!("[note:{}]", note.id),
            })
        })
        .collect::<Vec<_>>();
    let next_cursor = (cursor.saturating_add(entries.len()) < total)
        .then_some(cursor.saturating_add(entries.len()));
    execution(json!({
        "query": query,
        "entries": entries,
        "cursor": cursor,
        "nextCursor": next_cursor,
        "total": total,
    }))
}

pub(super) fn note_read(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let id = required_string(arguments, "id")?;
    let note = repository
        .get_note(id)
        .map_err(ModelError::invalid_configuration)?;
    let start = arguments
        .get("startLine")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let end = arguments
        .get("endLine")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| start.saturating_add(399) as u64)
        .max(start as u64) as usize;
    if end.saturating_sub(start) >= MAX_NOTE_LINES {
        return Err(ModelError::invalid_configuration(format!(
            "单次最多读取 {MAX_NOTE_LINES} 行笔记。"
        )));
    }
    let total_lines = note.content.lines().count();
    if total_lines > 0 && start > total_lines {
        return Err(ModelError::invalid_configuration(format!(
            "笔记只有 {total_lines} 行，起始行 {start} 超出范围。"
        )));
    }
    let actual_end = end.min(total_lines);
    let selected = note
        .content
        .lines()
        .enumerate()
        .filter(|(index, _)| (*index + 1) >= start && (*index + 1) <= actual_end)
        .map(|(index, line)| format!("{:>6}: {line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let max_bytes = arguments
        .get("maxBytes")
        .and_then(Value::as_u64)
        .unwrap_or(12_000)
        .clamp(1, MAX_OUTPUT_BYTES as u64) as usize;
    let (content, truncated) = truncate_utf8_bytes(&selected, max_bytes);
    let mut result = execution(json!({
        "id": note.id,
        "title": note.title,
        "content": content,
        "startLine": start,
        "endLine": actual_end,
        "totalLines": total_lines,
        "truncated": truncated,
        "hasMoreLines": actual_end < total_lines,
        "reference": format!("[note:{id}#L{start}-L{actual_end}]"),
    }))?;
    result.output_truncated = truncated;
    Ok(result)
}

pub(super) fn note_create(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let title = required_string(arguments, "title")?.to_string();
    let content = required_string(arguments, "content")?.to_string();
    let group_name = arguments
        .get("groupName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let note = repository
        .create_note(LibraryNoteCreate {
            item_id: None,
            title,
            content,
            group_name,
        })
        .map_err(ModelError::invalid_configuration)?;
    execution(json!({
        "status": "created",
        "id": note.id,
        "title": note.title,
        "groupName": note.group_name,
        "updatedAt": note.updated_at,
        "reference": format!("[note:{}]", note.id),
    }))
}

pub(super) fn note_update(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let id = required_string(arguments, "id")?.to_string();
    let title = required_string(arguments, "title")?.to_string();
    let content = required_string(arguments, "content")?.to_string();
    let note = repository
        .update_note(LibraryNoteUpdate {
            note_id: id,
            title,
            content,
        })
        .map_err(ModelError::invalid_configuration)?;
    execution(json!({
        "status": "updated",
        "id": note.id,
        "title": note.title,
        "updatedAt": note.updated_at,
        "reference": format!("[note:{}]", note.id),
    }))
}

fn execution(value: Value) -> Result<ToolExecution, ModelError> {
    let content = serde_json::to_string(&value).map_err(|error| {
        ModelError::invalid_configuration(format!("序列化笔记工具结果失败：{error}"))
    })?;
    Ok(ToolExecution {
        preview: truncate_chars(&content, MAX_PREVIEW_CHARS),
        output_chars: content.chars().count(),
        content,
        is_error: false,
        activated_skill_id: None,
        output_truncated: false,
    })
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, ModelError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ModelError::invalid_configuration(format!("缺少工具参数 {key}。")))
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let head = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn truncate_utf8_bytes(value: &str, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value.to_string(), false);
    }
    let mut end = limit.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use uuid::Uuid;

    use super::{note_create, note_list, note_read, note_update};
    use crate::library::LibraryRepository;

    #[test]
    fn creates_lists_reads_and_updates_notes() {
        let root = std::env::temp_dir().join(format!("mnemora-agent-notes-{}", Uuid::new_v4()));
        let repository = LibraryRepository::new(root.clone());

        let created = note_create(
            &repository,
            &json!({ "title": "测试笔记", "content": "第一行\n第二行", "groupName": "Agent" }),
        )
        .unwrap();
        let created_json: serde_json::Value = serde_json::from_str(&created.content).unwrap();
        let id = created_json["id"].as_str().unwrap();

        let listed = note_list(&repository, &json!({ "query": "测试", "limit": 10 })).unwrap();
        assert!(listed.content.contains(id));

        let read = note_read(
            &repository,
            &json!({ "id": id, "startLine": 2, "endLine": 2 }),
        )
        .unwrap();
        assert!(read.content.contains("第二行"));

        let updated = note_update(
            &repository,
            &json!({ "id": id, "title": "更新标题", "content": "更新正文" }),
        )
        .unwrap();
        assert!(updated.content.contains("更新标题"));
        assert_eq!(repository.get_note(id).unwrap().content, "更新正文");

        let _ = fs::remove_dir_all(root);
    }
}
