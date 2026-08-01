//! Notion 官方 API 适配器。
//!
//! 适配器只在手动同步期间存在，不建立后台连接。模型生成的 Markdown 会被转换为
//! 有上限的基础 Notion Block；更新页面时先追加新内容，再归档旧 Block，避免网络
//! 中断直接清空已有页面。

use super::{markdown::SyncDocument, types::NotionSettings};
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};

const NOTION_API_BASE: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";
const MAX_BLOCKS: usize = 500;
const MAX_BLOCKS_PER_REQUEST: usize = 100;
const MAX_RICH_TEXT_CHARS: usize = 2_000;
const MAX_ERROR_BODY_CHARS: usize = 2_000;

#[derive(Debug, Deserialize)]
struct CreatedPage {
    id: String,
}

#[derive(Debug, Deserialize)]
struct BlockChildrenPage {
    #[serde(default)]
    results: Vec<NotionBlock>,
    #[serde(default)]
    has_more: bool,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NotionBlock {
    id: String,
}

pub async fn sync_document(
    client: &Client,
    settings: &NotionSettings,
    token: &str,
    document: &SyncDocument,
    mapped_page_id: Option<&str>,
) -> Result<String, String> {
    let parent_page_id = settings.parent_page_id.trim();
    if parent_page_id.is_empty() {
        return Err("请先填写 Notion 父页面 ID。".to_string());
    }
    let blocks = markdown_to_blocks(&document.markdown)?;

    match mapped_page_id.filter(|value| !value.trim().is_empty()) {
        Some(page_id) => {
            update_page_title(client, token, page_id, &document.title).await?;
            replace_page_children(client, token, page_id, blocks).await?;
            Ok(page_id.to_string())
        }
        None => create_page(client, token, parent_page_id, &document.title, blocks).await,
    }
}

async fn create_page(
    client: &Client,
    token: &str,
    parent_page_id: &str,
    title: &str,
    blocks: Vec<Value>,
) -> Result<String, String> {
    let first_batch = blocks
        .iter()
        .take(MAX_BLOCKS_PER_REQUEST)
        .cloned()
        .collect::<Vec<_>>();
    let body = json!({
        "parent": { "type": "page_id", "page_id": parent_page_id },
        "properties": {
            "title": {
                "type": "title",
                "title": rich_text(title),
            }
        },
        "children": first_batch,
    });
    let page: CreatedPage = send_json(client, token, Method::POST, "/pages", Some(body)).await?;
    append_children(
        client,
        token,
        &page.id,
        &blocks[MAX_BLOCKS_PER_REQUEST.min(blocks.len())..],
    )
    .await?;
    Ok(page.id)
}

async fn update_page_title(
    client: &Client,
    token: &str,
    page_id: &str,
    title: &str,
) -> Result<(), String> {
    let body = json!({
        "properties": {
            "title": {
                "type": "title",
                "title": rich_text(title),
            }
        }
    });
    let _: Value = send_json(
        client,
        token,
        Method::PATCH,
        &format!("/pages/{page_id}"),
        Some(body),
    )
    .await?;
    Ok(())
}

async fn replace_page_children(
    client: &Client,
    token: &str,
    page_id: &str,
    blocks: Vec<Value>,
) -> Result<(), String> {
    let old_block_ids = list_child_block_ids(client, token, page_id).await?;
    append_children(client, token, page_id, &blocks).await?;

    for block_id in old_block_ids {
        let _: Value = send_json(
            client,
            token,
            Method::PATCH,
            &format!("/blocks/{block_id}"),
            Some(json!({ "archived": true })),
        )
        .await?;
    }
    Ok(())
}

async fn append_children(
    client: &Client,
    token: &str,
    page_id: &str,
    blocks: &[Value],
) -> Result<(), String> {
    for batch in blocks.chunks(MAX_BLOCKS_PER_REQUEST) {
        let _: Value = send_json(
            client,
            token,
            Method::PATCH,
            &format!("/blocks/{page_id}/children"),
            Some(json!({ "children": batch })),
        )
        .await?;
    }
    Ok(())
}

async fn list_child_block_ids(
    client: &Client,
    token: &str,
    page_id: &str,
) -> Result<Vec<String>, String> {
    let mut block_ids = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut path = format!("/blocks/{page_id}/children?page_size=100");
        if let Some(value) = cursor.as_deref() {
            path.push_str("&start_cursor=");
            path.push_str(value);
        }
        let page: BlockChildrenPage = send_json(client, token, Method::GET, &path, None).await?;
        block_ids.extend(page.results.into_iter().map(|block| block.id));
        if block_ids.len() > MAX_BLOCKS {
            return Err(format!(
                "Notion 页面已有内容超过 {MAX_BLOCKS} 个顶层区块，已停止替换以避免长时间请求。"
            ));
        }
        if !page.has_more {
            break;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Err("Notion 返回了无效的分页信息。".to_string());
        }
    }
    Ok(block_ids)
}

async fn send_json<T: for<'de> Deserialize<'de>>(
    client: &Client,
    token: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<T, String> {
    let mut request = client
        .request(method, format!("{NOTION_API_BASE}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Notion-Version", NOTION_VERSION)
        .header("Content-Type", "application/json");
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("连接 Notion 失败：{error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(notion_error(
            status,
            response.text().await.unwrap_or_default(),
            token,
        ));
    }
    response
        .json::<T>()
        .await
        .map_err(|error| format!("解析 Notion 响应失败：{error}"))
}

fn notion_error(status: StatusCode, body: String, token: &str) -> String {
    let detail = body
        .replace(token, "[REDACTED]")
        .chars()
        .filter(|character| !character.is_control() || *character == ' ')
        .take(MAX_ERROR_BODY_CHARS)
        .collect::<String>();
    if detail.is_empty() {
        format!("Notion 请求失败（HTTP {}）。", status.as_u16())
    } else {
        format!("Notion 请求失败（HTTP {}）：{detail}", status.as_u16())
    }
}

fn markdown_to_blocks(markdown: &str) -> Result<Vec<Value>, String> {
    let mut blocks = Vec::new();
    let mut paragraph = Vec::new();
    let mut code = Vec::new();
    let mut code_language = String::new();
    let mut in_code = false;

    for line in without_frontmatter(markdown).lines() {
        if line.starts_with("```") {
            if in_code {
                push_code_blocks(&mut blocks, &code.join("\n"), &code_language);
                code.clear();
                code_language.clear();
                in_code = false;
            } else {
                flush_paragraph(&mut blocks, &mut paragraph);
                code_language = line.trim_start_matches('`').trim().to_string();
                in_code = true;
            }
            continue;
        }
        if in_code {
            code.push(line);
            continue;
        }
        if line.trim().is_empty() {
            flush_paragraph(&mut blocks, &mut paragraph);
            continue;
        }
        if let Some((level, text)) = heading(line) {
            flush_paragraph(&mut blocks, &mut paragraph);
            push_text_blocks(&mut blocks, &format!("heading_{level}"), text.trim());
        } else if let Some(text) = line.strip_prefix("> ") {
            flush_paragraph(&mut blocks, &mut paragraph);
            push_text_blocks(&mut blocks, "quote", text);
        } else if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            flush_paragraph(&mut blocks, &mut paragraph);
            push_text_blocks(&mut blocks, "bulleted_list_item", text);
        } else {
            paragraph.push(line);
        }
        if blocks.len() > MAX_BLOCKS {
            return Err(format!("单篇笔记转换后超过 {MAX_BLOCKS} 个 Notion 区块。"));
        }
    }
    if in_code {
        push_code_blocks(&mut blocks, &code.join("\n"), &code_language);
    }
    flush_paragraph(&mut blocks, &mut paragraph);
    if blocks.len() > MAX_BLOCKS {
        return Err(format!("单篇笔记转换后超过 {MAX_BLOCKS} 个 Notion 区块。"));
    }
    if blocks.is_empty() {
        blocks.push(text_block("paragraph", ""));
    }
    Ok(blocks)
}

fn without_frontmatter(markdown: &str) -> &str {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return markdown;
    };
    rest.split_once("\n---\n")
        .map(|(_, content)| content.trim_start_matches('\n'))
        .unwrap_or(markdown)
}

fn flush_paragraph(blocks: &mut Vec<Value>, lines: &mut Vec<&str>) {
    if lines.is_empty() {
        return;
    }
    push_text_blocks(blocks, "paragraph", &lines.join("\n"));
    lines.clear();
}

fn push_text_blocks(blocks: &mut Vec<Value>, block_type: &str, text: &str) {
    for chunk in split_text(text, MAX_RICH_TEXT_CHARS) {
        blocks.push(text_block(block_type, chunk));
    }
}

fn push_code_blocks(blocks: &mut Vec<Value>, code: &str, language: &str) {
    let language = notion_code_language(language);
    for chunk in split_text(code, MAX_RICH_TEXT_CHARS) {
        blocks.push(json!({
            "object": "block",
            "type": "code",
            "code": {
                "rich_text": rich_text(chunk),
                "language": language,
            }
        }));
    }
}

fn text_block(block_type: &str, text: &str) -> Value {
    let mut block = serde_json::Map::new();
    block.insert("object".to_string(), json!("block"));
    block.insert("type".to_string(), json!(block_type));
    block.insert(
        block_type.to_string(),
        json!({ "rich_text": rich_text(text) }),
    );
    Value::Object(block)
}

fn rich_text(text: &str) -> Vec<Value> {
    vec![json!({ "type": "text", "text": { "content": text } })]
}

fn split_text(value: &str, max_chars: usize) -> Vec<&str> {
    if value.is_empty() {
        return vec![""];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut count = 0;
    for (index, character) in value.char_indices() {
        if count == max_chars {
            chunks.push(&value[start..index]);
            start = index;
            count = 0;
        }
        count += 1;
        if character == '\n' && count >= max_chars.saturating_sub(80) {
            let end = index + character.len_utf8();
            chunks.push(&value[start..end]);
            start = end;
            count = 0;
        }
    }
    if start < value.len() {
        chunks.push(&value[start..]);
    }
    chunks
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if hashes == 0 || hashes > 6 || line.as_bytes().get(hashes) != Some(&b' ') {
        return None;
    }
    Some((hashes.min(3) as u8, &line[hashes + 1..]))
}

fn notion_code_language(language: &str) -> &'static str {
    match language.to_ascii_lowercase().as_str() {
        "bash" | "sh" | "shell" => "shell",
        "c" => "c",
        "cpp" | "c++" => "c++",
        "csharp" | "cs" => "c#",
        "css" => "css",
        "go" => "go",
        "html" => "html",
        "java" => "java",
        "javascript" | "js" => "javascript",
        "json" => "json",
        "kotlin" => "kotlin",
        "markdown" | "md" => "markdown",
        "php" => "php",
        "python" | "py" => "python",
        "ruby" => "ruby",
        "rust" | "rs" => "rust",
        "sql" => "sql",
        "swift" => "swift",
        "typescript" | "ts" => "typescript",
        "xml" => "xml",
        "yaml" | "yml" => "yaml",
        _ => "plain text",
    }
}

#[cfg(test)]
mod tests {
    use super::{heading, markdown_to_blocks, split_text, without_frontmatter};

    #[test]
    fn converts_basic_markdown_with_bounded_blocks() {
        let blocks =
            markdown_to_blocks("# 标题\n\n正文\n\n- 条目\n\n```rust\nfn main() {}\n```").unwrap();
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0]["type"], "heading_1");
        assert_eq!(blocks[3]["type"], "code");
    }

    #[test]
    fn splits_unicode_by_characters() {
        assert_eq!(split_text("一二三四五", 2), vec!["一二", "三四", "五"]);
    }

    #[test]
    fn limits_heading_levels_to_notion_supported_levels() {
        assert_eq!(heading("#### 标题"), Some((3, "标题")));
    }

    #[test]
    fn hides_obsidian_frontmatter_from_notion_body() {
        assert_eq!(
            without_frontmatter("---\ntitle: test\n---\n\n# 正文"),
            "# 正文"
        );
    }
}
