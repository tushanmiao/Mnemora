//! 知识库跨前后端的数据合同和状态机纯函数。

use serde::{Deserialize, Serialize};

use crate::library::types::normalize_identifier;

pub const MAX_KNOWLEDGE_QUERY_CHARS: usize = 500;
pub const MAX_KNOWLEDGE_RESULT_LIMIT: usize = 50;
pub const DEFAULT_KNOWLEDGE_RESULT_LIMIT: usize = 8;
pub const MAX_KNOWLEDGE_JOB_LIMIT: usize = 200;

pub const MINERU_CONSENT_POLICY_VERSION: &str = "mineru-cloud-v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeQueryScope {
    #[default]
    Library,
    CurrentLiterature,
    CurrentNote,
}

impl KnowledgeQueryScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::CurrentLiterature => "currentLiterature",
            Self::CurrentNote => "currentNote",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KnowledgeJobState {
    Queued,
    Running,
    Cancelling,
    Paused,
    Succeeded,
    Partial,
    Failed,
    Cancelled,
    Stale,
}

impl KnowledgeJobState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Paused => "paused",
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Stale => "stale",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "cancelling" => Self::Cancelling,
            "paused" => Self::Paused,
            "succeeded" => Self::Succeeded,
            "partial" => Self::Partial,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "stale" => Self::Stale,
            _ => return None,
        })
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Partial | Self::Failed | Self::Cancelled | Self::Stale
        )
    }

    pub(crate) fn document_state(self) -> &'static str {
        match self {
            Self::Succeeded => "ready",
            Self::Partial => "partial",
            _ => "pending",
        }
    }
}

/// Plan 13/15 共用的最小状态转换门禁。实际持久化仍需在事务中做 CAS。
pub(crate) fn can_transition_job_state(from: KnowledgeJobState, to: KnowledgeJobState) -> bool {
    if from.is_terminal() {
        return false;
    }
    matches!(
        (from, to),
        (KnowledgeJobState::Queued, KnowledgeJobState::Running)
            | (KnowledgeJobState::Queued, KnowledgeJobState::Cancelled)
            | (KnowledgeJobState::Queued, KnowledgeJobState::Failed)
            | (KnowledgeJobState::Queued, KnowledgeJobState::Stale)
            | (KnowledgeJobState::Running, KnowledgeJobState::Cancelling)
            | (KnowledgeJobState::Running, KnowledgeJobState::Succeeded)
            | (KnowledgeJobState::Running, KnowledgeJobState::Partial)
            | (KnowledgeJobState::Running, KnowledgeJobState::Failed)
            | (KnowledgeJobState::Running, KnowledgeJobState::Cancelled)
            | (KnowledgeJobState::Running, KnowledgeJobState::Stale)
            | (KnowledgeJobState::Cancelling, KnowledgeJobState::Cancelled)
            | (KnowledgeJobState::Cancelling, KnowledgeJobState::Failed)
            | (KnowledgeJobState::Cancelling, KnowledgeJobState::Stale)
            | (KnowledgeJobState::Paused, KnowledgeJobState::Queued)
            | (KnowledgeJobState::Paused, KnowledgeJobState::Cancelling)
    )
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchRequest {
    pub query: String,
    #[serde(default)]
    pub scope: KnowledgeQueryScope,
    #[serde(default)]
    pub current_literature_id: Option<String>,
    #[serde(default)]
    pub current_note_id: Option<String>,
    #[serde(default)]
    pub selected_document_ids: Vec<String>,
    #[serde(default)]
    pub element_types: Vec<String>,
    #[serde(default = "default_result_limit")]
    pub limit: usize,
}

fn default_result_limit() -> usize {
    DEFAULT_KNOWLEDGE_RESULT_LIMIT
}

impl KnowledgeSearchRequest {
    pub(crate) fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.query = self.query.trim().to_string();
        if self.query.is_empty() {
            return Err("知识库查询不能为空。".to_string());
        }
        if self.query.chars().count() > MAX_KNOWLEDGE_QUERY_CHARS {
            return Err("知识库查询不能超过 500 个字符。".to_string());
        }
        if !(1..=MAX_KNOWLEDGE_RESULT_LIMIT).contains(&self.limit) {
            return Err(format!(
                "知识库查询结果数量必须在 1 到 {MAX_KNOWLEDGE_RESULT_LIMIT} 之间。"
            ));
        }
        if let Some(value) = self.current_literature_id.as_mut() {
            *value = normalize_identifier("当前文献 ID", value)?;
        }
        if let Some(value) = self.current_note_id.as_mut() {
            *value = normalize_identifier("当前笔记 ID", value)?;
        }
        let mut selected = Vec::with_capacity(self.selected_document_ids.len());
        for value in self.selected_document_ids {
            let value = normalize_identifier("知识文档 ID", &value)?;
            if !selected.contains(&value) {
                selected.push(value);
            }
            if selected.len() > MAX_KNOWLEDGE_RESULT_LIMIT * 4 {
                return Err("知识库筛选文档数量过多。".to_string());
            }
        }
        self.selected_document_ids = selected;
        self.element_types = self
            .element_types
            .into_iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .take(16)
            .collect();
        match self.scope {
            KnowledgeQueryScope::CurrentLiterature if self.current_literature_id.is_none() => {
                return Err("当前文献范围缺少文献 ID。".to_string())
            }
            KnowledgeQueryScope::CurrentNote if self.current_note_id.is_none() => {
                return Err("当前笔记范围缺少笔记 ID。".to_string())
            }
            _ => {}
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeOverview {
    pub document_count: usize,
    pub literature_count: usize,
    pub note_count: usize,
    pub ready_count: usize,
    pub pending_count: usize,
    pub failed_count: usize,
    pub active_job_count: usize,
    pub fts5_available: bool,
    pub tokenizer: String,
    pub lexical_degraded: bool,
    pub embedding_ready_count: usize,
    pub embedding_pending_count: usize,
    pub embedding_failed_count: usize,
    pub embedding_dimensions: Vec<u32>,
    pub last_indexed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeMineruTokenStatus {
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeConsentStatus {
    pub document_id: String,
    pub source_id: String,
    pub source_hash: String,
    pub provider_id: String,
    /// The effective permission for the current source hash.  `None` means
    /// that neither a current document consent nor a valid global consent is
    /// available.  The legacy `scope` field below is retained for clients
    /// that still expect a string.
    pub effective_scope: Option<String>,
    pub scope: String,
    pub granted: bool,
    pub document_granted: bool,
    pub global_granted: bool,
    /// `none`, `granted`, `revoked`, or `stale` (a document consent for an
    /// older source hash).  Keeping these states separate prevents the UI
    /// from presenting an unrecorded decision as an explicit rejection.
    pub document_consent_state: String,
    pub global_consent_state: String,
    pub document_source_hash_matches: bool,
    pub revoked: bool,
    pub document_granted_at: Option<u64>,
    pub global_granted_at: Option<u64>,
    pub document_revoked_at: Option<u64>,
    pub global_revoked_at: Option<u64>,
    pub granted_at: Option<u64>,
    pub revoked_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeGlobalConsentStatus {
    pub state: String,
    pub granted: bool,
    pub granted_at: Option<u64>,
    pub revoked_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeDocumentStatus {
    pub id: String,
    pub source_class: String,
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
    pub state: String,
    /// Current cloud-upload authorization gate for literature.  Notes always
    /// expose `not_required` and never inherit a PDF consent.
    pub cloud_consent_state: String,
    pub extraction_quality: Option<String>,
    pub active_revision_id: Option<String>,
    pub source_hash: String,
    pub chunk_count: usize,
    pub asset_count: usize,
    pub warning_count: usize,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeJobView {
    pub id: String,
    pub job_kind: String,
    pub document_id: Option<String>,
    pub revision_id: Option<String>,
    pub state: String,
    pub stage: String,
    pub completed_units: usize,
    pub total_units: usize,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub finished_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchHit {
    pub chunk_id: String,
    pub document_id: String,
    pub source_class: String,
    pub source_id: String,
    pub title: String,
    pub text: String,
    pub snippet: String,
    pub heading_path: Vec<String>,
    pub element_types: Vec<String>,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub source_hash: String,
    pub revision_id: String,
    pub extraction_quality: String,
    pub score: f64,
    pub lexical_score: Option<f64>,
    pub vector_score: Option<f64>,
    pub fused_score: Option<f64>,
    pub lexical_rank: Option<usize>,
    pub vector_rank: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSearchResponse {
    pub query: String,
    pub scope: String,
    pub hits: Vec<KnowledgeSearchHit>,
    pub lexical_degraded: bool,
    pub insufficient_evidence: bool,
    pub requested_mode: String,
    pub actual_mode: String,
    pub fallback_reason: Option<String>,
    pub vector_dimensions: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeChunkView {
    pub id: String,
    pub document_id: String,
    pub revision_id: String,
    pub block_kind: String,
    pub text: String,
    pub search_text: String,
    pub heading_path: Vec<String>,
    pub element_ids: Vec<String>,
    pub asset_ids: Vec<String>,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub byte_start: u64,
    pub byte_end: u64,
    pub source_hash: String,
    pub extraction_quality: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRebuildResult {
    pub queued_pdf_count: usize,
    pub indexed_note_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeEmbeddingRebuildResult {
    pub queued_job_count: usize,
    pub cached_chunk_count: usize,
    pub pending_chunk_count: usize,
}

#[cfg(test)]
mod tests {
    use super::{
        can_transition_job_state, KnowledgeJobState, KnowledgeQueryScope, KnowledgeSearchRequest,
    };

    #[test]
    fn job_state_machine_rejects_terminal_reopening() {
        assert!(can_transition_job_state(
            KnowledgeJobState::Queued,
            KnowledgeJobState::Running
        ));
        assert!(can_transition_job_state(
            KnowledgeJobState::Running,
            KnowledgeJobState::Succeeded
        ));
        assert!(!can_transition_job_state(
            KnowledgeJobState::Succeeded,
            KnowledgeJobState::Running
        ));
        assert!(!can_transition_job_state(
            KnowledgeJobState::Running,
            KnowledgeJobState::Queued
        ));
    }

    #[test]
    fn search_request_requires_ids_for_narrow_scopes() {
        let request = KnowledgeSearchRequest {
            query: "term".to_string(),
            scope: KnowledgeQueryScope::CurrentNote,
            current_literature_id: None,
            current_note_id: None,
            selected_document_ids: vec![],
            element_types: vec![],
            limit: 8,
        };
        assert!(request.normalize_and_validate().is_err());
    }
}
