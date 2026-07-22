//! `SKILL.md` 的 YAML frontmatter 解析和字段校验。

use std::{fs, path::Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::types::{SkillProvenance, SkillRecord, SkillSource, SkillSummary};

pub const MAX_SKILL_MD_BYTES: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SkillFrontmatter {
    id: Option<String>,
    name: String,
    description: String,
    version: Option<ScalarText>,
    #[serde(default)]
    triggers: StringList,
    #[serde(default)]
    argument_hint: Option<String>,
    #[serde(default, alias = "allowed-tools", alias = "mcp-tools")]
    recommended_tools: StringList,
    #[serde(default)]
    required_tools: StringList,
    #[serde(default)]
    disable_model_invocation: bool,
    license: Option<String>,
    compatibility: Option<String>,
    #[serde(default)]
    metadata: SkillMetadata,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ScalarText {
    Text(String),
    Integer(i64),
    Float(f64),
}

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum StringList {
    Text(String),
    Values(Vec<String>),
    #[default]
    Empty,
}

impl StringList {
    fn into_values(self) -> Vec<String> {
        match self {
            Self::Text(value) => value
                .split(|character: char| character.is_whitespace() || character == ',')
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
            Self::Values(values) => values,
            Self::Empty => Vec::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct SkillMetadata {
    version: Option<ScalarText>,
    #[serde(default)]
    mnemora: MnemoraMetadata,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct MnemoraMetadata {
    #[serde(alias = "source_repository", alias = "sourceRepository")]
    source_repository: Option<String>,
    #[serde(alias = "source_path", alias = "sourcePath")]
    source_path: Option<String>,
    #[serde(alias = "source_revision", alias = "sourceRevision")]
    source_revision: Option<String>,
    attribution: Option<String>,
    #[serde(default)]
    adapted: bool,
    #[serde(alias = "adaptation_notes", alias = "adaptationNotes")]
    adaptation_notes: Option<String>,
}

pub(crate) fn parse_skill(
    directory: &Path,
    source: SkillSource,
    enabled: bool,
) -> Result<SkillRecord, String> {
    let path = directory.join("SKILL.md");
    let metadata =
        fs::metadata(&path).map_err(|error| format!("技能目录缺少可读取的 SKILL.md：{error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SKILL_MD_BYTES {
        return Err(format!(
            "SKILL.md 必须是 1 到 {} KB 的普通文件。",
            MAX_SKILL_MD_BYTES / 1024
        ));
    }
    let bytes = fs::read(&path).map_err(|error| format!("读取 SKILL.md 失败：{error}"))?;
    let raw = String::from_utf8(bytes.clone())
        .map_err(|_| "SKILL.md 必须使用 UTF-8 编码。".to_string())?;
    let normalized = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let (yaml, body) = split_frontmatter(&normalized)?;
    let body = body.trim().to_string();
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml)
        .map_err(|error| format!("SKILL.md 元数据不是有效 YAML：{error}"))?;
    let skill_id = resolve_skill_id(directory, &frontmatter)?;
    let version = resolve_version(&frontmatter)?;
    validate_frontmatter(&frontmatter, &version)?;
    validate_provenance(source, &frontmatter)?;

    let mut triggers = frontmatter
        .triggers
        .into_values()
        .iter()
        .map(|value| normalize_trigger(value))
        .collect::<Result<Vec<_>, _>>()?;
    triggers.sort();
    triggers.dedup();
    let mut recommended_tools = frontmatter.recommended_tools.into_values();
    recommended_tools.sort();
    recommended_tools.dedup();
    let mut required_tools = frontmatter.required_tools.into_values();
    required_tools.sort();
    required_tools.dedup();

    let provenance = SkillProvenance {
        repository: frontmatter.metadata.mnemora.source_repository,
        path: frontmatter.metadata.mnemora.source_path,
        revision: frontmatter.metadata.mnemora.source_revision,
        attribution: frontmatter.metadata.mnemora.attribution,
        adapted: frontmatter.metadata.mnemora.adapted,
        adaptation_notes: frontmatter.metadata.mnemora.adaptation_notes,
    };

    let content_hash = format!("sha256:{:x}", Sha256::digest(&bytes));
    Ok(SkillRecord {
        summary: SkillSummary {
            id: skill_id,
            name: frontmatter.name,
            description: frontmatter.description,
            version,
            source,
            enabled,
            triggers,
            argument_hint: frontmatter.argument_hint,
            recommended_tools,
            required_tools,
            disable_model_invocation: frontmatter.disable_model_invocation,
            license: frontmatter.license,
            compatibility: frontmatter.compatibility,
            provenance,
            content_hash,
        },
        markdown: normalized,
        body,
        directory: directory.to_path_buf(),
    })
}

fn split_frontmatter(raw: &str) -> Result<(&str, &str), String> {
    let rest = raw
        .strip_prefix("---\n")
        .ok_or_else(|| "SKILL.md 必须以 YAML frontmatter（---）开头。".to_string())?;
    let closing = rest
        .find("\n---\n")
        .ok_or_else(|| "SKILL.md 缺少 frontmatter 结束标记。".to_string())?;
    let yaml = &rest[..closing];
    let body = &rest[closing + "\n---\n".len()..];
    if body.trim().is_empty() {
        return Err("SKILL.md 正文不能为空。".to_string());
    }
    Ok((yaml, body))
}

fn resolve_skill_id(directory: &Path, value: &SkillFrontmatter) -> Result<String, String> {
    if let Some(skill_id) = value.id.as_deref() {
        validate_skill_id(skill_id)?;
        return Ok(skill_id.to_string());
    }
    for candidate in [
        directory.file_name().and_then(|name| name.to_str()),
        Some(&value.name),
    ]
    .into_iter()
    .flatten()
    {
        if validate_skill_id(candidate).is_ok() {
            return Ok(candidate.to_string());
        }
    }
    Err("SKILL.md 未声明 id，且目录名和 name 都不能作为安全的 Skill ID。".to_string())
}

fn resolve_version(value: &SkillFrontmatter) -> Result<String, String> {
    let version = value
        .version
        .as_ref()
        .or(value.metadata.version.as_ref())
        .map(|value| match value {
            ScalarText::Text(value) => value.clone(),
            ScalarText::Integer(value) => value.to_string(),
            ScalarText::Float(value) => value.to_string(),
        })
        .unwrap_or_else(|| "unversioned".to_string());
    if version.trim().is_empty() || version.len() > 32 {
        return Err("Skill version 不能为空且不能超过 32 个字节。".to_string());
    }
    Ok(version)
}

fn validate_frontmatter(value: &SkillFrontmatter, version: &str) -> Result<(), String> {
    if value.name.trim().is_empty() || value.name.chars().count() > 80 {
        return Err("Skill name 不能为空且不能超过 80 个字符。".to_string());
    }
    if value.description.trim().is_empty() || value.description.chars().count() > 500 {
        return Err("Skill description 不能为空且不能超过 500 个字符。".to_string());
    }
    if version.trim().is_empty() || version.len() > 32 {
        return Err("Skill version 不能为空且不能超过 32 个字节。".to_string());
    }
    if value
        .argument_hint
        .as_ref()
        .is_some_and(|hint| hint.chars().count() > 120)
    {
        return Err("Skill argument-hint 不能超过 120 个字符。".to_string());
    }
    let trigger_count = match &value.triggers {
        StringList::Text(value) => value.split_whitespace().count(),
        StringList::Values(values) => values.len(),
        StringList::Empty => 0,
    };
    if trigger_count > 16 {
        return Err("单个 Skill 不能声明超过 16 个触发词。".to_string());
    }
    let recommended_tools = string_list_values(&value.recommended_tools);
    let required_tools = string_list_values(&value.required_tools);
    if recommended_tools.len() > 32 || required_tools.len() > 32 {
        return Err("单个 Skill 不能声明超过 32 个工具。".to_string());
    }
    for tool in recommended_tools.iter().chain(required_tools.iter()) {
        validate_tool_name(tool)?;
    }
    validate_optional_metadata("license", value.license.as_deref(), 160)?;
    validate_optional_metadata("compatibility", value.compatibility.as_deref(), 1000)?;
    validate_optional_metadata(
        "source repository",
        value.metadata.mnemora.source_repository.as_deref(),
        500,
    )?;
    if value
        .metadata
        .mnemora
        .source_repository
        .as_deref()
        .is_some_and(|repository| !repository.starts_with("https://"))
    {
        return Err("source repository 必须使用 HTTPS。".to_string());
    }
    validate_optional_metadata(
        "source path",
        value.metadata.mnemora.source_path.as_deref(),
        500,
    )?;
    validate_optional_metadata(
        "source revision",
        value.metadata.mnemora.source_revision.as_deref(),
        128,
    )?;
    validate_optional_metadata(
        "attribution",
        value.metadata.mnemora.attribution.as_deref(),
        500,
    )?;
    validate_optional_metadata(
        "adaptation notes",
        value.metadata.mnemora.adaptation_notes.as_deref(),
        1000,
    )?;
    Ok(())
}

fn validate_provenance(source: SkillSource, value: &SkillFrontmatter) -> Result<(), String> {
    let provenance = &value.metadata.mnemora;
    if provenance.adapted && provenance.source_repository.is_none() {
        return Err("标记为 adapted 的 Skill 必须记录 source repository。".to_string());
    }
    if source != SkillSource::Builtin || provenance.source_repository.is_none() {
        return Ok(());
    }
    if value.license.as_deref().is_none_or(str::is_empty)
        || provenance.source_path.as_deref().is_none_or(str::is_empty)
    {
        return Err("带上游来源的内置 Skill 必须同时记录 license 和 source path。".to_string());
    }
    let revision = provenance
        .source_revision
        .as_deref()
        .ok_or_else(|| "带上游来源的内置 Skill 必须固定 source revision。".to_string())?;
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("内置 Skill 的 source revision 必须是 40 位 Commit SHA。".to_string());
    }
    Ok(())
}

fn string_list_values(value: &StringList) -> Vec<&str> {
    match value {
        StringList::Text(value) => value
            .split(|character: char| character.is_whitespace() || character == ',')
            .filter(|value| !value.is_empty())
            .collect(),
        StringList::Values(values) => values.iter().map(String::as_str).collect(),
        StringList::Empty => Vec::new(),
    }
}

fn validate_optional_metadata(
    field: &str,
    value: Option<&str>,
    max_chars: usize,
) -> Result<(), String> {
    if let Some(value) = value {
        if value.trim().is_empty()
            || value.chars().count() > max_chars
            || value.chars().any(char::is_control)
        {
            return Err(format!("Skill {field} 字段无效。"));
        }
    }
    Ok(())
}

pub fn validate_skill_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 64 {
        return Err("Skill ID 必须是 1 到 64 个字符。".to_string());
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
    {
        return Err("Skill ID 只能包含小写字母、数字和单个连字符。".to_string());
    }
    Ok(())
}

fn validate_tool_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(format!("Skill 工具名称无效：{value}"));
    }
    Ok(())
}

fn normalize_trigger(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    let trigger = if value.starts_with('/') {
        value
    } else {
        format!("/{value}")
    };
    let name = trigger.trim_start_matches('/');
    if name.is_empty()
        || name.len() > 63
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(format!("Skill 触发词无效：{trigger}"));
    }
    Ok(trigger)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::parse_skill;
    use crate::skills::types::SkillSource;

    #[test]
    fn parses_frontmatter_and_normalizes_triggers() {
        let root = std::env::temp_dir().join(format!("mnemora-skill-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nid: demo-skill\nname: 示例\ndescription: 示例技能。\nversion: 1.0.0\ntriggers: [demo, /test]\nrecommended-tools: [skill]\n---\n\n# 示例\n",
        )
        .unwrap();
        let record = parse_skill(&root, SkillSource::User, true).unwrap();
        assert_eq!(record.summary.id, "demo-skill");
        assert_eq!(record.summary.triggers, vec!["/demo", "/test"]);
        assert_eq!(record.body, "# 示例");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unsafe_skill_id() {
        let root = std::env::temp_dir().join(format!("mnemora-skill-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nid: ../bad\nname: 示例\ndescription: 示例技能。\nversion: 1\n---\n正文\n",
        )
        .unwrap();
        assert!(parse_skill(&root, SkillSource::User, true).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn accepts_standard_agent_skill_metadata_and_string_tools() {
        let root = std::env::temp_dir().join(format!(
            "mnemora-standard-agent-skill-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let skill_dir = root.join("standard-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: standard-skill\ndescription: 标准 Agent Skill。\nallowed-tools: Read Write\nlicense: MIT\nmetadata:\n  version: 1.2\n  mnemora:\n    source-repository: https://github.com/example/skills\n    source-path: skills/standard-skill\n    source-revision: 0123456789abcdef0123456789abcdef01234567\n    adapted: true\n    adaptation-notes: 绑定到 Mnemora 工具。\n---\n正文\n",
        )
        .unwrap();

        let record = parse_skill(&skill_dir, SkillSource::User, true).unwrap();
        assert_eq!(record.summary.id, "standard-skill");
        assert_eq!(record.summary.version, "1.2");
        assert_eq!(record.summary.recommended_tools, vec!["Read", "Write"]);
        assert_eq!(record.summary.license.as_deref(), Some("MIT"));
        assert_eq!(
            record.summary.provenance.repository.as_deref(),
            Some("https://github.com/example/skills")
        );
        assert!(record.summary.provenance.adapted);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn defaults_missing_standard_version_without_rejecting_import() {
        let root = std::env::temp_dir().join(format!(
            "mnemora-unversioned-agent-skill-{}",
            uuid::Uuid::new_v4()
        ));
        let skill_dir = root.join("unversioned-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: unversioned-skill\ndescription: 没有自定义版本字段。\n---\n正文\n",
        )
        .unwrap();

        let record = parse_skill(&skill_dir, SkillSource::User, true).unwrap();
        assert_eq!(record.summary.version, "unversioned");
        let _ = fs::remove_dir_all(root);
    }
}
