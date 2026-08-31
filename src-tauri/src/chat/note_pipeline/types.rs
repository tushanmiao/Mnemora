use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::library::types::{
    NoteEditProposal, NotePipelinePhase, NotePipelineRun, NotePipelineSectionStatus,
};

pub const MAX_DEEP_NOTE_SEMANTIC_CALLS: u32 = 640;
pub const MAX_DEEP_NOTE_UPSTREAM_REQUESTS: u32 = 640;
/// 单次 DeepNote 可以完整覆盖的来源 Chunk 上限；批量文件入口必须与它对齐，
/// 不能先接受 100 个文件、再在第 97 个处失败或丢弃。
pub const MAX_DEEP_NOTE_SOURCE_CHUNKS: usize = 96;

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
    /// 当前路由控制器施加的单次输入上限；与上下文窗口和静态安全上限三重取最小。
    #[serde(default)]
    pub adaptive_chunk_limit_tokens: u64,
    #[serde(default)]
    pub adaptive_route_key: String,
    #[serde(default)]
    pub adaptive_route_state: String,
    #[serde(default)]
    pub adaptive_profile_samples: u64,
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
    Superseded,
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
            Self::Superseded => "superseded",
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
            "superseded" => Ok(Self::Superseded),
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
    /// 逻辑模型调用的规划估算，只用于诊断节点/修订是否异常放大。
    ///
    /// P1 起不再把它当成 provider 预算：一次逻辑调用可能包含普通重试与流式回落，
    /// 中转站实际承受的请求数由 `upstream_request_*` 两个字段约束。
    pub semantic_call_limit: u32,
    pub semantic_calls_used: u32,
    /// 真正发到 provider 的物理 HTTP 请求上限。
    #[serde(default = "default_upstream_request_limit")]
    pub upstream_request_limit: u32,
    /// 已原子放行的物理 HTTP 请求数。权威来源是 `modelAttemptStarted` 事件。
    #[serde(default)]
    pub upstream_requests_used: u32,
    pub node_attempt_limit: u8,
    pub section_revision_limit: u8,
    pub replan_limit: u8,
    pub replans_used: u8,
    pub max_parallel_nodes: u8,
    #[serde(default = "default_max_parallel_chunks")]
    pub max_parallel_chunks: u8,
    /// 单个 section 的累计墙钟上限（毫秒）。
    ///
    /// 与 `semantic_call_limit` 是两个维度，缺一不可：请求数管住「调用了多少次」，
    /// 墙钟管住「花了多少时间」。中转站慢下来的时候请求数远没到上限，但时间已经
    /// 烧完了，只看请求数的预算模型此时完全失效。
    #[serde(default = "default_section_wall_clock_ms")]
    pub section_wall_clock_ms: u64,
    /// 整个 run 的累计墙钟上限（毫秒）。
    ///
    /// 到点做**部分交付**而不是整体失败：已完成的 section 是有价值的产出，
    /// 把它们连同未完成标记一起交出去，比丢掉全部重来对用户更有用。
    #[serde(default = "default_run_wall_clock_ms")]
    pub run_wall_clock_ms: u64,
    /// 已累计的上游等待时长（毫秒），从事件表里各次调用的 `durationMs` **求和**。
    ///
    /// 注意这是「累计调用耗时」而非真实经过时间：并发为 2 时，两路各 5 分钟会被
    /// 算成 10 分钟。所以 `run_wall_clock_ms` 的 90 分钟在满并发下约等于 45 分钟
    /// 真实时长 —— 这是刻意的，预算要管住的是**上游总消耗**，并发越高单位时间
    /// 烧掉的上游配额越多。
    ///
    /// 不用 `now - created_at` 代替：run 可以被暂停、可以排队等并发位，那些时间
    /// 不是上游造成的，算进来会让预算在系统空闲时也被吃掉。
    #[serde(default)]
    pub upstream_wall_clock_ms: u64,
}

fn default_max_parallel_chunks() -> u8 {
    2
}

fn default_upstream_request_limit() -> u32 {
    MAX_DEEP_NOTE_UPSTREAM_REQUESTS
}

/// 单 section 15 分钟。
///
/// 这个值刻意**小于** `node_attempt_limit`（5）乘单次 attempt 超时（起草档 7 分钟）
/// 的乘积（35 分钟）。也就是说上游一旦变慢，是墙钟先响而不是尝试次数先耗尽 ——
/// 这正是想要的：次数管的是「反复失败」，时间管的是「反复变慢」，后者在中转站
/// 场景下才是常态。上游正常时一个 section 三五次调用远用不到 15 分钟，闸门不会误伤。
fn default_section_wall_clock_ms() -> u64 {
    15 * 60 * 1000
}

/// 整个 run 90 分钟。超过这个时长用户早就离开了界面，继续跑下去的价值低于
/// 立刻交付已完成部分。
fn default_run_wall_clock_ms() -> u64 {
    90 * 60 * 1000
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
            upstream_request_limit: default_upstream_request_limit(),
            upstream_requests_used: 0,
            node_attempt_limit,
            section_revision_limit,
            replan_limit: 4,
            replans_used: 0,
            max_parallel_nodes: 2,
            max_parallel_chunks: default_max_parallel_chunks(),
            section_wall_clock_ms: default_section_wall_clock_ms(),
            run_wall_clock_ms: default_run_wall_clock_ms(),
            upstream_wall_clock_ms: 0,
        }
    }

    /// 累加一次上游等待时长。
    ///
    /// 生产路径不走这里 —— 真实通路是从事件表汇总后整体赋值（见
    /// `service::refresh_run_budget`），因为并行 section 持有的是 runtime 快照
    /// 而非 `&mut`，增量只能落在事件表里。保留这个方法是为了让预算的耗尽语义
    /// 能被单测独立验证，不必拼一个 AppState 和一张事件表。
    ///
    /// 饱和加法：墙钟只用来判断「是否超预算」，溢出回绕会让一个已经严重超时的 run
    /// 看起来毫无消耗，那是最坏的失效方向。
    #[allow(dead_code)]
    pub fn record_upstream_wall_clock(&mut self, elapsed_ms: u64) {
        self.upstream_wall_clock_ms = self.upstream_wall_clock_ms.saturating_add(elapsed_ms);
    }

    /// run 级墙钟是否已耗尽。
    ///
    /// 取 `>=`：正好等于上限就该停了，再放一次调用进去必然超。
    pub fn run_wall_clock_exhausted(&self) -> bool {
        self.upstream_wall_clock_ms >= self.run_wall_clock_ms
    }

    /// 某个 section 的累计墙钟是否已耗尽。
    ///
    /// 入参是该 section 的**累计活跃时长**（由 `DeepNoteRuntimeState::section_active_ms`
    /// 维护），不是「现在减去开始时刻」—— 后者会把暂停与关机的时间算进预算。
    /// 这里只做纯比较，方便单测不依赖系统时钟。
    pub fn section_wall_clock_exhausted(&self, section_active_ms: u64) -> bool {
        section_active_ms >= self.section_wall_clock_ms
    }

    pub fn upstream_request_exhausted(&self) -> bool {
        self.upstream_requests_used >= self.upstream_request_limit
    }
}

#[cfg(test)]
mod budget_tests {
    use super::{DeepNoteBudget, MAX_DEEP_NOTE_UPSTREAM_REQUESTS};

    #[test]
    fn drafting_budget_covers_every_bounded_attempt_and_revision() {
        let budget = DeepNoteBudget::for_section_count(8);
        let required = 4 + 8
            * (u32::from(budget.node_attempt_limit) + u32::from(budget.section_revision_limit));
        assert!(budget.semantic_call_limit >= required);
    }

    #[test]
    fn upstream_request_limit_is_consumed_instead_of_raised() {
        let mut budget = DeepNoteBudget::for_section_count(1);
        assert_eq!(
            budget.upstream_request_limit,
            MAX_DEEP_NOTE_UPSTREAM_REQUESTS
        );
        budget.upstream_requests_used = budget.upstream_request_limit - 1;
        assert!(!budget.upstream_request_exhausted());
        budget.upstream_requests_used += 1;
        assert!(budget.upstream_request_exhausted());
        assert_eq!(
            budget.upstream_request_limit, MAX_DEEP_NOTE_UPSTREAM_REQUESTS,
            "消耗预算不能反过来抬高上限"
        );
    }

    /// 墙钟和请求数是两个独立维度。中转站慢下来的时候请求数远没到上限、时间已经
    /// 烧完了，只看请求数的预算模型此时完全失效 —— 这个测试锁住「请求数充足也能
    /// 因为墙钟耗尽而停下」。
    #[test]
    fn wall_clock_is_budgeted_independently_of_request_count() {
        let mut budget = DeepNoteBudget::for_section_count(4);
        assert!(!budget.run_wall_clock_exhausted());
        budget.record_upstream_wall_clock(budget.run_wall_clock_ms - 1);
        assert!(
            !budget.run_wall_clock_exhausted(),
            "还差 1 毫秒不该判定耗尽"
        );
        budget.record_upstream_wall_clock(1);
        assert!(
            budget.run_wall_clock_exhausted(),
            "正好达到上限就该停：再放一次调用进去必然超"
        );
        assert_eq!(
            budget.semantic_calls_used, 0,
            "墙钟耗尽与请求数用量无关，两者不应互相影响"
        );
    }

    /// 累加用饱和加法：溢出回绕会让一个严重超时的 run 看起来毫无消耗，
    /// 那是最坏的失效方向。
    #[test]
    fn wall_clock_accumulation_saturates_instead_of_wrapping() {
        let mut budget = DeepNoteBudget::for_section_count(1);
        budget.record_upstream_wall_clock(u64::MAX);
        budget.record_upstream_wall_clock(1_000);
        assert_eq!(budget.upstream_wall_clock_ms, u64::MAX);
        assert!(budget.run_wall_clock_exhausted());
    }

    #[test]
    fn section_wall_clock_uses_active_duration_not_call_count() {
        let budget = DeepNoteBudget::for_section_count(3);
        assert!(!budget.section_wall_clock_exhausted(0));
        assert!(!budget.section_wall_clock_exhausted(budget.section_wall_clock_ms - 1));
        assert!(budget.section_wall_clock_exhausted(budget.section_wall_clock_ms));
        assert!(
            budget.section_wall_clock_ms < budget.run_wall_clock_ms,
            "单个 section 的预算必须小于整个 run，否则 section 闸永远不会先响"
        );
    }

    /// 存量运行时 JSON 没有这几个字段，反序列化必须落到默认值而不是报错 ——
    /// 否则升级后所有在途 run 都读不回来。
    #[test]
    fn existing_runtime_json_deserializes_without_wall_clock_fields() {
        let json = serde_json::json!({
            "semanticCallLimit": 100,
            "semanticCallsUsed": 7,
            "nodeAttemptLimit": 5,
            "sectionRevisionLimit": 5,
            "replanLimit": 4,
            "replansUsed": 1,
            "maxParallelNodes": 2,
        })
        .to_string();
        let budget: DeepNoteBudget =
            serde_json::from_str(&json).expect("缺少墙钟字段的存量预算必须能反序列化");
        assert_eq!(budget.semantic_calls_used, 7);
        assert_eq!(
            budget.upstream_request_limit,
            MAX_DEEP_NOTE_UPSTREAM_REQUESTS
        );
        assert_eq!(budget.upstream_requests_used, 0);
        assert_eq!(budget.upstream_wall_clock_ms, 0);
        assert!(budget.run_wall_clock_ms > 0, "默认值必须是可用的正数上限");
        assert!(budget.section_wall_clock_ms > 0);
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
    /// section id → 该 section 已累计的**活跃**执行时长（毫秒）。
    ///
    /// 记「累计活跃时长」而不是「首次开始的时刻」，是因为后者会把关机、暂停、
    /// 等并发席位的时间一起算进预算。一个跑了 3 分钟就被中断、次日才续跑的
    /// section，用时刻差算出来是十几个小时，会被闸门当成早已超时而直接跳过 ——
    /// 用户拿到一篇静默缺章的笔记，而那个 section 其实几乎没花上游时间。
    ///
    /// 这也让 section 级与 run 级的口径一致：`upstream_wall_clock_ms` 同样只累计
    /// 真实发生的调用耗时（见该字段注释），两级预算不该用两种时间。
    ///
    /// 独立于 `DeepNoteDagNode`：节点没有任何计时字段，而给节点加字段要动
    /// 计划快照的结构；放在 runtime state 里带 `#[serde(default)]` 就能让存量
    /// 运行时 JSON 直接反序列化成空表，不需要迁移。
    #[serde(default)]
    pub section_active_ms: BTreeMap<String, u64>,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineCancelResult {
    pub run: NotePipelineRun,
    pub forced: bool,
    pub diagnostic_path: Option<String>,
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
