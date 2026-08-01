//! 飞书新版文档按需同步适配器。
//!
//! 每次手动同步只获取一次 tenant_access_token，按顺序写入文档，任务结束后立即
//! 丢弃令牌和请求状态。这里不启动刷新任务、长连接、定时器或后台文件监听。
use std::time::Duration;

use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::time::sleep;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{markdown::SyncDocument, types::FeishuSettings};

const FEISHU_API_BASE: &str = "https://open.feishu.cn/open-apis";
const MAX_BLOCKS: usize = 500;
const MAX_BLOCKS_PER_REQUEST: usize = 50;
const MAX_TEXT_CHARS: usize = 20_000;
const MAX_ERROR_BODY_CHARS: usize = 2_000;
const MAX_RETRY_ATTEMPTS: usize = 3;
const EDIT_REQUEST_INTERVAL: Duration = Duration::from_millis(380);

#[derive(Debug, Deserialize)]
struct FeishuResponse<T> {
    code: i64,
    #[serde(default)]
    msg: String,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct TenantTokenResponse {
    code: i64,
    #[serde(default)]
    msg: String,
    tenant_access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateDocumentData {
    document: CreatedDocument,
}

#[derive(Debug, Deserialize)]
struct CreatedDocument {
    document_id: String,
}

#[derive(Debug, Deserialize)]
struct DocumentInfoData {
    document: DocumentInfo,
}

#[derive(Debug, Deserialize)]
struct DocumentInfo {
    #[allow(dead_code)]
    document_id: String,
}

/// 仅在一次手动同步期间存活的飞书会话。
pub struct FeishuSession<'a> {
    client: &'a Client,
    token: Zeroizing<String>,
}

impl<'a> FeishuSession<'a> {
    pub async fn connect(
        client: &'a Client,
        settings: &FeishuSettings,
        app_secret: &str,
    ) -> Result<Self, String> {
        let response = client
            .post(format!(
                "{FEISHU_API_BASE}/auth/v3/tenant_access_token/internal"
            ))
            .json(&json!({
                "app_id": settings.app_id,
                "app_secret": app_secret,
            }))
            .send()
            .await
            .map_err(|error| format!("连接飞书开放平台失败：{error}"))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(feishu_http_error(status, &body));
        }
        let payload: TenantTokenResponse = serde_json::from_str(&body)
            .map_err(|error| format!("解析飞书身份验证响应失败：{error}"))?;
        if payload.code != 0 {
            return Err(feishu_api_error(payload.code, &payload.msg));
        }
        let token = payload
            .tenant_access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "飞书未返回 tenant_access_token。".to_string())?;
        Ok(Self {
            client,
            token: Zeroizing::new(token),
        })
    }

    pub async fn sync_document(
        &self,
        settings: &FeishuSettings,
        document: &SyncDocument,
        mapped_document_id: Option<&str>,
    ) -> Result<String, String> {
        let blocks = markdown_to_blocks(&document.markdown)?;
        let document_id = match mapped_document_id.filter(|value| !value.trim().is_empty()) {
            Some(document_id) => {
                self.ensure_document_exists(document_id).await?;
                self.replace_children(document_id, &blocks).await?;
                document_id.to_string()
            }
            None => {
                let document_id = self.create_document(settings, &document.title).await?;
                self.append_children(&document_id, &blocks).await?;
                document_id
            }
        };
        Ok(document_id)
    }

    async fn create_document(
        &self,
        settings: &FeishuSettings,
        title: &str,
    ) -> Result<String, String> {
        let mut body = json!({ "title": bounded_title(title) });
        if !settings.folder_token.is_empty() {
            body["folder_token"] = json!(settings.folder_token);
        }
        let data: CreateDocumentData = self
            .send_json(Method::POST, "/docx/v1/documents", Some(body), false)
            .await?;
        if data.document.document_id.trim().is_empty() {
            return Err("飞书创建文档后未返回 document_id。".to_string());
        }
        Ok(data.document.document_id)
    }

    async fn ensure_document_exists(&self, document_id: &str) -> Result<(), String> {
        let data: DocumentInfoData = self
            .send_json(
                Method::GET,
                &format!("/docx/v1/documents/{document_id}"),
                None,
                false,
            )
            .await?;
        if data.document.document_id.trim().is_empty() {
            return Err("飞书文档不可用。".to_string());
        }
        Ok(())
    }

    async fn replace_children(&self, document_id: &str, blocks: &[Value]) -> Result<(), String> {
        let old_count = self.top_level_child_count(document_id).await?;
        if !blocks.is_empty() {
            // 先追加新内容，避免网络中断时把已有文档清空。
            self.append_children(document_id, blocks).await?;
        }
        if old_count > 0 {
            self.send_json::<Value>(
                Method::DELETE,
                &format!(
                    "/docx/v1/documents/{document_id}/blocks/{document_id}/children/batch_delete?document_revision_id=-1&client_token={}",
                    Uuid::new_v4()
                ),
                Some(json!({ "start_index": 0, "end_index": old_count })),
                true,
            )
            .await?;
        }
        Ok(())
    }

    async fn top_level_child_count(&self, document_id: &str) -> Result<usize, String> {
        let data: Value = self
            .send_json(
                Method::GET,
                &format!(
                    "/docx/v1/documents/{document_id}/blocks/{document_id}/children?document_revision_id=-1&page_size=500&with_descendants=false"
                ),
                None,
                false,
            )
            .await?;
        let count = data
            .get("items")
            .or_else(|| data.get("children"))
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err("飞书文档顶层块超过单次读取上限，已停止替换以避免误删。".to_string());
        }
        if count > MAX_BLOCKS * 2 {
            return Err("飞书文档顶层块过多，已停止替换以避免长时间请求。".to_string());
        }
        Ok(count)
    }

    async fn append_children(&self, document_id: &str, blocks: &[Value]) -> Result<(), String> {
        for batch in blocks.chunks(MAX_BLOCKS_PER_REQUEST) {
            self.send_json::<Value>(
                Method::POST,
                &format!(
                    "/docx/v1/documents/{document_id}/blocks/{document_id}/children?document_revision_id=-1&client_token={}",
                    Uuid::new_v4()
                ),
                Some(json!({ "index": -1, "children": batch })),
                true,
            )
            .await?;
        }
        Ok(())
    }

    async fn send_json<T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        is_edit: bool,
    ) -> Result<T, String> {
        for attempt in 0..MAX_RETRY_ATTEMPTS {
            let mut request = self
                .client
                .request(method.clone(), format!("{FEISHU_API_BASE}{path}"))
                .bearer_auth(self.token.as_str())
                .header("Content-Type", "application/json; charset=utf-8");
            if let Some(body) = body.as_ref() {
                request = request.json(body);
            }
            let response = request
                .send()
                .await
                .map_err(|error| format!("请求飞书开放平台失败：{error}"))?;
            let status = response.status();
            let raw = response.text().await.unwrap_or_default();
            let payload = serde_json::from_str::<FeishuResponse<T>>(&raw);
            let retryable = status == StatusCode::TOO_MANY_REQUESTS
                || status.is_server_error()
                || payload
                    .as_ref()
                    .is_ok_and(|value| matches!(value.code, 99991400 | 99991401 | 99991402));
            if retryable && attempt + 1 < MAX_RETRY_ATTEMPTS {
                sleep(Duration::from_millis(500 * (1 << attempt))).await;
                continue;
            }
            if !status.is_success() {
                return Err(feishu_http_error(status, &raw));
            }
            let payload = payload.map_err(|error| format!("解析飞书响应失败：{error}"))?;
            if payload.code != 0 {
                return Err(feishu_api_error(payload.code, &payload.msg));
            }
            if is_edit {
                sleep(EDIT_REQUEST_INTERVAL).await;
            }
            return payload
                .data
                .ok_or_else(|| "飞书响应缺少 data 字段。".to_string());
        }
        Err("飞书请求重试次数已耗尽。".to_string())
    }
}

fn markdown_to_blocks(markdown: &str) -> Result<Vec<Value>, String> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut code: Vec<&str> = Vec::new();
    let mut code_language = String::new();
    let mut in_code = false;

    for line in without_frontmatter(markdown).lines() {
        if line.starts_with("```") {
            if in_code {
                push_text_blocks(
                    &mut blocks,
                    14,
                    "code",
                    &code.join("\n"),
                    code_style(&code_language),
                );
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
            let block_type = 2 + level.min(3) as u64;
            let field = format!("heading{}", level.min(3));
            push_text_blocks(&mut blocks, block_type, &field, text.trim(), None);
        } else if let Some(text) = line.strip_prefix("> ") {
            flush_paragraph(&mut blocks, &mut paragraph);
            push_text_blocks(&mut blocks, 15, "quote", text, None);
        } else if let Some(text) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            flush_paragraph(&mut blocks, &mut paragraph);
            push_text_blocks(&mut blocks, 12, "bullet", text, None);
        } else {
            paragraph.push(line);
        }
        if blocks.len() > MAX_BLOCKS {
            return Err(format!("单篇笔记转换后超过 {MAX_BLOCKS} 个飞书文档块。"));
        }
    }
    if in_code {
        push_text_blocks(
            &mut blocks,
            14,
            "code",
            &code.join("\n"),
            code_style(&code_language),
        );
    }
    flush_paragraph(&mut blocks, &mut paragraph);
    if blocks.is_empty() {
        push_text_blocks(&mut blocks, 2, "text", "", None);
    }
    if blocks.len() > MAX_BLOCKS {
        return Err(format!("单篇笔记转换后超过 {MAX_BLOCKS} 个飞书文档块。"));
    }
    Ok(blocks)
}

fn flush_paragraph(blocks: &mut Vec<Value>, lines: &mut Vec<&str>) {
    if lines.is_empty() {
        return;
    }
    push_text_blocks(blocks, 2, "text", &lines.join("\n"), None);
    lines.clear();
}

fn push_text_blocks(
    blocks: &mut Vec<Value>,
    block_type: u64,
    field: &str,
    text: &str,
    style: Option<Value>,
) {
    for chunk in split_text(text, MAX_TEXT_CHARS) {
        let mut content = json!({
            "elements": [{ "text_run": { "content": chunk } }],
            "style": {},
        });
        if let Some(style) = style.clone() {
            content["style"] = style;
        }
        blocks.push(json!({
            "block_type": block_type,
            (field): content,
        }));
    }
}

fn without_frontmatter(markdown: &str) -> &str {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return markdown;
    };
    rest.split_once("\n---\n")
        .map(|(_, content)| content.trim_start_matches('\n'))
        .unwrap_or(markdown)
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if hashes == 0 || hashes > 6 || line.as_bytes().get(hashes) != Some(&b' ') {
        return None;
    }
    Some((hashes as u8, &line[hashes + 1..]))
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

fn code_style(language: &str) -> Option<Value> {
    Some(json!({
        "language": feishu_code_language(language),
        "wrap": true,
    }))
}

fn feishu_code_language(language: &str) -> u64 {
    match language.to_ascii_lowercase().as_str() {
        "bash" | "sh" => 7,
        "c" => 10,
        "cpp" | "c++" => 9,
        "css" => 12,
        "go" => 22,
        "html" => 24,
        "java" => 29,
        "javascript" | "js" => 30,
        "json" => 28,
        "markdown" | "md" => 39,
        "python" | "py" => 49,
        "rust" | "rs" => 53,
        "sql" => 56,
        "typescript" | "ts" => 63,
        "xml" => 66,
        "yaml" | "yml" => 67,
        _ => 1,
    }
}

fn bounded_title(value: &str) -> String {
    let title = value.trim().chars().take(800).collect::<String>();
    if title.is_empty() {
        "Mnemora 笔记".to_string()
    } else {
        title
    }
}

fn feishu_http_error(status: StatusCode, body: &str) -> String {
    let detail = sanitize_error(body);
    if detail.is_empty() {
        format!("飞书请求失败（HTTP {}）。", status.as_u16())
    } else {
        format!("飞书请求失败（HTTP {}）：{detail}", status.as_u16())
    }
}

fn feishu_api_error(code: i64, message: &str) -> String {
    let detail = sanitize_error(message);
    if detail.is_empty() {
        format!("飞书请求失败（错误码 {code}）。")
    } else {
        format!("飞书请求失败（错误码 {code}）：{detail}")
    }
}

fn sanitize_error(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || *character == ' ')
        .take(MAX_ERROR_BODY_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{bounded_title, feishu_code_language, markdown_to_blocks, split_text};

    #[test]
    fn converts_markdown_to_bounded_feishu_blocks() {
        let blocks = markdown_to_blocks(
            "---\ntitle: test\n---\n\n# 标题\n\n正文\n\n- 条目\n\n```rust\nfn main() {}\n```",
        )
        .unwrap();
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0]["block_type"], 3);
        assert_eq!(blocks[3]["block_type"], 14);
        assert_eq!(blocks[3]["code"]["style"]["language"], 53);
    }

    #[test]
    fn handles_unicode_and_safe_defaults() {
        assert_eq!(split_text("一二三四五", 2), vec!["一二", "三四", "五"]);
        assert_eq!(bounded_title("  "), "Mnemora 笔记");
        assert_eq!(feishu_code_language("unknown"), 1);
    }
}
