//! 文献库前后端命令契约及输入校验。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

pub const DEFAULT_LIBRARY_PAGE_SIZE: usize = 200;
pub const MAX_LIBRARY_PAGE_SIZE: usize = 500;
pub const MAX_LIBRARY_TITLE_CHARS: usize = 500;
pub const MAX_LIBRARY_AUTHORS: usize = 32;
pub const MAX_LIBRARY_AUTHOR_CHARS: usize = 200;
pub const MAX_LIBRARY_TAGS: usize = 32;
pub const MAX_LIBRARY_TAG_CHARS: usize = 80;
pub const MAX_LIBRARY_COLLECTIONS_PER_ITEM: usize = 64;
pub const MAX_COLLECTION_NAME_CHARS: usize = 120;
pub const MAX_PDF_RANGE_BYTES: u64 = 1024 * 1024;
pub const MAX_READING_PAGE_INDEX: u32 = 1_000_000;
pub const MIN_READING_ZOOM: f64 = 0.5;
pub const MAX_READING_ZOOM: f64 = 3.0;
pub const MAX_ANNOTATION_TEXT_CHARS: usize = 20_000;
pub const MAX_ANNOTATION_COMMENT_CHARS: usize = 20_000;
pub const MAX_ANNOTATION_RECTS: usize = 256;
pub const MAX_NOTE_TITLE_CHARS: usize = 500;
pub const MAX_NOTE_CONTENT_CHARS: usize = 500_000;
pub const MAX_NOTE_IMPORT_FILES: usize = 50;
pub const MAX_NOTE_IMPORT_BYTES: u64 = 2 * 1024 * 1024;
/// 单篇笔记的章节级来源条数上限，避免写入过多溯源记录。
pub const MAX_NOTE_SOURCES: usize = 2000;
pub const MAX_NOTE_PIPELINE_SECTIONS: usize = 40;
pub const MAX_NOTE_PIPELINE_JSON_BYTES: usize = 512 * 1024;
/// 章节 id 字符上限（对应提纲 sections[].id，允许模型生成较宽松取值）。
pub const MAX_NOTE_SECTION_ID_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LibraryView {
    #[default]
    All,
    Recent,
    Favorites,
    Unfiled,
    Trash,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySort {
    #[default]
    Updated,
    Title,
    Year,
    Imported,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryListRequest {
    #[serde(default)]
    pub view: LibraryView,
    #[serde(default)]
    pub search_query: String,
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub sort: LibrarySort,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_library_page_size")]
    pub limit: usize,
}

fn default_library_page_size() -> usize {
    DEFAULT_LIBRARY_PAGE_SIZE
}

impl LibraryListRequest {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.search_query = self.search_query.trim().to_string();
        if self.search_query.chars().count() > 500 {
            return Err("文献搜索内容过长。".to_string());
        }
        if let Some(collection_id) = self.collection_id.as_mut() {
            *collection_id = normalize_identifier("分类 ID", collection_id)?;
        }
        if !(1..=MAX_LIBRARY_PAGE_SIZE).contains(&self.limit) {
            return Err(format!(
                "文献列表每页数量必须在 1 到 {MAX_LIBRARY_PAGE_SIZE} 之间。"
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFileSummary {
    pub id: String,
    pub original_name: String,
    pub file_size: u64,
    pub file_hash: String,
    pub mime_type: String,
    pub created_at: u64,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub publication_year: Option<i32>,
    pub publication_title: String,
    pub doi: String,
    pub abstract_text: String,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub collection_ids: Vec<String>,
    pub collection_names: Vec<String>,
    pub file: LibraryFileSummary,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_opened_at: Option<u64>,
    pub deleted_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryListPage {
    pub items: Vec<LibraryItem>,
    pub offset: usize,
    pub total: usize,
    pub has_more: bool,
}

/// PDF 阅读位置的轻量持久化状态。
///
/// `scroll_offset` 使用当前页内部的 0～1 比例，而不是像素值，避免窗口尺寸或缩放
/// 改变后恢复位置明显偏移。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryReadingState {
    pub item_id: String,
    pub page_index: u32,
    pub scroll_offset: f64,
    pub zoom: f64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryReadingStateUpdate {
    pub item_id: String,
    #[serde(default)]
    pub page_index: u32,
    #[serde(default)]
    pub scroll_offset: f64,
    #[serde(default = "default_reading_zoom")]
    pub zoom: f64,
}

fn default_reading_zoom() -> f64 {
    1.0
}

impl LibraryReadingStateUpdate {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.item_id = normalize_identifier("文献 ID", &self.item_id)?;
        if self.page_index > MAX_READING_PAGE_INDEX {
            return Err("阅读页码超出允许范围。".to_string());
        }
        if !self.scroll_offset.is_finite() || !(0.0..=1.0).contains(&self.scroll_offset) {
            return Err("阅读滚动位置必须在 0 到 1 之间。".to_string());
        }
        if !self.zoom.is_finite() || !(MIN_READING_ZOOM..=MAX_READING_ZOOM).contains(&self.zoom) {
            return Err(format!(
                "阅读缩放比例必须在 {MIN_READING_ZOOM} 到 {MAX_READING_ZOOM} 之间。"
            ));
        }
        Ok(self)
    }
}

/// PDF 批注类型。文本批注保存一组矩形，区域批注只保存一个矩形。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LibraryAnnotationKind {
    Highlight,
    Underline,
    Area,
}

/// 批注颜色使用固定集合，避免前端把任意 CSS 值写入数据库。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LibraryAnnotationColor {
    Yellow,
    Green,
    Blue,
    Pink,
    Purple,
}

impl LibraryAnnotationColor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Pink => "pink",
            Self::Purple => "purple",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "yellow" => Ok(Self::Yellow),
            "green" => Ok(Self::Green),
            "blue" => Ok(Self::Blue),
            "pink" => Ok(Self::Pink),
            "purple" => Ok(Self::Purple),
            _ => Err("批注颜色无效。".to_string()),
        }
    }
}

impl LibraryAnnotationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Highlight => "highlight",
            Self::Underline => "underline",
            Self::Area => "area",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "highlight" => Ok(Self::Highlight),
            "underline" => Ok(Self::Underline),
            "area" => Ok(Self::Area),
            _ => Err("批注类型无效。".to_string()),
        }
    }
}

/// 相对 PDF 页面宽高的矩形坐标，所有值均位于 0～1。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAnnotationRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LibraryAnnotationRect {
    fn validate(&self) -> Result<(), String> {
        let values = [self.x, self.y, self.width, self.height];
        if values.iter().any(|value| !value.is_finite()) {
            return Err("批注区域包含无效坐标。".to_string());
        }
        if self.x < 0.0
            || self.y < 0.0
            || self.width <= 0.0
            || self.height <= 0.0
            || self.x + self.width > 1.000_001
            || self.y + self.height > 1.000_001
        {
            return Err("批注区域必须位于 PDF 页面范围内。".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAnnotation {
    pub id: String,
    pub item_id: String,
    pub kind: LibraryAnnotationKind,
    pub page_index: u32,
    pub color: LibraryAnnotationColor,
    pub text: String,
    pub comment: String,
    pub rects: Vec<LibraryAnnotationRect>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAnnotationCreate {
    pub item_id: String,
    pub kind: LibraryAnnotationKind,
    pub page_index: u32,
    pub color: LibraryAnnotationColor,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub comment: String,
    pub rects: Vec<LibraryAnnotationRect>,
}

impl LibraryAnnotationCreate {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.item_id = normalize_identifier("文献 ID", &self.item_id)?;
        if self.page_index > MAX_READING_PAGE_INDEX {
            return Err("批注页码超出允许范围。".to_string());
        }
        self.text = normalize_multiline_text("批注摘录", &self.text, MAX_ANNOTATION_TEXT_CHARS)?;
        self.comment =
            normalize_multiline_text("批注评论", &self.comment, MAX_ANNOTATION_COMMENT_CHARS)?;
        validate_annotation_rects(self.kind, &self.rects)?;
        if self.kind != LibraryAnnotationKind::Area && self.text.is_empty() {
            return Err("文本批注必须包含选中的文字。".to_string());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAnnotationUpdate {
    pub annotation_id: String,
    pub color: LibraryAnnotationColor,
    #[serde(default)]
    pub comment: String,
}

impl LibraryAnnotationUpdate {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.annotation_id = normalize_identifier("批注 ID", &self.annotation_id)?;
        self.comment =
            normalize_multiline_text("批注评论", &self.comment, MAX_ANNOTATION_COMMENT_CHARS)?;
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNote {
    pub id: String,
    pub item_id: Option<String>,
    pub item_title: Option<String>,
    pub title: String,
    pub content: String,
    /// 所属分组名；None 表示未分类。分组只作用于独立笔记（item_id 为空）。
    pub group_name: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// 笔记列表只返回短预览，完整正文仅在打开具体笔记时读取。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteSummary {
    pub id: String,
    pub item_id: Option<String>,
    pub item_title: Option<String>,
    pub title: String,
    pub content_preview: String,
    pub content_chars: usize,
    pub group_name: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub content_bytes: usize,
}

/// 笔记分组（轻量标签式，按名称关联；空分组也会保留）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteGroup {
    pub name: String,
    pub note_count: usize,
    pub created_at: u64,
}

/// 分组名与集合名共用同一长度上限，语义一致。
pub fn normalize_note_group_name(value: &str) -> Result<String, String> {
    normalize_text("分组名称", value, MAX_COLLECTION_NAME_CHARS, false)
}

/// 笔记章节来源类型：来自对话消息，或 AI 补充的背景知识。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NoteSourceOrigin {
    Conversation,
    AiSupplement,
}

impl NoteSourceOrigin {
    /// 数据库存储用的稳定字符串（与 CHECK 约束一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::AiSupplement => "ai_supplement",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "conversation" => Ok(Self::Conversation),
            "ai_supplement" => Ok(Self::AiSupplement),
            _ => Err("笔记来源类型无效。".to_string()),
        }
    }
}

/// 笔记章节级来源（读取用）。会话删除后 conversation_id / message_id 置 NULL。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSource {
    pub id: String,
    pub note_id: String,
    pub section_id: String,
    pub origin: NoteSourceOrigin,
    /// 会话删除后置 NULL：来源显示为“原会话已删除”。
    pub conversation_id: Option<String>,
    /// 会话删除后置 NULL。
    pub message_id: Option<String>,
    /// M2 增量合并锚点，可空。
    pub summarized_until_message_id: Option<String>,
    pub created_at: u64,
}

/// 写入一条笔记来源；note_id 由调用方保证存在。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSourceCreate {
    pub section_id: String,
    pub origin: NoteSourceOrigin,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub summarized_until_message_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotePipelinePhase {
    Preflight,
    Analyzing,
    AwaitingOutline,
    Compiling,
    Queued,
    Drafting,
    Validating,
    Replanning,
    Assembling,
    Persisting,
    Paused,
    Blocked,
    Done,
    Cancelled,
    Error,
}

impl NotePipelinePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Analyzing => "analyzing",
            Self::AwaitingOutline => "awaiting_outline",
            Self::Compiling => "compiling",
            Self::Queued => "queued",
            Self::Drafting => "drafting",
            Self::Validating => "validating",
            Self::Replanning => "replanning",
            Self::Assembling => "assembling",
            Self::Persisting => "persisting",
            Self::Paused => "paused",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "preflight" => Ok(Self::Preflight),
            "analyzing" => Ok(Self::Analyzing),
            "awaiting_outline" => Ok(Self::AwaitingOutline),
            "compiling" => Ok(Self::Compiling),
            "queued" => Ok(Self::Queued),
            "drafting" => Ok(Self::Drafting),
            "validating" => Ok(Self::Validating),
            "replanning" => Ok(Self::Replanning),
            "assembling" => Ok(Self::Assembling),
            "persisting" => Ok(Self::Persisting),
            "paused" => Ok(Self::Paused),
            "blocked" => Ok(Self::Blocked),
            "done" => Ok(Self::Done),
            "cancelled" => Ok(Self::Cancelled),
            "error" => Ok(Self::Error),
            _ => Err("深度笔记任务阶段无效。".to_string()),
        }
    }

    pub fn is_resumable(self) -> bool {
        matches!(
            self,
            Self::Preflight
                | Self::Analyzing
                | Self::AwaitingOutline
                | Self::Compiling
                | Self::Queued
                | Self::Drafting
                | Self::Validating
                | Self::Replanning
                | Self::Assembling
                | Self::Persisting
                | Self::Paused
                | Self::Blocked
                | Self::Cancelled
                | Self::Error
        )
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotePipelineSectionStatus {
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

impl NotePipelineSectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::NeedsReview => "needs_review",
            Self::NeedsRevision => "needs_revision",
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
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "needs_review" => Ok(Self::NeedsReview),
            "needs_revision" => Ok(Self::NeedsRevision),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            "skipped" => Ok(Self::Skipped),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err("深度笔记章节状态无效。".to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePipelineRun {
    pub id: String,
    pub conversation_id: String,
    pub note_id: Option<String>,
    pub phase: NotePipelinePhase,
    pub outline_json: String,
    pub selected_section_ids: Vec<String>,
    pub provider_id: String,
    pub model_id: String,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
    pub retry_attempts: u8,
    pub input_snapshot_hash: String,
    pub current_plan_version: u32,
    pub execution_version: u32,
    pub budget_json: String,
    pub preflight_json: String,
    pub sidecar_json: String,
    pub idempotency_key: String,
    pub completed_section_ids: Vec<String>,
    pub failed_section_ids: Vec<String>,
    pub warnings: Vec<String>,
    pub error_message: Option<String>,
    pub abandoned: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
pub struct NotePipelineRunCreate {
    pub id: String,
    pub conversation_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub max_output_tokens: u32,
    pub thinking_enabled: bool,
    pub retry_attempts: u8,
    pub input_snapshot_hash: String,
    pub budget_json: String,
    pub preflight_json: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone)]
pub struct NotePipelineSection {
    pub run_id: String,
    pub section_id: String,
    pub position: usize,
    pub section_json: String,
    pub markdown: String,
    pub status: NotePipelineSectionStatus,
    pub attempt_count: u8,
    pub revision_count: u8,
    pub evidence_ids: Vec<String>,
    pub validation_json: String,
    pub input_hash: String,
    pub error_message: Option<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
pub struct NotePipelineSectionCreate {
    pub section_id: String,
    pub position: usize,
    pub section_json: String,
    pub input_hash: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteVersion {
    pub id: String,
    pub note_id: String,
    pub title: String,
    pub reason: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEditProposal {
    pub id: String,
    pub note_id: String,
    pub conversation_id: String,
    pub source_message_id: Option<String>,
    pub expected_note_updated_at: u64,
    pub old_title: String,
    pub new_title: String,
    pub old_content: String,
    pub new_content: String,
    pub diff: String,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct NoteEditProposalCreate {
    pub id: String,
    pub note_id: String,
    pub conversation_id: String,
    pub source_message_id: Option<String>,
    pub expected_note_updated_at: u64,
    pub old_title: String,
    pub new_title: String,
    pub old_content: String,
    pub new_content: String,
    pub diff: String,
    pub sources: Vec<NoteSourceCreate>,
}

impl NoteSourceCreate {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.section_id = normalize_text(
            "章节 ID",
            &self.section_id,
            MAX_NOTE_SECTION_ID_CHARS,
            false,
        )?;
        self.conversation_id = self
            .conversation_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| normalize_identifier("会话 ID", value))
            .transpose()?;
        self.message_id = self
            .message_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| normalize_identifier("消息 ID", value))
            .transpose()?;
        self.summarized_until_message_id = self
            .summarized_until_message_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| normalize_identifier("消息 ID", value))
            .transpose()?;
        // 对话来源必须带会话锚点；AI 补充来源没有会话锚点。
        if self.origin == NoteSourceOrigin::Conversation && self.conversation_id.is_none() {
            return Err("对话来源必须包含会话 ID。".to_string());
        }
        if self.origin == NoteSourceOrigin::AiSupplement
            && (self.conversation_id.is_some()
                || self.message_id.is_some()
                || self.summarized_until_message_id.is_some())
        {
            return Err("AI 补充来源不能包含会话锚点。".to_string());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteCreate {
    #[serde(default)]
    pub item_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub group_name: Option<String>,
}

impl LibraryNoteCreate {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.item_id = self
            .item_id
            .as_deref()
            .map(|value| normalize_identifier("文献 ID", value))
            .transpose()?;
        self.title = normalize_text("笔记标题", &self.title, MAX_NOTE_TITLE_CHARS, false)?;
        self.content = normalize_multiline_text("笔记正文", &self.content, MAX_NOTE_CONTENT_CHARS)?;
        self.group_name = self
            .group_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(normalize_note_group_name)
            .transpose()?;
        Ok(self)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteUpdate {
    pub note_id: String,
    pub title: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteRename {
    pub note_id: String,
    pub title: String,
}

impl LibraryNoteRename {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.note_id = normalize_identifier("笔记 ID", &self.note_id)?;
        self.title = normalize_text("笔记标题", &self.title, MAX_NOTE_TITLE_CHARS, false)?;
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteImportFailure {
    pub path: String,
    pub file_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryNoteImportResult {
    pub imported: Vec<LibraryNote>,
    pub failed: Vec<LibraryNoteImportFailure>,
}

impl LibraryNoteUpdate {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.note_id = normalize_identifier("笔记 ID", &self.note_id)?;
        self.title = normalize_text("笔记标题", &self.title, MAX_NOTE_TITLE_CHARS, false)?;
        self.content = normalize_multiline_text("笔记正文", &self.content, MAX_NOTE_CONTENT_CHARS)?;
        Ok(self)
    }
}

fn validate_annotation_rects(
    kind: LibraryAnnotationKind,
    rects: &[LibraryAnnotationRect],
) -> Result<(), String> {
    if rects.is_empty() || rects.len() > MAX_ANNOTATION_RECTS {
        return Err(format!(
            "一条批注必须包含 1 到 {MAX_ANNOTATION_RECTS} 个区域。"
        ));
    }
    if kind == LibraryAnnotationKind::Area && rects.len() != 1 {
        return Err("区域批注只能包含一个矩形区域。".to_string());
    }
    for rect in rects {
        rect.validate()?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCollection {
    pub id: String,
    pub name: String,
    pub item_count: usize,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItemUpdate {
    pub item_id: String,
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub publication_year: Option<i32>,
    #[serde(default)]
    pub publication_title: String,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    pub abstract_text: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub collection_ids: Vec<String>,
}

impl LibraryItemUpdate {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.item_id = normalize_identifier("文献 ID", &self.item_id)?;
        self.title = normalize_text("标题", &self.title, MAX_LIBRARY_TITLE_CHARS, false)?;
        self.publication_title =
            normalize_text("期刊或出版物", &self.publication_title, 500, true)?;
        self.doi = normalize_text("DOI", &self.doi, 300, true)?;
        self.abstract_text = normalize_text("摘要", &self.abstract_text, 100_000, true)?;
        if let Some(year) = self.publication_year {
            if !(1000..=3000).contains(&year) {
                return Err("出版年份必须在 1000 到 3000 之间。".to_string());
            }
        }
        self.authors = normalize_string_list(
            "作者",
            self.authors,
            MAX_LIBRARY_AUTHORS,
            MAX_LIBRARY_AUTHOR_CHARS,
        )?;
        self.tags =
            normalize_string_list("标签", self.tags, MAX_LIBRARY_TAGS, MAX_LIBRARY_TAG_CHARS)?;
        if self.collection_ids.len() > MAX_LIBRARY_COLLECTIONS_PER_ITEM {
            return Err(format!(
                "一篇文献最多可以加入 {MAX_LIBRARY_COLLECTIONS_PER_ITEM} 个分类。"
            ));
        }
        let mut collection_ids = Vec::new();
        let mut seen = HashSet::new();
        for collection_id in self.collection_ids {
            let collection_id = normalize_identifier("分类 ID", &collection_id)?;
            if seen.insert(collection_id.clone()) {
                collection_ids.push(collection_id);
            }
        }
        self.collection_ids = collection_ids;
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryImportFailure {
    pub path: String,
    pub file_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryImportResult {
    pub imported: Vec<LibraryItem>,
    pub duplicates: Vec<LibraryItem>,
    pub failed: Vec<LibraryImportFailure>,
}

pub fn normalize_collection_name(value: &str) -> Result<String, String> {
    normalize_text("分类名称", value, MAX_COLLECTION_NAME_CHARS, false)
}

pub fn normalize_identifier(label: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 {
        return Err(format!("{label}无效。"));
    }
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err(format!("{label}包含不允许的字符。"));
    }
    Ok(value.to_string())
}

fn normalize_text(
    label: &str,
    value: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<String, String> {
    let value = value.trim();
    if !allow_empty && value.is_empty() {
        return Err(format!("{label}不能为空。"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label}不能超过 {max_chars} 个字符。"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label}包含不允许的控制字符。"));
    }
    Ok(value.to_string())
}

fn normalize_multiline_text(label: &str, value: &str, max_chars: usize) -> Result<String, String> {
    let value = value.trim();
    if value.chars().count() > max_chars {
        return Err(format!("{label}不能超过 {max_chars} 个字符。"));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(format!("{label}包含不允许的控制字符。"));
    }
    Ok(value.to_string())
}

fn normalize_string_list(
    label: &str,
    values: Vec<String>,
    max_items: usize,
    max_chars: usize,
) -> Result<Vec<String>, String> {
    if values.len() > max_items {
        return Err(format!("{label}最多允许 {max_items} 项。"));
    }
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let value = normalize_text(label, &value, max_chars, true)?;
        if value.is_empty() {
            continue;
        }
        let key = value.to_lowercase();
        if seen.insert(key) {
            normalized.push(value);
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{
        LibraryAnnotationCreate, LibraryItemUpdate, LibraryListRequest, LibraryNoteCreate,
        NoteSourceCreate, NoteSourceOrigin, MAX_LIBRARY_PAGE_SIZE,
    };

    #[test]
    fn list_request_rejects_oversized_pages() {
        let request = LibraryListRequest {
            limit: MAX_LIBRARY_PAGE_SIZE + 1,
            ..serde_json::from_str("{}").unwrap()
        };
        assert!(request.normalize_and_validate().is_err());
    }

    #[test]
    fn item_update_trims_and_deduplicates_people_tags_and_collections() {
        let update: LibraryItemUpdate = serde_json::from_value(serde_json::json!({
            "itemId": "item-1",
            "title": "  Paper  ",
            "authors": ["Alice", " alice ", "Bob"],
            "tags": ["AI", " ai ", "PDF"],
            "collectionIds": ["collection-1", "collection-1"]
        }))
        .unwrap();
        let update = update.normalize_and_validate().unwrap();
        assert_eq!(update.title, "Paper");
        assert_eq!(update.authors, vec!["Alice", "Bob"]);
        assert_eq!(update.tags, vec!["AI", "PDF"]);
        assert_eq!(update.collection_ids, vec!["collection-1"]);
    }

    #[test]
    fn annotation_and_note_inputs_validate_bounds_and_multiline_content() {
        let annotation: LibraryAnnotationCreate = serde_json::from_value(serde_json::json!({
            "itemId": "item-1",
            "kind": "highlight",
            "pageIndex": 2,
            "color": "yellow",
            "text": " selected text ",
            "comment": "line one\nline two",
            "rects": [{ "x": 0.1, "y": 0.2, "width": 0.3, "height": 0.04 }]
        }))
        .unwrap();
        let annotation = annotation.normalize_and_validate().unwrap();
        assert_eq!(annotation.text, "selected text");
        assert_eq!(annotation.comment, "line one\nline two");

        let invalid: LibraryAnnotationCreate = serde_json::from_value(serde_json::json!({
            "itemId": "item-1",
            "kind": "area",
            "pageIndex": 0,
            "color": "blue",
            "text": "",
            "rects": [{ "x": 0.9, "y": 0.2, "width": 0.2, "height": 0.1 }]
        }))
        .unwrap();
        assert!(invalid.normalize_and_validate().is_err());

        let note: LibraryNoteCreate = serde_json::from_value(serde_json::json!({
            "itemId": "item-1",
            "title": " Note ",
            "content": "paragraph one\n\nparagraph two"
        }))
        .unwrap();
        let note = note.normalize_and_validate().unwrap();
        assert_eq!(note.title, "Note");
        assert!(note.content.contains("paragraph two"));
    }

    #[test]
    fn note_source_requires_conversation_anchor_and_normalizes_ids() {
        let source = NoteSourceCreate {
            section_id: " sec-1 ".to_string(),
            origin: NoteSourceOrigin::Conversation,
            conversation_id: Some(" conversation-1 ".to_string()),
            message_id: Some(" message-1 ".to_string()),
            summarized_until_message_id: None,
        }
        .normalize_and_validate()
        .unwrap();
        assert_eq!(source.section_id, "sec-1");
        assert_eq!(source.conversation_id.as_deref(), Some("conversation-1"));
        assert_eq!(source.message_id.as_deref(), Some("message-1"));

        let invalid = NoteSourceCreate {
            section_id: "sec-1".to_string(),
            origin: NoteSourceOrigin::Conversation,
            conversation_id: None,
            message_id: None,
            summarized_until_message_id: None,
        };
        assert!(invalid.normalize_and_validate().is_err());

        let invalid_ai_source = NoteSourceCreate {
            section_id: "sec-2".to_string(),
            origin: NoteSourceOrigin::AiSupplement,
            conversation_id: Some("conversation-1".to_string()),
            message_id: None,
            summarized_until_message_id: None,
        };
        assert!(invalid_ai_source.normalize_and_validate().is_err());
    }
}
