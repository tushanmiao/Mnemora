//! Skill 模块的公开数据合同和内部状态结构。

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    Builtin,
    User,
}

/// Skill 可以出现在哪些工作模式中。模式只影响目录暴露，不代表 Skill 已经被加载。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillMode {
    Chat,
    Work,
    Notes,
}

impl SkillMode {
    pub const fn all() -> [Self; 3] {
        [Self::Chat, Self::Work, Self::Notes]
    }
}

/// Skill 的风险等级，用于决定是否允许模型自动激活以及设置页的提示。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillRisk {
    #[default]
    Low,
    Medium,
    High,
}

/// Skill 的预估资源成本。它不是运行时内存测量，而是加载前的轻量预算提示。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillResourceCost {
    #[default]
    Low,
    Medium,
    High,
}

/// Skill 的上游来源信息只用于审计和界面展示，不会注入模型上下文。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillProvenance {
    pub repository: Option<String>,
    pub path: Option<String>,
    pub revision: Option<String>,
    pub attribution: Option<String>,
    pub adapted: bool,
    pub adaptation_notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillFileKind {
    SkillMd,
    Reference,
    Script,
    Asset,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileEntry {
    pub path: String,
    pub kind: SkillFileKind,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResourceRead {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub size_bytes: u64,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub source: SkillSource,
    pub enabled: bool,
    /// 内置 Skill 首次发现时是否默认启用；用户仍可在设置中修改。
    pub default_enabled: bool,
    pub supported_modes: Vec<SkillMode>,
    #[serde(default)]
    pub risk: SkillRisk,
    #[serde(default)]
    pub resource_cost: SkillResourceCost,
    pub triggers: Vec<String>,
    pub argument_hint: Option<String>,
    pub recommended_tools: Vec<String>,
    pub required_tools: Vec<String>,
    pub disable_model_invocation: bool,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub provenance: SkillProvenance,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetail {
    #[serde(flatten)]
    pub summary: SkillSummary,
    pub markdown: String,
    pub files: Vec<SkillFileEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillListResult {
    pub skills: Vec<SkillSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillImportKind {
    Directory,
    Zip,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillImportRequest {
    pub path: String,
    pub kind: SkillImportKind,
    #[serde(default)]
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillImportStatus {
    Installed,
    AlreadyExists,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillImportResult {
    pub status: SkillImportStatus,
    pub skill: SkillSummary,
}

#[derive(Debug, Clone)]
pub(crate) struct SkillRecord {
    pub summary: SkillSummary,
    pub markdown: String,
    pub body: String,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillStateFile {
    pub version: u32,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillStateEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillStateEntry {
    pub enabled: bool,
}
