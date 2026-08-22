use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::library::types::{
    NoteEditProposal, NotePipelinePhase, NotePipelineRun, NotePipelineSectionStatus,
};

pub const MAX_DEEP_NOTE_SEMANTIC_CALLS: u32 = 640;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteSectionKind {
    Prerequisite,
    Concept,
    Comparison,
    Pitfall,
    Example,
    Summary,
    Selfcheck,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteSection {
    pub id: String,
    pub heading: String,
    pub kind: DeepNoteSectionKind,
    pub brief: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub evidence_requirements: Vec<String>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub source_scope: Vec<String>,
    #[serde(default = "default_target_depth")]
    pub target_depth: String,
    #[serde(default)]
    pub allow_ai_supplement: bool,
    #[serde(default)]
    pub needs_supplement: bool,
    #[serde(default)]
    pub source_message_ids: Vec<String>,
}

fn default_target_depth() -> String {
    "standard".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteOutline {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub audience: String,
    #[serde(default)]
    pub scope: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub weak_points: Vec<String>,
    #[serde(default)]
    pub hidden_questions: Vec<String>,
    #[serde(default)]
    pub knowledge_gaps: Vec<String>,
    #[serde(default)]
    pub misconceptions: Vec<String>,
    #[serde(default)]
    pub causal_chains: Vec<String>,
    #[serde(default)]
    pub visualization_opportunities: Vec<String>,
    #[serde(default)]
    pub allow_ai_supplement: bool,
    #[serde(default)]
    pub evidence_policy: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
    pub sections: Vec<DeepNoteSection>,
}

impl DeepNoteOutline {
    pub fn validate(mut self, valid_message_ids: &HashSet<String>) -> Result<Self, String> {
        self.title = self.title.trim().trim_start_matches('#').trim().to_string();
        if self.title.is_empty() || self.title.chars().count() > 500 {
            return Err("深度笔记标题为空或过长。".to_string());
        }
        self.summary = self.summary.trim().to_string();
        self.goal = self.goal.trim().to_string();
        self.audience = self.audience.trim().to_string();
        self.scope = self.scope.trim().to_string();
        self.evidence_policy = self.evidence_policy.trim().to_string();
        self.weak_points = normalize_string_list(&self.weak_points);
        self.hidden_questions = normalize_string_list(&self.hidden_questions);
        self.knowledge_gaps = normalize_string_list(&self.knowledge_gaps);
        self.misconceptions = normalize_string_list(&self.misconceptions);
        self.causal_chains = normalize_string_list(&self.causal_chains);
        self.visualization_opportunities = normalize_string_list(&self.visualization_opportunities);
        if self.sections.is_empty() || self.sections.len() > 40 {
            return Err("深度笔记提纲必须包含 1 到 40 个章节。".to_string());
        }
        let mut ids = HashSet::new();
        for section in &mut self.sections {
            section.id = section.id.trim().to_string();
            section.heading = section
                .heading
                .trim()
                .trim_start_matches('#')
                .trim()
                .to_string();
            section.brief = section.brief.trim().to_string();
            section.purpose = section.purpose.trim().to_string();
            if section.purpose.is_empty() {
                section.purpose = section.brief.clone();
            }
            if section.id.is_empty() || section.heading.is_empty() || section.brief.is_empty() {
                return Err("深度笔记章节缺少 id、heading 或 brief。".to_string());
            }
            if !ids.insert(section.id.clone()) {
                return Err(format!("深度笔记提纲包含重复章节 ID：{}。", section.id));
            }
            section
                .source_message_ids
                .retain(|message_id| valid_message_ids.contains(message_id));
            section.source_message_ids.sort();
            section.source_message_ids.dedup();
            section.depends_on = normalize_string_list(&section.depends_on);
            section.evidence_requirements = normalize_string_list(&section.evidence_requirements);
            section.success_criteria = normalize_string_list(&section.success_criteria);
            section.source_scope = normalize_string_list(&section.source_scope);
            if section.success_criteria.is_empty() {
                section
                    .success_criteria
                    .push(format!("完整说明{}，并与笔记目标一致", section.heading));
            }
            if section.target_depth.trim().is_empty() {
                section.target_depth = default_target_depth();
            }
            section.target_depth = section.target_depth.trim().to_string();
            if section.needs_supplement {
                section.allow_ai_supplement = true;
            }
        }
        for section in &self.sections {
            for dependency in &section.depends_on {
                if dependency == &section.id || !ids.contains(dependency) {
                    return Err(format!(
                        "章节“{}”包含无效依赖：{dependency}。",
                        section.heading
                    ));
                }
            }
        }
        validate_section_dag(&self.sections)?;
        self.source_ids = normalize_string_list(&self.source_ids);
        Ok(self)
    }

    pub fn select(&self, selected: &HashSet<String>) -> Result<Self, String> {
        let mut outline = self.clone();
        outline
            .sections
            .retain(|section| selected.contains(&section.id));
        if outline.sections.is_empty() {
            return Err("请至少保留一个章节。".to_string());
        }
        Ok(outline)
    }
}

fn normalize_string_list(values: &[String]) -> Vec<String> {
    let mut result = values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    result.sort();
    result.dedup();
    result
}

fn validate_section_dag(sections: &[DeepNoteSection]) -> Result<(), String> {
    let mut indegree = sections
        .iter()
        .map(|section| (section.id.clone(), section.depends_on.len()))
        .collect::<HashMap<_, _>>();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for section in sections {
        for dependency in &section.depends_on {
            dependents
                .entry(dependency.as_str())
                .or_default()
                .push(section.id.as_str());
        }
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect::<VecDeque<_>>();
    let mut visited = 0usize;
    while let Some(id) = ready.pop_front() {
        visited += 1;
        for dependent in dependents.get(id.as_str()).into_iter().flatten() {
            if let Some(count) = indegree.get_mut(*dependent) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.push_back((*dependent).to_string());
                }
            }
        }
    }
    if visited != sections.len() {
        return Err("深度笔记章节依赖存在循环。".to_string());
    }
    Ok(())
}

fn stable_hash(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteCapabilities {
    #[serde(default)]
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    pub reasoning: Option<bool>,
    pub structured_outputs: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteModelSnapshot {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub api_model: String,
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    pub capabilities: DeepNoteCapabilities,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteInputSnapshot {
    pub conversation_revision: u64,
    pub message_ids: Vec<String>,
    #[serde(default)]
    pub message_content_hashes: Vec<String>,
    pub attachment_ids: Vec<String>,
    pub attachment_content_hashes: Vec<String>,
    #[serde(default)]
    pub attachment_message_ids: Vec<String>,
    pub selected_literature_ids: Vec<String>,
    pub selected_note_ids: Vec<String>,
    pub model: DeepNoteModelSnapshot,
    pub permission_mode: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteSourceUnitKind {
    Body,
    Attachment,
    LiteratureSelection,
    NoteSelection,
}

impl DeepNoteSourceUnitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::Attachment => "attachment",
            Self::LiteratureSelection => "literatureSelection",
            Self::NoteSelection => "noteSelection",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "body" => Ok(Self::Body),
            "attachment" => Ok(Self::Attachment),
            "literatureSelection" | "literature_selection" => Ok(Self::LiteratureSelection),
            "noteSelection" | "note_selection" => Ok(Self::NoteSelection),
            _ => Err(format!("未知的深度笔记来源单元类型：{value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteSourceUnitStatus {
    Pending,
    Extracted,
    Covered,
    Failed,
    Unsupported,
}

impl DeepNoteSourceUnitStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Extracted => "extracted",
            Self::Covered => "covered",
            Self::Failed => "failed",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "extracted" => Ok(Self::Extracted),
            "covered" => Ok(Self::Covered),
            "failed" => Ok(Self::Failed),
            "unsupported" => Ok(Self::Unsupported),
            _ => Err(format!("未知的深度笔记来源单元状态：{value}")),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteSourceUnit {
    pub unit_id: String,
    pub note_id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub kind: DeepNoteSourceUnitKind,
    pub attachment_id: Option<String>,
    pub content_hash: String,
    pub parser_id: String,
    pub parser_version: String,
    pub status: DeepNoteSourceUnitStatus,
    pub chunk_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub error_message: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteSourceKind {
    Conversation,
    Text,
    Code,
    Pdf,
    Docx,
    Xlsx,
    Image,
    Literature,
    Note,
}

impl DeepNoteSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Text => "text",
            Self::Code => "code",
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Image => "image",
            Self::Literature => "literature",
            Self::Note => "note",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "conversation" => Ok(Self::Conversation),
            "text" => Ok(Self::Text),
            "code" => Ok(Self::Code),
            "pdf" => Ok(Self::Pdf),
            "docx" => Ok(Self::Docx),
            "xlsx" => Ok(Self::Xlsx),
            "image" => Ok(Self::Image),
            "literature" => Ok(Self::Literature),
            "note" => Ok(Self::Note),
            _ => Err(format!("未知的深度笔记来源类型：{value}")),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteSourceChunk {
    pub chunk_id: String,
    pub source_kind: DeepNoteSourceKind,
    pub source_id: String,
    pub message_id: Option<String>,
    pub attachment_id: Option<String>,
    pub library_item_id: Option<String>,
    pub location: String,
    pub excerpt: String,
    pub content_hash: String,
    pub ocr_confidence: Option<f32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteEvidenceStatus {
    Verified,
    Conflicting,
    Insufficient,
    Invalidated,
}

impl DeepNoteEvidenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Conflicting => "conflicting",
            Self::Insufficient => "insufficient",
            Self::Invalidated => "invalidated",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "verified" => Ok(Self::Verified),
            "conflicting" => Ok(Self::Conflicting),
            "insufficient" => Ok(Self::Insufficient),
            "invalidated" => Ok(Self::Invalidated),
            _ => Err(format!("未知的深度笔记证据状态：{value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteSupportLevel {
    Direct,
    Partial,
    Context,
    AiSupplement,
}

impl DeepNoteSupportLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Partial => "partial",
            Self::Context => "context",
            Self::AiSupplement => "aiSupplement",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "direct" => Ok(Self::Direct),
            "partial" => Ok(Self::Partial),
            "context" => Ok(Self::Context),
            "aiSupplement" | "ai_supplement" => Ok(Self::AiSupplement),
            _ => Err(format!("未知的深度笔记证据支持级别：{value}")),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteEvidenceArtifact {
    pub evidence_id: String,
    pub section_id: String,
    pub source_chunk_ids: Vec<String>,
    pub claim: String,
    pub model_synthesis: String,
    pub source_excerpt: String,
    pub support_level: DeepNoteSupportLevel,
    pub status: DeepNoteEvidenceStatus,
    pub content_hash: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteLedger {
    pub note_goal: String,
    pub audience: String,
    pub canonical_terms: Vec<String>,
    pub verified_facts: Vec<String>,
    pub evidence_claim_links: Vec<String>,
    pub covered_topics: Vec<String>,
    pub open_questions: Vec<String>,
    pub conflicts: Vec<String>,
    pub ai_supplements: Vec<String>,
    pub section_summaries: Vec<String>,
    pub global_constraints: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteLocalReaderCapabilities {
    pub text: bool,
    pub pdf: bool,
    pub docx: bool,
    pub xlsx: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteSkillProfileKind {
    Planner,
    Writer,
    Reviewer,
}

impl DeepNoteSkillProfileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Writer => "writer",
            Self::Reviewer => "reviewer",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteSkillSnapshot {
    pub profile: DeepNoteSkillProfileKind,
    pub skill_id: String,
    pub name: String,
    pub version: String,
    pub content_hash: String,
    pub rendered_prompt: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteSkillProfiles {
    pub planner: Vec<DeepNoteSkillSnapshot>,
    pub writer: Vec<DeepNoteSkillSnapshot>,
    pub reviewer: Vec<DeepNoteSkillSnapshot>,
}

impl DeepNoteSkillProfiles {
    pub fn for_profile(&self, profile: DeepNoteSkillProfileKind) -> &[DeepNoteSkillSnapshot] {
        match profile {
            DeepNoteSkillProfileKind::Planner => &self.planner,
            DeepNoteSkillProfileKind::Writer => &self.writer,
            DeepNoteSkillProfileKind::Reviewer => &self.reviewer,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteContextBudget {
    pub context_window_tokens: Option<u64>,
    pub estimated_input_tokens: u64,
    pub planner_output_reserve_tokens: u64,
    pub prompt_overhead_tokens: u64,
    pub safety_margin_tokens: u64,
    pub usable_input_tokens: u64,
    pub direct_input_limit_tokens: u64,
    pub chunk_target_tokens: u64,
    pub chunk_count: usize,
    pub processed_chunk_count: usize,
    pub total_message_count: usize,
    pub processed_message_count: usize,
    pub coverage_complete: bool,
    pub omitted_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteNodeType {
    AnalyzeInput,
    ReconSource,
    ExtractEvidence,
    BuildLedger,
    DraftSection,
    ValidateSection,
    ReviewSection,
    ReviseSection,
    ValidateGlobal,
    ApplyPatch,
    AssembleNote,
    PersistNote,
}

impl DeepNoteNodeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AnalyzeInput => "analyzeInput",
            Self::ReconSource => "reconSource",
            Self::ExtractEvidence => "extractEvidence",
            Self::BuildLedger => "buildLedger",
            Self::DraftSection => "draftSection",
            Self::ValidateSection => "validateSection",
            Self::ReviewSection => "reviewSection",
            Self::ReviseSection => "reviseSection",
            Self::ValidateGlobal => "validateGlobal",
            Self::ApplyPatch => "applyPatch",
            Self::AssembleNote => "assembleNote",
            Self::PersistNote => "persistNote",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "analyzeInput" => Ok(Self::AnalyzeInput),
            "reconSource" => Ok(Self::ReconSource),
            "extractEvidence" => Ok(Self::ExtractEvidence),
            "buildLedger" => Ok(Self::BuildLedger),
            "draftSection" => Ok(Self::DraftSection),
            "validateSection" => Ok(Self::ValidateSection),
            "reviewSection" => Ok(Self::ReviewSection),
            "reviseSection" => Ok(Self::ReviseSection),
            "validateGlobal" => Ok(Self::ValidateGlobal),
            "applyPatch" => Ok(Self::ApplyPatch),
            "assembleNote" => Ok(Self::AssembleNote),
            "persistNote" => Ok(Self::PersistNote),
            _ => Err(format!("未知的深度笔记执行节点类型：{value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeepNoteNodeStatus {
    Pending,
    Ready,
    InProgress,
    Completed,
    NeedsReview,
    NeedsRevision,
    Failed,
    Blocked,
    Skipped,
    Interrupted,
}

impl DeepNoteNodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::InProgress => "inProgress",
            Self::Completed => "completed",
            Self::NeedsReview => "needsReview",
            Self::NeedsRevision => "needsRevision",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Skipped => "skipped",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "inProgress" | "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "needsReview" | "needs_review" => Ok(Self::NeedsReview),
            "needsRevision" | "needs_revision" => Ok(Self::NeedsRevision),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            "skipped" => Ok(Self::Skipped),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(format!("未知的深度笔记执行节点状态：{value}")),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteDagNode {
    pub node_id: String,
    pub node_type: DeepNoteNodeType,
    pub section_id: Option<String>,
    pub depends_on: Vec<String>,
    pub status: DeepNoteNodeStatus,
    pub attempt_count: u8,
    pub evidence_ids: Vec<String>,
    pub input_hash: String,
    pub output_ref: Option<String>,
    pub validation_json: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteBudget {
    pub semantic_call_limit: u32,
    pub semantic_calls_used: u32,
    pub node_attempt_limit: u8,
    pub section_revision_limit: u8,
    pub replan_limit: u8,
    pub replans_used: u8,
    pub max_parallel_nodes: u8,
}

impl DeepNoteBudget {
    pub fn for_section_count(section_count: usize) -> Self {
        let node_attempt_limit = 5;
        let section_revision_limit = 5;
        let per_section_calls = u32::from(node_attempt_limit) + u32::from(section_revision_limit);
        Self {
            // Four calls cover visual-source/planner setup and fallback paths. Each
            // selected section then receives enough budget for every bounded fresh
            // draft attempt plus every bounded semantic revision.
            semantic_call_limit: (4 + section_count as u32 * per_section_calls)
                .min(MAX_DEEP_NOTE_SEMANTIC_CALLS),
            semantic_calls_used: 0,
            node_attempt_limit,
            section_revision_limit,
            replan_limit: 4,
            replans_used: 0,
            max_parallel_nodes: 2,
        }
    }

    pub fn reserve_semantic_calls(&mut self, additional_calls: u32) {
        self.semantic_call_limit = self.semantic_call_limit.max(
            self.semantic_calls_used
                .saturating_add(additional_calls)
                .min(MAX_DEEP_NOTE_SEMANTIC_CALLS),
        );
    }
}

#[cfg(test)]
mod budget_tests {
    use super::{DeepNoteBudget, MAX_DEEP_NOTE_SEMANTIC_CALLS};

    #[test]
    fn drafting_budget_covers_every_bounded_attempt_and_revision() {
        let budget = DeepNoteBudget::for_section_count(8);
        let required = 4 + 8
            * (u32::from(budget.node_attempt_limit) + u32::from(budget.section_revision_limit));
        assert!(budget.semantic_call_limit >= required);
    }

    #[test]
    fn semantic_reservation_is_relative_to_calls_already_used() {
        let mut budget = DeepNoteBudget::for_section_count(1);
        budget.semantic_calls_used = 50;
        budget.reserve_semantic_calls(24);
        assert_eq!(budget.semantic_call_limit, 74);

        budget.semantic_calls_used = MAX_DEEP_NOTE_SEMANTIC_CALLS - 2;
        budget.reserve_semantic_calls(24);
        assert_eq!(budget.semantic_call_limit, MAX_DEEP_NOTE_SEMANTIC_CALLS);
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteValidationReport {
    pub passed: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub checked_evidence_ids: Vec<String>,
    pub criteria_coverage: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNotePlanVersion {
    pub run_id: String,
    pub plan_id: String,
    pub version: u32,
    pub plan: DeepNoteOutline,
    pub compiled_dag: Vec<DeepNoteDagNode>,
    pub plan_hash: String,
    pub revision_reason: String,
    pub confirmed_at: Option<u64>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNotePreflight {
    pub ready: bool,
    pub model: DeepNoteModelSnapshot,
    pub requires_tools: bool,
    pub requires_local_readers: bool,
    pub requires_vision: bool,
    pub local_readers: DeepNoteLocalReaderCapabilities,
    pub missing_capabilities: Vec<String>,
    pub warnings: Vec<String>,
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteEventRecord {
    pub sequence: u64,
    pub event_type: String,
    pub node_id: Option<String>,
    pub payload_json: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineActivity {
    pub kind: String,
    pub call_id: String,
    pub operation: String,
    pub attempt: u8,
    pub max_retries: u8,
    pub started_at: u64,
    pub timeout_ms: u64,
    pub delay_ms: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteRunDetail {
    pub run: NotePipelineRun,
    pub preflight: Option<DeepNotePreflight>,
    pub input_snapshot: Option<DeepNoteInputSnapshot>,
    pub plan_version: Option<DeepNotePlanVersion>,
    pub budget: DeepNoteBudget,
    pub context_budget: DeepNoteContextBudget,
    pub source_chunk_count: usize,
    pub nodes: Vec<DeepNoteDagNode>,
    pub sections: Vec<DeepNoteSectionProgress>,
    pub source_chunks: Vec<DeepNoteSourceChunk>,
    pub evidence: Vec<DeepNoteEvidenceArtifact>,
    pub ledger: DeepNoteLedger,
    pub skill_profiles: DeepNoteSkillProfiles,
    pub events: Vec<DeepNoteEventRecord>,
    pub markdown_preview: String,
    pub sidecar_json: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteSectionProgress {
    pub section_id: String,
    pub position: usize,
    pub status: NotePipelineSectionStatus,
    pub attempt_count: u8,
    pub revision_count: u8,
    pub error_message: Option<String>,
    pub markdown_chars: usize,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteRuntimeState {
    pub preflight: DeepNotePreflight,
    pub input_snapshot: DeepNoteInputSnapshot,
    pub plan_version: Option<DeepNotePlanVersion>,
    pub budget: DeepNoteBudget,
    pub ledger: DeepNoteLedger,
    #[serde(default)]
    pub skill_profiles: DeepNoteSkillProfiles,
    #[serde(default)]
    pub context_budget: DeepNoteContextBudget,
    #[serde(default)]
    pub force_rebuild: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepNoteStartInspection {
    pub status: String,
    pub note_id: Option<String>,
    pub note_title: Option<String>,
    pub covered_message_id: Option<String>,
    pub covered_message_count: usize,
    pub new_message_count: usize,
    pub new_attachment_count: usize,
    pub requires_full_rebuild: bool,
    pub unsupported_attachment_names: Vec<String>,
    pub message: String,
}

pub fn compile_plan(
    run_id: &str,
    version: u32,
    plan: DeepNoteOutline,
    input_snapshot_hash: &str,
    revision_reason: &str,
) -> Result<DeepNotePlanVersion, String> {
    validate_section_dag(&plan.sections)?;
    let plan_json =
        serde_json::to_string(&plan).map_err(|error| format!("序列化深度笔记计划失败：{error}"))?;
    let plan_hash = stable_hash(&plan_json);
    let plan_id = format!("plan-{}", &plan_hash[..16]);
    let mut nodes = Vec::new();
    nodes.push(dag_node(
        "analyze-input",
        DeepNoteNodeType::AnalyzeInput,
        None,
        Vec::new(),
        input_snapshot_hash,
    ));
    nodes.push(dag_node(
        "recon-source",
        DeepNoteNodeType::ReconSource,
        None,
        vec!["analyze-input".to_string()],
        input_snapshot_hash,
    ));
    for section in &plan.sections {
        let evidence_id = format!("evidence:{}", section.id);
        nodes.push(dag_node(
            &evidence_id,
            DeepNoteNodeType::ExtractEvidence,
            Some(section.id.clone()),
            vec!["recon-source".to_string()],
            input_snapshot_hash,
        ));
    }
    let evidence_nodes = plan
        .sections
        .iter()
        .map(|section| format!("evidence:{}", section.id))
        .collect::<Vec<_>>();
    nodes.push(dag_node(
        "build-ledger",
        DeepNoteNodeType::BuildLedger,
        None,
        evidence_nodes,
        input_snapshot_hash,
    ));
    for section in &plan.sections {
        let mut draft_dependencies = vec!["build-ledger".to_string()];
        draft_dependencies.extend(
            section
                .depends_on
                .iter()
                .map(|dependency| format!("validate:{dependency}")),
        );
        let draft_id = format!("draft:{}", section.id);
        let validate_id = format!("validate:{}", section.id);
        nodes.push(dag_node(
            &draft_id,
            DeepNoteNodeType::DraftSection,
            Some(section.id.clone()),
            draft_dependencies,
            input_snapshot_hash,
        ));
        nodes.push(dag_node(
            &validate_id,
            DeepNoteNodeType::ValidateSection,
            Some(section.id.clone()),
            vec![draft_id],
            input_snapshot_hash,
        ));
    }
    let section_validations = plan
        .sections
        .iter()
        .map(|section| format!("validate:{}", section.id))
        .collect::<Vec<_>>();
    nodes.push(dag_node(
        "validate-global",
        DeepNoteNodeType::ValidateGlobal,
        None,
        section_validations,
        input_snapshot_hash,
    ));
    nodes.push(dag_node(
        "assemble-note",
        DeepNoteNodeType::AssembleNote,
        None,
        vec!["validate-global".to_string()],
        input_snapshot_hash,
    ));
    nodes.push(dag_node(
        "persist-note",
        DeepNoteNodeType::PersistNote,
        None,
        vec!["assemble-note".to_string()],
        input_snapshot_hash,
    ));
    validate_compiled_dag(&nodes)?;
    Ok(DeepNotePlanVersion {
        run_id: run_id.to_string(),
        plan_id,
        version,
        plan,
        compiled_dag: nodes,
        plan_hash,
        revision_reason: revision_reason.trim().to_string(),
        confirmed_at: None,
        created_at: 0,
    })
}

fn dag_node(
    node_id: &str,
    node_type: DeepNoteNodeType,
    section_id: Option<String>,
    depends_on: Vec<String>,
    input_snapshot_hash: &str,
) -> DeepNoteDagNode {
    DeepNoteDagNode {
        node_id: node_id.to_string(),
        node_type,
        section_id,
        status: if depends_on.is_empty() {
            DeepNoteNodeStatus::Ready
        } else {
            DeepNoteNodeStatus::Pending
        },
        depends_on,
        attempt_count: 0,
        evidence_ids: Vec::new(),
        input_hash: stable_hash(format!("{input_snapshot_hash}:{node_id}")),
        output_ref: None,
        validation_json: String::new(),
        error_message: None,
    }
}

fn validate_compiled_dag(nodes: &[DeepNoteDagNode]) -> Result<(), String> {
    let ids = nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<HashSet<_>>();
    if ids.len() != nodes.len() {
        return Err("编译后的深度笔记 DAG 包含重复节点 ID。".to_string());
    }
    let pseudo_sections = nodes
        .iter()
        .map(|node| DeepNoteSection {
            id: node.node_id.clone(),
            heading: node.node_id.clone(),
            kind: DeepNoteSectionKind::Concept,
            brief: node.node_id.clone(),
            purpose: String::new(),
            depends_on: node.depends_on.clone(),
            evidence_requirements: Vec::new(),
            success_criteria: Vec::new(),
            source_scope: Vec::new(),
            target_depth: default_target_depth(),
            allow_ai_supplement: false,
            needs_supplement: false,
            source_message_ids: Vec::new(),
        })
        .collect::<Vec<_>>();
    if pseudo_sections
        .iter()
        .flat_map(|node| node.depends_on.iter())
        .any(|dependency| !ids.contains(dependency.as_str()))
    {
        return Err("编译后的深度笔记 DAG 包含悬空依赖。".to_string());
    }
    validate_section_dag(&pseudo_sections)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineStartRequest {
    pub conversation_id: String,
    #[serde(default)]
    pub replace_invalidated: bool,
    #[serde(default)]
    pub force_rebuild: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineConfirmRequest {
    pub run_id: String,
    pub selected_section_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineAdjustRequest {
    pub run_id: String,
    pub requirement: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NotePipelineProgress {
    Progress {
        run_id: String,
        phase: NotePipelinePhase,
        current: Option<usize>,
        total: Option<usize>,
        message: String,
        activity: Option<NotePipelineActivity>,
    },
    OutlineReady {
        run: NotePipelineRun,
    },
    Done {
        run: NotePipelineRun,
        degraded: bool,
    },
    Paused {
        run: NotePipelineRun,
    },
    Cancelled {
        run: NotePipelineRun,
    },
    Error {
        run_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEditPrepareRequest {
    pub note_id: String,
    pub conversation_id: String,
    #[serde(default)]
    pub selected_text: String,
    #[serde(default)]
    pub section_heading: String,
    #[serde(default)]
    pub requirement: String,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotePatchAction {
    AddSection,
    AppendToSection,
    ReplaceSection,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMergePlanItem {
    pub action: NotePatchAction,
    #[serde(default)]
    pub target_heading: String,
    pub heading: String,
    pub brief: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMergePlan {
    #[serde(default)]
    pub title: String,
    pub operations: Vec<NoteMergePlanItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatch {
    pub action: NotePatchAction,
    #[serde(default)]
    pub target_heading: String,
    pub heading: String,
    pub markdown: String,
    #[serde(default)]
    pub needs_supplement: bool,
    #[serde(default)]
    pub source_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatchSet {
    #[serde(default)]
    pub title: String,
    pub patches: Vec<NotePatch>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEditPrepareResult {
    pub proposal: NoteEditProposal,
    pub warnings: Vec<String>,
    pub source_units: Vec<DeepNoteSourceUnit>,
    pub attachment_count: usize,
    pub requires_global_review: bool,
    pub global_review_passed: bool,
}
