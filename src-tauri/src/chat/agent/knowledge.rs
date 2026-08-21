//! Read-only access to Mnemora's durable notes and literature library.

use std::path::Path;

use serde_json::{json, Value};

use crate::{
    ai::error::ModelError,
    library::{
        types::{LibraryListRequest, LibrarySort, LibraryView},
        LibraryRepository,
    },
};

use super::types::ToolExecution;

const MAX_RESULTS: usize = 50;
const MAX_NOTE_LINES: usize = 2_000;
const MAX_OUTPUT_BYTES: usize = 32_000;
const MAX_PDF_PAGES: usize = 12;
const MAX_PREVIEW_CHARS: usize = 2_000;

pub(super) fn knowledge_list(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let kind = arguments
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("all");
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
        .clamp(1, MAX_RESULTS as u64) as usize;
    let fetch_limit = cursor.saturating_add(limit).min(200).max(1);
    let mut entries = Vec::new();

    if matches!(kind, "all" | "note") {
        let (notes, _) = repository
            .list_notes_page_for_agent(query, 0, fetch_limit)
            .map_err(ModelError::invalid_configuration)?;
        entries.extend(notes.into_iter().map(|note| {
            json!({
                "kind": "note",
                "id": note.id,
                "title": note.title,
                "preview": note.content_preview,
                "contentChars": note.content_chars,
                "group": note.group_name,
                "relatedDocumentId": note.item_id,
                "updatedAt": note.updated_at,
                "reference": format!("[knowledge:note:{}]", note.id),
            })
        }));
    }
    if matches!(kind, "all" | "document") {
        let documents = repository
            .list_items(LibraryListRequest {
                view: LibraryView::All,
                search_query: query.to_string(),
                collection_id: None,
                sort: LibrarySort::Updated,
                offset: 0,
                limit: fetch_limit.min(500),
            })
            .map_err(ModelError::invalid_configuration)?;
        entries.extend(documents.items.into_iter().map(|item| {
            json!({
                "kind": "document",
                "id": item.id,
                "title": item.title,
                "authors": item.authors,
                "publicationYear": item.publication_year,
                "abstract": truncate_chars(&item.abstract_text, 600),
                "fileName": item.file.original_name,
                "updatedAt": item.updated_at,
                "reference": format!("[knowledge:document:{}]", item.id),
            })
        }));
    }
    entries.sort_by(|left, right| {
        right["updatedAt"]
            .as_u64()
            .cmp(&left["updatedAt"].as_u64())
            .then_with(|| left["title"].as_str().cmp(&right["title"].as_str()))
    });
    let total = entries.len();
    let page = entries
        .into_iter()
        .skip(cursor)
        .take(limit)
        .collect::<Vec<_>>();
    let next_cursor =
        (cursor.saturating_add(page.len()) < total).then_some(cursor.saturating_add(page.len()));
    execution(json!({
        "kind": kind,
        "query": query,
        "entries": page,
        "nextCursor": next_cursor,
        "windowTotal": total,
    }))
}

pub(super) fn knowledge_search(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let query = required_string(arguments, "query")?;
    let kind = arguments
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("all");
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, MAX_RESULTS as u64) as usize;
    let mut matches = Vec::new();

    if matches!(kind, "all" | "note") {
        let (notes, _) = repository
            .list_notes_page_for_agent(query, 0, limit)
            .map_err(ModelError::invalid_configuration)?;
        for summary in notes {
            let note = repository
                .get_note(&summary.id)
                .map_err(ModelError::invalid_configuration)?;
            matches.push(json!({
                "kind": "note",
                "id": note.id,
                "title": note.title,
                "snippet": matching_snippet(&note.content, query, 800),
                "reference": format!("[knowledge:note:{}]", note.id),
            }));
            if matches.len() >= limit {
                break;
            }
        }
    }
    if matches.len() < limit && matches!(kind, "all" | "document") {
        let documents = repository
            .list_items(LibraryListRequest {
                view: LibraryView::All,
                search_query: query.to_string(),
                collection_id: None,
                sort: LibrarySort::Updated,
                offset: 0,
                limit: limit.saturating_sub(matches.len()).max(1),
            })
            .map_err(ModelError::invalid_configuration)?;
        matches.extend(documents.items.into_iter().map(|item| {
            json!({
                "kind": "document",
                "id": item.id,
                "title": item.title,
                "authors": item.authors,
                "snippet": matching_snippet(&item.abstract_text, query, 800),
                "reference": format!("[knowledge:document:{}]", item.id),
            })
        }));
    }
    execution(json!({
        "query": query,
        "searchMode": "boundedLexical",
        "matches": matches,
        "status": if matches.is_empty() { "successNoResults" } else { "success" },
    }))
}

pub(super) fn knowledge_read(
    repository: &LibraryRepository,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let kind = required_string(arguments, "kind")?;
    let id = required_string(arguments, "id")?;
    match kind {
        "note" => read_note(repository, id, arguments),
        "document" => read_document(repository, id, arguments),
        _ => Err(ModelError::invalid_configuration(
            "knowledge_read.kind 必须是 note 或 document。",
        )),
    }
}

fn read_note(
    repository: &LibraryRepository,
    id: &str,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
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
    let selected = note
        .content
        .lines()
        .enumerate()
        .filter(|(index, _)| (*index + 1) >= start && (*index + 1) <= end)
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
        "kind": "note",
        "id": note.id,
        "title": note.title,
        "content": content,
        "startLine": start,
        "endLine": end,
        "reference": format!("[knowledge:note:{id}#L{start}-L{end}]"),
        "truncated": truncated,
    }))?;
    result.output_truncated = truncated;
    Ok(result)
}

fn read_document(
    repository: &LibraryRepository,
    id: &str,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let pages = arguments
        .get("pages")
        .and_then(Value::as_array)
        .ok_or_else(|| ModelError::invalid_configuration("读取文献必须提供 pages 数组。"))?;
    if pages.is_empty() || pages.len() > MAX_PDF_PAGES {
        return Err(ModelError::invalid_configuration(format!(
            "单次必须读取 1 到 {MAX_PDF_PAGES} 页文献。"
        )));
    }
    let path = repository
        .primary_file_path(id)
        .map_err(ModelError::invalid_configuration)?;
    read_pdf_pages(&path, id, pages)
}

fn read_pdf_pages(path: &Path, id: &str, pages: &[Value]) -> Result<ToolExecution, ModelError> {
    let mut page_numbers = pages
        .iter()
        .map(|value| {
            value
                .as_u64()
                .filter(|page| *page > 0 && *page <= u32::MAX as u64)
                .map(|page| page as u32)
                .ok_or_else(|| ModelError::invalid_configuration("文献页码必须是正整数。"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    page_numbers.sort_unstable();
    page_numbers.dedup();
    let document = lopdf::Document::load(path)
        .map_err(|error| ModelError::invalid_configuration(format!("PDF 解析失败：{error}")))?;
    let available = document.get_pages();
    let mut sections = Vec::new();
    for page in page_numbers {
        if !available.contains_key(&page) {
            return Err(ModelError::invalid_configuration(format!(
                "文献不包含第 {page} 页。"
            )));
        }
        let text = document.extract_text(&[page]).map_err(|error| {
            ModelError::invalid_configuration(format!("读取文献第 {page} 页失败：{error}"))
        })?;
        sections.push(json!({
            "page": page,
            "text": if text.trim().is_empty() {
                "[该页没有可提取的文本层，不能据此猜测页面内容。]"
            } else {
                text.trim()
            },
            "reference": format!("[knowledge:document:{id}#page={page}]"),
        }));
    }
    execution(json!({ "kind": "document", "id": id, "pages": sections }))
}

fn matching_snippet(content: &str, query: &str, limit: usize) -> String {
    if content.is_empty() {
        return String::new();
    }
    let lower = content.to_lowercase();
    let query = query.to_lowercase();
    let byte_start = lower.find(&query).unwrap_or(0);
    let mut start = byte_start.saturating_sub(limit / 4).min(content.len());
    while start > 0 && !content.is_char_boundary(start) {
        start -= 1;
    }
    truncate_chars(&content[start..], limit)
}

fn execution(value: Value) -> Result<ToolExecution, ModelError> {
    let content = serde_json::to_string(&value).map_err(|error| {
        ModelError::invalid_configuration(format!("序列化知识库结果失败：{error}"))
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
