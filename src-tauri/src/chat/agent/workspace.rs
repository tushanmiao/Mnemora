//! Bounded, read-only workspace tools.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use globset::{GlobBuilder, GlobMatcher};
use serde_json::{json, Value};
use walkdir::{DirEntry, WalkDir};

use crate::ai::error::ModelError;

use super::types::ToolExecution;

const MAX_WALK_DEPTH: usize = 12;
const MAX_LIST_RESULTS: usize = 200;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_READ_LINES: usize = 2_000;
const MAX_READ_BYTES: usize = 32_000;
const MAX_TEXT_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PREVIEW_CHARS: usize = 2_000;

const EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".idea",
    ".vscode",
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    "__pycache__",
    ".next",
    ".nuxt",
];

pub(super) fn workspace_list(root: &Path, arguments: &Value) -> Result<ToolExecution, ModelError> {
    let relative = optional_string(arguments, "path").unwrap_or_default();
    let start = resolve_workspace_path(root, relative, true)?;
    let depth = optional_u64(arguments, "depth").unwrap_or(1).clamp(1, 4) as usize;
    let cursor = optional_u64(arguments, "cursor").unwrap_or(0) as usize;
    let limit = optional_u64(arguments, "limit")
        .unwrap_or(80)
        .clamp(1, MAX_LIST_RESULTS as u64) as usize;
    let root = canonical_root(root)?;
    let mut entries = collect_entries(&root, &start, depth)?
        .into_iter()
        .map(|entry| {
            let relative = normalized_relative(&root, entry.path())?;
            let metadata = entry.metadata().map_err(|error| {
                ModelError::invalid_configuration(format!("读取工作区条目失败：{error}"))
            })?;
            Ok(json!({
                "path": relative,
                "kind": if metadata.is_dir() { "directory" } else { "file" },
                "sizeBytes": metadata.is_file().then_some(metadata.len()),
            }))
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    entries.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    let total = entries.len();
    let page = entries
        .into_iter()
        .skip(cursor)
        .take(limit)
        .collect::<Vec<_>>();
    let next_cursor = cursor
        .saturating_add(page.len())
        .lt(&total)
        .then_some(cursor.saturating_add(page.len()));
    execution(json!({
        "root": ".",
        "path": relative,
        "entries": page,
        "nextCursor": next_cursor,
        "total": total,
    }))
}

pub(super) fn workspace_glob(root: &Path, arguments: &Value) -> Result<ToolExecution, ModelError> {
    let pattern = required_string(arguments, "pattern")?;
    let matcher = GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map_err(|error| ModelError::invalid_configuration(format!("Glob 模式无效：{error}")))?
        .compile_matcher();
    let cursor = optional_u64(arguments, "cursor").unwrap_or(0) as usize;
    let limit = optional_u64(arguments, "limit")
        .unwrap_or(80)
        .clamp(1, MAX_LIST_RESULTS as u64) as usize;
    let root = canonical_root(root)?;
    let mut matches = collect_entries(&root, &root, MAX_WALK_DEPTH)?
        .into_iter()
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            normalized_relative(&root, entry.path())
                .ok()
                .filter(|relative| matcher.is_match(relative))
        })
        .collect::<Vec<_>>();
    matches.sort();
    let total = matches.len();
    let page = matches
        .into_iter()
        .skip(cursor)
        .take(limit)
        .collect::<Vec<_>>();
    let next_cursor = cursor
        .saturating_add(page.len())
        .lt(&total)
        .then_some(cursor.saturating_add(page.len()));
    execution(json!({
        "pattern": pattern,
        "paths": page,
        "nextCursor": next_cursor,
        "total": total,
    }))
}

pub(super) fn workspace_search(
    root: &Path,
    arguments: &Value,
) -> Result<ToolExecution, ModelError> {
    let query = required_string(arguments, "query")?;
    let case_sensitive = arguments
        .get("caseSensitive")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let matcher = optional_string(arguments, "glob")
        .filter(|value| !value.trim().is_empty())
        .map(|value| build_matcher(value.to_string()))
        .transpose()?;
    let limit = optional_u64(arguments, "limit")
        .unwrap_or(80)
        .clamp(1, MAX_SEARCH_RESULTS as u64) as usize;
    let root = canonical_root(root)?;
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut matches = Vec::new();
    let mut skipped_binary = 0usize;
    for entry in collect_entries(&root, &root, MAX_WALK_DEPTH)? {
        if matches.len() >= limit || !entry.file_type().is_file() {
            continue;
        }
        let relative = normalized_relative(&root, entry.path())?;
        if matcher
            .as_ref()
            .is_some_and(|matcher| !matcher.is_match(&relative))
        {
            continue;
        }
        let metadata = entry.metadata().map_err(|error| {
            ModelError::invalid_configuration(format!("读取工作区文件失败：{error}"))
        })?;
        if metadata.len() > MAX_TEXT_FILE_BYTES {
            continue;
        }
        let bytes = fs::read(entry.path()).map_err(|error| {
            ModelError::invalid_configuration(format!("读取工作区文件失败：{error}"))
        })?;
        if bytes.iter().take(8_192).any(|byte| *byte == 0) {
            skipped_binary = skipped_binary.saturating_add(1);
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (index, line) in text.lines().enumerate() {
            let haystack = if case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };
            if haystack.contains(&needle) {
                matches.push(json!({
                    "path": relative,
                    "line": index + 1,
                    "text": truncate_chars(line.trim(), 500),
                    "reference": format!("[workspace:{relative}#L{}]", index + 1),
                }));
                if matches.len() >= limit {
                    break;
                }
            }
        }
    }
    execution(json!({
        "query": query,
        "matches": matches,
        "truncated": matches.len() >= limit,
        "skippedBinaryFiles": skipped_binary,
    }))
}

pub(super) fn workspace_read(root: &Path, arguments: &Value) -> Result<ToolExecution, ModelError> {
    let relative = required_string(arguments, "path")?;
    let path = resolve_workspace_path(root, relative, false)?;
    let metadata = fs::metadata(&path).map_err(|error| {
        ModelError::invalid_configuration(format!("读取工作区文件失败：{error}"))
    })?;
    if metadata.len() > MAX_TEXT_FILE_BYTES {
        return Err(ModelError::invalid_configuration(
            "工作区文本文件超过 4 MB 读取上限。",
        ));
    }
    let bytes = fs::read(&path).map_err(|error| {
        ModelError::invalid_configuration(format!("读取工作区文件失败：{error}"))
    })?;
    if bytes.iter().take(8_192).any(|byte| *byte == 0) {
        return Err(ModelError::invalid_configuration(
            "该文件看起来是二进制文件，不能作为文本读取。",
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| ModelError::invalid_configuration("工作区文件不是 UTF-8 文本。"))?;
    let start = optional_u64(arguments, "startLine").unwrap_or(1).max(1) as usize;
    let end = optional_u64(arguments, "endLine")
        .unwrap_or_else(|| start.saturating_add(399) as u64)
        .max(start as u64) as usize;
    if end.saturating_sub(start) >= MAX_READ_LINES {
        return Err(ModelError::invalid_configuration(format!(
            "单次最多读取 {MAX_READ_LINES} 行工作区文本。"
        )));
    }
    let selected = text
        .lines()
        .enumerate()
        .filter(|(index, _)| (*index + 1) >= start && (*index + 1) <= end)
        .map(|(index, line)| format!("{:>6}: {line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let max_bytes = optional_u64(arguments, "maxBytes")
        .unwrap_or(12_000)
        .clamp(1, MAX_READ_BYTES as u64) as usize;
    let (content, truncated) = truncate_utf8_bytes(&selected, max_bytes);
    let value = json!({
        "path": relative.replace('\\', "/"),
        "startLine": start,
        "endLine": end,
        "content": content,
        "reference": format!("[workspace:{}#L{}-L{}]", relative.replace('\\', "/"), start, end),
        "truncated": truncated,
    });
    let mut result = execution(value)?;
    result.output_truncated = truncated;
    Ok(result)
}

fn canonical_root(root: &Path) -> Result<PathBuf, ModelError> {
    let root = root
        .canonicalize()
        .map_err(|error| ModelError::invalid_configuration(format!("工作目录不可用：{error}")))?;
    if !root.is_dir() {
        return Err(ModelError::invalid_configuration("工作目录不是文件夹。"));
    }
    Ok(root)
}

fn resolve_workspace_path(
    root: &Path,
    relative: &str,
    allow_directory: bool,
) -> Result<PathBuf, ModelError> {
    let root = canonical_root(root)?;
    let relative = Path::new(relative.trim());
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ModelError::invalid_configuration(
            "工作区路径必须是根目录内的相对路径。",
        ));
    }
    if is_sensitive_path(relative) {
        return Err(ModelError::invalid_configuration(
            "该路径属于凭据、私钥或环境变量等敏感文件，工具拒绝读取。",
        ));
    }
    let mut candidate = root.clone();
    for component in relative.components() {
        if let Component::Normal(name) = component {
            candidate.push(name);
            let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
                ModelError::invalid_configuration(format!("工作区路径不存在：{error}"))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(ModelError::invalid_configuration(
                    "工作区工具不跟随符号链接。",
                ));
            }
        }
    }
    let resolved = candidate
        .canonicalize()
        .map_err(|error| ModelError::invalid_configuration(format!("工作区路径不存在：{error}")))?;
    if !resolved.starts_with(&root) {
        return Err(ModelError::invalid_configuration(
            "工作区路径越过了配置的根目录。",
        ));
    }
    let metadata = fs::symlink_metadata(&resolved).map_err(|error| {
        ModelError::invalid_configuration(format!("读取工作区路径失败：{error}"))
    })?;
    if allow_directory && !metadata.is_dir() {
        return Err(ModelError::invalid_configuration(
            "该工作区路径不是文件夹。",
        ));
    }
    if !allow_directory && !metadata.is_file() {
        return Err(ModelError::invalid_configuration("该工作区路径不是文件。"));
    }
    Ok(resolved)
}

fn collect_entries(root: &Path, start: &Path, depth: usize) -> Result<Vec<DirEntry>, ModelError> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(start)
        .follow_links(false)
        .max_depth(depth.saturating_add(1).min(MAX_WALK_DEPTH + 1))
        .into_iter()
        .filter_entry(|entry| should_visit(root, entry))
    {
        let entry = entry.map_err(|error| {
            ModelError::invalid_configuration(format!("遍历工作区失败：{error}"))
        })?;
        if entry.path() == start || entry.file_type().is_symlink() {
            continue;
        }
        if is_sensitive_path(entry.path().strip_prefix(root).unwrap_or(entry.path())) {
            continue;
        }
        entries.push(entry);
    }
    Ok(entries)
}

fn should_visit(root: &Path, entry: &DirEntry) -> bool {
    if entry.path() == root {
        return true;
    }
    if entry.file_type().is_symlink() {
        return false;
    }
    let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
    !(entry.file_type().is_dir() && EXCLUDED_DIRECTORIES.contains(&name.as_str()))
        && !is_sensitive_path(entry.path().strip_prefix(root).unwrap_or(entry.path()))
}

fn is_sensitive_path(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        value == ".env"
            || value.starts_with(".env.")
            || value == "credentials"
            || value.contains("credential")
            || value.contains("secret")
            || value == "id_rsa"
            || value == "id_ed25519"
            || value.ends_with(".pem")
            || value.ends_with(".key")
            || value.ends_with(".p12")
            || value.ends_with(".pfx")
    })
}

fn normalized_relative(root: &Path, path: &Path) -> Result<String, ModelError> {
    path.strip_prefix(root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| ModelError::invalid_configuration("工作区路径越过了根目录。"))
}

fn build_matcher(pattern: String) -> Result<GlobMatcher, ModelError> {
    GlobBuilder::new(&pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|error| ModelError::invalid_configuration(format!("Glob 模式无效：{error}")))
}

fn execution(value: Value) -> Result<ToolExecution, ModelError> {
    let content = serde_json::to_string(&value).map_err(|error| {
        ModelError::invalid_configuration(format!("序列化工具结果失败：{error}"))
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

fn optional_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).map(str::trim)
}

fn optional_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
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

    use super::{workspace_read, workspace_search};

    #[test]
    fn reads_and_searches_only_inside_workspace() {
        let root = std::env::temp_dir().join(format!("mnemora-workspace-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        fs::write(root.join(".env"), "TOKEN=secret").unwrap();

        let read = workspace_read(&root, &json!({ "path": "src/main.rs" })).unwrap();
        assert!(read.content.contains("println"));
        let search = workspace_search(&root, &json!({ "query": "hello" })).unwrap();
        assert!(search.content.contains("src/main.rs"));
        assert!(workspace_read(&root, &json!({ "path": ".env" })).is_err());
        assert!(workspace_read(&root, &json!({ "path": "../outside" })).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links_in_any_workspace_path_component() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("mnemora-workspace-link-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("mnemora-workspace-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "outside").unwrap();
        symlink(&outside, root.join("linked")).unwrap();

        assert!(workspace_read(&root, &json!({ "path": "linked/secret.txt" })).is_err());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_symbolic_links_in_any_workspace_path_component() {
        use std::os::windows::fs::symlink_dir;

        let root = std::env::temp_dir().join(format!("mnemora-workspace-link-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("mnemora-workspace-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "outside").unwrap();
        if symlink_dir(&outside, root.join("linked")).is_ok() {
            assert!(workspace_read(&root, &json!({ "path": "linked/secret.txt" })).is_err());
            let _ = fs::remove_dir(root.join("linked"));
        }

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
