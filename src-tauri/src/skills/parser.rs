//! `SKILL.md` 的 YAML frontmatter 解析和字段校验。

use std::{fs, path::Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::types::{
    SkillMode, SkillProvenance, SkillRecord, SkillResourceCost, SkillRisk, SkillSource,
    SkillSummary,
};

pub const MAX_SKILL_MD_BYTES: u64 = 256 * 1024;
const MNEMORA_METADATA_FILE: &str = "mnemora.json";

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
    #[serde(alias = "default_enabled", alias = "defaultEnabled")]
    default_enabled: Option<bool>,
    #[serde(default, alias = "modes", alias = "supportedModes")]
    supported_modes: Vec<SkillMode>,
    #[serde(default)]
    risk: SkillRisk,
    #[serde(default, alias = "resourceCost")]
    resource_cost: SkillResourceCost,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillSidecar {
    id: Option<String>,
    version: Option<String>,
    #[serde(default)]
    triggers: Vec<String>,
    argument_hint: Option<String>,
    #[serde(default)]
    recommended_tools: Vec<String>,
    #[serde(default)]
    required_tools: Vec<String>,
    default_enabled: Option<bool>,
    #[serde(default)]
    supported_modes: Vec<SkillMode>,
    risk: Option<SkillRisk>,
    resource_cost: Option<SkillResourceCost>,
    license: Option<String>,
    compatibility: Option<String>,
    #[serde(default)]
    provenance: SkillSidecarProvenance,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillSidecarProvenance {
    repository: Option<String>,
    path: Option<String>,
    revision: Option<String>,
    attribution: Option<String>,
    #[serde(default)]
    adapted: bool,
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
    let sidecar = read_sidecar(directory)?;
    let skill_id = resolve_skill_id(directory, &frontmatter, &sidecar)?;
    let version = resolve_version(&frontmatter, &sidecar)?;
    validate_frontmatter(&frontmatter, &version)?;
    validate_provenance(source, &frontmatter)?;
    validate_sidecar(source, &sidecar)?;

    let mut triggers = frontmatter
        .triggers
        .into_values()
        .iter()
        .map(|value| normalize_trigger(value))
        .collect::<Result<Vec<_>, _>>()?;
    triggers.extend(
        sidecar
            .triggers
            .iter()
            .map(|value| normalize_trigger(value))
            .collect::<Result<Vec<_>, _>>()?,
    );
    triggers.sort();
    triggers.dedup();
    let mut recommended_tools = frontmatter.recommended_tools.into_values();
    recommended_tools.extend(sidecar.recommended_tools.iter().cloned());
    recommended_tools.sort();
    recommended_tools.dedup();
    let mut required_tools = frontmatter.required_tools.into_values();
    required_tools.extend(sidecar.required_tools.iter().cloned());
    required_tools.sort();
    required_tools.dedup();
    let supported_modes = if !sidecar.supported_modes.is_empty() {
        normalized_modes(sidecar.supported_modes.clone())
    } else if frontmatter.metadata.mnemora.supported_modes.is_empty() {
        SkillMode::all().to_vec()
    } else {
        normalized_modes(frontmatter.metadata.mnemora.supported_modes.clone())
    };

    let provenance = if sidecar.provenance.repository.is_some() {
        SkillProvenance {
            repository: sidecar.provenance.repository,
            path: sidecar.provenance.path,
            revision: sidecar.provenance.revision,
            attribution: sidecar.provenance.attribution,
            adapted: sidecar.provenance.adapted,
            adaptation_notes: sidecar.provenance.adaptation_notes,
        }
    } else {
        SkillProvenance {
            repository: frontmatter.metadata.mnemora.source_repository,
            path: frontmatter.metadata.mnemora.source_path,
            revision: frontmatter.metadata.mnemora.source_revision,
            attribution: frontmatter.metadata.mnemora.attribution,
            adapted: frontmatter.metadata.mnemora.adapted,
            adaptation_notes: frontmatter.metadata.mnemora.adaptation_notes,
        }
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
            // 缺少该字段时按旧版本行为默认启用；新内置 Skill 可显式写 false。
            default_enabled: sidecar
                .default_enabled
                .or(frontmatter.metadata.mnemora.default_enabled)
                .unwrap_or(true),
            supported_modes,
            risk: sidecar.risk.unwrap_or(frontmatter.metadata.mnemora.risk),
            resource_cost: sidecar
                .resource_cost
                .unwrap_or(frontmatter.metadata.mnemora.resource_cost),
            triggers,
            argument_hint: sidecar.argument_hint.or(frontmatter.argument_hint),
            recommended_tools,
            required_tools,
            disable_model_invocation: frontmatter.disable_model_invocation,
            license: sidecar.license.or(frontmatter.license),
            compatibility: sidecar.compatibility.or(frontmatter.compatibility),
            provenance,
            content_hash,
        },
        markdown: normalized,
        body,
        directory: directory.to_path_buf(),
    })
}

fn read_sidecar(directory: &Path) -> Result<SkillSidecar, String> {
    let path = directory.join(MNEMORA_METADATA_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillSidecar::default())
        }
        Err(error) => return Err(format!("读取 {MNEMORA_METADATA_FILE} 失败：{error}")),
    };
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return Err(format!(
            "{MNEMORA_METADATA_FILE} 必须是 1 到 64 KB 的 JSON 文件。"
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("{MNEMORA_METADATA_FILE} 不是有效 JSON：{error}"))
}

fn normalized_modes(mut modes: Vec<SkillMode>) -> Vec<SkillMode> {
    modes.sort_by_key(|mode| match mode {
        SkillMode::Chat => 0,
        SkillMode::Work => 1,
        SkillMode::Notes => 2,
    });
    modes.dedup();
    modes
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

fn resolve_skill_id(
    directory: &Path,
    value: &SkillFrontmatter,
    sidecar: &SkillSidecar,
) -> Result<String, String> {
    if let Some(skill_id) = sidecar.id.as_deref() {
        validate_skill_id(skill_id)?;
        return Ok(skill_id.to_string());
    }
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

fn resolve_version(value: &SkillFrontmatter, sidecar: &SkillSidecar) -> Result<String, String> {
    let version = sidecar
        .version
        .clone()
        .or_else(|| value.version.as_ref().map(scalar_text))
        .or_else(|| value.metadata.version.as_ref().map(scalar_text))
        .unwrap_or_else(|| "unversioned".to_string());
    if version.trim().is_empty() || version.len() > 32 {
        return Err("Skill version 不能为空且不能超过 32 个字节。".to_string());
    }
    Ok(version)
}

fn scalar_text(value: &ScalarText) -> String {
    match value {
        ScalarText::Text(value) => value.clone(),
        ScalarText::Integer(value) => value.to_string(),
        ScalarText::Float(value) => value.to_string(),
    }
}

fn validate_frontmatter(value: &SkillFrontmatter, version: &str) -> Result<(), String> {
    if value.name.trim().is_empty() || value.name.chars().count() > 80 {
        return Err("Skill name 不能为空且不能超过 80 个字符。".to_string());
    }
    if value.description.trim().is_empty() || value.description.chars().count() > 4_000 {
        return Err("Skill description 不能为空且不能超过 4000 个字符。".to_string());
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

fn validate_sidecar(source: SkillSource, value: &SkillSidecar) -> Result<(), String> {
    if let Some(id) = value.id.as_deref() {
        validate_skill_id(id)?;
    }
    if value.triggers.len() > 16 {
        return Err("mnemora.json 不能声明超过 16 个触发词。".to_string());
    }
    if value.recommended_tools.len() > 32 || value.required_tools.len() > 32 {
        return Err("mnemora.json 不能声明超过 32 个工具。".to_string());
    }
    for tool in value
        .recommended_tools
        .iter()
        .chain(value.required_tools.iter())
    {
        validate_tool_name(tool)?;
    }
    validate_optional_metadata("license", value.license.as_deref(), 160)?;
    validate_optional_metadata("compatibility", value.compatibility.as_deref(), 1000)?;
    validate_optional_metadata(
        "source repository",
        value.provenance.repository.as_deref(),
        500,
    )?;
    validate_optional_metadata("source path", value.provenance.path.as_deref(), 500)?;
    validate_optional_metadata("source revision", value.provenance.revision.as_deref(), 128)?;
    validate_optional_metadata("attribution", value.provenance.attribution.as_deref(), 500)?;
    validate_optional_metadata(
        "adaptation notes",
        value.provenance.adaptation_notes.as_deref(),
        1000,
    )?;
    if let Some(repository) = value.provenance.repository.as_deref() {
        if !repository.starts_with("https://") {
            return Err("mnemora.json 的 source repository 必须使用 HTTPS。".to_string());
        }
        if source == SkillSource::Builtin {
            if value.license.as_deref().is_none_or(str::is_empty)
                || value.provenance.path.as_deref().is_none_or(str::is_empty)
            {
                return Err("带上游来源的内置 Skill 必须记录 license 和 source path。".to_string());
            }
            let revision = value
                .provenance
                .revision
                .as_deref()
                .ok_or_else(|| "内置 Skill 必须固定 source revision。".to_string())?;
            if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("内置 Skill 的 source revision 必须是 40 位 Commit SHA。".to_string());
            }
        }
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
    fn parses_mnemora_mode_risk_and_resource_metadata() {
        let root =
            std::env::temp_dir().join(format!("mnemora-skill-meta-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nid: work-skill\nname: Work Skill\ndescription: Work mode skill.\nversion: 1.0.0\nmetadata:\n  mnemora:\n    supported-modes: [work, notes, work]\n    risk: medium\n    resource-cost: high\n---\n正文\n",
        )
        .unwrap();
        let record = parse_skill(&root, SkillSource::User, true).unwrap();
        assert_eq!(
            record.summary.supported_modes,
            vec![
                crate::skills::types::SkillMode::Work,
                crate::skills::types::SkillMode::Notes,
            ]
        );
        assert_eq!(record.summary.risk, crate::skills::types::SkillRisk::Medium);
        assert_eq!(
            record.summary.resource_cost,
            crate::skills::types::SkillResourceCost::High
        );
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

    #[test]
    fn sidecar_adds_mnemora_discovery_without_rewriting_official_skill() {
        let root =
            std::env::temp_dir().join(format!("mnemora-official-skill-{}", uuid::Uuid::new_v4()));
        let skill_dir = root.join("official-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        let upstream = "---\nname: upstream-name\ndescription: Official upstream instructions.\nallowed-tools: Read Bash\n---\n# Original body\n";
        fs::write(skill_dir.join("SKILL.md"), upstream).unwrap();
        fs::write(
            skill_dir.join("mnemora.json"),
            r#"{
              "id": "official-skill",
              "version": "official-0123456",
              "defaultEnabled": false,
              "supportedModes": ["chat", "notes"],
              "triggers": ["/official"],
              "requiredTools": ["workspace_read"],
              "license": "MIT",
              "provenance": {
                "repository": "https://github.com/example/upstream",
                "path": "skills/official-skill/SKILL.md",
                "revision": "0123456789abcdef0123456789abcdef01234567",
                "adapted": false
              }
            }"#,
        )
        .unwrap();

        let record = parse_skill(&skill_dir, SkillSource::Builtin, true).unwrap();
        assert_eq!(record.markdown, upstream);
        assert_eq!(record.summary.name, "upstream-name");
        assert_eq!(record.summary.triggers, vec!["/official"]);
        assert_eq!(record.summary.required_tools, vec!["workspace_read"]);
        assert!(!record.summary.default_enabled);
        assert!(!record.summary.provenance.adapted);
        let _ = fs::remove_dir_all(root);
    }
}
