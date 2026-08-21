//! Structured artifact hand-off. This tool does not write to disk; it gives
//! the model a bounded, typed checkpoint that can be referenced in the final
//! answer and represented consistently in ToolTrace.

use serde_json::{json, Value};

use crate::ai::error::ModelError;

use super::types::ToolExecution;

const MAX_ARTIFACT_CHARS: usize = 100_000;
const MAX_PREVIEW_CHARS: usize = 2_000;

pub(super) fn present_artifact(arguments: &Value) -> Result<ToolExecution, ModelError> {
    let title = required_string(arguments, "title")?;
    let kind = required_string(arguments, "kind")?;
    let content = required_string(arguments, "content")?;
    if title.chars().count() > 200 {
        return Err(ModelError::invalid_configuration(
            "Artifact 标题不能超过 200 个字符。",
        ));
    }
    if !matches!(
        kind,
        "markdown" | "code" | "json" | "mermaid" | "html" | "text"
    ) {
        return Err(ModelError::invalid_configuration(
            "Artifact 类型必须是 markdown、code、json、mermaid、html 或 text。",
        ));
    }
    if content.chars().count() > MAX_ARTIFACT_CHARS {
        return Err(ModelError::invalid_configuration(format!(
            "Artifact 内容不能超过 {MAX_ARTIFACT_CHARS} 个字符。"
        )));
    }
    if kind == "json" {
        serde_json::from_str::<Value>(content).map_err(|error| {
            ModelError::invalid_configuration(format!("Artifact JSON 无效：{error}"))
        })?;
    }
    let language = arguments
        .get("language")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let artifact_id = format!("artifact-{}", uuid::Uuid::new_v4());
    let result = json!({
        "status": "presented",
        "artifactId": artifact_id,
        "title": title,
        "kind": kind,
        "language": language,
        "content": content,
        "persistent": false,
        "notice": "当前 Artifact 是本轮运行中的结构化交付，不会自动写入用户文件。",
    });
    let serialized = serde_json::to_string(&result).map_err(|error| {
        ModelError::invalid_configuration(format!("序列化 Artifact 失败：{error}"))
    })?;
    Ok(ToolExecution {
        preview: truncate_chars(
            &format!("已准备 Artifact：{title}（{kind}）\n{content}"),
            MAX_PREVIEW_CHARS,
        ),
        output_chars: serialized.chars().count(),
        content: serialized,
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
