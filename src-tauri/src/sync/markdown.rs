//! 把文献笔记、文献元数据和 PDF 批注转换为统一 Markdown 文档。

use crate::library::types::{LibraryAnnotation, LibraryItem, LibraryNote};

pub struct SyncDocument {
    pub note_id: String,
    pub title: String,
    pub markdown: String,
}

pub fn render_document(
    note: &LibraryNote,
    item: Option<&LibraryItem>,
    annotations: &[LibraryAnnotation],
    include_metadata: bool,
    include_annotations: bool,
) -> SyncDocument {
    let mut markdown = String::new();
    markdown.push_str("---\n");
    markdown.push_str(&format!("mnemora_id: {}\n", yaml_string(&note.id)));
    markdown.push_str(if item.is_some() {
        "mnemora_type: literature-note\n"
    } else {
        "mnemora_type: markdown-note\n"
    });
    markdown.push_str(&format!("title: {}\n", yaml_string(&note.title)));
    if let Some(item) = item {
        markdown.push_str(&format!("literature: {}\n", yaml_string(&item.title)));
    }
    markdown.push_str(&format!("updated_at: {}\n", note.updated_at));
    markdown.push_str("---\n\n");
    markdown.push_str("# ");
    markdown.push_str(&note.title);
    markdown.push_str("\n\n");

    if include_metadata && item.is_some() {
        let item = item.expect("item checked above");
        markdown.push_str("## 文献信息\n\n");
        markdown.push_str(&format!("- 标题：{}\n", item.title));
        if !item.authors.is_empty() {
            markdown.push_str(&format!("- 作者：{}\n", item.authors.join("、")));
        }
        if let Some(year) = item.publication_year {
            markdown.push_str(&format!("- 年份：{year}\n"));
        }
        if !item.publication_title.is_empty() {
            markdown.push_str(&format!("- 出版物：{}\n", item.publication_title));
        }
        if !item.doi.is_empty() {
            markdown.push_str(&format!("- DOI：{}\n", item.doi));
        }
        if !item.tags.is_empty() {
            markdown.push_str(&format!("- 标签：{}\n", item.tags.join("、")));
        }
        markdown.push('\n');
    }

    markdown.push_str("## 笔记\n\n");
    markdown.push_str(&note.content);
    markdown.push_str("\n\n");

    if include_annotations && !annotations.is_empty() {
        markdown.push_str("## PDF 批注\n\n");
        for annotation in annotations {
            markdown.push_str(&format!("### 第 {} 页\n\n", annotation.page_index + 1));
            if !annotation.text.is_empty() {
                for line in annotation.text.lines() {
                    markdown.push_str("> ");
                    markdown.push_str(line);
                    markdown.push('\n');
                }
                markdown.push('\n');
            }
            if !annotation.comment.is_empty() {
                markdown.push_str(&annotation.comment);
                markdown.push_str("\n\n");
            }
        }
    }

    SyncDocument {
        note_id: note.id.clone(),
        title: note.title.clone(),
        markdown,
    }
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

#[cfg(test)]
mod tests {
    use super::yaml_string;

    #[test]
    fn yaml_strings_are_quoted_and_escaped() {
        assert_eq!(yaml_string("a: b\nnext"), "\"a: b\\nnext\"");
    }
}
