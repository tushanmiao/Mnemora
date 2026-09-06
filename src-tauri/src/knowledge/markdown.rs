//! 轻量、可复现的 Markdown 结构读取器。
//!
//! 这里不把 Markdown 渲染成 HTML，而是保留检索需要的结构：标题路径、块类型、
//! UTF-8 byte/line 坐标、代码/表格/公式边界以及图片引用。这样索引不会依赖前端
//! renderer，也不会把图片二进制误写进 FTS。

use std::path::{Component, Path};

use sha2::{Digest, Sha256};

pub const MARKDOWN_PARSER_ID: &str = "mnemora.markdown";
pub const MARKDOWN_PARSER_VERSION: &str = "1";
pub const MARKDOWN_NORMALIZATION_VERSION: &str = "lf-bom-v1";
pub const MARKDOWN_CHUNK_POLICY_VERSION: &str = "structure-v1";

#[derive(Debug, Clone)]
pub(crate) struct MarkdownDocument {
    pub canonical: String,
    pub source_hash: String,
    pub canonical_hash: String,
    pub line_count: usize,
    pub blocks: Vec<MarkdownBlock>,
}

#[derive(Debug, Clone)]
pub(crate) struct MarkdownBlock {
    pub kind: String,
    pub element_type: String,
    pub text: String,
    pub search_text: String,
    pub line_start: usize,
    pub line_end: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub char_start: usize,
    pub char_end: usize,
    pub heading_path: Vec<String>,
    pub image_refs: Vec<MarkdownImageRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkdownImageRef {
    pub source: String,
    pub alt: String,
    pub title: String,
}

#[derive(Debug, Clone)]
struct LineSpan {
    number: usize,
    start: usize,
    end_with_newline: usize,
    text: String,
}

pub(crate) fn parse_markdown(content: &str) -> MarkdownDocument {
    let canonical = normalize_newlines_and_bom(content);
    let source_hash = sha256_hex(content.as_bytes());
    let canonical_hash = sha256_hex(canonical.as_bytes());
    let lines = line_spans(&canonical);
    let blocks = parse_blocks(&canonical, &lines);
    MarkdownDocument {
        canonical,
        source_hash,
        canonical_hash,
        line_count: lines.len(),
        blocks,
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn normalize_newlines_and_bom(content: &str) -> String {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut normalized = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

fn line_spans(content: &str) -> Vec<LineSpan> {
    if content.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (offset, piece) in content.match_indices('\n') {
        let end_with_newline = offset + 1;
        let end = offset;
        lines.push(LineSpan {
            number: lines.len() + 1,
            start,
            end_with_newline,
            text: content[start..end].to_string(),
        });
        start = end_with_newline;
        // `piece` is intentionally consumed by match_indices; keeping this
        // assertion makes the byte-coordinate contract obvious without a
        // second scan.
        debug_assert_eq!(piece, "\n");
    }
    if start < content.len() {
        lines.push(LineSpan {
            number: lines.len() + 1,
            start,
            end_with_newline: content.len(),
            text: content[start..].to_string(),
        });
    }
    lines
}

fn parse_blocks(content: &str, lines: &[LineSpan]) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut index = 0usize;
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut reference_images = std::collections::HashMap::<String, (String, String)>::new();

    while index < lines.len() {
        let line = &lines[index];
        if line.text.trim().is_empty() {
            index += 1;
            continue;
        }

        if let Some((key, source, title)) = parse_image_definition(&line.text) {
            reference_images.insert(key, (source, title));
            let mut block = make_block(
                content,
                lines,
                index,
                index,
                "definition",
                "reference",
                &heading_stack,
            );
            block.search_text.clear();
            blocks.push(block);
            index += 1;
            continue;
        }

        if index == 0 && line.text.trim() == "---" {
            if let Some(end) = find_front_matter_end(lines, index) {
                blocks.push(make_block(
                    content,
                    lines,
                    index,
                    end,
                    "front_matter",
                    "text",
                    &heading_stack,
                ));
                index = end + 1;
                continue;
            }
        }

        if let Some((level, title)) = parse_heading(&line.text) {
            while heading_stack
                .last()
                .is_some_and(|(previous_level, _)| *previous_level >= level)
            {
                heading_stack.pop();
            }
            heading_stack.push((level, title));
            blocks.push(make_block(
                content,
                lines,
                index,
                index,
                "heading",
                "title",
                &heading_stack,
            ));
            index += 1;
            continue;
        }

        if let Some(fence) = parse_fence_start(&line.text) {
            let end = find_fence_end(lines, index, &fence);
            blocks.push(make_block(
                content,
                lines,
                index,
                end,
                "code",
                "code",
                &heading_stack,
            ));
            index = end + 1;
            continue;
        }

        if is_math_start(&line.text) {
            let end = find_math_end(lines, index);
            blocks.push(make_block(
                content,
                lines,
                index,
                end,
                "formula",
                "formula",
                &heading_stack,
            ));
            index = end + 1;
            continue;
        }

        if is_table_start(lines, index) {
            let mut end = index + 1;
            while end < lines.len() && is_table_row(&lines[end].text) {
                end += 1;
            }
            blocks.push(make_block(
                content,
                lines,
                index,
                end.saturating_sub(1),
                "table",
                "table",
                &heading_stack,
            ));
            index = end;
            continue;
        }

        if is_list_or_quote(&line.text) {
            let mut end = index + 1;
            while end < lines.len()
                && (!lines[end].text.trim().is_empty())
                && (is_list_or_quote(&lines[end].text)
                    || is_indented_continuation(&lines[end].text))
            {
                end += 1;
            }
            blocks.push(make_block(
                content,
                lines,
                index,
                end.saturating_sub(1),
                if line.text.trim_start().starts_with('>') {
                    "quote"
                } else {
                    "list"
                },
                if line.text.trim_start().starts_with('>') {
                    "quote"
                } else {
                    "list"
                },
                &heading_stack,
            ));
            index = end;
            continue;
        }

        let mut end = index + 1;
        while end < lines.len() {
            let next = &lines[end].text;
            if next.trim().is_empty()
                || parse_heading(next).is_some()
                || parse_fence_start(next).is_some()
                || is_math_start(next)
                || is_table_start(lines, end)
                || is_list_or_quote(next)
            {
                break;
            }
            end += 1;
        }
        let mut block = make_block(
            content,
            lines,
            index,
            end.saturating_sub(1),
            "paragraph",
            "paragraph",
            &heading_stack,
        );
        // Resolve reference-style images after definitions have been scanned
        // enough to be useful. Definitions appearing later are resolved by a
        // second pass below.
        block.image_refs = collect_image_refs(&block.text, &reference_images);
        block.search_text = search_projection(&block.text, &block.image_refs);
        blocks.push(block);
        index = end;
    }

    // Reference definitions can occur after their use. Resolve them without
    // changing the original byte/line coordinates.
    let definitions = collect_image_definitions(content);
    for block in &mut blocks {
        block.image_refs = collect_image_refs(&block.text, &definitions);
        block.search_text = search_projection(&block.text, &block.image_refs);
    }
    blocks
}

fn make_block(
    content: &str,
    lines: &[LineSpan],
    start_index: usize,
    end_index: usize,
    kind: &str,
    element_type: &str,
    heading_stack: &[(usize, String)],
) -> MarkdownBlock {
    let start = lines[start_index].start;
    let end = lines[end_index].end_with_newline;
    let text = content[start..end].to_string();
    let image_refs = collect_image_refs(&text, &std::collections::HashMap::new());
    let search_text = search_projection(&text, &image_refs);
    let char_start = content[..start].chars().count();
    let char_end = char_start + text.chars().count();
    MarkdownBlock {
        kind: kind.to_string(),
        element_type: element_type.to_string(),
        text,
        search_text,
        line_start: lines[start_index].number,
        line_end: lines[end_index].number,
        byte_start: start,
        byte_end: end,
        char_start,
        char_end,
        heading_path: heading_stack
            .iter()
            .map(|(_, title)| title.clone())
            .collect(),
        image_refs,
    }
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=6).contains(&level) || trimmed.chars().nth(level) != Some(' ') {
        return None;
    }
    let title = trimmed[level..]
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_string();
    (!title.is_empty()).then_some((level, title))
}

#[derive(Debug, Clone)]
struct Fence {
    marker: char,
    length: usize,
}

fn parse_fence_start(line: &str) -> Option<Fence> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then_some(Fence { marker, length })
}

fn find_fence_end(lines: &[LineSpan], start: usize, fence: &Fence) -> usize {
    for (index, line) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = line.text.trim_start();
        let count = trimmed
            .chars()
            .take_while(|character| *character == fence.marker)
            .count();
        if count >= fence.length && trimmed[count..].trim().is_empty() {
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

fn is_math_start(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed == "$$" || trimmed == "\\[" || trimmed.starts_with("$$ ")
}

fn find_math_end(lines: &[LineSpan], start: usize) -> usize {
    let opening = lines[start].text.trim();
    let closing = if opening.starts_with("\\[") {
        "\\]"
    } else {
        "$$"
    };
    for (index, line) in lines.iter().enumerate().skip(start + 1) {
        if line.text.trim() == closing {
            return index;
        }
    }
    lines.len().saturating_sub(1)
}

fn is_table_start(lines: &[LineSpan], index: usize) -> bool {
    index + 1 < lines.len()
        && is_table_row(&lines[index].text)
        && is_table_separator(&lines[index + 1].text)
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && !trimmed.is_empty()
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim().trim_matches('|');
    let cells = trimmed
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty());
    let mut seen = false;
    for cell in cells {
        let cell = cell.trim_matches(':').trim();
        if cell.len() < 3 || !cell.chars().all(|character| character == '-') {
            return false;
        }
        seen = true;
    }
    seen
}

fn is_list_or_quote(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('>')
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed.split_once('.').is_some_and(|(prefix, rest)| {
            !rest.is_empty() && prefix.chars().all(|c| c.is_ascii_digit())
        })
}

fn is_indented_continuation(line: &str) -> bool {
    line.starts_with("  ") || line.starts_with('\t')
}

fn find_front_matter_end(lines: &[LineSpan], start: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| (line.text.trim() == "---").then_some(index))
}

fn parse_image_definition(line: &str) -> Option<(String, String, String)> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("[!")?;
    let close = rest.find("]:")?;
    let key = rest[..close].trim().to_ascii_lowercase();
    let value = rest[close + 2..].trim();
    let (source, title) = split_destination(value);
    (!key.is_empty() && !source.is_empty()).then_some((key, source, title))
}

fn collect_image_definitions(content: &str) -> std::collections::HashMap<String, (String, String)> {
    let mut definitions = std::collections::HashMap::new();
    for line in content.lines() {
        if let Some((key, source, title)) = parse_image_definition(line) {
            definitions.insert(key, (source, title));
        }
    }
    definitions
}

fn collect_image_refs(
    text: &str,
    definitions: &std::collections::HashMap<String, (String, String)>,
) -> Vec<MarkdownImageRef> {
    let bytes = text.as_bytes();
    let mut refs = Vec::new();
    let mut cursor = 0usize;
    while cursor + 2 <= bytes.len() {
        if bytes[cursor] != b'!' || bytes[cursor + 1] != b'[' {
            cursor += 1;
            continue;
        }
        let alt_start = cursor + 2;
        let Some(alt_end_rel) = text[alt_start..].find(']') else {
            cursor += 2;
            continue;
        };
        let alt_end = alt_start + alt_end_rel;
        let alt = text[alt_start..alt_end].trim().to_string();
        let after = alt_end + 1;
        if after < bytes.len() && bytes[after] == b'(' {
            if let Some(close_rel) = text[after + 1..].find(')') {
                let value = text[after + 1..after + 1 + close_rel].trim();
                let (source, title) = split_destination(value);
                if !source.is_empty() {
                    refs.push(MarkdownImageRef { source, alt, title });
                }
                cursor = after + 2 + close_rel;
                continue;
            }
        } else if after < bytes.len() && bytes[after] == b'[' {
            if let Some(close_rel) = text[after + 1..].find(']') {
                let key = if close_rel == 0 {
                    alt.to_ascii_lowercase()
                } else {
                    text[after + 1..after + 1 + close_rel]
                        .trim()
                        .to_ascii_lowercase()
                };
                if let Some((source, title)) = definitions.get(&key) {
                    refs.push(MarkdownImageRef {
                        source: source.clone(),
                        alt,
                        title: title.clone(),
                    });
                }
                cursor = after + 2 + close_rel;
                continue;
            }
        }
        cursor = alt_end + 1;
    }
    refs
}

fn split_destination(value: &str) -> (String, String) {
    let value = value.trim().trim_matches('<').trim_matches('>');
    if value.is_empty() {
        return (String::new(), String::new());
    }
    if let Some((source, rest)) = value.split_once(char::is_whitespace) {
        let title = rest.trim().trim_matches('"').trim_matches('\'').to_string();
        (source.to_string(), title)
    } else {
        (value.to_string(), String::new())
    }
}

pub(crate) fn safe_relative_asset_path(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with('#')
        || value.contains("://")
        || value.starts_with("data:")
        || value.contains('?')
        || Path::new(value).is_absolute()
    {
        return None;
    }
    let value = percent_decode_path(value)?;
    if value.is_empty()
        || value.starts_with('#')
        || value.contains("://")
        || value.starts_with("data:")
        || value.contains('?')
        || value.contains('#')
        || value.chars().any(char::is_control)
    {
        return None;
    }
    let value = value.replace('\\', "/");
    let path = Path::new(&value);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(value)
}

fn percent_decode_path(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn search_projection(text: &str, image_refs: &[MarkdownImageRef]) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;
    while index < text.len() {
        let rest = &text[index..];
        if rest.starts_with("![") {
            if let Some(end) = rest.find(']') {
                let alt = &rest[2..end];
                output.push_str(alt);
                index += end + 1;
                continue;
            }
        }
        if rest.starts_with('[') {
            if let Some(end) = rest.find(']') {
                let after = end + 1;
                if rest[after..].starts_with('(') {
                    output.push_str(&rest[1..end]);
                    if let Some(close) = rest[after + 1..].find(')') {
                        index += after + 2 + close;
                        continue;
                    }
                }
            }
        }
        let character = rest.chars().next().expect("slice is non-empty");
        if !matches!(character, '`' | '*' | '_' | '~' | '#') {
            output.push(character);
        } else if character == '\n' {
            output.push('\n');
        }
        index += character.len_utf8();
    }
    for image in image_refs {
        if !image.alt.trim().is_empty() {
            output.push(' ');
            output.push_str(image.alt.trim());
        }
        if let Some(path) = safe_relative_asset_path(&image.source) {
            output.push(' ');
            output.push_str(
                Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&path),
            );
        }
        if !image.title.trim().is_empty() {
            output.push(' ');
            output.push_str(image.title.trim());
        }
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{parse_markdown, safe_relative_asset_path};

    #[test]
    fn normalizes_bom_and_crlf_and_keeps_coordinates() {
        let document = parse_markdown("\u{feff}# 标题\r\n\r\n正文");
        assert_eq!(document.canonical, "# 标题\n\n正文");
        assert_eq!(document.line_count, 3);
        assert_eq!(document.blocks[0].line_start, 1);
        assert_eq!(document.blocks[1].line_start, 3);
        assert_eq!(document.blocks[0].heading_path, vec!["标题"]);
    }

    #[test]
    fn recognizes_structure_and_images_without_indexing_binary_data() {
        let document = parse_markdown(
            "# A\n\n![实验图](attachments/figure.png \"caption\")\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nlet x = 1;\n```\n",
        );
        assert!(document.blocks.iter().any(|block| block.kind == "table"));
        assert!(document.blocks.iter().any(|block| block.kind == "code"));
        let image_block = document
            .blocks
            .iter()
            .find(|block| !block.image_refs.is_empty())
            .unwrap();
        assert_eq!(image_block.image_refs[0].alt, "实验图");
        assert!(image_block.search_text.contains("实验图"));
        assert!(!image_block.search_text.contains("PNG_BINARY"));
    }

    #[test]
    fn rejects_remote_and_traversing_asset_paths() {
        assert!(safe_relative_asset_path("attachments/a.png").is_some());
        assert_eq!(
            safe_relative_asset_path("attachments/figure%20one.png").as_deref(),
            Some("attachments/figure one.png")
        );
        assert!(safe_relative_asset_path("../a.png").is_none());
        assert!(safe_relative_asset_path("%2e%2e/secret.png").is_none());
        assert!(safe_relative_asset_path("https://example.com/a.png").is_none());
        assert!(safe_relative_asset_path("data:image/png;base64,abc").is_none());
    }
}
