//! 知识库派生数据仓库、Markdown lexical index 和有界检索。

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use image::ImageReader;
use rusqlite::{
    params, params_from_iter, types::Value, Connection, OptionalExtension, Transaction,
    TransactionBehavior,
};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::library::{
    note_files::resolve_note_directory, types::normalize_identifier, LibraryRepository,
};

use super::{
    embedding::{
        cosine_similarity_normalized, decode_f32_le, encode_f32_le, normalize_l2,
        reciprocal_rank_fusion, EmbeddingProviderSpec, EMBEDDING_NORMALIZATION,
    },
    markdown::{
        parse_markdown, safe_relative_asset_path, search_projection, MarkdownBlock,
        MarkdownDocument, MarkdownImageRef, MARKDOWN_CHUNK_POLICY_VERSION,
        MARKDOWN_NORMALIZATION_VERSION, MARKDOWN_PARSER_ID, MARKDOWN_PARSER_VERSION,
    },
    types::{
        can_transition_job_state, KnowledgeChunkView, KnowledgeConsentStatus,
        KnowledgeDocumentStatus, KnowledgeEmbeddingRebuildResult, KnowledgeJobState,
        KnowledgeJobView, KnowledgeOverview, KnowledgeQueryScope, KnowledgeRebuildResult,
        KnowledgeSearchHit, KnowledgeSearchRequest, KnowledgeSearchResponse,
        MINERU_CONSENT_POLICY_VERSION,
    },
};
use crate::settings::app_types::KnowledgeRetrievalMode;

const STALE_JOB_AFTER_MS: i64 = 5 * 60 * 1_000;
const KNOWLEDGE_JOB_LEASE_MS: i64 = 60 * 1_000;
const MAX_MARKDOWN_ASSET_BYTES: u64 = 50 * 1024 * 1024;
const PDF_PARSER_ID: &str = "mineru-cloud-v4";
const PDF_PARSER_VERSION: &str = "4";
const PDF_NORMALIZATION_VERSION: &str = "pdf-elements-v1";
const PDF_CHUNK_POLICY_VERSION: &str = "structured-v1";
const LOCAL_PDF_PARSER_ID: &str = "lopdf-text-fallback";
const LOCAL_PDF_PARSER_VERSION: &str = "1";
const PDF_REVISION_MANIFEST_FILE: &str = "mnemora_manifest.json";
const PDF_CANONICAL_CONTENT_FILE: &str = "content.txt";
const MAX_PDF_CANONICAL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct KnowledgeRepository {
    database_path: PathBuf,
    library_root: PathBuf,
}

#[derive(Debug, Clone)]
struct SourceDocument {
    id: String,
    source_id: String,
    title: String,
    source_hash: String,
    active_revision_id: Option<String>,
    state: String,
    cloud_consent_state: String,
}

#[derive(Debug, Clone)]
struct NoteInput {
    title: String,
    content: String,
    directory_path: Option<String>,
    content_hash: Option<String>,
}

#[derive(Debug, Clone)]
struct PreparedAsset {
    id: String,
    relative_path: String,
    mime_type: String,
    byte_size: u64,
    sha256: String,
    alt_text: String,
    caption: String,
    source_asset_name: String,
}

#[derive(Debug, Clone)]
struct PreparedChunk {
    id: String,
    block_kind: String,
    text: String,
    search_text: String,
    heading_path_json: String,
    element_ids_json: String,
    asset_ids_json: String,
    line_start: Option<usize>,
    line_end: Option<usize>,
    byte_start: usize,
    byte_end: usize,
    char_start: usize,
    char_end: usize,
    ordinal: usize,
    is_overlap: bool,
}

#[derive(Debug, Clone)]
struct PreparedElement {
    id: String,
    ordinal: usize,
    element_type: String,
    block_kind: String,
    text: String,
    raw_text: String,
    search_text: String,
    heading_path_json: String,
    line_start: usize,
    line_end: usize,
    byte_start: usize,
    byte_end: usize,
    char_start: usize,
    char_end: usize,
    asset_ids: Vec<String>,
}

/// A lease returned by the durable job queue.  Every later write made by a
/// worker must carry this identity; a stale worker can therefore never publish
/// a result after another runtime has taken over the job.
#[derive(Debug, Clone)]
pub(crate) struct KnowledgeJobClaim {
    pub job_id: String,
    pub document_id: String,
    pub source_id: String,
    pub source_hash: String,
    pub original_name: String,
    pub consent_id: Option<String>,
    pub cloud_consent_granted: bool,
    pub execution_version: i64,
    pub state_version: i64,
    pub runtime_instance_id: String,
    pub lease_token: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EmbeddingJobClaim {
    pub common: KnowledgeJobClaim,
    pub revision_id: String,
    pub embedding_key: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EmbeddingChunkInput {
    pub chunk_id: String,
    pub content_hash: String,
    pub search_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EmbeddingWrite {
    pub chunk_id: String,
    pub content_hash: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EmbeddingQueueSummary {
    pub queued_job_count: usize,
    pub cached_chunk_count: usize,
    pub pending_chunk_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PdfCommitResult {
    pub revision_id: String,
    pub partial: bool,
}

#[derive(Debug, Clone)]
struct PdfElementRecord {
    id: String,
    ordinal: usize,
    element_type: String,
    block_kind: String,
    provider_element_id: Option<String>,
    page_index: Option<u32>,
    page_end: Option<u32>,
    page_width: Option<f64>,
    page_height: Option<f64>,
    bbox: Option<[f64; 4]>,
    text: String,
    raw_text: String,
    ocr_text: String,
    formula_latex: String,
    table_html: String,
    table_json: String,
    caption: String,
    heading_path: Vec<String>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    byte_start: Option<usize>,
    byte_end: Option<usize>,
    char_start: Option<usize>,
    char_end: Option<usize>,
    asset_names: Vec<String>,
    asset_ids: Vec<String>,
    metadata: JsonValue,
    quality_flags: Vec<String>,
}

#[derive(Debug, Clone)]
struct PdfAssetRecord {
    id: String,
    archive_name: String,
    relative_path: String,
    asset_kind: String,
    mime_type: String,
    byte_size: u64,
    sha256: String,
    width_px: Option<u32>,
    height_px: Option<u32>,
    page_index: Option<u32>,
    page_width: Option<f64>,
    page_height: Option<f64>,
    bbox: Option<[f64; 4]>,
    alt_text: String,
    caption: String,
    source_asset_name: String,
    metadata: JsonValue,
}

#[derive(Debug, Clone)]
struct PdfChunkRecord {
    id: String,
    ordinal: usize,
    block_kind: String,
    text: String,
    search_text: String,
    heading_path: Vec<String>,
    element_ids: Vec<String>,
    asset_ids: Vec<String>,
    page_start: Option<u32>,
    page_end: Option<u32>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    byte_start: usize,
    byte_end: usize,
    char_start: Option<usize>,
    char_end: Option<usize>,
    page_bboxes: Vec<JsonValue>,
    quality_flags: Vec<String>,
    is_overlap: bool,
}

#[derive(Debug, Clone)]
struct PreparedPdfRevision {
    revision_id: String,
    source_hash: String,
    canonical_text: String,
    page_count: u32,
    elements: Vec<PdfElementRecord>,
    assets: Vec<PdfAssetRecord>,
    chunks: Vec<PdfChunkRecord>,
    parser_id: String,
    parser_version: String,
    provider_id: String,
    parser_config_hash: String,
    parser_config_json: String,
    provider_task_id: Option<String>,
    provider_batch_id: Option<String>,
    provider_result_hash: Option<String>,
    consent_id: Option<String>,
    remote_upload: bool,
    normalization_version: String,
    chunk_policy_version: String,
    extraction_quality: String,
    quality_flags: Vec<String>,
    warnings: Vec<String>,
    content_path: String,
    manifest_path: String,
    provider_archive_path: String,
}

impl KnowledgeRepository {
    pub fn new(library: &LibraryRepository) -> Self {
        Self {
            database_path: library.database_path.clone(),
            library_root: library.root_directory.clone(),
        }
    }

    fn open_connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.database_path)
            .map_err(|error| format!("打开知识库数据库失败：{error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("设置知识库数据库等待时间失败：{error}"))?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| format!("启用知识库外键失败：{error}"))?;
        Ok(connection)
    }

    pub fn overview(&self) -> Result<KnowledgeOverview, String> {
        let connection = self.open_connection()?;
        let (fts5_available, tokenizer, lexical_degraded) = connection
            .query_row(
                "SELECT fts5_available, tokenizer, lexical_degraded
                 FROM knowledge_index_capabilities WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? != 0,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)? != 0,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("读取知识库索引能力失败：{error}"))?
            .unwrap_or((false, "none".to_string(), true));
        let document_count = count_documents(&connection, "1 = 1", &[])?;
        let literature_count = count_documents(&connection, "source_class = 'literature'", &[])?;
        let note_count = count_documents(&connection, "source_class = 'note'", &[])?;
        let ready_count = count_documents(
            &connection,
            "state IN ('ready', 'lexical_ready') AND active_revision_id IS NOT NULL",
            &[],
        )?;
        let pending_count = count_documents(
            &connection,
            "state IN ('pending', 'awaiting_consent', 'remote_pending', 'remote_running', 'normalizing')",
            &[],
        )?;
        let failed_count = count_documents(
            &connection,
            "state IN ('failed', 'degraded', 'stale', 'partial')",
            &[],
        )?;
        let active_job_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_index_jobs
                 WHERE state IN ('queued', 'running', 'cancelling', 'paused')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("统计知识库任务失败：{error}"))?;
        let last_indexed_at: Option<i64> = connection
            .query_row(
                "SELECT MAX(updated_at) FROM knowledge_revisions
                 WHERE status IN ('ready', 'lexical_ready', 'partial')",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("读取知识库最近索引时间失败：{error}"))?;
        let (embedding_ready_count, embedding_pending_rows, embedding_failed_rows) = connection
            .query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN e.status = 'ready' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.status = 'pending' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.status IN ('failed', 'cancelled', 'stale') THEN 1 ELSE 0 END), 0)
                 FROM knowledge_embeddings e
                 JOIN knowledge_chunks c ON c.id = e.chunk_id
                 JOIN knowledge_revisions r ON r.id = c.revision_id
                 JOIN knowledge_documents d ON d.id = r.document_id
                 WHERE d.state <> 'deleted' AND d.active_revision_id = r.id
                   AND r.source_hash = d.current_source_hash",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .map_err(|error| format!("统计知识库向量状态失败：{error}"))?;
        // Unknown-dimension jobs intentionally do not create `pending`
        // embedding rows: the schema requires a dimension before a row can be
        // inserted.  Derive the live state from durable embedding jobs so the
        // overview remains accurate while the first remote response is still
        // in flight.  Failed job diagnostics are limited to the newest
        // terminal job for a revision; an old route cancelled by a later
        // rebuild must not keep the current index in a permanent warning.
        let embedding_pending_job_units: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(
                    CASE WHEN j.total_units > j.completed_units
                         THEN j.total_units - j.completed_units ELSE 0 END
                 ), 0)
                 FROM knowledge_index_jobs j
                 JOIN knowledge_revisions r ON r.id = j.revision_id
                 JOIN knowledge_documents d ON d.id = r.document_id
                 WHERE j.job_kind = 'embed'
                   AND j.state IN ('queued', 'running', 'cancelling', 'paused')
                   AND d.state <> 'deleted'
                   AND d.active_revision_id = r.id
                   AND j.requested_source_hash = d.current_source_hash
                   AND r.source_hash = d.current_source_hash",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("统计知识库待生成向量任务失败：{error}"))?;
        let embedding_failed_job_units: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(
                    CASE WHEN j.total_units > j.completed_units
                         THEN j.total_units - j.completed_units ELSE 1 END
                 ), 0)
                 FROM knowledge_index_jobs j
                 JOIN knowledge_revisions r ON r.id = j.revision_id
                 JOIN knowledge_documents d ON d.id = r.document_id
                 WHERE j.job_kind = 'embed'
                   AND j.state IN ('failed', 'cancelled', 'stale')
                   AND d.state <> 'deleted'
                   AND d.active_revision_id = r.id
                   AND j.requested_source_hash = d.current_source_hash
                   AND r.source_hash = d.current_source_hash
                   AND NOT EXISTS (
                       SELECT 1 FROM knowledge_index_jobs newer
                       WHERE newer.job_kind = 'embed'
                         AND newer.revision_id = j.revision_id
                         AND (newer.created_at > j.created_at
                              OR (newer.created_at = j.created_at AND newer.id > j.id))
                   )",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("统计知识库失败向量任务失败：{error}"))?;
        let embedding_pending_count =
            to_usize(embedding_pending_rows).max(to_usize(embedding_pending_job_units));
        let embedding_failed_count =
            to_usize(embedding_failed_rows).max(to_usize(embedding_failed_job_units));
        let mut dimensions_statement = connection
            .prepare(
                "SELECT DISTINCT e.dimensions
                 FROM knowledge_embeddings e
                 JOIN knowledge_chunks c ON c.id = e.chunk_id
                 JOIN knowledge_revisions r ON r.id = c.revision_id
                 JOIN knowledge_documents d ON d.id = r.document_id
                 WHERE e.status = 'ready' AND d.state <> 'deleted'
                   AND d.active_revision_id = r.id
                   AND r.source_hash = d.current_source_hash
                 ORDER BY e.dimensions ASC",
            )
            .map_err(|error| format!("准备知识库向量维度统计失败：{error}"))?;
        let embedding_dimensions = dimensions_statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(|error| format!("查询知识库向量维度失败：{error}"))?
            .map(|row| {
                row.map(to_u32)
                    .map_err(|error| format!("读取知识库向量维度失败：{error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(KnowledgeOverview {
            document_count: to_usize(document_count),
            literature_count: to_usize(literature_count),
            note_count: to_usize(note_count),
            ready_count: to_usize(ready_count),
            pending_count: to_usize(pending_count),
            failed_count: to_usize(failed_count),
            active_job_count: to_usize(active_job_count),
            fts5_available,
            tokenizer,
            lexical_degraded,
            embedding_ready_count: to_usize(embedding_ready_count),
            embedding_pending_count,
            embedding_failed_count,
            embedding_dimensions,
            last_indexed_at: last_indexed_at.map(to_u64),
        })
    }

    pub fn list_documents(&self, limit: usize) -> Result<Vec<KnowledgeDocumentStatus>, String> {
        let limit = limit.clamp(1, 500);
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT d.id, d.source_class, d.source_kind, d.source_id, d.title,
                        d.state, d.cloud_consent_state, d.active_revision_id,
                        d.current_source_hash, d.updated_at,
                        r.extraction_quality, r.chunk_count, r.asset_count, r.warning_json
                 FROM knowledge_documents d
                 LEFT JOIN knowledge_revisions r ON r.id = d.active_revision_id
                 ORDER BY d.updated_at DESC, d.id ASC LIMIT ?",
            )
            .map_err(|error| format!("准备知识文档列表失败：{error}"))?;
        let rows = statement
            .query_map(params![i64::try_from(limit).unwrap_or(500)], |row| {
                let warning_json = row.get::<_, Option<String>>(13)?.unwrap_or_default();
                Ok(KnowledgeDocumentStatus {
                    id: row.get(0)?,
                    source_class: row.get(1)?,
                    source_kind: row.get(2)?,
                    source_id: row.get(3)?,
                    title: row.get(4)?,
                    state: row.get(5)?,
                    cloud_consent_state: row.get(6)?,
                    active_revision_id: row.get(7)?,
                    source_hash: row.get(8)?,
                    updated_at: to_u64(row.get::<_, i64>(9)?),
                    extraction_quality: row.get(10)?,
                    chunk_count: to_usize(row.get::<_, Option<i64>>(11)?.unwrap_or(0)),
                    asset_count: to_usize(row.get::<_, Option<i64>>(12)?.unwrap_or(0)),
                    warning_count: serde_json::from_str::<Vec<serde_json::Value>>(&warning_json)
                        .map(|items| items.len())
                        .unwrap_or(0),
                })
            })
            .map_err(|error| format!("查询知识文档列表失败：{error}"))?;
        let mut documents = Vec::new();
        for row in rows {
            documents.push(row.map_err(|error| format!("读取知识文档列表失败：{error}"))?);
        }
        Ok(documents)
    }

    pub fn list_jobs(&self, limit: usize) -> Result<Vec<KnowledgeJobView>, String> {
        let limit = limit.clamp(1, super::types::MAX_KNOWLEDGE_JOB_LIMIT);
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, job_kind, document_id, revision_id, state, stage,
                        completed_units, total_units, error_code, error_message,
                        created_at, updated_at, finished_at
                 FROM knowledge_index_jobs
                 ORDER BY CASE state
                    WHEN 'running' THEN 0 WHEN 'cancelling' THEN 1 WHEN 'queued' THEN 2
                    ELSE 3 END, updated_at DESC, id ASC LIMIT ?",
            )
            .map_err(|error| format!("准备知识库任务列表失败：{error}"))?;
        let rows = statement
            .query_map(params![i64::try_from(limit).unwrap_or(200)], |row| {
                Ok(KnowledgeJobView {
                    id: row.get(0)?,
                    job_kind: row.get(1)?,
                    document_id: row.get(2)?,
                    revision_id: row.get(3)?,
                    state: row.get(4)?,
                    stage: row.get(5)?,
                    completed_units: to_usize(row.get::<_, i64>(6)?),
                    total_units: to_usize(row.get::<_, i64>(7)?),
                    error_code: row.get(8)?,
                    error_message: row.get(9)?,
                    created_at: to_u64(row.get::<_, i64>(10)?),
                    updated_at: to_u64(row.get::<_, i64>(11)?),
                    finished_at: row.get::<_, Option<i64>>(12)?.map(to_u64),
                })
            })
            .map_err(|error| format!("查询知识库任务列表失败：{error}"))?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|error| format!("读取知识库任务失败：{error}"))?);
        }
        Ok(jobs)
    }

    /// Register a PDF as a knowledge source without creating a processing
    /// job.  This is used by automatic library notifications when the user
    /// has disabled "parse after import"; registration is local metadata only.
    pub fn register_literature(&self, item_id: &str) -> Result<KnowledgeDocumentStatus, String> {
        self.ensure_literature(item_id, false)
    }

    /// Create a controlled extraction job for a PDF.  The job is still gated
    /// by the current MinerU consent and can therefore remain in
    /// `awaiting_consent` until the user explicitly authorizes the upload.
    pub fn enqueue_literature(&self, item_id: &str) -> Result<KnowledgeDocumentStatus, String> {
        self.ensure_literature(item_id, true)
    }

    fn ensure_literature(
        &self,
        item_id: &str,
        queue_processing: bool,
    ) -> Result<KnowledgeDocumentStatus, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        let source = connection
            .query_row(
                "SELECT i.title, f.file_hash, i.deleted_at
                 FROM library_items i
                 JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
                 WHERE i.id = ? AND i.item_type = 'pdf'",
                params![item_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("读取文献知识源失败：{error}"))?
            .ok_or_else(|| "文献不存在或没有主 PDF 文件。".to_string())?;
        if source.2.is_some() {
            return self.mark_literature_deleted(&item_id);
        }
        let active_consent_id = active_literature_consent_id(&connection, &item_id, &source.1)?;
        let consent_granted = active_consent_id.is_some();
        let initial_state = if consent_granted {
            "pending"
        } else {
            "awaiting_consent"
        };
        let cloud_consent_state = if consent_granted {
            "granted"
        } else {
            "awaiting"
        };
        let now = now_ms();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("开始登记 PDF 知识源失败：{error}"))?;
        let (document, _) = ensure_document_tx(
            &transaction,
            "literature",
            "pdf",
            &item_id,
            &source.0,
            &source.1,
            Some(&item_id),
            None,
            initial_state,
            cloud_consent_state,
            now,
        )?;
        transaction
            .execute(
                "UPDATE knowledge_documents
                 SET cloud_consent_state = ?,
                     state = CASE
                       WHEN active_revision_id IS NULL
                            AND state IN ('awaiting_consent', 'failed', 'stale', 'pending')
                       THEN ? ELSE state END,
                     updated_at = ?
                 WHERE id = ? AND state <> 'deleted'",
                params![
                    cloud_consent_state,
                    if consent_granted {
                        "pending"
                    } else {
                        "awaiting_consent"
                    },
                    now,
                    document.id
                ],
            )
            .map_err(|error| format!("更新 PDF 云端授权门状态失败：{error}"))?;
        if queue_processing {
            let (job_id, _) = ensure_job_tx(
                &transaction,
                "extract",
                &document.id,
                &source.1,
                if consent_granted {
                    "queued"
                } else {
                    "awaiting_consent"
                },
                0,
                now,
            )?;
            transaction
                .execute(
                    "UPDATE knowledge_index_jobs
                     SET state = 'queued', stage = ?, consent_id = ?,
                         cancel_requested_at = NULL, error_code = NULL,
                         error_message = NULL, finished_at = NULL, updated_at = ?
                     WHERE id = ? AND state IN ('queued', 'paused')",
                    params![
                        if consent_granted {
                            "queued"
                        } else {
                            "awaiting_consent"
                        },
                        active_consent_id,
                        now,
                        job_id
                    ],
                )
                .map_err(|error| format!("failed to attach PDF consent to job: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交 PDF 知识源登记失败：{error}"))?;
        self.get_document_status(&document.id)
    }

    /// Record an explicit user decision that permits the current PDF bytes to
    /// be sent to MinerU Cloud.  A consent is bound to the source hash, so a
    /// later replacement of the library file must be authorized again.
    pub fn grant_literature_consent(
        &self,
        item_id: &str,
        scope: &str,
    ) -> Result<KnowledgeDocumentStatus, String> {
        let item_id = normalize_identifier("literature ID", item_id)?;
        let scope = scope.trim();
        if !matches!(scope, "document" | "global") {
            return Err("consent scope must be document or global".to_string());
        }
        // Consent is an explicit user action.  Registering the source first
        // creates only local metadata; the transaction below is the point at
        // which the upload permission and the processing job are coupled.
        let _ = self.register_literature(&item_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin MinerU consent transaction: {error}"))?;
        let (document_id, source_hash): (String, String) = transaction
            .query_row(
                "SELECT d.id, d.current_source_hash
                 FROM knowledge_documents d
                 JOIN library_items i ON i.id = d.library_item_id
                 WHERE d.source_class = 'literature' AND d.source_id = ?
                   AND i.item_type = 'pdf' AND i.deleted_at IS NULL",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("failed to read literature consent target: {error}"))?
            .ok_or_else(|| "literature source is unavailable".to_string())?;
        if source_hash.trim().is_empty() {
            return Err("literature source has no content hash".to_string());
        }
        let now = now_ms();
        let consent_id = if scope == "global" {
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT id FROM knowledge_cloud_consents
                     WHERE provider_id = 'mineru-cloud' AND policy_version = ?
                       AND scope_key = 'local-library' AND scope = 'global'
                       AND revoked_at IS NULL
                     ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
                    params![MINERU_CONSENT_POLICY_VERSION],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| format!("failed to read global MinerU consent: {error}"))?;
            if let Some(id) = existing {
                transaction
                    .execute(
                        "UPDATE knowledge_cloud_consents
                         SET updated_at = ?, granted_at = ?, revoked_at = NULL
                         WHERE id = ?",
                        params![now, now, id],
                    )
                    .map_err(|error| format!("failed to refresh global MinerU consent: {error}"))?;
                id
            } else {
                let id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO knowledge_cloud_consents (
                            id, scope_key, document_id, source_hash, provider_id,
                            policy_version, scope, granted_at, token_fingerprint,
                            pages_estimate, created_at, updated_at
                         ) VALUES (?, 'local-library', NULL, '', 'mineru-cloud', ?, 'global', ?, '', 0, ?, ?) ",
                        params![id, MINERU_CONSENT_POLICY_VERSION, now, now, now],
                    )
                    .map_err(|error| format!("failed to create global MinerU consent: {error}"))?;
                id
            }
        } else {
            transaction
                .execute(
                    "UPDATE knowledge_cloud_consents
                      SET revoked_at = COALESCE(revoked_at, ?), updated_at = ?
                      WHERE document_id = ? AND scope = 'document'
                        AND provider_id = 'mineru-cloud'
                        AND policy_version = ? AND scope_key = 'local-library'
                        AND revoked_at IS NULL",
                    params![now, now, document_id, MINERU_CONSENT_POLICY_VERSION],
                )
                .map_err(|error| format!("failed to close previous MinerU consent: {error}"))?;
            let id = Uuid::new_v4().to_string();
            transaction
                .execute(
                    "INSERT INTO knowledge_cloud_consents (
                        id, scope_key, document_id, source_hash, provider_id,
                        policy_version, scope, granted_at, token_fingerprint,
                        pages_estimate, created_at, updated_at
                     ) VALUES (?, 'local-library', ?, ?, 'mineru-cloud', ?, 'document', ?, '', ?, ?, ?)",
                    params![
                        id,
                        document_id,
                        source_hash,
                        MINERU_CONSENT_POLICY_VERSION,
                        now,
                         0,
                        now,
                        now
                    ],
                )
                .map_err(|error| format!("failed to create MinerU document consent: {error}"))?;
            id
        };

        if scope == "global" {
            // A global decision applies to every current PDF source in this
            // local library.  Existing ready revisions are retained; only
            // missing, failed, stale, or consent-waiting sources receive a
            // new extraction job.
            let sources = load_active_pdf_sources(&transaction)?;
            for (source_id, title, hash) in sources {
                let (document, _) = ensure_document_tx(
                    &transaction,
                    "literature",
                    "pdf",
                    &source_id,
                    &title,
                    &hash,
                    Some(&source_id),
                    None,
                    "pending",
                    "granted",
                    now,
                )?;
                transaction
                    .execute(
                        "UPDATE knowledge_documents
                         SET cloud_consent_state = 'granted',
                             state = CASE
                               WHEN active_revision_id IS NULL
                                    OR state IN ('awaiting_consent', 'failed', 'stale')
                               THEN 'pending' ELSE state END,
                             updated_at = ?
                         WHERE id = ? AND state <> 'deleted'",
                        params![now, document.id],
                    )
                    .map_err(|error| {
                        format!("failed to refresh global PDF document state: {error}")
                    })?;
                let (job_id, _) = ensure_job_tx(
                    &transaction,
                    "extract",
                    &document.id,
                    &hash,
                    "queued",
                    0,
                    now,
                )?;
                let effective_consent =
                    active_literature_consent_id_on(&transaction, &source_id, &hash)?
                        .or_else(|| Some(consent_id.clone()));
                bind_extract_job_tx(&transaction, &job_id, effective_consent.as_deref(), now)?;
            }
        } else {
            transaction
                .execute(
                    "UPDATE knowledge_documents
                     SET cloud_consent_state = 'granted',
                         state = CASE
                           WHEN active_revision_id IS NULL
                                OR state IN ('awaiting_consent', 'failed', 'stale')
                           THEN 'pending' ELSE state END,
                         updated_at = ? WHERE id = ?",
                    params![now, document_id],
                )
                .map_err(|error| format!("failed to grant MinerU document consent: {error}"))?;
        }

        // Requeue a failed/cancelled extraction for the current hash and bind
        // the consent id.  `ensure_job_tx` gives retries a new idempotency key
        // while preserving the old diagnostic record.
        let (job_id, _) = ensure_job_tx(
            &transaction,
            "extract",
            &document_id,
            &source_hash,
            "queued",
            0,
            now,
        )?;
        let effective_selected_consent_id = if scope == "global" {
            active_literature_consent_id_on(&transaction, &item_id, &source_hash)?
                .or_else(|| Some(consent_id.clone()))
        } else {
            Some(consent_id.clone())
        };
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET consent_id = ?, stage = 'queued', state = 'queued',
                     cancel_requested_at = NULL, error_code = NULL,
                     error_message = NULL, finished_at = NULL, updated_at = ?
                 WHERE id = ? AND state IN ('queued', 'paused')",
                params![effective_selected_consent_id, now, job_id],
            )
            .map_err(|error| format!("failed to attach MinerU consent to job: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit MinerU consent: {error}"))?;
        self.get_document_status(&document_id)
    }

    /// Grant library-wide consent without depending on the renderer's current
    /// document list.  A PDF can already exist in `library_items` while the
    /// asynchronous knowledge registration is still catching up; selecting a
    /// source from the authoritative library here keeps the global action
    /// usable during that short window.  The existing global branch then
    /// discovers and registers every active PDF in the same consent flow.
    pub fn grant_global_literature_consent(&self) -> Result<usize, String> {
        let item_id: Option<String> = {
            let connection = self.open_connection()?;
            connection
                .query_row(
                    "SELECT i.id
                     FROM library_items i
                     JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
                     WHERE i.item_type = 'pdf' AND i.deleted_at IS NULL
                     ORDER BY i.updated_at DESC, i.id ASC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| {
                    format!("failed to find an active PDF for global consent: {error}")
                })?
        };
        let item_id =
            item_id.ok_or_else(|| "literature library has no active PDF sources".to_string())?;
        // The existing implementation performs the consent write and all
        // document/job updates atomically after registering this authoritative
        // first source.  It also preserves document-scoped consent precedence.
        let _ = self.grant_literature_consent(&item_id, "global")?;
        let connection = self.open_connection()?;
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_documents
                 WHERE source_class = 'literature' AND source_kind = 'pdf' AND state <> 'deleted'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to count globally consented PDF sources: {error}"))?;
        Ok(to_usize(count))
    }

    pub fn revoke_literature_consent(&self, item_id: &str) -> Result<bool, String> {
        let item_id = normalize_identifier("literature ID", item_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin MinerU consent revoke: {error}"))?;
        let target: Option<(String, String)> = transaction
            .query_row(
                "SELECT id, current_source_hash FROM knowledge_documents
                 WHERE source_class = 'literature' AND source_id = ?",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("failed to find literature consent target: {error}"))?;
        let Some((document_id, source_hash)) = target else {
            transaction
                .commit()
                .map_err(|error| format!("failed to finish empty consent revoke: {error}"))?;
            return Ok(false);
        };
        let running: bool = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM knowledge_index_jobs
                    WHERE document_id = ? AND state IN ('running', 'cancelling')
                )",
                params![document_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to inspect active MinerU job: {error}"))?;
        if running {
            return Err("stop the active MinerU job before revoking consent".to_string());
        }
        let now = now_ms();
        let changed = transaction
            .execute(
                "UPDATE knowledge_cloud_consents
                 SET revoked_at = COALESCE(revoked_at, ?), updated_at = ?
                 WHERE document_id = ? AND scope = 'document'
                   AND provider_id = 'mineru-cloud'
                   AND policy_version = ? AND scope_key = 'local-library'
                   AND revoked_at IS NULL",
                params![now, now, document_id, MINERU_CONSENT_POLICY_VERSION],
            )
            .map_err(|error| format!("failed to revoke MinerU consent: {error}"))?;
        let global_consent_id = active_global_literature_consent_id_on(&transaction)?;
        let document_consent_id =
            active_document_literature_consent_id_on(&transaction, &document_id, &source_hash)?;
        let effective_consent_id = document_consent_id.or(global_consent_id);
        let effective_granted = effective_consent_id.is_some();
        transaction
            .execute(
                "UPDATE knowledge_documents
                 SET cloud_consent_state = ?,
                     state = CASE WHEN active_revision_id IS NULL
                                  THEN ? ELSE state END,
                     updated_at = ? WHERE id = ?",
                params![
                    if effective_granted {
                        "granted"
                    } else {
                        "revoked"
                    },
                    if effective_granted {
                        "pending"
                    } else {
                        "awaiting_consent"
                    },
                    now,
                    document_id
                ],
            )
            .map_err(|error| format!("failed to mark literature consent revoked: {error}"))?;
        bind_extract_jobs_for_document_tx(
            &transaction,
            &document_id,
            effective_consent_id.as_deref(),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit MinerU consent revoke: {error}"))?;
        Ok(changed > 0)
    }

    /// Revoke the library-wide MinerU permission without touching any
    /// document-scoped decisions.  Existing active revisions remain readable;
    /// only future cloud uploads are gated again.
    pub fn revoke_global_literature_consent(&self) -> Result<bool, String> {
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin global MinerU consent revoke: {error}"))?;
        let running: bool = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM knowledge_index_jobs j
                    JOIN knowledge_documents d ON d.id = j.document_id
                    WHERE j.job_kind = 'extract'
                      AND d.source_class = 'literature'
                      AND j.state IN ('running', 'cancelling')
                )",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to inspect active global MinerU jobs: {error}"))?;
        if running {
            return Err("stop active MinerU jobs before revoking global consent".to_string());
        }
        let now = now_ms();
        let changed = transaction
            .execute(
                "UPDATE knowledge_cloud_consents
                 SET revoked_at = COALESCE(revoked_at, ?), updated_at = ?
                 WHERE provider_id = 'mineru-cloud'
                   AND policy_version = ? AND scope_key = 'local-library'
                   AND scope = 'global' AND revoked_at IS NULL",
                params![now, now, MINERU_CONSENT_POLICY_VERSION],
            )
            .map_err(|error| format!("failed to revoke global MinerU consent: {error}"))?;
        if changed == 0 {
            transaction.commit().map_err(|error| {
                format!("failed to finish empty global consent revoke: {error}")
            })?;
            return Ok(false);
        }
        let documents = load_literature_documents(&transaction)?;
        for (document_id, source_hash) in documents {
            let document_consent =
                active_document_literature_consent_id_on(&transaction, &document_id, &source_hash)?;
            transaction
                .execute(
                    "UPDATE knowledge_documents
                     SET cloud_consent_state = ?,
                         state = CASE WHEN active_revision_id IS NULL
                                      THEN ? ELSE state END,
                         updated_at = ? WHERE id = ?",
                    params![
                        if document_consent.is_some() {
                            "granted"
                        } else {
                            "revoked"
                        },
                        if document_consent.is_some() {
                            "pending"
                        } else {
                            "awaiting_consent"
                        },
                        now,
                        document_id
                    ],
                )
                .map_err(|error| {
                    format!("failed to update global consent document state: {error}")
                })?;
            bind_extract_jobs_for_document_tx(
                &transaction,
                &document_id,
                document_consent.as_deref(),
                now,
            )?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit global MinerU consent revoke: {error}"))?;
        Ok(true)
    }

    pub fn literature_consent_status(
        &self,
        item_id: &str,
    ) -> Result<Option<KnowledgeConsentStatus>, String> {
        let item_id = normalize_identifier("literature ID", item_id)?;
        let connection = self.open_connection()?;
        let target: Option<(String, String, String)> = connection
            .query_row(
                "SELECT id, source_id, current_source_hash
                 FROM knowledge_documents
                 WHERE source_class = 'literature' AND source_id = ?",
                params![item_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| format!("failed to read MinerU consent target: {error}"))?;
        let Some((document_id, source_id, source_hash)) = target else {
            return Ok(None);
        };
        let current_document: Option<(String, i64, Option<i64>)> = connection
            .query_row(
                "SELECT id, granted_at, revoked_at
                 FROM knowledge_cloud_consents
                 WHERE document_id = ? AND source_hash = ? AND scope = 'document'
                   AND provider_id = 'mineru-cloud' AND policy_version = ?
                   AND scope_key = 'local-library'
                 ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
                params![document_id, source_hash, MINERU_CONSENT_POLICY_VERSION],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| format!("failed to read current document consent: {error}"))?;
        let any_document_consent: bool = connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM knowledge_cloud_consents
                    WHERE document_id = ? AND scope = 'document'
                      AND provider_id = 'mineru-cloud' AND policy_version = ?
                      AND scope_key = 'local-library'
                )",
                params![document_id, MINERU_CONSENT_POLICY_VERSION],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to inspect historical document consent: {error}"))?;
        let global: Option<(String, i64, Option<i64>)> = connection
            .query_row(
                "SELECT id, granted_at, revoked_at
                 FROM knowledge_cloud_consents
                 WHERE scope = 'global' AND provider_id = 'mineru-cloud'
                   AND policy_version = ? AND scope_key = 'local-library'
                 ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
                params![MINERU_CONSENT_POLICY_VERSION],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| format!("failed to read global consent: {error}"))?;

        let document_granted = current_document
            .as_ref()
            .is_some_and(|(_, _, revoked_at)| revoked_at.is_none());
        let global_granted = global
            .as_ref()
            .is_some_and(|(_, _, revoked_at)| revoked_at.is_none());
        let document_consent_state = if document_granted {
            "granted"
        } else if current_document
            .as_ref()
            .is_some_and(|(_, _, revoked_at)| revoked_at.is_some())
        {
            "revoked"
        } else if any_document_consent {
            "stale"
        } else {
            "none"
        };
        let global_consent_state = if global_granted {
            "granted"
        } else if global.is_some() {
            "revoked"
        } else {
            "none"
        };
        let (effective_scope, effective_granted_at) = if document_granted {
            (
                Some("document".to_string()),
                current_document
                    .as_ref()
                    .map(|(_, granted_at, _)| *granted_at),
            )
        } else if global_granted {
            (
                Some("global".to_string()),
                global.as_ref().map(|(_, granted_at, _)| *granted_at),
            )
        } else {
            (None, None)
        };
        let document_revoked_at = current_document
            .as_ref()
            .and_then(|(_, _, value)| value.map(to_u64));
        let global_revoked_at = global.as_ref().and_then(|(_, _, value)| value.map(to_u64));
        let revoked_at = [document_revoked_at, global_revoked_at]
            .into_iter()
            .flatten()
            .max();
        Ok(Some(KnowledgeConsentStatus {
            document_id,
            source_id,
            source_hash,
            provider_id: "mineru-cloud".to_string(),
            effective_scope: effective_scope.clone(),
            scope: effective_scope.unwrap_or_else(|| "none".to_string()),
            granted: document_granted || global_granted,
            document_granted,
            global_granted,
            document_consent_state: document_consent_state.to_string(),
            global_consent_state: global_consent_state.to_string(),
            document_source_hash_matches: current_document.is_some(),
            revoked: !(document_granted || global_granted)
                && (document_consent_state == "revoked" || global_consent_state == "revoked"),
            document_granted_at: current_document
                .as_ref()
                .map(|(_, granted_at, _)| to_u64(*granted_at)),
            global_granted_at: global
                .as_ref()
                .map(|(_, granted_at, _)| to_u64(*granted_at)),
            document_revoked_at,
            global_revoked_at,
            granted_at: effective_granted_at.map(to_u64),
            revoked_at,
        }))
    }

    pub fn global_literature_consent_status(
        &self,
    ) -> Result<super::types::KnowledgeGlobalConsentStatus, String> {
        let connection = self.open_connection()?;
        let latest: Option<(i64, Option<i64>)> = connection
            .query_row(
                "SELECT granted_at, revoked_at
                 FROM knowledge_cloud_consents
                 WHERE provider_id = 'mineru-cloud' AND policy_version = ?
                   AND scope_key = 'local-library' AND scope = 'global'
                 ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
                params![MINERU_CONSENT_POLICY_VERSION],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("failed to read global MinerU consent status: {error}"))?;
        let granted = latest
            .as_ref()
            .is_some_and(|(_, revoked_at)| revoked_at.is_none());
        let revoked_at = latest
            .as_ref()
            .and_then(|(_, revoked_at)| revoked_at.map(to_u64));
        Ok(super::types::KnowledgeGlobalConsentStatus {
            state: if granted {
                "granted"
            } else if latest.is_some() {
                "revoked"
            } else {
                "none"
            }
            .to_string(),
            granted,
            granted_at: latest.as_ref().map(|(granted_at, _)| to_u64(*granted_at)),
            revoked_at,
        })
    }

    /// Claim one queued extraction.  The consent check is repeated in this
    /// transaction so a renderer cannot race a revoke operation and upload a
    /// PDF after permission has been withdrawn.
    pub(crate) fn claim_next_extract_job(
        &self,
        runtime_instance_id: &str,
    ) -> Result<Option<KnowledgeJobClaim>, String> {
        self.claim_next_extract_job_with_fallback(runtime_instance_id, false)
    }

    /// Claim an extraction.  Local text fallback is a recovery path after a
    /// consented claim (for example when MinerU is unavailable); it must never
    /// be used as an implicit way to bypass the cloud-consent gate.
    pub(crate) fn claim_next_extract_job_with_fallback(
        &self,
        runtime_instance_id: &str,
        _allow_local_fallback: bool,
    ) -> Result<Option<KnowledgeJobClaim>, String> {
        let runtime_instance_id = normalize_identifier("runtime instance ID", runtime_instance_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin knowledge job claim: {error}"))?;
        let candidate: Option<(
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            bool,
            i64,
            i64,
        )> = transaction
            .query_row(
                "SELECT j.id, d.id, d.source_id, d.current_source_hash,
                        f.original_name, j.consent_id,
                         EXISTS(
                             SELECT 1 FROM knowledge_cloud_consents c
                             WHERE c.provider_id = 'mineru-cloud'
                               AND c.policy_version = ?
                               AND c.scope_key = 'local-library'
                               AND c.revoked_at IS NULL
                               AND (
                                  (c.scope = 'global')
                                  OR (c.scope = 'document' AND c.id = j.consent_id AND c.document_id = d.id
                                      AND c.source_hash = d.current_source_hash)
                               )
                         ),
                        j.execution_version, j.state_version
                 FROM knowledge_index_jobs j
                 JOIN knowledge_documents d ON d.id = j.document_id
                 JOIN library_items i ON i.id = d.library_item_id
                 JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
                 WHERE j.job_kind = 'extract' AND j.state = 'queued'
                   AND d.source_class = 'literature' AND d.source_kind = 'pdf'
                   AND d.state <> 'deleted' AND i.deleted_at IS NULL
                   AND j.requested_source_hash = d.current_source_hash
                   AND d.cloud_consent_state = 'granted'
                   AND EXISTS (
                       SELECT 1 FROM knowledge_cloud_consents c
                       WHERE c.provider_id = 'mineru-cloud'
                         AND c.policy_version = ?
                         AND c.scope_key = 'local-library'
                         AND c.revoked_at IS NULL
                         AND (
                             c.scope = 'global'
                             OR (c.scope = 'document' AND c.id = j.consent_id
                                 AND c.document_id = d.id
                                 AND c.source_hash = d.current_source_hash)
                         )
                   )
                 ORDER BY j.priority DESC, j.created_at ASC, j.id ASC LIMIT 1",
                params![MINERU_CONSENT_POLICY_VERSION, MINERU_CONSENT_POLICY_VERSION],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("failed to select queued knowledge job: {error}"))?;
        let Some((
            job_id,
            document_id,
            source_id,
            source_hash,
            original_name,
            consent_id,
            cloud_consent_granted,
            execution_version,
            state_version,
        )) = candidate
        else {
            transaction
                .commit()
                .map_err(|error| format!("failed to finish empty knowledge claim: {error}"))?;
            return Ok(None);
        };
        let now = now_ms();
        let lease_token = Uuid::new_v4().to_string();
        let next_state_version = state_version.saturating_add(1);
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'running', stage = 'validating',
                     attempt = attempt + 1,
                     started_at = COALESCE(started_at, ?), heartbeat_at = ?,
                     lease_token = ?, lease_owner = ?, runtime_instance_id = ?,
                     lease_expires_at = ?, state_version = ?, updated_at = ?
                 WHERE id = ? AND state = 'queued'
                   AND execution_version = ? AND state_version = ?",
                params![
                    now,
                    now,
                    lease_token,
                    runtime_instance_id,
                    runtime_instance_id,
                    now.saturating_add(KNOWLEDGE_JOB_LEASE_MS),
                    next_state_version,
                    now,
                    job_id,
                    execution_version,
                    state_version,
                ],
            )
            .map_err(|error| format!("failed to claim knowledge job: {error}"))?;
        if changed != 1 {
            transaction
                .commit()
                .map_err(|error| format!("failed to commit rejected knowledge claim: {error}"))?;
            return Ok(None);
        }
        insert_job_event_tx(
            &transaction,
            &job_id,
            "jobStarted",
            Some("queued"),
            Some("running"),
            execution_version,
            next_state_version,
            r#"{"stage":"validating","worker":"knowledge"}"#,
            None,
            Some(&runtime_instance_id),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit knowledge job claim: {error}"))?;
        Ok(Some(KnowledgeJobClaim {
            job_id,
            document_id,
            source_id,
            source_hash,
            original_name,
            consent_id,
            cloud_consent_granted,
            execution_version,
            state_version: next_state_version,
            runtime_instance_id,
            lease_token,
        }))
    }

    pub(crate) fn claim_next_embedding_job(
        &self,
        runtime_instance_id: &str,
        embedding_key: &str,
    ) -> Result<Option<EmbeddingJobClaim>, String> {
        let runtime_instance_id = normalize_identifier("runtime instance ID", runtime_instance_id)?;
        let embedding_key = embedding_key.trim();
        if embedding_key.is_empty() || embedding_key.len() > 256 {
            return Err("Embedding key is invalid.".to_string());
        }
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin embedding job claim: {error}"))?;
        let candidate: Option<(String, String, String, String, String, String, i64, i64)> =
            transaction
                .query_row(
                    "SELECT j.id, d.id, d.source_id, d.current_source_hash,
                        d.title, r.id, j.execution_version, j.state_version
                 FROM knowledge_index_jobs j
                 JOIN knowledge_documents d ON d.id = j.document_id
                 JOIN knowledge_revisions r ON r.id = j.revision_id
                 WHERE j.job_kind = 'embed' AND j.state = 'queued'
                   AND j.requested_config_hash = ?
                   AND d.state <> 'deleted' AND d.include_in_default_scope = 1
                   AND d.active_revision_id = r.id
                   AND j.requested_source_hash = d.current_source_hash
                   AND r.source_hash = d.current_source_hash
                   AND r.status IN ('ready', 'lexical_ready', 'embedding_pending', 'partial')
                 ORDER BY j.priority DESC, j.created_at ASC, j.id ASC LIMIT 1",
                    [embedding_key],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| format!("failed to select queued embedding job: {error}"))?;
        let Some((
            job_id,
            document_id,
            source_id,
            source_hash,
            title,
            revision_id,
            execution_version,
            state_version,
        )) = candidate
        else {
            transaction
                .commit()
                .map_err(|error| format!("failed to finish empty embedding claim: {error}"))?;
            return Ok(None);
        };
        let now = now_ms();
        let lease_token = Uuid::new_v4().to_string();
        let next_state_version = state_version.saturating_add(1);
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'running', stage = 'embedding', attempt = attempt + 1,
                     provider_state = 'running',
                     started_at = COALESCE(started_at, ?), heartbeat_at = ?,
                     lease_token = ?, lease_owner = ?, runtime_instance_id = ?,
                     lease_expires_at = ?, state_version = ?, updated_at = ?
                 WHERE id = ? AND state = 'queued'
                   AND execution_version = ? AND state_version = ?",
                params![
                    now,
                    now,
                    lease_token,
                    runtime_instance_id,
                    runtime_instance_id,
                    now.saturating_add(KNOWLEDGE_JOB_LEASE_MS),
                    next_state_version,
                    now,
                    job_id,
                    execution_version,
                    state_version,
                ],
            )
            .map_err(|error| format!("failed to claim embedding job: {error}"))?;
        if changed != 1 {
            transaction
                .commit()
                .map_err(|error| format!("failed to commit rejected embedding claim: {error}"))?;
            return Ok(None);
        }
        insert_job_event_tx(
            &transaction,
            &job_id,
            "jobStarted",
            Some("queued"),
            Some("running"),
            execution_version,
            next_state_version,
            r#"{"stage":"embedding","worker":"embedding"}"#,
            None,
            Some(&runtime_instance_id),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit embedding job claim: {error}"))?;
        Ok(Some(EmbeddingJobClaim {
            common: KnowledgeJobClaim {
                job_id,
                document_id,
                source_id,
                source_hash,
                original_name: title,
                consent_id: None,
                cloud_consent_granted: false,
                execution_version,
                state_version: next_state_version,
                runtime_instance_id,
                lease_token,
            },
            revision_id,
            embedding_key: embedding_key.to_string(),
        }))
    }

    pub(crate) fn embedding_chunks_for_claim(
        &self,
        claim: &EmbeddingJobClaim,
    ) -> Result<Vec<EmbeddingChunkInput>, String> {
        if self.claim_cancel_requested(&claim.common)? {
            return Err("EMBEDDING_CANCELLED: embedding lease is no longer active".to_string());
        }
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT c.id, c.content_hash, c.search_text
                 FROM knowledge_chunks c
                 JOIN knowledge_revisions r ON r.id = c.revision_id
                 JOIN knowledge_documents d ON d.id = r.document_id
                 WHERE c.revision_id = ? AND d.id = ?
                   AND d.active_revision_id = r.id
                   AND d.current_source_hash = r.source_hash
                   AND NOT EXISTS (
                       SELECT 1 FROM knowledge_embeddings e
                       WHERE e.chunk_id = c.id AND e.embedding_key = ?
                         AND e.content_hash = c.content_hash AND e.status = 'ready'
                   )
                 ORDER BY c.ordinal ASC",
            )
            .map_err(|error| format!("准备待生成知识库向量查询失败：{error}"))?;
        let rows = statement
            .query_map(
                params![
                    claim.revision_id,
                    claim.common.document_id,
                    claim.embedding_key
                ],
                |row| {
                    Ok(EmbeddingChunkInput {
                        chunk_id: row.get(0)?,
                        content_hash: row.get(1)?,
                        search_text: row.get(2)?,
                    })
                },
            )
            .map_err(|error| format!("查询待生成知识库向量失败：{error}"))?;
        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(row.map_err(|error| format!("读取待生成知识库向量失败：{error}"))?);
        }
        Ok(chunks)
    }

    pub(crate) fn write_embedding_batch(
        &self,
        claim: &EmbeddingJobClaim,
        spec: &EmbeddingProviderSpec,
        writes: Vec<EmbeddingWrite>,
        completed_units: usize,
        total_units: usize,
        retry_count: usize,
    ) -> Result<(), String> {
        if writes.is_empty() {
            return Ok(());
        }
        if spec.embedding_key != claim.embedding_key {
            return Err("Embedding route changed while the job was running.".to_string());
        }
        let mut encoded = Vec::with_capacity(writes.len());
        let mut dimensions = None;
        for write in writes {
            let vector = normalize_l2(write.vector).map_err(|error| error.to_string())?;
            if dimensions.is_some_and(|value| value != vector.len()) {
                return Err("EMBEDDING_DIMENSION_MISMATCH: batch dimensions differ".to_string());
            }
            dimensions = Some(vector.len());
            encoded.push((
                write.chunk_id,
                write.content_hash,
                encode_f32_le(&vector).map_err(|error| error.to_string())?,
            ));
        }
        let dimensions = dimensions.unwrap_or_default();
        if spec
            .expected_dimensions
            .is_some_and(|expected| expected != dimensions)
        {
            return Err(format!(
                "EMBEDDING_DIMENSION_MISMATCH: model metadata expects {} dimensions but batch has {dimensions}",
                spec.expected_dimensions.unwrap_or_default()
            ));
        }
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始写入知识库向量批次失败：{error}"))?;
        let current_dimensions: Option<i64> = transaction
            .query_row(
                "SELECT dimensions FROM knowledge_embeddings
                 WHERE embedding_key = ? AND status = 'ready'
                 ORDER BY updated_at DESC LIMIT 1",
                [&claim.embedding_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("检查知识库向量维度失败：{error}"))?;
        if current_dimensions.is_some_and(|value| value != dimensions as i64) {
            return Err(format!(
                "EMBEDDING_DIMENSION_MISMATCH: existing index has {} dimensions but batch has {dimensions}",
                current_dimensions.unwrap_or_default()
            ));
        }
        let now = now_ms();
        for (chunk_id, content_hash, vector_blob) in encoded {
            let current_hash: Option<String> = transaction
                .query_row(
                    "SELECT content_hash FROM knowledge_chunks
                     WHERE id = ? AND revision_id = ?",
                    params![chunk_id, claim.revision_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|error| format!("验证知识库向量 chunk 失败：{error}"))?;
            if current_hash.as_deref() != Some(content_hash.as_str()) {
                return Err(
                    "EMBEDDING_CONTENT_CHANGED: chunk changed before vector commit".to_string(),
                );
            }
            transaction
                .execute(
                    "INSERT INTO knowledge_embeddings (
                        chunk_id, embedding_key, provider_id, model_id,
                        model_revision, dimensions, normalization, content_hash,
                        vector_blob, status, retry_count, created_at, updated_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'ready', ?, ?, ?)
                     ON CONFLICT(chunk_id, embedding_key) DO UPDATE SET
                        provider_id = excluded.provider_id,
                        model_id = excluded.model_id,
                        model_revision = excluded.model_revision,
                        dimensions = excluded.dimensions,
                        normalization = excluded.normalization,
                        content_hash = excluded.content_hash,
                        vector_blob = excluded.vector_blob,
                        status = 'ready', error_code = NULL, error_message = NULL,
                        retry_count = excluded.retry_count,
                        updated_at = excluded.updated_at",
                    params![
                        chunk_id,
                        spec.embedding_key,
                        spec.provider_id,
                        spec.model_id,
                        spec.model_revision,
                        i64::try_from(dimensions).unwrap_or(i64::MAX),
                        EMBEDDING_NORMALIZATION,
                        content_hash,
                        vector_blob,
                        i64::try_from(retry_count).unwrap_or(i64::MAX),
                        now,
                        now,
                    ],
                )
                .map_err(|error| format!("写入知识库向量失败：{error}"))?;
        }
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET stage = 'embedding', provider_state = 'running',
                     completed_units = ?, total_units = ?, heartbeat_at = ?,
                     lease_expires_at = ?, updated_at = ?
                 WHERE id = ? AND state = 'running'
                   AND execution_version = ? AND state_version = ?
                   AND runtime_instance_id = ? AND lease_token = ?",
                params![
                    i64::try_from(completed_units).unwrap_or(i64::MAX),
                    i64::try_from(total_units).unwrap_or(i64::MAX),
                    now,
                    now.saturating_add(KNOWLEDGE_JOB_LEASE_MS),
                    now,
                    claim.common.job_id,
                    claim.common.execution_version,
                    claim.common.state_version,
                    claim.common.runtime_instance_id,
                    claim.common.lease_token,
                ],
            )
            .map_err(|error| format!("更新知识库向量任务进度失败：{error}"))?;
        if changed != 1 {
            return Err("Embedding job progress CAS was rejected.".to_string());
        }
        transaction
            .commit()
            .map_err(|error| format!("提交知识库向量批次失败：{error}"))
    }

    pub(crate) fn complete_embedding_claim(
        &self,
        claim: &EmbeddingJobClaim,
    ) -> Result<bool, String> {
        let connection = self.open_connection()?;
        let missing: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_chunks c
                 WHERE c.revision_id = ? AND NOT EXISTS (
                    SELECT 1 FROM knowledge_embeddings e
                    WHERE e.chunk_id = c.id AND e.embedding_key = ?
                      AND e.content_hash = c.content_hash AND e.status = 'ready'
                 )",
                params![claim.revision_id, claim.embedding_key],
                |row| row.get(0),
            )
            .map_err(|error| format!("验证知识库向量完整性失败：{error}"))?;
        drop(connection);
        if missing != 0 {
            return Err(format!(
                "EMBEDDING_INCOMPLETE: {missing} chunks still have no ready vector"
            ));
        }
        let completed =
            self.complete_claimed_job(&claim.common, Some(&claim.revision_id), false)?;
        if completed {
            let connection = self.open_connection()?;
            let now = now_ms();
            connection
                .execute(
                    "UPDATE knowledge_revisions
                     SET status = 'ready', updated_at = ?
                     WHERE id = ? AND status IN ('lexical_ready', 'embedding_pending')",
                    params![now, claim.revision_id],
                )
                .map_err(|error| format!("更新知识库向量就绪状态失败：{error}"))?;
        }
        Ok(completed)
    }

    pub(crate) fn heartbeat_claim(
        &self,
        claim: &KnowledgeJobClaim,
        stage: &str,
        completed_units: i64,
        total_units: i64,
        provider_state: Option<&str>,
    ) -> Result<bool, String> {
        let completed_units = completed_units.max(0);
        let total_units = total_units.max(completed_units);
        let now = now_ms();
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE knowledge_index_jobs
                 SET stage = ?, completed_units = ?, total_units = ?,
                     provider_state = COALESCE(?, provider_state),
                     heartbeat_at = ?, lease_expires_at = ?, updated_at = ?
                 WHERE id = ? AND state IN ('running', 'cancelling')
                   AND execution_version = ? AND state_version = ?
                   AND runtime_instance_id = ? AND lease_token = ?",
                params![
                    stage,
                    completed_units,
                    total_units,
                    provider_state,
                    now,
                    now.saturating_add(KNOWLEDGE_JOB_LEASE_MS),
                    now,
                    claim.job_id,
                    claim.execution_version,
                    claim.state_version,
                    claim.runtime_instance_id,
                    claim.lease_token,
                ],
            )
            .map_err(|error| format!("failed to heartbeat knowledge job: {error}"))?;
        if changed == 1 {
            let document_state = match stage {
                "remote_pending" => Some("remote_pending"),
                "remote_running" => Some("remote_running"),
                "normalizing_elements" | "validating_archive" => Some("normalizing"),
                _ => None,
            };
            if let Some(document_state) = document_state {
                let _ = connection.execute(
                    "UPDATE knowledge_documents SET state = ?, updated_at = ?
                     WHERE id = ? AND state <> 'deleted' AND active_revision_id IS NULL",
                    params![document_state, now, claim.document_id],
                );
            }
        }
        Ok(changed == 1)
    }

    pub(crate) fn claim_cancel_requested(&self, claim: &KnowledgeJobClaim) -> Result<bool, String> {
        let connection = self.open_connection()?;
        let current: Option<(String, i64, i64, Option<String>, Option<String>)> = connection
            .query_row(
                "SELECT state, execution_version, state_version,
                        runtime_instance_id, lease_token
                 FROM knowledge_index_jobs WHERE id = ?",
                params![claim.job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("failed to inspect knowledge cancellation: {error}"))?;
        Ok(
            current.is_none_or(|(state, execution, version, owner, token)| {
                matches!(state.as_str(), "cancelling" | "cancelled")
                    || execution != claim.execution_version
                    || version != claim.state_version
                    || owner.as_deref() != Some(claim.runtime_instance_id.as_str())
                    || token.as_deref() != Some(claim.lease_token.as_str())
            }),
        )
    }

    pub(crate) fn complete_claimed_job(
        &self,
        claim: &KnowledgeJobClaim,
        revision_id: Option<&str>,
        partial: bool,
    ) -> Result<bool, String> {
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin knowledge completion: {error}"))?;
        let current: Option<(String, i64, i64, Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT state, execution_version, state_version,
                        runtime_instance_id, lease_token
                 FROM knowledge_index_jobs WHERE id = ?",
                params![claim.job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("failed to read knowledge completion target: {error}"))?;
        let Some((state, execution, version, owner, token)) = current else {
            transaction
                .commit()
                .map_err(|error| format!("failed to close missing knowledge job: {error}"))?;
            return Ok(false);
        };
        if state != "running"
            || execution != claim.execution_version
            || version != claim.state_version
            || owner.as_deref() != Some(claim.runtime_instance_id.as_str())
            || token.as_deref() != Some(claim.lease_token.as_str())
        {
            transaction
                .commit()
                .map_err(|error| format!("failed to close stale knowledge completion: {error}"))?;
            return Ok(false);
        }
        let now = now_ms();
        let terminal = if partial { "partial" } else { "succeeded" };
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET revision_id = COALESCE(?, revision_id), state = ?, stage = 'done',
                     completed_units = total_units, finished_at = ?, updated_at = ?,
                     heartbeat_at = ?, lease_token = NULL, lease_owner = NULL,
                     lease_expires_at = NULL, runtime_instance_id = NULL,
                     state_version = state_version + 1
                 WHERE id = ? AND state = 'running'
                   AND execution_version = ? AND state_version = ?
                   AND runtime_instance_id = ? AND lease_token = ?",
                params![
                    revision_id,
                    terminal,
                    now,
                    now,
                    now,
                    claim.job_id,
                    claim.execution_version,
                    claim.state_version,
                    claim.runtime_instance_id,
                    claim.lease_token,
                ],
            )
            .map_err(|error| format!("failed to complete knowledge job: {error}"))?;
        if changed != 1 {
            transaction.commit().map_err(|error| {
                format!("failed to commit rejected knowledge completion: {error}")
            })?;
            return Ok(false);
        }
        insert_job_event_tx(
            &transaction,
            &claim.job_id,
            if partial {
                "jobPartial"
            } else {
                "jobSucceeded"
            },
            Some("running"),
            Some(terminal),
            claim.execution_version,
            claim.state_version.saturating_add(1),
            if partial {
                r#"{"stage":"done","quality":"partial"}"#
            } else {
                r#"{"stage":"done"}"#
            },
            None,
            Some(&claim.runtime_instance_id),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit knowledge completion: {error}"))?;
        Ok(true)
    }

    pub(crate) fn fail_claimed_job(
        &self,
        claim: &KnowledgeJobClaim,
        code: &str,
        message: &str,
    ) -> Result<bool, String> {
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin knowledge failure: {error}"))?;
        let now = now_ms();
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'failed', stage = 'cleaning', error_code = ?,
                     error_message = ?, finished_at = ?, updated_at = ?, heartbeat_at = ?,
                     lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL,
                     runtime_instance_id = NULL, state_version = state_version + 1
                 WHERE id = ? AND state = 'running'
                   AND execution_version = ? AND state_version = ?
                   AND runtime_instance_id = ? AND lease_token = ?",
                params![
                    bounded_error(code),
                    bounded_error(message),
                    now,
                    now,
                    now,
                    claim.job_id,
                    claim.execution_version,
                    claim.state_version,
                    claim.runtime_instance_id,
                    claim.lease_token,
                ],
            )
            .map_err(|error| format!("failed to fail knowledge job: {error}"))?;
        if changed != 1 {
            transaction
                .commit()
                .map_err(|error| format!("failed to commit rejected knowledge failure: {error}"))?;
            return Ok(false);
        }
        transaction
            .execute(
                "UPDATE knowledge_documents
                 SET state = 'failed', updated_at = ?
                 WHERE id = ? AND active_revision_id IS NULL AND state <> 'deleted'",
                params![now, claim.document_id],
            )
            .map_err(|error| format!("failed to update failed knowledge document: {error}"))?;
        let payload = serde_json::json!({
            "stage": "cleaning",
            "errorCode": bounded_error(code),
        })
        .to_string();
        insert_job_event_tx(
            &transaction,
            &claim.job_id,
            "jobFailed",
            Some("running"),
            Some("failed"),
            claim.execution_version,
            claim.state_version.saturating_add(1),
            &payload,
            None,
            Some(&claim.runtime_instance_id),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit knowledge failure: {error}"))?;
        Ok(true)
    }

    /// Normalize and publish a validated MinerU result.  The archive is first
    /// materialized in a new revision directory; only after all provider data,
    /// assets, elements, chunks, and FTS rows are ready does one SQLite
    /// transaction switch the document's active revision and close the job.
    /// A stale lease or a concurrent cancellation therefore cannot make a
    /// half-written revision visible to search.
    pub(crate) fn commit_mineru_extraction(
        &self,
        claim: &KnowledgeJobClaim,
        extraction: &super::mineru::MineruExtraction,
        config: &super::mineru::MineruConfig,
        chunk_target: usize,
        chunk_max: usize,
    ) -> Result<PdfCommitResult, String> {
        let revision_id = Uuid::new_v4().to_string();
        let destination = self.revision_directory(&revision_id)?;
        let archive_manifest =
            super::mineru::extract_result_archive_atomic(&extraction.result_zip, &destination)
                .map_err(mineru_error_string)?;
        let result = (|| {
            let prepared = prepare_cloud_pdf_revision(
                &self.library_root,
                &revision_id,
                claim,
                extraction,
                &archive_manifest,
                config,
                chunk_target,
                chunk_max,
            )?;
            write_revision_manifest(
                &destination,
                &cloud_revision_manifest_json(claim, extraction, config, &prepared),
            )?;
            self.commit_prepared_pdf_revision(claim, &prepared)
        })();
        match &result {
            Ok(commit) if commit.revision_id != revision_id => {
                // The same source/configuration may already have been
                // committed by a previous worker.  The newly materialized
                // archive is then an unreferenced duplicate and must not be
                // left behind as a future "revision".
                let _ = fs::remove_dir_all(&destination);
            }
            Err(_) => {
                let _ = fs::remove_dir_all(&destination);
            }
            _ => {}
        }
        result
    }

    /// Publish the explicitly documented local text-only fallback.  This path
    /// never creates image/table/formula/OCR records and is marked degraded so
    /// callers cannot mistake it for a complete MinerU extraction.
    pub(crate) fn commit_local_pdf_fallback(
        &self,
        claim: &KnowledgeJobClaim,
        fallback: &super::mineru::LocalPdfExtraction,
        cloud_error: Option<&str>,
        chunk_target: usize,
        chunk_max: usize,
    ) -> Result<PdfCommitResult, String> {
        let revision_id = Uuid::new_v4().to_string();
        let destination = self.revision_directory(&revision_id)?;
        create_local_revision_directory(&destination, &fallback.full_markdown)?;
        let result = (|| {
            let prepared = prepare_local_pdf_revision(
                &revision_id,
                claim,
                fallback,
                cloud_error,
                chunk_target,
                chunk_max,
            )?;
            write_revision_manifest(
                &destination,
                &local_revision_manifest_json(claim, fallback, cloud_error, &prepared),
            )?;
            self.commit_prepared_pdf_revision(claim, &prepared)
        })();
        match &result {
            Ok(commit) if commit.revision_id != revision_id => {
                let _ = fs::remove_dir_all(&destination);
            }
            Err(_) => {
                let _ = fs::remove_dir_all(&destination);
            }
            _ => {}
        }
        result
    }

    fn revision_directory(&self, revision_id: &str) -> Result<PathBuf, String> {
        let revision_id = normalize_identifier("revision ID", revision_id)?;
        Ok(self
            .library_root
            .join("knowledge")
            .join("revisions")
            .join(revision_id))
    }

    fn commit_prepared_pdf_revision(
        &self,
        claim: &KnowledgeJobClaim,
        prepared: &PreparedPdfRevision,
    ) -> Result<PdfCommitResult, String> {
        if prepared.source_hash != claim.source_hash {
            return Err("PDF source hash changed before revision commit".to_string());
        }
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin PDF revision commit: {error}"))?;

        let current: Option<(
            String,
            String,
            String,
            String,
            i64,
            i64,
            Option<String>,
            Option<String>,
            Option<i64>,
        )> = transaction
            .query_row(
                "SELECT j.state, d.state, d.current_source_hash, f.file_hash,
                        j.execution_version, j.state_version,
                        j.runtime_instance_id, j.lease_token, j.cancel_requested_at
                 FROM knowledge_index_jobs j
                 JOIN knowledge_documents d ON d.id = j.document_id
                 JOIN library_items i ON i.id = d.library_item_id
                 JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
                 WHERE j.id = ?",
                params![claim.job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("failed to validate PDF revision lease: {error}"))?;
        let Some((
            job_state,
            _document_state,
            document_hash,
            file_hash,
            execution_version,
            state_version,
            runtime_id,
            lease_token,
            cancel_requested_at,
        )) = current
        else {
            return Err("PDF revision lease no longer exists".to_string());
        };
        if job_state != "running"
            || cancel_requested_at.is_some()
            || document_hash != claim.source_hash
            || file_hash != claim.source_hash
            || execution_version != claim.execution_version
            || state_version != claim.state_version
            || runtime_id.as_deref() != Some(claim.runtime_instance_id.as_str())
            || lease_token.as_deref() != Some(claim.lease_token.as_str())
        {
            return Err(
                "PDF revision lease is stale, cancelled, or source content changed".to_string(),
            );
        }

        let consent_valid: bool = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM knowledge_cloud_consents c
                    WHERE c.provider_id = 'mineru-cloud'
                      AND c.policy_version = ?
                      AND c.revoked_at IS NULL
                      AND (
                          c.scope = 'global'
                          OR (c.id = ? AND c.document_id = ? AND c.source_hash = ?)
                      )
                )",
                params![
                    MINERU_CONSENT_POLICY_VERSION,
                    claim.consent_id,
                    claim.document_id,
                    claim.source_hash,
                ],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to validate PDF consent at commit: {error}"))?;
        if prepared.remote_upload && !consent_valid {
            return Err("MinerU consent was revoked before revision commit".to_string());
        }

        set_claim_stage_tx(&transaction, claim, "writing_revision", now_ms())?;

        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT id, status FROM knowledge_revisions
                 WHERE document_id = ? AND source_hash = ? AND parser_id = ?
                   AND parser_version = ? AND parser_config_hash = ?
                   AND normalization_version = ? AND chunk_policy_version = ?
                   AND status IN ('ready', 'lexical_ready', 'partial')
                 ORDER BY created_at DESC LIMIT 1",
                params![
                    claim.document_id,
                    prepared.source_hash,
                    prepared.parser_id,
                    prepared.parser_version,
                    prepared.parser_config_hash,
                    prepared.normalization_version,
                    prepared.chunk_policy_version,
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("failed to inspect existing PDF revision: {error}"))?;
        if let Some((existing_id, status)) = existing {
            let partial = status == "partial";
            let document_state = if prepared.remote_upload {
                if partial {
                    "partial"
                } else {
                    "ready"
                }
            } else {
                "degraded"
            };
            transaction
                .execute(
                    "UPDATE knowledge_documents
                     SET active_revision_id = ?, state = ?, include_in_default_scope = 1,
                         current_source_hash = ?,
                         cloud_consent_state = CASE WHEN ? = 1 THEN 'granted' ELSE cloud_consent_state END,
                         updated_at = ?
                     WHERE id = ? AND state <> 'deleted'",
                    params![
                        existing_id,
                        document_state,
                        prepared.source_hash,
                        if prepared.remote_upload { 1 } else { 0 },
                        now_ms(),
                        claim.document_id
                    ],
                )
                .map_err(|error| format!("failed to activate existing PDF revision: {error}"))?;
            finish_claimed_job_tx(&transaction, claim, &existing_id, partial, now_ms())?;
            transaction
                .commit()
                .map_err(|error| format!("failed to commit existing PDF revision: {error}"))?;
            return Ok(PdfCommitResult {
                revision_id: existing_id,
                partial,
            });
        }

        let now = now_ms();
        transaction
            .execute(
                "INSERT INTO knowledge_revisions (
                    id, document_id, source_hash, canonical_hash, parser_id, parser_version,
                    provider_id, parser_config_hash, provider_task_id, provider_batch_id,
                    provider_result_hash, parser_config_json, consent_id, remote_upload,
                    normalization_version, chunk_policy_version, content_path, manifest_path,
                    provider_archive_path, page_count, line_count, byte_count,
                    extraction_quality, quality_flags, status, warning_json, created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'building', ?, ?, ?)",
                params![
                    prepared.revision_id,
                    claim.document_id,
                    prepared.source_hash,
                    super::markdown::sha256_hex(prepared.canonical_text.as_bytes()),
                    prepared.parser_id,
                    prepared.parser_version,
                    prepared.provider_id,
                    prepared.parser_config_hash,
                    prepared.provider_task_id,
                    prepared.provider_batch_id,
                    prepared.provider_result_hash,
                    prepared.parser_config_json,
                    prepared.consent_id,
                    if prepared.remote_upload { 1 } else { 0 },
                    prepared.normalization_version,
                    prepared.chunk_policy_version,
                    prepared.content_path,
                    prepared.manifest_path,
                    prepared.provider_archive_path,
                    i64::from(prepared.page_count),
                    i64::try_from(prepared.canonical_text.lines().count()).unwrap_or(i64::MAX),
                    i64::try_from(prepared.canonical_text.len()).unwrap_or(i64::MAX),
                    prepared.extraction_quality,
                    serde_json::to_string(&prepared.quality_flags).unwrap_or_else(|_| "[]".to_string()),
                    serde_json::to_string(&prepared.warnings).unwrap_or_else(|_| "[]".to_string()),
                    now,
                    now,
                ],
            )
            .map_err(|error| format!("failed to insert PDF revision: {error}"))?;

        let mut next_fts_rowid: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(fts_rowid), 0) + 1 FROM knowledge_chunks",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to allocate PDF FTS row id: {error}"))?;
        if next_fts_rowid <= 0 {
            next_fts_rowid = 1;
        }

        let document_title: String = transaction
            .query_row(
                "SELECT title FROM knowledge_documents WHERE id = ?",
                params![claim.document_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("failed to read PDF document title: {error}"))?;

        for asset in &prepared.assets {
            transaction
                .execute(
                    "INSERT INTO knowledge_assets (
                        id, revision_id, asset_kind, relative_path, mime_type, byte_size,
                        sha256, width_px, height_px, page_index, page_width, page_height,
                        norm_x1, norm_y1, norm_x2, norm_y2, page_x1, page_y1, page_x2, page_y2,
                        alt_text, caption, source_asset_name, metadata_json, created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        asset.id,
                        prepared.revision_id,
                        asset.asset_kind,
                        asset.relative_path,
                        asset.mime_type,
                        i64::try_from(asset.byte_size).unwrap_or(i64::MAX),
                        asset.sha256,
                        asset.width_px.map(i64::from),
                        asset.height_px.map(i64::from),
                        asset.page_index.map(i64::from),
                        asset.page_width,
                        asset.page_height,
                        asset.bbox.map(|value| value[0]),
                        asset.bbox.map(|value| value[1]),
                        asset.bbox.map(|value| value[2]),
                        asset.bbox.map(|value| value[3]),
                        asset.bbox.map(|value| value[0] * asset.page_width.unwrap_or(0.0)),
                        asset.bbox.map(|value| value[1] * asset.page_height.unwrap_or(0.0)),
                        asset.bbox.map(|value| value[2] * asset.page_width.unwrap_or(0.0)),
                        asset.bbox.map(|value| value[3] * asset.page_height.unwrap_or(0.0)),
                        asset.alt_text,
                        asset.caption,
                        asset.source_asset_name,
                        asset.metadata.to_string(),
                        now,
                    ],
                )
                .map_err(|error| format!("failed to insert PDF asset: {error}"))?;
        }

        for element in &prepared.elements {
            let page_width = element.page_width;
            let page_height = element.page_height;
            transaction
                .execute(
                    "INSERT INTO knowledge_elements (
                        id, revision_id, element_type, ordinal, provider_element_id,
                        page_index, page_end, page_width, page_height,
                        norm_x1, norm_y1, norm_x2, norm_y2, page_x1, page_y1, page_x2, page_y2,
                        reading_order, line_start, line_end, byte_start, byte_end, char_start, char_end,
                        heading_path_json, text, raw_text, ocr_text, formula_latex, table_html,
                        table_json, caption, source_ref_json, metadata_json, content_hash, created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        element.id,
                        prepared.revision_id,
                        element.element_type,
                        i64::try_from(element.ordinal).unwrap_or(i64::MAX),
                        element.provider_element_id,
                        element.page_index.map(i64::from),
                        element.page_end.map(i64::from),
                        page_width,
                        page_height,
                        element.bbox.map(|value| value[0]),
                        element.bbox.map(|value| value[1]),
                        element.bbox.map(|value| value[2]),
                        element.bbox.map(|value| value[3]),
                        element.bbox.map(|value| value[0] * page_width.unwrap_or(0.0)),
                        element.bbox.map(|value| value[1] * page_height.unwrap_or(0.0)),
                        element.bbox.map(|value| value[2] * page_width.unwrap_or(0.0)),
                        element.bbox.map(|value| value[3] * page_height.unwrap_or(0.0)),
                        i64::try_from(element.ordinal).unwrap_or(i64::MAX),
                        element.line_start.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        element.line_end.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        element.byte_start.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        element.byte_end.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        element.char_start.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        element.char_end.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        serde_json::to_string(&element.heading_path).unwrap_or_else(|_| "[]".to_string()),
                        element.text,
                        element.raw_text,
                        element.ocr_text,
                        element.formula_latex,
                        element.table_html,
                        element.table_json,
                        element.caption,
                        element.metadata.get("sourceRef").cloned().unwrap_or_else(|| serde_json::json!({})).to_string(),
                        element.metadata.to_string(),
                        super::markdown::sha256_hex(element.raw_text.as_bytes()),
                        now,
                    ],
                )
                .map_err(|error| format!("failed to insert PDF element: {error}"))?;
            for (asset_ordinal, asset_id) in element.asset_ids.iter().enumerate() {
                transaction
                    .execute(
                        "INSERT INTO knowledge_element_assets (element_id, asset_id, role, ordinal)
                         VALUES (?, ?, 'primary', ?)",
                        params![
                            element.id,
                            asset_id,
                            i64::try_from(asset_ordinal).unwrap_or(i64::MAX)
                        ],
                    )
                    .map_err(|error| format!("failed to link PDF element asset: {error}"))?;
            }
        }

        set_claim_stage_tx(&transaction, claim, "building_fts", now_ms())?;
        for (index, chunk) in prepared.chunks.iter().enumerate() {
            let previous = index
                .checked_sub(1)
                .map(|value| prepared.chunks[value].id.clone());
            let next =
                (index + 1 < prepared.chunks.len()).then(|| prepared.chunks[index + 1].id.clone());
            let element_types = chunk
                .element_ids
                .iter()
                .filter_map(|id| prepared.elements.iter().find(|element| &element.id == id))
                .map(|element| element.element_type.clone())
                .collect::<Vec<_>>();
            let heading_path = chunk.heading_path.join(" / ");
            let search_text = chunk.search_text.clone();
            transaction
                .execute(
                    "INSERT INTO knowledge_chunks (
                        id, revision_id, ordinal, block_kind, text, search_text, content_hash,
                        page_start, page_end, line_start, line_end, byte_start, byte_end,
                        char_start, char_end, heading_path_json, element_ids_json, asset_ids_json,
                        page_bbox_json, quality_flags, metadata_json, prev_chunk_id, next_chunk_id,
                        fts_rowid, token_estimate, is_overlap, created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        chunk.id,
                        prepared.revision_id,
                        i64::try_from(chunk.ordinal).unwrap_or(i64::MAX),
                        chunk.block_kind,
                        chunk.text,
                        search_text,
                        super::markdown::sha256_hex(chunk.text.as_bytes()),
                        chunk.page_start.map(i64::from),
                        chunk.page_end.map(i64::from),
                        chunk.line_start.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        chunk.line_end.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        i64::try_from(chunk.byte_start).unwrap_or(i64::MAX),
                        i64::try_from(chunk.byte_end).unwrap_or(i64::MAX),
                        chunk.char_start.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        chunk.char_end.map(|value| i64::try_from(value).unwrap_or(i64::MAX)),
                        serde_json::to_string(&chunk.heading_path).unwrap_or_else(|_| "[]".to_string()),
                        serde_json::to_string(&chunk.element_ids).unwrap_or_else(|_| "[]".to_string()),
                        serde_json::to_string(&chunk.asset_ids).unwrap_or_else(|_| "[]".to_string()),
                        serde_json::to_string(&chunk.page_bboxes).unwrap_or_else(|_| "[]".to_string()),
                        serde_json::to_string(&chunk.quality_flags).unwrap_or_else(|_| "[]".to_string()),
                        serde_json::json!({ "elementTypes": element_types }).to_string(),
                        previous,
                         next,
                         next_fts_rowid,
                         i64::try_from(chunk.text.chars().count().div_ceil(4)).unwrap_or(i64::MAX),
                         if chunk.is_overlap { 1 } else { 0 },
                         now,
                    ],
                )
                .map_err(|error| format!("failed to insert PDF chunk: {error}"))?;
            transaction
                .execute(
                    "INSERT INTO knowledge_fts_source (
                        rowid, chunk_id, revision_id, document_id, title, heading_path,
                        element_types, body, multimodal_text, search_text
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        next_fts_rowid,
                        chunk.id,
                        prepared.revision_id,
                        claim.document_id,
                        document_title,
                        heading_path,
                        element_types.join(","),
                        chunk.text,
                        chunk.search_text,
                        chunk.search_text,
                    ],
                )
                .map_err(|error| format!("failed to insert PDF FTS projection: {error}"))?;
            next_fts_rowid = next_fts_rowid.saturating_add(1);
        }

        if prepared.elements.is_empty() || prepared.chunks.is_empty() {
            return Err("PDF extraction produced no searchable content".to_string());
        }
        let partial = !prepared.warnings.is_empty() || !prepared.quality_flags.is_empty();
        let revision_status = if partial { "partial" } else { "lexical_ready" };
        let document_state = if prepared.remote_upload {
            if partial {
                "partial"
            } else {
                "ready"
            }
        } else {
            "degraded"
        };
        transaction
            .execute(
                "UPDATE knowledge_revisions
                 SET element_count = ?, asset_count = ?, chunk_count = ?, status = ?,
                     extraction_quality = ?, quality_flags = ?, warning_json = ?,
                     completed_at = ?, updated_at = ?
                 WHERE id = ? AND status = 'building'",
                params![
                    i64::try_from(prepared.elements.len()).unwrap_or(i64::MAX),
                    i64::try_from(prepared.assets.len()).unwrap_or(i64::MAX),
                    i64::try_from(prepared.chunks.len()).unwrap_or(i64::MAX),
                    revision_status,
                    prepared.extraction_quality,
                    serde_json::to_string(&prepared.quality_flags)
                        .unwrap_or_else(|_| "[]".to_string()),
                    serde_json::to_string(&prepared.warnings).unwrap_or_else(|_| "[]".to_string()),
                    now,
                    now,
                    prepared.revision_id,
                ],
            )
            .map_err(|error| format!("failed to finalize PDF revision: {error}"))?;
        if let Some(old_revision_id) = transaction
            .query_row(
                "SELECT active_revision_id FROM knowledge_documents WHERE id = ?",
                params![claim.document_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|error| format!("failed to read previous PDF revision: {error}"))?
            .flatten()
        {
            transaction
                .execute(
                    "UPDATE knowledge_revisions SET status = 'stale', updated_at = ?
                     WHERE id = ? AND id <> ? AND status NOT IN ('failed', 'cancelled', 'stale')",
                    params![now, old_revision_id, prepared.revision_id],
                )
                .map_err(|error| format!("failed to stale previous PDF revision: {error}"))?;
        }
        transaction
            .execute(
                "UPDATE knowledge_documents
                 SET title = COALESCE((SELECT title FROM library_items WHERE id = library_item_id), title),
                     current_source_hash = ?, active_revision_id = ?, state = ?,
                     include_in_default_scope = 1,
                     cloud_consent_state = CASE WHEN ? = 1 THEN 'granted' ELSE cloud_consent_state END,
                     updated_at = ?
                 WHERE id = ? AND state <> 'deleted'",
                params![
                    prepared.source_hash,
                    prepared.revision_id,
                    document_state,
                    if prepared.remote_upload { 1 } else { 0 },
                    now,
                    claim.document_id,
                ],
            )
            .map_err(|error| format!("failed to activate PDF revision: {error}"))?;
        finish_claimed_job_tx(&transaction, claim, &prepared.revision_id, partial, now)?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit PDF revision transaction: {error}"))?;
        Ok(PdfCommitResult {
            revision_id: prepared.revision_id.clone(),
            partial,
        })
    }

    /// 同步建立一个 Markdown 笔记的本地 revision/element/chunk/FTS。
    ///
    /// 笔记正文不会离开本机；该方法只读取现有 library note 表和受控 note
    /// 目录中的图片资产。
    pub fn sync_note(&self, note_id: &str) -> Result<KnowledgeDocumentStatus, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let input = self.load_note(&note_id)?;
        let parsed = parse_markdown(&input.content);
        let (assets, asset_by_source, mut warnings) = self.prepare_note_assets(&input, &parsed);
        // The bytes loaded from `library_notes` are the source of truth.  Do not
        // let a stale/corrupt cached content_hash identify a different revision.
        let source_hash = parsed.source_hash.clone();
        if input
            .content_hash
            .as_deref()
            .is_some_and(|stored| !stored.trim().is_empty() && stored != source_hash)
        {
            warnings.push("NOTE_CONTENT_HASH_MISMATCH".to_string());
        }
        let now = now_ms();

        let mut connection = self.open_connection()?;
        let (document, job_id, already_ready) = {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| format!("开始登记 Markdown 知识源失败：{error}"))?;
            let (document, changed) = ensure_document_tx(
                &transaction,
                "note",
                "markdown_note",
                &note_id,
                &input.title,
                &source_hash,
                None,
                Some(&note_id),
                "pending",
                "not_required",
                now,
            )?;
            let (job_id, job_created) = ensure_job_tx(
                &transaction,
                "rebuild",
                &document.id,
                &source_hash,
                "queued",
                0,
                now,
            )?;
            let already_ready = !changed
                && !job_created
                && document.state == "ready"
                && document.active_revision_id.is_some()
                && document.source_hash == source_hash;
            transaction
                .commit()
                .map_err(|error| format!("提交 Markdown 知识源登记失败：{error}"))?;
            (document, job_id, already_ready)
        };
        if already_ready {
            return self.get_document_status(&document.id);
        }

        let result = self.commit_markdown_revision(
            &mut connection,
            &document,
            &job_id,
            &source_hash,
            &parsed,
            &assets,
            &asset_by_source,
            &mut warnings,
            now,
        );
        if let Err(error) = result {
            let _ = self.fail_job(&job_id, "MARKDOWN_INDEX_FAILED", &error);
            return Err(error);
        }
        self.get_document_status(&document.id)
    }

    pub fn rebuild_all(&self) -> Result<KnowledgeRebuildResult, String> {
        let connection = self.open_connection()?;
        let mut pdf_ids = Vec::new();
        {
            let mut statement = connection
                .prepare(
                    "SELECT id FROM library_items
                     WHERE item_type = 'pdf' AND deleted_at IS NULL ORDER BY updated_at DESC",
                )
                .map_err(|error| format!("准备全量 PDF 知识源列表失败：{error}"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("查询全量 PDF 知识源失败：{error}"))?;
            for row in rows {
                pdf_ids.push(row.map_err(|error| format!("读取 PDF 知识源失败：{error}"))?);
            }
        }
        let mut note_ids = Vec::new();
        {
            let mut statement = connection
                .prepare(
                    "SELECT n.id FROM library_notes n
                     LEFT JOIN library_items i ON i.id = n.item_id
                     WHERE n.item_id IS NULL OR i.deleted_at IS NULL
                     ORDER BY n.updated_at DESC",
                )
                .map_err(|error| format!("准备全量 Markdown 知识源列表失败：{error}"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("查询全量 Markdown 知识源失败：{error}"))?;
            for row in rows {
                note_ids.push(row.map_err(|error| format!("读取 Markdown 知识源失败：{error}"))?);
            }
        }
        let mut queued_pdf_count = 0usize;
        for item_id in pdf_ids {
            if self.enqueue_literature(&item_id).is_ok() {
                queued_pdf_count += 1;
            }
        }
        let mut indexed_note_count = 0usize;
        let mut failed_count = 0usize;
        for note_id in note_ids {
            match self.sync_note(&note_id) {
                Ok(_) => indexed_note_count += 1,
                Err(_) => failed_count += 1,
            }
        }
        Ok(KnowledgeRebuildResult {
            queued_pdf_count,
            indexed_note_count,
            failed_count,
        })
    }

    /// Queue vector generation for every active PDF/Markdown revision (or one
    /// selected document). Cached vectors with the same content hash and route
    /// are copied locally before a network job is created.
    pub fn enqueue_embedding_jobs(
        &self,
        spec: &EmbeddingProviderSpec,
        document_id: Option<&str>,
        force: bool,
    ) -> Result<KnowledgeEmbeddingRebuildResult, String> {
        if spec.embedding_key.trim().is_empty()
            || spec.provider_id.trim().is_empty()
            || spec.model_id.trim().is_empty()
        {
            return Err("Embedding provider configuration is incomplete.".to_string());
        }
        let document_id = document_id
            .map(|value| normalize_identifier("知识文档 ID", value))
            .transpose()?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始排队知识库向量任务失败：{error}"))?;
        let now = now_ms();
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'stale', stage = 'cleaning',
                     error_code = 'EMBEDDING_CONFIG_CHANGED',
                     error_message = 'Embedding route changed before the job started.',
                     finished_at = ?, updated_at = ?, state_version = state_version + 1
                 WHERE job_kind = 'embed' AND state IN ('queued', 'paused')
                   AND requested_config_hash <> ?",
                params![now, now, spec.embedding_key],
            )
            .map_err(|error| format!("停用旧知识库向量任务失败：{error}"))?;
        let mut sql = String::from(
            "SELECT d.id, r.id, d.current_source_hash
             FROM knowledge_documents d
             JOIN knowledge_revisions r ON r.id = d.active_revision_id
             WHERE d.state <> 'deleted' AND d.include_in_default_scope = 1
               AND r.source_hash = d.current_source_hash
               AND r.status IN ('ready', 'lexical_ready', 'embedding_pending', 'partial')",
        );
        let mut values = Vec::<Value>::new();
        if let Some(document_id) = &document_id {
            sql.push_str(" AND d.id = ?");
            values.push(Value::Text(document_id.clone()));
        }
        sql.push_str(" ORDER BY d.updated_at DESC, d.id ASC");
        let revisions = {
            let mut statement = transaction
                .prepare(&sql)
                .map_err(|error| format!("准备知识库向量排队查询失败：{error}"))?;
            let rows = statement
                .query_map(params_from_iter(values.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|error| format!("查询知识库向量排队目标失败：{error}"))?;
            let mut revisions = Vec::new();
            for row in rows {
                revisions
                    .push(row.map_err(|error| format!("读取知识库向量排队目标失败：{error}"))?);
            }
            revisions
        };

        let mut summary = EmbeddingQueueSummary::default();
        for (document_id, revision_id, source_hash) in revisions {
            let queued = transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM knowledge_index_jobs
                        WHERE job_kind = 'embed' AND revision_id = ?
                          AND requested_config_hash = ?
                          AND state IN ('queued', 'running', 'cancelling', 'paused')
                     )",
                    params![revision_id, spec.embedding_key],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| format!("检查知识库向量活动任务失败：{error}"))?;
            if queued {
                continue;
            }
            if force {
                transaction
                    .execute(
                        "UPDATE knowledge_embeddings
                         SET status = 'stale', vector_blob = NULL, updated_at = ?
                         WHERE embedding_key = ? AND chunk_id IN (
                            SELECT id FROM knowledge_chunks WHERE revision_id = ?
                         )",
                        params![now_ms(), spec.embedding_key, revision_id],
                    )
                    .map_err(|error| format!("停用待重建知识库向量失败：{error}"))?;
            }

            let chunks = {
                let mut statement = transaction
                    .prepare(
                        "SELECT id, content_hash FROM knowledge_chunks
                         WHERE revision_id = ? ORDER BY ordinal ASC",
                    )
                    .map_err(|error| format!("准备知识库向量 chunk 查询失败：{error}"))?;
                let rows = statement
                    .query_map([&revision_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|error| format!("查询知识库向量 chunk 失败：{error}"))?;
                let mut chunks = Vec::new();
                for row in rows {
                    chunks
                        .push(row.map_err(|error| format!("读取知识库向量 chunk 失败：{error}"))?);
                }
                chunks
            };
            let mut pending = 0usize;
            for (chunk_id, content_hash) in chunks {
                let ready = !force
                    && transaction
                        .query_row(
                            "SELECT EXISTS(
                                SELECT 1 FROM knowledge_embeddings
                                WHERE chunk_id = ? AND embedding_key = ?
                                  AND content_hash = ? AND status = 'ready'
                             )",
                            params![chunk_id, spec.embedding_key, content_hash],
                            |row| row.get::<_, bool>(0),
                        )
                        .map_err(|error| format!("检查知识库向量缓存失败：{error}"))?;
                if ready {
                    continue;
                }
                let cached: Option<(i64, Vec<u8>, i64)> = if force {
                    None
                } else {
                    transaction
                        .query_row(
                            "SELECT dimensions, vector_blob, retry_count
                             FROM knowledge_embeddings
                             WHERE embedding_key = ? AND content_hash = ?
                               AND status = 'ready' AND vector_blob IS NOT NULL
                             ORDER BY updated_at DESC, chunk_id ASC LIMIT 1",
                            params![spec.embedding_key, content_hash],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        )
                        .optional()
                        .map_err(|error| format!("读取知识库共享向量缓存失败：{error}"))?
                };
                let cached = cached.filter(|(dimensions, _, _)| {
                    spec.expected_dimensions
                        .map(|expected| *dimensions == i64::try_from(expected).unwrap_or(-1))
                        .unwrap_or(true)
                });
                if let Some((dimensions, vector_blob, retry_count)) = cached {
                    let changed = transaction
                        .execute(
                            "INSERT INTO knowledge_embeddings (
                                chunk_id, embedding_key, provider_id, model_id,
                                model_revision, dimensions, normalization, content_hash,
                                vector_blob, status, retry_count, created_at, updated_at
                             ) VALUES (?, ?, ?, ?, ?, ?, 'l2', ?, ?, 'ready', ?, ?, ?)
                             ON CONFLICT(chunk_id, embedding_key) DO UPDATE SET
                                provider_id = excluded.provider_id,
                                model_id = excluded.model_id,
                                model_revision = excluded.model_revision,
                                dimensions = excluded.dimensions,
                                normalization = excluded.normalization,
                                content_hash = excluded.content_hash,
                                vector_blob = excluded.vector_blob,
                                status = 'ready', error_code = NULL, error_message = NULL,
                                retry_count = excluded.retry_count,
                                updated_at = excluded.updated_at",
                            params![
                                chunk_id,
                                spec.embedding_key,
                                spec.provider_id,
                                spec.model_id,
                                spec.model_revision,
                                dimensions,
                                content_hash,
                                vector_blob,
                                retry_count,
                                now_ms(),
                                now_ms(),
                            ],
                        )
                        .map_err(|error| format!("复用知识库共享向量缓存失败：{error}"))?;
                    summary.cached_chunk_count = summary.cached_chunk_count.saturating_add(changed);
                } else {
                    pending = pending.saturating_add(1);
                }
            }
            if pending == 0 {
                transaction
                    .execute(
                        "UPDATE knowledge_revisions
                         SET status = 'ready', updated_at = ?
                         WHERE id = ? AND status IN ('lexical_ready', 'embedding_pending')",
                        params![now_ms(), revision_id],
                    )
                    .map_err(|error| format!("更新缓存命中的向量状态失败：{error}"))?;
                continue;
            }
            summary.pending_chunk_count = summary.pending_chunk_count.saturating_add(pending);
            let now = now_ms();
            let idempotency_key = format!("knowledge:embed:{revision_id}:{}", spec.embedding_key);
            let existing: Option<(String, String)> = transaction
                .query_row(
                    "SELECT id, state FROM knowledge_index_jobs WHERE idempotency_key = ?",
                    [&idempotency_key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| format!("检查知识库向量幂等任务失败：{error}"))?;
            if let Some((job_id, state)) = existing {
                if matches!(
                    state.as_str(),
                    "succeeded" | "partial" | "failed" | "cancelled" | "stale"
                ) {
                    transaction
                        .execute(
                            "UPDATE knowledge_index_jobs
                             SET state = 'queued', stage = 'waiting_embedding',
                                 requested_source_hash = ?, requested_config_hash = ?,
                                 total_units = ?, completed_units = 0,
                                 execution_version = execution_version + 1,
                                 state_version = state_version + 1, attempt = 0,
                                 provider_state = NULL, error_code = NULL, error_message = NULL,
                                 started_at = NULL, heartbeat_at = NULL, finished_at = NULL,
                                 cancel_requested_at = NULL, runtime_instance_id = NULL,
                                 lease_token = NULL, lease_owner = NULL, lease_expires_at = NULL,
                                 updated_at = ?
                             WHERE id = ?",
                            params![
                                source_hash,
                                spec.embedding_key,
                                i64::try_from(pending).unwrap_or(i64::MAX),
                                now,
                                job_id,
                            ],
                        )
                        .map_err(|error| format!("重新排队知识库向量任务失败：{error}"))?;
                    summary.queued_job_count = summary.queued_job_count.saturating_add(1);
                }
            } else {
                transaction
                    .execute(
                        "INSERT INTO knowledge_index_jobs (
                            id, job_kind, document_id, revision_id,
                            requested_source_hash, requested_config_hash,
                            state, stage, priority, total_units, completed_units,
                            created_at, updated_at, idempotency_key
                         ) VALUES (?, 'embed', ?, ?, ?, ?, 'queued', 'waiting_embedding',
                                   -10, ?, 0, ?, ?, ?)",
                        params![
                            Uuid::new_v4().to_string(),
                            document_id,
                            revision_id,
                            source_hash,
                            spec.embedding_key,
                            i64::try_from(pending).unwrap_or(i64::MAX),
                            now,
                            now,
                            idempotency_key,
                        ],
                    )
                    .map_err(|error| format!("创建知识库向量任务失败：{error}"))?;
                summary.queued_job_count = summary.queued_job_count.saturating_add(1);
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("提交知识库向量排队失败：{error}"))?;
        Ok(KnowledgeEmbeddingRebuildResult {
            queued_job_count: summary.queued_job_count,
            cached_chunk_count: summary.cached_chunk_count,
            pending_chunk_count: summary.pending_chunk_count,
        })
    }

    pub fn search(
        &self,
        request: KnowledgeSearchRequest,
    ) -> Result<KnowledgeSearchResponse, String> {
        let request = request.normalize_and_validate()?;
        let connection = self.open_connection()?;
        let (fts_available, tokenizer, lexical_degraded) = connection
            .query_row(
                "SELECT fts5_available, tokenizer, lexical_degraded
                 FROM knowledge_index_capabilities WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? != 0,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)? != 0,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("读取知识库检索能力失败：{error}"))?
            .unwrap_or((false, "none".to_string(), true));

        let use_fts = fts_available && request.query.chars().count() >= 3;
        let mut conditions = vec![
            "d.state <> 'deleted'".to_string(),
            "d.include_in_default_scope = 1".to_string(),
            "d.active_revision_id = r.id".to_string(),
            "r.status IN ('ready', 'lexical_ready', 'partial')".to_string(),
            "r.source_hash = d.current_source_hash".to_string(),
        ];
        let mut values = Vec::<Value>::new();
        if use_fts {
            values.push(Value::Text(build_fts_match_query(&request.query)));
        }
        match request.scope {
            KnowledgeQueryScope::Library => {}
            KnowledgeQueryScope::CurrentLiterature => {
                conditions.push("d.source_class = 'literature'".to_string());
                conditions.push("d.library_item_id = ?".to_string());
                values.push(Value::Text(
                    request.current_literature_id.clone().unwrap_or_default(),
                ));
            }
            KnowledgeQueryScope::CurrentNote => {
                conditions.push("d.source_class = 'note'".to_string());
                conditions.push("d.note_id = ?".to_string());
                values.push(Value::Text(
                    request.current_note_id.clone().unwrap_or_default(),
                ));
            }
        }
        if !request.selected_document_ids.is_empty() {
            let placeholders = std::iter::repeat("?")
                .take(request.selected_document_ids.len())
                .collect::<Vec<_>>()
                .join(",");
            conditions.push(format!("d.id IN ({placeholders})"));
            for id in &request.selected_document_ids {
                values.push(Value::Text(id.clone()));
            }
        }
        for element_type in &request.element_types {
            conditions.push("(',' || s.element_types || ',') LIKE ('%,' || ? || ',%')".to_string());
            values.push(Value::Text(element_type.clone()));
        }
        let where_sql = conditions.join(" AND ");
        // Fetch enough candidates before the document-diversity cap. Without
        // this, four matches from one long note can leave fewer than top-k
        // results even when other documents also match.
        let candidate_limit = request.limit.saturating_mul(5).max(40).min(250);
        values.push(Value::Integer(
            i64::try_from(candidate_limit).unwrap_or(250),
        ));

        let fts_sql = format!(
            "SELECT s.chunk_id, d.id, d.source_class, d.source_id, d.title,
                    c.text, s.search_text, c.heading_path_json, s.element_types,
                    c.page_start, c.page_end, c.line_start, c.line_end,
                    r.source_hash, r.id, r.extraction_quality,
                    bm25(f, 8.0, 8.0, 2.0, 1.0, 2.0, 1.0) AS rank
             FROM knowledge_fts f
             JOIN knowledge_fts_source s ON s.rowid = f.rowid
             JOIN knowledge_chunks c ON c.id = s.chunk_id
             JOIN knowledge_revisions r ON r.id = c.revision_id
             JOIN knowledge_documents d ON d.id = r.document_id
             WHERE f MATCH ? AND {where_sql}
             ORDER BY rank ASC, c.id ASC LIMIT ?"
        );
        let like_pattern = escaped_like_pattern(&request.query);
        let like_sql = format!(
            "SELECT s.chunk_id, d.id, d.source_class, d.source_id, d.title,
                    c.text, s.search_text, c.heading_path_json, s.element_types,
                    c.page_start, c.page_end, c.line_start, c.line_end,
                    r.source_hash, r.id, r.extraction_quality,
                    1.0 AS rank
             FROM knowledge_fts_source s
             JOIN knowledge_chunks c ON c.id = s.chunk_id
             JOIN knowledge_revisions r ON r.id = c.revision_id
             JOIN knowledge_documents d ON d.id = r.document_id
             WHERE (s.search_text LIKE ? ESCAPE '\\'
                    OR s.title LIKE ? ESCAPE '\\'
                    OR s.heading_path LIKE ? ESCAPE '\\')
               AND {where_sql}
             ORDER BY d.updated_at DESC, c.ordinal ASC LIMIT ?"
        );

        let mut hits = if use_fts {
            match self.query_hits(&connection, &fts_sql, values.clone(), false, &request.query) {
                Ok(hits) => hits,
                Err(_) => {
                    let mut fallback = values;
                    // The FTS query occupies the first bound parameter, while the
                    // LIKE query needs three patterns before the shared filters.
                    if !fallback.is_empty() {
                        fallback.remove(0);
                    }
                    let mut like_values = vec![
                        Value::Text(like_pattern.clone()),
                        Value::Text(like_pattern.clone()),
                        Value::Text(like_pattern.clone()),
                    ];
                    like_values.extend(fallback);
                    self.query_hits(&connection, &like_sql, like_values, true, &request.query)?
                }
            }
        } else {
            let mut like_values = vec![
                Value::Text(like_pattern.clone()),
                Value::Text(like_pattern.clone()),
                Value::Text(like_pattern),
            ];
            like_values.extend(values);
            self.query_hits(&connection, &like_sql, like_values, true, &request.query)?
        };
        for (index, hit) in hits.iter_mut().enumerate() {
            hit.lexical_rank = Some(index + 1);
        }
        // The SQL already filters active revisions, but retain a deterministic
        // document-level cap so a long note cannot consume every result slot.
        let mut per_document = HashMap::<String, usize>::new();
        hits.retain(|hit| {
            let count = per_document.entry(hit.document_id.clone()).or_default();
            if *count >= 4 {
                false
            } else {
                *count += 1;
                true
            }
        });
        hits.truncate(request.limit);
        let insufficient_evidence = hits.is_empty();
        Ok(KnowledgeSearchResponse {
            query: request.query,
            scope: request.scope.as_str().to_string(),
            hits,
            lexical_degraded: lexical_degraded || tokenizer != "trigram" || !use_fts,
            insufficient_evidence,
            requested_mode: "lexical".to_string(),
            actual_mode: "lexical".to_string(),
            fallback_reason: None,
            vector_dimensions: None,
        })
    }

    /// Execute vector-only or hybrid retrieval against the active revisions.
    /// The query vector is normalized again at this trust boundary; callers
    /// cannot make malformed provider output reach SQLite scoring.
    pub fn search_with_vector(
        &self,
        request: KnowledgeSearchRequest,
        mode: KnowledgeRetrievalMode,
        embedding_key: &str,
        query_vector: Vec<f32>,
    ) -> Result<KnowledgeSearchResponse, String> {
        if mode == KnowledgeRetrievalMode::Lexical {
            return self.search(request);
        }
        let request = request.normalize_and_validate()?;
        let embedding_key = embedding_key.trim();
        if embedding_key.is_empty() || embedding_key.len() > 256 {
            return Err("KNOWLEDGE_VECTOR_CONFIG_INVALID: embedding key is invalid".to_string());
        }
        let query_vector = normalize_l2(query_vector).map_err(|error| error.to_string())?;
        let candidate_limit = request.limit.saturating_mul(5).max(40).min(250);
        let connection = self.open_connection()?;
        let vector_hits = self.query_vector_hits(
            &connection,
            &request,
            embedding_key,
            &query_vector,
            candidate_limit,
        )?;
        if vector_hits.is_empty() {
            return Err(
                "KNOWLEDGE_VECTOR_INDEX_UNAVAILABLE: no ready vectors match the active scope"
                    .to_string(),
            );
        }
        let vector_dimensions = Some(query_vector.len());

        if mode == KnowledgeRetrievalMode::Vector {
            let hits = limit_with_document_diversity(vector_hits, request.limit);
            return Ok(KnowledgeSearchResponse {
                query: request.query,
                scope: request.scope.as_str().to_string(),
                insufficient_evidence: hits.is_empty(),
                hits,
                lexical_degraded: false,
                requested_mode: "vector".to_string(),
                actual_mode: "vector".to_string(),
                fallback_reason: None,
                vector_dimensions,
            });
        }

        let mut lexical_request = request.clone();
        lexical_request.limit = candidate_limit.min(super::types::MAX_KNOWLEDGE_RESULT_LIMIT);
        let lexical_response = self.search(lexical_request)?;
        let lexical_ids = lexical_response
            .hits
            .iter()
            .map(|hit| hit.chunk_id.clone())
            .collect::<Vec<_>>();
        let vector_ids = vector_hits
            .iter()
            .map(|hit| hit.chunk_id.clone())
            .collect::<Vec<_>>();
        let mut lexical_by_id = lexical_response
            .hits
            .into_iter()
            .map(|hit| (hit.chunk_id.clone(), hit))
            .collect::<HashMap<_, _>>();
        let mut vector_by_id = vector_hits
            .into_iter()
            .map(|hit| (hit.chunk_id.clone(), hit))
            .collect::<HashMap<_, _>>();
        let mut fused_hits = Vec::new();
        for fused in reciprocal_rank_fusion(&lexical_ids, &vector_ids, 60) {
            let lexical = lexical_by_id.remove(&fused.chunk_id);
            let vector = vector_by_id.remove(&fused.chunk_id);
            let mut hit = lexical
                .clone()
                .or_else(|| vector.clone())
                .ok_or_else(|| "RRF produced an unknown knowledge chunk".to_string())?;
            hit.lexical_score = lexical.as_ref().and_then(|value| value.lexical_score);
            hit.vector_score = vector.as_ref().and_then(|value| value.vector_score);
            hit.lexical_rank = fused.lexical_rank;
            hit.vector_rank = fused.vector_rank;
            hit.fused_score = Some(fused.score);
            hit.score = fused.score;
            fused_hits.push(hit);
        }
        let hits = limit_with_document_diversity(fused_hits, request.limit);
        Ok(KnowledgeSearchResponse {
            query: request.query,
            scope: request.scope.as_str().to_string(),
            insufficient_evidence: hits.is_empty(),
            hits,
            lexical_degraded: lexical_response.lexical_degraded,
            requested_mode: "hybrid".to_string(),
            actual_mode: "hybrid".to_string(),
            fallback_reason: None,
            vector_dimensions,
        })
    }

    pub fn get_chunk(&self, chunk_id: &str) -> Result<KnowledgeChunkView, String> {
        let chunk_id = normalize_identifier("知识 chunk ID", chunk_id)?;
        let connection = self.open_connection()?;
        connection
            .query_row(
                "SELECT c.id, d.id, c.revision_id, c.block_kind, c.text, c.search_text,
                        c.heading_path_json, c.element_ids_json, c.asset_ids_json,
                        c.page_start, c.page_end, c.line_start, c.line_end,
                        c.byte_start, c.byte_end, r.source_hash, r.extraction_quality
                 FROM knowledge_chunks c
                 JOIN knowledge_revisions r ON r.id = c.revision_id
                 JOIN knowledge_documents d ON d.id = r.document_id
                WHERE c.id = ? AND d.state <> 'deleted'
                   AND d.active_revision_id = r.id
                   AND r.source_hash = d.current_source_hash
                   AND r.status IN ('ready', 'lexical_ready', 'partial')",
                params![chunk_id],
                |row| {
                    Ok(KnowledgeChunkView {
                        id: row.get(0)?,
                        document_id: row.get(1)?,
                        revision_id: row.get(2)?,
                        block_kind: row.get(3)?,
                        text: row.get(4)?,
                        search_text: row.get(5)?,
                        heading_path: parse_json_vec(row.get::<_, String>(6)?),
                        element_ids: parse_json_vec(row.get::<_, String>(7)?),
                        asset_ids: parse_json_vec(row.get::<_, String>(8)?),
                        page_start: row.get::<_, Option<i64>>(9)?.map(to_u32),
                        page_end: row.get::<_, Option<i64>>(10)?.map(to_u32),
                        line_start: row.get::<_, Option<i64>>(11)?.map(to_u32),
                        line_end: row.get::<_, Option<i64>>(12)?.map(to_u32),
                        byte_start: to_u64(row.get::<_, i64>(13)?),
                        byte_end: to_u64(row.get::<_, i64>(14)?),
                        source_hash: row.get(15)?,
                        extraction_quality: row.get(16)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("读取知识 chunk 失败：{error}"))?
            .ok_or_else(|| "知识 chunk 不存在、未激活或已被回收。".to_string())
    }

    /// Return every active embedding job whose route is no longer allowed.
    /// The caller persists cancellation before signalling in-memory tokens so
    /// a response already in flight cannot commit through its old lease.
    pub(crate) fn active_embedding_job_ids_except(
        &self,
        keep_embedding_key: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let keep_embedding_key = keep_embedding_key
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if keep_embedding_key.is_some_and(|value| value.len() > 256) {
            return Err("Embedding key is invalid.".to_string());
        }
        let connection = self.open_connection()?;
        let (sql, value) = if let Some(embedding_key) = keep_embedding_key {
            (
                "SELECT id FROM knowledge_index_jobs
                 WHERE job_kind = 'embed'
                   AND state IN ('queued', 'running', 'cancelling', 'paused')
                   AND requested_config_hash <> ?
                 ORDER BY created_at ASC, id ASC",
                Some(embedding_key),
            )
        } else {
            (
                "SELECT id FROM knowledge_index_jobs
                 WHERE job_kind = 'embed'
                   AND state IN ('queued', 'running', 'cancelling', 'paused')
                 ORDER BY created_at ASC, id ASC",
                None,
            )
        };
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| format!("准备活动向量任务列表失败：{error}"))?;
        let mut job_ids = Vec::new();
        if let Some(value) = value {
            let rows = statement
                .query_map([value], |row| row.get::<_, String>(0))
                .map_err(|error| format!("查询活动向量任务失败：{error}"))?;
            for row in rows {
                job_ids.push(row.map_err(|error| format!("读取活动向量任务失败：{error}"))?);
            }
        } else {
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| format!("查询活动向量任务失败：{error}"))?;
            for row in rows {
                job_ids.push(row.map_err(|error| format!("读取活动向量任务失败：{error}"))?);
            }
        }
        Ok(job_ids)
    }

    pub fn cancel_job(&self, job_id: &str) -> Result<bool, String> {
        let job_id = normalize_identifier("知识库任务 ID", job_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始取消知识库任务失败：{error}"))?;
        let current = transaction
            .query_row(
                "SELECT state, execution_version, state_version FROM knowledge_index_jobs WHERE id = ?",
                params![job_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
            )
            .optional()
            .map_err(|error| format!("读取知识库任务状态失败：{error}"))?;
        let Some((state, execution_version, state_version)) = current else {
            return Ok(false);
        };
        let Some(from) = KnowledgeJobState::parse(&state) else {
            return Err("知识库任务状态无效。".to_string());
        };
        let target = match from {
            KnowledgeJobState::Queued => KnowledgeJobState::Cancelled,
            KnowledgeJobState::Running | KnowledgeJobState::Paused => KnowledgeJobState::Cancelling,
            KnowledgeJobState::Cancelling
            | KnowledgeJobState::Succeeded
            | KnowledgeJobState::Partial
            | KnowledgeJobState::Failed
            | KnowledgeJobState::Cancelled
            | KnowledgeJobState::Stale => return Ok(false),
        };
        if !can_transition_job_state(from, target) {
            return Ok(false);
        }
        let now = now_ms();
        let next_state_version = state_version.saturating_add(1);
        // A queued job has no worker that can acknowledge cancellation. Close
        // it atomically and advance execution_version so a racing claimant or
        // delayed provider callback cannot publish a result. Running/paused
        // jobs first enter `cancelling`; their lease remains available for the
        // worker to observe the request and clean up.
        let (next_execution_version, changed) = if target == KnowledgeJobState::Cancelled {
            let next_execution_version = execution_version.saturating_add(1);
            let changed = transaction
                .execute(
                    "UPDATE knowledge_index_jobs
                     SET state = 'cancelled', stage = 'done',
                         execution_version = ?, state_version = ?,
                         cancel_requested_at = COALESCE(cancel_requested_at, ?),
                         error_code = 'CANCELLED_BY_USER',
                         error_message = '用户取消了知识库任务。',
                         finished_at = ?, heartbeat_at = ?,
                         lease_token = NULL, lease_owner = NULL,
                         lease_expires_at = NULL, runtime_instance_id = NULL,
                         updated_at = ?
                     WHERE id = ? AND state = ? AND execution_version = ?
                       AND state_version = ?",
                    params![
                        next_execution_version,
                        next_state_version,
                        now,
                        now,
                        now,
                        now,
                        job_id,
                        state,
                        execution_version,
                        state_version,
                    ],
                )
                .map_err(|error| format!("更新知识库任务取消状态失败：{error}"))?;
            (next_execution_version, changed)
        } else {
            let changed = transaction
                .execute(
                    "UPDATE knowledge_index_jobs
                     SET state = ?, stage = 'cleaning', state_version = ?,
                         cancel_requested_at = COALESCE(cancel_requested_at, ?),
                         updated_at = ?
                     WHERE id = ? AND state = ? AND execution_version = ?
                       AND state_version = ?",
                    params![
                        target.as_str(),
                        next_state_version,
                        now,
                        now,
                        job_id,
                        state,
                        execution_version,
                        state_version,
                    ],
                )
                .map_err(|error| format!("更新知识库任务取消状态失败：{error}"))?;
            (execution_version, changed)
        };
        if changed == 0 {
            return Ok(false);
        }
        insert_job_event_tx(
            &transaction,
            &job_id,
            if target == KnowledgeJobState::Cancelled {
                "jobCancelled"
            } else {
                "cancelRequested"
            },
            Some(&state),
            Some(target.as_str()),
            next_execution_version,
            next_state_version,
            if target == KnowledgeJobState::Cancelled {
                r#"{"reason":"user","terminal":true}"#
            } else {
                r#"{"reason":"user","terminal":false}"#
            },
            None,
            None,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("提交知识库任务取消失败：{error}"))?;
        Ok(true)
    }

    /// 将已经收到取消请求的任务收敛到终态。
    ///
    /// 只有 `cancelling` 任务可以由 Worker 调用此方法；终态写入同时递增
    /// `execution_version` 并清理租约，从而让仍在网络层运行的旧实例和迟到的
    /// MinerU 结果失效。重复调用已经取消的任务是幂等的，其它状态则拒绝越级收敛。
    pub fn finalize_cancelled_job(&self, job_id: &str, reason: &str) -> Result<bool, String> {
        let job_id = normalize_identifier("知识库任务 ID", job_id)?;
        let reason = bounded_error(reason);
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始完成知识库任务取消失败：{error}"))?;
        let current = transaction
            .query_row(
                "SELECT state, execution_version, state_version
                 FROM knowledge_index_jobs WHERE id = ?",
                params![job_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("读取知识库任务取消状态失败：{error}"))?;
        let Some((state, execution_version, state_version)) = current else {
            return Ok(false);
        };
        if state == KnowledgeJobState::Cancelled.as_str() {
            return Ok(true);
        }
        if state != KnowledgeJobState::Cancelling.as_str() {
            return Ok(false);
        }
        let next_execution_version = execution_version.saturating_add(1);
        let next_state_version = state_version.saturating_add(1);
        let now = now_ms();
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'cancelled', stage = 'done',
                     execution_version = ?, state_version = ?,
                     error_code = 'CANCELLED', error_message = ?,
                     finished_at = ?, heartbeat_at = ?,
                     lease_token = NULL, lease_owner = NULL,
                     lease_expires_at = NULL, runtime_instance_id = NULL,
                     updated_at = ?
                 WHERE id = ? AND state = 'cancelling'
                   AND execution_version = ? AND state_version = ?",
                params![
                    next_execution_version,
                    next_state_version,
                    reason,
                    now,
                    now,
                    now,
                    job_id,
                    execution_version,
                    state_version,
                ],
            )
            .map_err(|error| format!("完成知识库任务取消失败：{error}"))?;
        if changed != 1 {
            return Ok(false);
        }
        let payload = serde_json::json!({
            "reason": reason,
            "terminal": true,
        })
        .to_string();
        insert_job_event_tx(
            &transaction,
            &job_id,
            "jobCancelled",
            Some("cancelling"),
            Some("cancelled"),
            next_execution_version,
            next_state_version,
            &payload,
            None,
            None,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("提交完成知识库任务取消失败：{error}"))?;
        Ok(true)
    }

    pub fn recover_stale_jobs(&self) -> Result<usize, String> {
        let connection = self.open_connection()?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("开始恢复知识库任务失败：{error}"))?;
        let cutoff = now_ms().saturating_sub(STALE_JOB_AFTER_MS);
        let mut statement = transaction
            .prepare(
                "SELECT id, state, execution_version, state_version
                 FROM knowledge_index_jobs
                 WHERE state IN ('running', 'cancelling')
                   AND COALESCE(heartbeat_at, updated_at) < ?",
            )
            .map_err(|error| format!("准备恢复知识库任务失败：{error}"))?;
        let rows = statement
            .query_map(params![cutoff], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| format!("查询过期知识库任务失败：{error}"))?;
        let stale = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取过期知识库任务失败：{error}"))?;
        drop(statement);
        let now = now_ms();
        let mut recovered = 0usize;
        for (id, state, execution_version, state_version) in stale {
            let next_execution_version = execution_version.saturating_add(1);
            let next_state_version = state_version.saturating_add(1);
            let changed = transaction
                .execute(
                    "UPDATE knowledge_index_jobs
                     SET state = 'stale', stage = 'cleaning',
                         execution_version = ?, state_version = ?,
                         lease_token = NULL, lease_owner = NULL,
                         lease_expires_at = NULL, runtime_instance_id = NULL,
                         error_code = 'STALE_RECOVERED',
                         error_message = '任务实例已失去心跳，等待重新构建。',
                         finished_at = ?, updated_at = ?
                     WHERE id = ? AND execution_version = ? AND state_version = ?",
                    params![
                        next_execution_version,
                        next_state_version,
                        now,
                        now,
                        id,
                        execution_version,
                        state_version
                    ],
                )
                .map_err(|error| format!("标记过期知识库任务失败：{error}"))?;
            if changed > 0 {
                insert_job_event_tx(
                    &transaction,
                    &id,
                    "staleRecovered",
                    Some(&state),
                    Some("stale"),
                    next_execution_version,
                    next_state_version,
                    r#"{"reason":"heartbeatExpired"}"#,
                    None,
                    None,
                    now,
                )?;
                recovered += 1;
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("提交知识库任务恢复失败：{error}"))?;
        Ok(recovered)
    }

    /// Converge a claimed job after its in-process worker panicked or its
    /// join handle failed.  This is deliberately a lease/CAS operation: a
    /// replacement worker may already own a newer execution, in which case a
    /// late panic must be ignored rather than corrupting the replacement.
    pub(crate) fn mark_claim_stale(
        &self,
        claim: &KnowledgeJobClaim,
        code: &str,
        message: &str,
    ) -> Result<bool, String> {
        let code = bounded_error(code);
        let message = bounded_error(message);
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to begin stale worker convergence: {error}"))?;
        let current: Option<(String, i64, i64, Option<String>, Option<String>)> = transaction
            .query_row(
                "SELECT state, execution_version, state_version,
                        runtime_instance_id, lease_token
                 FROM knowledge_index_jobs WHERE id = ?",
                params![claim.job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("failed to inspect panicked worker claim: {error}"))?;
        let Some((state, execution_version, state_version, runtime_id, lease_token)) = current
        else {
            transaction
                .commit()
                .map_err(|error| format!("failed to close missing stale worker claim: {error}"))?;
            return Ok(false);
        };
        if !matches!(state.as_str(), "running" | "cancelling")
            || execution_version != claim.execution_version
            || state_version != claim.state_version
            || runtime_id.as_deref() != Some(claim.runtime_instance_id.as_str())
            || lease_token.as_deref() != Some(claim.lease_token.as_str())
        {
            transaction
                .commit()
                .map_err(|error| format!("failed to close stale worker CAS miss: {error}"))?;
            return Ok(false);
        }
        let now = now_ms();
        let next_execution_version = execution_version.saturating_add(1);
        let next_state_version = state_version.saturating_add(1);
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'stale', stage = 'cleaning',
                     execution_version = ?, state_version = ?,
                     error_code = ?, error_message = ?, finished_at = ?, updated_at = ?,
                     heartbeat_at = ?, lease_token = NULL, lease_owner = NULL,
                     lease_expires_at = NULL, runtime_instance_id = NULL
                 WHERE id = ? AND state IN ('running', 'cancelling')
                   AND execution_version = ? AND state_version = ?
                   AND runtime_instance_id = ? AND lease_token = ?",
                params![
                    next_execution_version,
                    next_state_version,
                    code,
                    message,
                    now,
                    now,
                    now,
                    claim.job_id,
                    claim.execution_version,
                    claim.state_version,
                    claim.runtime_instance_id,
                    claim.lease_token,
                ],
            )
            .map_err(|error| format!("failed to mark panicked knowledge job stale: {error}"))?;
        if changed != 1 {
            transaction
                .commit()
                .map_err(|error| format!("failed to commit stale worker CAS miss: {error}"))?;
            return Ok(false);
        }
        transaction
            .execute(
                "UPDATE knowledge_documents
                 SET state = 'stale', updated_at = ?
                 WHERE id = ? AND active_revision_id IS NULL AND state <> 'deleted'",
                params![now, claim.document_id],
            )
            .map_err(|error| format!("failed to update stale knowledge document: {error}"))?;
        let payload = serde_json::json!({
            "stage": "cleaning",
            "errorCode": code,
            "reason": message,
        })
        .to_string();
        insert_job_event_tx(
            &transaction,
            &claim.job_id,
            "workerStale",
            Some(&state),
            Some("stale"),
            next_execution_version,
            next_state_version,
            &payload,
            None,
            Some(&claim.runtime_instance_id),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit stale worker convergence: {error}"))?;
        Ok(true)
    }

    pub fn mark_source_deleted(&self, source_class: &str, source_id: &str) -> Result<bool, String> {
        if source_class != "literature" && source_class != "note" {
            return Err("知识源类型不受支持。".to_string());
        }
        let source_id = normalize_identifier("知识源 ID", source_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始停用知识源失败：{error}"))?;
        let now = now_ms();
        let document_ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT id FROM knowledge_documents
                     WHERE source_class = ? AND source_id = ?",
                )
                .map_err(|error| format!("查询待停用知识文档失败：{error}"))?;
            let rows = statement
                .query_map(params![source_class, source_id], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| format!("读取待停用知识文档失败：{error}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("收集待停用知识文档失败：{error}"))?
        };
        let changed = transaction
            .execute(
                "UPDATE knowledge_documents
                 SET state = 'deleted', include_in_default_scope = 0,
                     active_revision_id = NULL, updated_at = ?
                 WHERE source_class = ? AND source_id = ? AND state <> 'deleted'",
                params![now, source_class, source_id],
            )
            .map_err(|error| format!("停用知识源失败：{error}"))?;
        transaction
            .execute(
                "UPDATE knowledge_revisions SET status = 'stale', updated_at = ?
                 WHERE document_id IN (
                     SELECT id FROM knowledge_documents WHERE source_class = ? AND source_id = ?
                 ) AND status NOT IN ('failed', 'cancelled', 'stale')",
                params![now, source_class, source_id],
            )
            .map_err(|error| format!("停用知识源 revision 失败：{error}"))?;
        let mut active_jobs = Vec::new();
        if !document_ids.is_empty() {
            let placeholders = std::iter::repeat("?")
                .take(document_ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT id, state, execution_version, state_version
                 FROM knowledge_index_jobs
                 WHERE document_id IN ({placeholders})
                   AND state IN ('queued', 'running', 'cancelling', 'paused')"
            );
            let values = document_ids
                .iter()
                .map(|id| Value::Text(id.clone()))
                .collect::<Vec<_>>();
            let mut statement = transaction
                .prepare(&sql)
                .map_err(|error| format!("查询待取消知识库任务失败：{error}"))?;
            let rows = statement
                .query_map(params_from_iter(values.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .map_err(|error| format!("读取待取消知识库任务失败：{error}"))?;
            active_jobs = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("收集待取消知识库任务失败：{error}"))?;
        }
        let mut cancelled_jobs = 0usize;
        for (job_id, state, execution_version, state_version) in active_jobs {
            let next_execution_version = execution_version.saturating_add(1);
            let next_state_version = state_version.saturating_add(1);
            let job_changed = transaction
                .execute(
                    "UPDATE knowledge_index_jobs
                     SET state = 'cancelled', stage = 'done',
                         execution_version = ?, state_version = ?,
                         cancel_requested_at = COALESCE(cancel_requested_at, ?),
                         error_code = 'SOURCE_DELETED',
                         error_message = '知识源已被删除，任务结果已作废。',
                         finished_at = ?, heartbeat_at = ?,
                         lease_token = NULL, lease_owner = NULL,
                         lease_expires_at = NULL, runtime_instance_id = NULL,
                         updated_at = ?
                     WHERE id = ? AND state = ? AND execution_version = ?
                       AND state_version = ?",
                    params![
                        next_execution_version,
                        next_state_version,
                        now,
                        now,
                        now,
                        now,
                        job_id,
                        state,
                        execution_version,
                        state_version,
                    ],
                )
                .map_err(|error| format!("取消停用知识源任务失败：{error}"))?;
            if job_changed == 1 {
                insert_job_event_tx(
                    &transaction,
                    &job_id,
                    "jobCancelled",
                    Some(&state),
                    Some("cancelled"),
                    next_execution_version,
                    next_state_version,
                    r#"{"reason":"sourceDeleted","terminal":true}"#,
                    None,
                    None,
                    now,
                )?;
                cancelled_jobs = cancelled_jobs.saturating_add(1);
            }
        }
        // `knowledge_fts_source` is the external-content projection for the FTS
        // table.  Keep it in sync even when the document was already marked
        // deleted (for example, after a previous interrupted cleanup).
        let removed_fts = transaction
            .execute(
                "DELETE FROM knowledge_fts_source
                 WHERE document_id IN (
                     SELECT id FROM knowledge_documents WHERE source_class = ? AND source_id = ?
                 )",
                params![source_class, source_id],
            )
            .map_err(|error| format!("清理停用知识源 FTS 投影失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("提交停用知识源失败：{error}"))?;
        Ok(changed > 0 || cancelled_jobs > 0 || removed_fts > 0)
    }

    fn mark_literature_deleted(&self, item_id: &str) -> Result<KnowledgeDocumentStatus, String> {
        let _ = self.mark_source_deleted("literature", item_id)?;
        let connection = self.open_connection()?;
        connection
            .query_row(
                "SELECT id FROM knowledge_documents WHERE source_class = 'literature' AND source_id = ?",
                params![item_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取已停用文献知识源失败：{error}"))?
            .map(|id| self.get_document_status(&id))
            .transpose()?
            .ok_or_else(|| "文献知识源不存在。".to_string())
    }

    fn load_note(&self, note_id: &str) -> Result<NoteInput, String> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                "SELECT n.id, n.title, n.content, n.directory_path, n.content_hash,
                        i.deleted_at
                 FROM library_notes n
                 LEFT JOIN library_items i ON i.id = n.item_id
                 WHERE n.id = ? AND (n.item_id IS NULL OR i.deleted_at IS NULL)",
                params![note_id],
                |row| {
                    Ok((
                        NoteInput {
                            title: row.get(1)?,
                            content: row.get(2)?,
                            directory_path: row.get(3)?,
                            content_hash: row.get(4)?,
                        },
                        row.get::<_, Option<i64>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("读取 Markdown 知识源失败：{error}"))?
            .and_then(|(note, deleted)| deleted.is_none().then_some(note))
            .ok_or_else(|| "笔记不存在或所属文献位于回收站。".to_string())
    }

    fn prepare_note_assets(
        &self,
        input: &NoteInput,
        parsed: &MarkdownDocument,
    ) -> (Vec<PreparedAsset>, HashMap<String, String>, Vec<String>) {
        let mut refs = HashMap::<String, MarkdownImageRef>::new();
        let mut rejected = HashSet::new();
        for block in &parsed.blocks {
            for image in &block.image_refs {
                if let Some(relative) = safe_relative_asset_path(&image.source) {
                    refs.entry(relative).or_insert_with(|| image.clone());
                } else {
                    rejected.insert(image.source.clone());
                }
            }
        }
        let mut warnings = rejected
            .into_iter()
            .map(|source| format!("MARKDOWN_ASSET_PATH_REJECTED:{}", bounded_error(&source)))
            .collect::<Vec<_>>();
        warnings.sort();
        let Some(directory) = input.directory_path.as_deref() else {
            if !refs.is_empty() {
                warnings.push("MARKDOWN_ASSET_ROOT_MISSING".to_string());
            }
            return (Vec::new(), HashMap::new(), warnings);
        };
        let Ok(root) = resolve_note_directory(&self.library_root, directory) else {
            warnings.push("MARKDOWN_ASSET_ROOT_UNSAFE".to_string());
            return (Vec::new(), HashMap::new(), warnings);
        };
        let Ok(root) = fs::canonicalize(&root) else {
            warnings.push("MARKDOWN_ASSET_ROOT_MISSING".to_string());
            return (Vec::new(), HashMap::new(), warnings);
        };
        if !root.is_dir() {
            warnings.push("MARKDOWN_ASSET_ROOT_NOT_DIRECTORY".to_string());
            return (Vec::new(), HashMap::new(), warnings);
        }
        let mut assets = Vec::new();
        let mut by_source = HashMap::new();
        for (relative, image) in refs {
            let Some(mime_type) = mime_type_for_path(&relative) else {
                warnings.push(format!("MARKDOWN_ASSET_TYPE_UNSUPPORTED:{relative}"));
                continue;
            };
            let path = root.join(&relative);
            let Ok(canonical) = fs::canonicalize(&path) else {
                warnings.push(format!("MARKDOWN_ASSET_MISSING:{relative}"));
                continue;
            };
            if !canonical.starts_with(&root) {
                warnings.push(format!("MARKDOWN_ASSET_OUTSIDE_ROOT:{relative}"));
                continue;
            }
            let Ok(metadata) = fs::metadata(&canonical) else {
                warnings.push(format!("MARKDOWN_ASSET_MISSING:{relative}"));
                continue;
            };
            if !metadata.is_file()
                || metadata.len() == 0
                || metadata.len() > MAX_MARKDOWN_ASSET_BYTES
            {
                warnings.push(format!("MARKDOWN_ASSET_SIZE_INVALID:{relative}"));
                continue;
            }
            let Ok(bytes) = fs::read(&canonical) else {
                warnings.push(format!("MARKDOWN_ASSET_READ_FAILED:{relative}"));
                continue;
            };
            let id = Uuid::new_v4().to_string();
            let asset = PreparedAsset {
                id: id.clone(),
                relative_path: format!("{}/{}", directory.replace('\\', "/"), relative),
                mime_type: mime_type.to_string(),
                byte_size: bytes.len() as u64,
                sha256: super::markdown::sha256_hex(&bytes),
                alt_text: image.alt.clone(),
                caption: image.title.clone(),
                source_asset_name: relative.clone(),
            };
            by_source.insert(relative, id);
            assets.push(asset);
        }
        (assets, by_source, warnings)
    }

    fn commit_markdown_revision(
        &self,
        connection: &mut Connection,
        document: &SourceDocument,
        job_id: &str,
        source_hash: &str,
        parsed: &MarkdownDocument,
        assets: &[PreparedAsset],
        asset_by_source: &HashMap<String, String>,
        warnings: &mut Vec<String>,
        now: i64,
    ) -> Result<(), String> {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始写入 Markdown revision 失败：{error}"))?;
        let current_source_hash: String = transaction
            .query_row(
                "SELECT content_hash FROM library_notes
                 WHERE id = ? AND (item_id IS NULL OR EXISTS (
                    SELECT 1 FROM library_items i
                    WHERE i.id = library_notes.item_id AND i.deleted_at IS NULL
                 ))",
                params![document.source_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("检查 Markdown 当前版本失败：{error}"))?
            .unwrap_or_default();
        if current_source_hash != source_hash {
            return Err("笔记在索引期间发生变化，已拒绝写入过期 revision。".to_string());
        }
        let existing = transaction
            .query_row(
                "SELECT id FROM knowledge_revisions
                 WHERE document_id = ? AND source_hash = ? AND parser_id = ?
                   AND parser_version = ? AND normalization_version = ?
                   AND chunk_policy_version = ?
                 ORDER BY created_at DESC LIMIT 1",
                params![
                    document.id,
                    source_hash,
                    MARKDOWN_PARSER_ID,
                    MARKDOWN_PARSER_VERSION,
                    MARKDOWN_NORMALIZATION_VERSION,
                    MARKDOWN_CHUNK_POLICY_VERSION,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("检查 Markdown 历史 revision 失败：{error}"))?;
        if let Some(revision_id) = existing {
            let complete: Option<(String, String, String)> = transaction
                .query_row(
                    "SELECT id, status, warning_json FROM knowledge_revisions
                     WHERE id = ? AND status IN ('ready', 'lexical_ready', 'partial')",
                    params![revision_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|error| format!("检查 Markdown revision 状态失败：{error}"))?;
            if let Some((revision_id, revision_status, revision_warning_json)) = complete {
                let terminal_state = if revision_status == "partial"
                    || has_warning_entries(&revision_warning_json)
                {
                    KnowledgeJobState::Partial
                } else {
                    KnowledgeJobState::Succeeded
                };
                transaction
                    .execute(
                        "UPDATE knowledge_documents
                         SET active_revision_id = ?, state = ?,
                             include_in_default_scope = 1, updated_at = ? WHERE id = ?",
                        params![
                            revision_id,
                            terminal_state.document_state(),
                            now,
                            document.id
                        ],
                    )
                    .map_err(|error| format!("复用 Markdown revision 失败：{error}"))?;
                finish_job_tx(
                    &transaction,
                    job_id,
                    Some(&revision_id),
                    terminal_state,
                    now,
                )?;
                transaction
                    .commit()
                    .map_err(|error| format!("提交复用 Markdown revision 失败：{error}"))?;
                return Ok(());
            }
        }

        let revision_id = Uuid::new_v4().to_string();
        let canonical_hash = &parsed.canonical_hash;
        let warning_json = serde_json::to_string(warnings)
            .map_err(|error| format!("序列化 Markdown 索引警告失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO knowledge_revisions (
                    id, document_id, source_hash, canonical_hash, parser_id, parser_version,
                    provider_id, normalization_version, chunk_policy_version, content_path,
                    line_count, byte_count, extraction_quality, quality_flags, status,
                    warning_json, created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, 'local', ?,
                           ?, '', ?, ?, 'local_text_only', ?, 'building', ?, ?, ?)",
                params![
                    revision_id,
                    document.id,
                    source_hash,
                    canonical_hash,
                    MARKDOWN_PARSER_ID,
                    MARKDOWN_PARSER_VERSION,
                    MARKDOWN_NORMALIZATION_VERSION,
                    MARKDOWN_CHUNK_POLICY_VERSION,
                    i64::try_from(parsed.line_count).unwrap_or(i64::MAX),
                    i64::try_from(parsed.canonical.len()).unwrap_or(i64::MAX),
                    warning_json,
                    warning_json,
                    now,
                    now,
                ],
            )
            .map_err(|error| format!("创建 Markdown revision 失败：{error}"))?;

        let mut prepared_elements = Vec::new();
        let mut prepared_chunks = Vec::new();
        for (element_ordinal, block) in parsed.blocks.iter().enumerate() {
            let element_id = Uuid::new_v4().to_string();
            let asset_ids = block
                .image_refs
                .iter()
                .filter_map(|image| safe_relative_asset_path(&image.source))
                .filter_map(|path| asset_by_source.get(&path).cloned())
                .collect::<Vec<_>>();
            let element_type = if !asset_ids.is_empty() && block.element_type == "paragraph" {
                "figure".to_string()
            } else {
                block.element_type.clone()
            };
            let heading_path_json = serde_json::to_string(&block.heading_path)
                .map_err(|error| format!("序列化 Markdown 标题路径失败：{error}"))?;
            prepared_elements.push(PreparedElement {
                id: element_id.clone(),
                ordinal: element_ordinal,
                element_type,
                block_kind: block.kind.clone(),
                text: block.search_text.clone(),
                raw_text: block.text.clone(),
                search_text: block.search_text.clone(),
                heading_path_json: heading_path_json.clone(),
                line_start: block.line_start,
                line_end: block.line_end,
                byte_start: block.byte_start,
                byte_end: block.byte_end,
                char_start: block.char_start,
                char_end: block.char_end,
                asset_ids: asset_ids.clone(),
            });
            let segments = split_block(block, 1_600, 2_400, 200);
            for segment in segments {
                let chunk_id = Uuid::new_v4().to_string();
                let chunk_search = search_projection(&segment.text, &block.image_refs);
                let line_start = line_number_at(&parsed.canonical, segment.byte_start);
                let line_end =
                    line_number_at(&parsed.canonical, segment.byte_end.saturating_sub(1));
                prepared_chunks.push(PreparedChunk {
                    id: chunk_id,
                    block_kind: block.kind.clone(),
                    text: segment.text,
                    search_text: chunk_search,
                    heading_path_json: heading_path_json.clone(),
                    element_ids_json: serde_json::to_string(&vec![element_id.clone()])
                        .map_err(|error| format!("序列化 Markdown 元素关联失败：{error}"))?,
                    asset_ids_json: serde_json::to_string(&asset_ids)
                        .map_err(|error| format!("序列化 Markdown 资产关联失败：{error}"))?,
                    line_start,
                    line_end,
                    byte_start: segment.byte_start,
                    byte_end: segment.byte_end,
                    char_start: segment.char_start,
                    char_end: segment.char_end,
                    ordinal: 0,
                    is_overlap: segment.is_overlap,
                });
            }
        }
        for (ordinal, chunk) in prepared_chunks.iter_mut().enumerate() {
            chunk.ordinal = ordinal;
        }

        let mut next_fts_rowid: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(fts_rowid), 0) + 1 FROM knowledge_chunks",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("分配知识库 FTS 行号失败：{error}"))?;
        if next_fts_rowid <= 0 {
            next_fts_rowid = 1;
        }
        for asset in assets {
            transaction
                .execute(
                    "INSERT INTO knowledge_assets (
                        id, revision_id, asset_kind, relative_path, mime_type, byte_size,
                        sha256, alt_text, caption, source_asset_name, created_at
                     ) VALUES (?, ?, 'embedded_image', ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        asset.id,
                        revision_id,
                        asset.relative_path,
                        asset.mime_type,
                        i64::try_from(asset.byte_size).unwrap_or(i64::MAX),
                        asset.sha256,
                        asset.alt_text,
                        asset.caption,
                        asset.source_asset_name,
                        now,
                    ],
                )
                .map_err(|error| format!("写入 Markdown 图片资产失败：{error}"))?;
        }
        for element in &prepared_elements {
            transaction
                .execute(
                    "INSERT INTO knowledge_elements (
                        id, revision_id, element_type, ordinal, reading_order,
                        line_start, line_end, byte_start, byte_end, char_start, char_end,
                        heading_path_json,
                        text, raw_text, caption, source_ref_json, metadata_json, content_hash,
                        created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '', '{}', ?, ?, ?)",
                    params![
                        element.id,
                        revision_id,
                        element.element_type,
                        element.ordinal as i64,
                        element.ordinal as i64,
                        element.line_start as i64,
                        element.line_end as i64,
                        element.byte_start as i64,
                        element.byte_end as i64,
                        element.char_start as i64,
                        element.char_end as i64,
                        element.heading_path_json,
                        element.text,
                        element.raw_text,
                        serde_json::json!({
                            "blockKind": element.block_kind,
                            "searchText": element.search_text,
                        })
                        .to_string(),
                        super::markdown::sha256_hex(element.raw_text.as_bytes()),
                        now,
                    ],
                )
                .map_err(|error| format!("写入 Markdown 元素失败：{error}"))?;
            for (ordinal, asset_id) in element.asset_ids.iter().enumerate() {
                transaction
                    .execute(
                        "INSERT INTO knowledge_element_assets (element_id, asset_id, role, ordinal)
                         VALUES (?, ?, 'primary', ?)",
                        params![element.id, asset_id, ordinal as i64],
                    )
                    .map_err(|error| format!("关联 Markdown 图片资产失败：{error}"))?;
            }
        }
        for index in 0..prepared_chunks.len() {
            let chunk = &prepared_chunks[index];
            let prev = index
                .checked_sub(1)
                .map(|value| prepared_chunks[value].id.clone());
            let next =
                (index + 1 < prepared_chunks.len()).then(|| prepared_chunks[index + 1].id.clone());
            transaction
                .execute(
                    "INSERT INTO knowledge_chunks (
                        id, revision_id, ordinal, block_kind, text, search_text, content_hash,
                        line_start, line_end, byte_start, byte_end, char_start, char_end,
                        heading_path_json, element_ids_json, asset_ids_json, quality_flags,
                        metadata_json, prev_chunk_id, next_chunk_id, fts_rowid, token_estimate,
                        is_overlap, created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '{}', ?, ?, ?, ?, ?, ?)",
                    params![
                        chunk.id,
                        revision_id,
                        chunk.ordinal as i64,
                        chunk.block_kind,
                        chunk.text,
                        chunk.search_text,
                        super::markdown::sha256_hex(chunk.text.as_bytes()),
                        chunk.line_start.map(|value| value as i64),
                        chunk.line_end.map(|value| value as i64),
                        chunk.byte_start as i64,
                        chunk.byte_end as i64,
                        chunk.char_start as i64,
                        chunk.char_end as i64,
                        chunk.heading_path_json,
                        chunk.element_ids_json,
                        chunk.asset_ids_json,
                        serde_json::to_string(warnings).unwrap_or_else(|_| "[]".to_string()),
                        prev,
                        next,
                        next_fts_rowid,
                        i64::try_from(chunk.text.chars().count().div_ceil(4)).unwrap_or(i64::MAX),
                        if chunk.is_overlap { 1 } else { 0 },
                        now,
                    ],
                )
                .map_err(|error| format!("写入 Markdown chunk 失败：{error}"))?;
            let element_types = prepared_elements
                .iter()
                .find(|element| chunk.element_ids_json.contains(&element.id))
                .map(|element| element.element_type.clone())
                .unwrap_or_else(|| "text".to_string());
            let heading_path = chunk
                .heading_path_json
                .trim_matches(['[', ']'])
                .replace('"', "");
            transaction
                .execute(
                    "INSERT INTO knowledge_fts_source (
                        rowid, chunk_id, revision_id, document_id, title, heading_path,
                        element_types, body, multimodal_text, search_text
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        next_fts_rowid,
                        chunk.id,
                        revision_id,
                        document.id,
                        document.title,
                        heading_path,
                        element_types,
                        chunk.search_text,
                        chunk.search_text,
                        chunk.search_text,
                    ],
                )
                .map_err(|error| format!("写入 Markdown FTS 投影失败：{error}"))?;
            next_fts_rowid = next_fts_rowid.saturating_add(1);
        }

        if let Some(old_revision_id) = document.active_revision_id.as_deref() {
            transaction
                .execute(
                    "UPDATE knowledge_revisions SET status = 'stale', updated_at = ?
                     WHERE id = ? AND id <> ?",
                    params![now, old_revision_id, revision_id],
                )
                .map_err(|error| format!("标记旧 Markdown revision 失败：{error}"))?;
        }
        let warning_json = serde_json::to_string(warnings)
            .map_err(|error| format!("更新 Markdown 警告失败：{error}"))?;
        let terminal_state = if warnings.is_empty() {
            KnowledgeJobState::Succeeded
        } else {
            KnowledgeJobState::Partial
        };
        // A revision has its own lifecycle vocabulary.  `succeeded` is a job
        // state; a successfully built lexical revision is `lexical_ready`
        // until a future embedding stage promotes it to `ready`.
        let revision_status = match terminal_state {
            KnowledgeJobState::Succeeded => "lexical_ready",
            KnowledgeJobState::Partial => "partial",
            _ => unreachable!("markdown indexing only produces terminal success states"),
        };
        transaction
            .execute(
                "UPDATE knowledge_revisions
                 SET element_count = ?, asset_count = ?, chunk_count = ?,
                     status = ?, warning_json = ?, completed_at = ?, updated_at = ?
                 WHERE id = ?",
                params![
                    prepared_elements.len() as i64,
                    assets.len() as i64,
                    prepared_chunks.len() as i64,
                    revision_status,
                    warning_json,
                    now,
                    now,
                    revision_id,
                ],
            )
            .map_err(|error| format!("完成 Markdown revision 失败：{error}"))?;
        transaction
            .execute(
                "UPDATE knowledge_documents
                 SET title = ?, current_source_hash = ?, active_revision_id = ?,
                     state = ?,
                     include_in_default_scope = 1, cloud_consent_state = 'not_required', updated_at = ?
                 WHERE id = ?",
                params![
                    document.title,
                    source_hash,
                    revision_id,
                    terminal_state.document_state(),
                    now,
                    document.id,
                ],
            )
            .map_err(|error| format!("激活 Markdown revision 失败：{error}"))?;
        finish_job_tx(
            &transaction,
            job_id,
            Some(&revision_id),
            terminal_state,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("提交 Markdown 索引事务失败：{error}"))?;
        Ok(())
    }

    fn get_document_status(&self, document_id: &str) -> Result<KnowledgeDocumentStatus, String> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                "SELECT d.id, d.source_class, d.source_kind, d.source_id, d.title,
                        d.state, d.cloud_consent_state, d.active_revision_id,
                        d.current_source_hash, d.updated_at,
                        r.extraction_quality, r.chunk_count, r.asset_count, r.warning_json
                 FROM knowledge_documents d
                 LEFT JOIN knowledge_revisions r ON r.id = d.active_revision_id
                 WHERE d.id = ?",
                params![document_id],
                |row| {
                    let warning_json = row.get::<_, Option<String>>(13)?.unwrap_or_default();
                    Ok(KnowledgeDocumentStatus {
                        id: row.get(0)?,
                        source_class: row.get(1)?,
                        source_kind: row.get(2)?,
                        source_id: row.get(3)?,
                        title: row.get(4)?,
                        state: row.get(5)?,
                        cloud_consent_state: row.get(6)?,
                        active_revision_id: row.get(7)?,
                        source_hash: row.get(8)?,
                        updated_at: to_u64(row.get::<_, i64>(9)?),
                        extraction_quality: row.get(10)?,
                        chunk_count: to_usize(row.get::<_, Option<i64>>(11)?.unwrap_or(0)),
                        asset_count: to_usize(row.get::<_, Option<i64>>(12)?.unwrap_or(0)),
                        warning_count: serde_json::from_str::<Vec<serde_json::Value>>(
                            &warning_json,
                        )
                        .map(|items| items.len())
                        .unwrap_or(0),
                    })
                },
            )
            .optional()
            .map_err(|error| format!("读取知识文档状态失败：{error}"))?
            .ok_or_else(|| "知识文档不存在。".to_string())
    }

    fn query_hits(
        &self,
        connection: &Connection,
        sql: &str,
        values: Vec<Value>,
        like_mode: bool,
        query: &str,
    ) -> Result<Vec<KnowledgeSearchHit>, String> {
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| format!("准备知识库检索失败：{error}"))?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), |row| {
                let heading_path = parse_json_vec(row.get::<_, String>(7)?);
                let search_text = row.get::<_, String>(6)?;
                let element_types = parse_csv_vec(row.get::<_, String>(8)?);
                let rank = row.get::<_, f64>(17).unwrap_or(1.0);
                let lexical_score = if like_mode { 1.0 } else { (-rank).max(0.0) };
                Ok(KnowledgeSearchHit {
                    chunk_id: row.get(0)?,
                    document_id: row.get(1)?,
                    source_class: row.get(2)?,
                    source_id: row.get(3)?,
                    title: row.get(4)?,
                    text: row.get(5)?,
                    snippet: make_snippet(&search_text, query),
                    heading_path,
                    element_types,
                    page_start: row.get::<_, Option<i64>>(9)?.map(to_u32),
                    page_end: row.get::<_, Option<i64>>(10)?.map(to_u32),
                    line_start: row.get::<_, Option<i64>>(11)?.map(to_u32),
                    line_end: row.get::<_, Option<i64>>(12)?.map(to_u32),
                    source_hash: row.get(13)?,
                    revision_id: row.get(14)?,
                    extraction_quality: row.get(15)?,
                    score: lexical_score,
                    lexical_score: Some(lexical_score),
                    vector_score: None,
                    fused_score: None,
                    lexical_rank: None,
                    vector_rank: None,
                })
            })
            .map_err(|error| format!("执行知识库检索失败：{error}"))?;
        let mut hits = Vec::new();
        for row in rows {
            hits.push(row.map_err(|error| format!("读取知识库检索结果失败：{error}"))?);
        }
        Ok(hits)
    }

    fn query_vector_hits(
        &self,
        connection: &Connection,
        request: &KnowledgeSearchRequest,
        embedding_key: &str,
        query_vector: &[f32],
        candidate_limit: usize,
    ) -> Result<Vec<KnowledgeSearchHit>, String> {
        let mut conditions = vec![
            "e.embedding_key = ?".to_string(),
            "e.status = 'ready'".to_string(),
            "d.state <> 'deleted'".to_string(),
            "d.include_in_default_scope = 1".to_string(),
            "d.active_revision_id = r.id".to_string(),
            "r.status IN ('ready', 'lexical_ready', 'partial')".to_string(),
            "r.source_hash = d.current_source_hash".to_string(),
        ];
        let mut values = vec![Value::Text(embedding_key.to_string())];
        match request.scope {
            KnowledgeQueryScope::Library => {}
            KnowledgeQueryScope::CurrentLiterature => {
                conditions.push("d.source_class = 'literature'".to_string());
                conditions.push("d.library_item_id = ?".to_string());
                values.push(Value::Text(
                    request.current_literature_id.clone().unwrap_or_default(),
                ));
            }
            KnowledgeQueryScope::CurrentNote => {
                conditions.push("d.source_class = 'note'".to_string());
                conditions.push("d.note_id = ?".to_string());
                values.push(Value::Text(
                    request.current_note_id.clone().unwrap_or_default(),
                ));
            }
        }
        if !request.selected_document_ids.is_empty() {
            let placeholders = std::iter::repeat("?")
                .take(request.selected_document_ids.len())
                .collect::<Vec<_>>()
                .join(",");
            conditions.push(format!("d.id IN ({placeholders})"));
            values.extend(
                request
                    .selected_document_ids
                    .iter()
                    .cloned()
                    .map(Value::Text),
            );
        }
        for element_type in &request.element_types {
            conditions.push("(',' || s.element_types || ',') LIKE ('%,' || ? || ',%')".to_string());
            values.push(Value::Text(element_type.clone()));
        }
        let sql = format!(
            "SELECT s.chunk_id, d.id, d.source_class, d.source_id, d.title,
                    c.text, s.search_text, c.heading_path_json, s.element_types,
                    c.page_start, c.page_end, c.line_start, c.line_end,
                    r.source_hash, r.id, r.extraction_quality,
                    e.dimensions, e.vector_blob
             FROM knowledge_embeddings e
             JOIN knowledge_chunks c ON c.id = e.chunk_id
             JOIN knowledge_fts_source s ON s.chunk_id = c.id
             JOIN knowledge_revisions r ON r.id = c.revision_id
             JOIN knowledge_documents d ON d.id = r.document_id
             WHERE {}",
            conditions.join(" AND ")
        );
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("准备知识库向量检索失败：{error}"))?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, String>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, i64>(16)?,
                    row.get::<_, Vec<u8>>(17)?,
                ))
            })
            .map_err(|error| format!("执行知识库向量检索失败：{error}"))?;
        let mut hits = Vec::new();
        for row in rows {
            let (
                chunk_id,
                document_id,
                source_class,
                source_id,
                title,
                text,
                search_text,
                heading_path_json,
                element_types_csv,
                page_start,
                page_end,
                line_start,
                line_end,
                source_hash,
                revision_id,
                extraction_quality,
                dimensions,
                vector_blob,
            ) = row.map_err(|error| format!("读取知识库向量候选失败：{error}"))?;
            let dimensions = usize::try_from(dimensions).map_err(|_| {
                "KNOWLEDGE_VECTOR_CORRUPT: stored vector dimensions are invalid".to_string()
            })?;
            if dimensions != query_vector.len() {
                return Err(format!(
                    "KNOWLEDGE_VECTOR_DIMENSION_MISMATCH: query has {} dimensions but index has {dimensions}",
                    query_vector.len()
                ));
            }
            let vector = decode_f32_le(&vector_blob, dimensions)
                .map_err(|error| format!("KNOWLEDGE_VECTOR_CORRUPT: {error}"))?;
            let vector_score = cosine_similarity_normalized(query_vector, &vector)
                .map_err(|error| format!("KNOWLEDGE_VECTOR_CORRUPT: {error}"))?;
            hits.push(KnowledgeSearchHit {
                chunk_id,
                document_id,
                source_class,
                source_id,
                title,
                text,
                snippet: make_snippet(&search_text, &request.query),
                heading_path: parse_json_vec(heading_path_json),
                element_types: parse_csv_vec(element_types_csv),
                page_start: page_start.map(to_u32),
                page_end: page_end.map(to_u32),
                line_start: line_start.map(to_u32),
                line_end: line_end.map(to_u32),
                source_hash,
                revision_id,
                extraction_quality,
                score: vector_score,
                lexical_score: None,
                vector_score: Some(vector_score),
                fused_score: None,
                lexical_rank: None,
                vector_rank: None,
            });
        }
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        hits.truncate(candidate_limit);
        for (index, hit) in hits.iter_mut().enumerate() {
            hit.vector_rank = Some(index + 1);
        }
        Ok(hits)
    }

    fn fail_job(&self, job_id: &str, code: &str, message: &str) -> Result<(), String> {
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("开始记录知识库任务失败状态失败：{error}"))?;
        let current = transaction
            .query_row(
                "SELECT state, execution_version, state_version, document_id
                 FROM knowledge_index_jobs WHERE id = ?",
                params![job_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("读取知识库任务失败状态失败：{error}"))?;
        let Some((state, execution_version, state_version, document_id)) = current else {
            return Ok(());
        };
        let Some(from) = KnowledgeJobState::parse(&state) else {
            return Err("知识库任务状态无效。".to_string());
        };
        if from.is_terminal() || from == KnowledgeJobState::Cancelling {
            transaction
                .commit()
                .map_err(|error| format!("提交知识库任务失败状态检查失败：{error}"))?;
            return Ok(());
        }
        let now = now_ms();
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'failed', stage = 'cleaning', error_code = ?,
                     error_message = ?, finished_at = ?, updated_at = ?,
                     heartbeat_at = ?, lease_token = NULL, lease_owner = NULL,
                     lease_expires_at = NULL, runtime_instance_id = NULL,
                     state_version = state_version + 1
                 WHERE id = ? AND state = ? AND execution_version = ? AND state_version = ?",
                params![
                    code,
                    bounded_error(message),
                    now,
                    now,
                    now,
                    job_id,
                    state,
                    execution_version,
                    state_version,
                ],
            )
            .map_err(|error| format!("更新知识库任务失败状态失败：{error}"))?;
        if changed == 0 {
            transaction
                .commit()
                .map_err(|error| format!("提交知识库任务失败状态冲突失败：{error}"))?;
            return Ok(());
        }
        if let Some(document_id) = document_id {
            transaction
                .execute(
                    "UPDATE knowledge_documents
                     SET state = 'failed', updated_at = ?
                     WHERE id = ? AND active_revision_id IS NULL
                       AND state IN (
                           'pending', 'awaiting_consent', 'remote_pending',
                           'remote_running', 'normalizing', 'stale'
                       )",
                    params![now, document_id],
                )
                .map_err(|error| format!("更新失败知识文档状态失败：{error}"))?;
        }
        insert_job_event_tx(
            &transaction,
            job_id,
            "jobFailed",
            Some(&state),
            Some("failed"),
            execution_version,
            state_version.saturating_add(1),
            r#"{"stage":"cleaning"}"#,
            None,
            None,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("提交知识库任务失败状态失败：{error}"))
    }
}

fn mineru_error_string(error: super::mineru::MineruError) -> String {
    format!("{}: {}", error.code(), error)
}

fn revision_root(library_root: &Path, revision_id: &str) -> Result<PathBuf, String> {
    let revision_id = normalize_identifier("revision ID", revision_id)?;
    Ok(library_root
        .join("knowledge")
        .join("revisions")
        .join(revision_id))
}

fn revision_relative_path(revision_id: &str, file_name: &str) -> String {
    format!("knowledge/revisions/{revision_id}/{file_name}")
}

fn atomic_write_new(path: &Path, bytes: &[u8], label: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label} path has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {label} directory: {error}"))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("{label} file name is invalid"))?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}", Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("failed to create temporary {label}: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("failed to write {label}: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync {label}: {error}"))?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|error| format!("failed to publish {label}: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_revision_manifest(destination: &Path, manifest: &JsonValue) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("failed to serialize knowledge revision manifest: {error}"))?;
    atomic_write_new(
        &destination.join(PDF_REVISION_MANIFEST_FILE),
        &bytes,
        "knowledge revision manifest",
    )
}

fn write_canonical_projection(destination: &Path, canonical_text: &str) -> Result<(), String> {
    if canonical_text.as_bytes().len() > MAX_PDF_CANONICAL_BYTES {
        return Err("PDF canonical projection exceeds the local safety limit".to_string());
    }
    atomic_write_new(
        &destination.join(PDF_CANONICAL_CONTENT_FILE),
        canonical_text.as_bytes(),
        "PDF canonical content",
    )
}

fn create_local_revision_directory(destination: &Path, full_markdown: &str) -> Result<(), String> {
    if full_markdown.as_bytes().len() > MAX_PDF_CANONICAL_BYTES {
        return Err("local PDF fallback content exceeds the local safety limit".to_string());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| "local PDF revision has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create local PDF revision parent: {error}"))?;
    if destination.exists() {
        return Err("local PDF revision destination already exists".to_string());
    }
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "local PDF revision directory name is invalid".to_string())?;
    let staging = parent.join(format!(".{name}.staging-{}", Uuid::new_v4()));
    fs::create_dir(&staging)
        .map_err(|error| format!("failed to create local PDF staging directory: {error}"))?;
    let result = (|| {
        atomic_write_new(
            &staging.join("full.md"),
            full_markdown.as_bytes(),
            "local PDF full markdown",
        )?;
        let directory = fs::File::open(&staging)
            .map_err(|error| format!("failed to open local PDF staging directory: {error}"))?;
        directory
            .sync_all()
            .map_err(|error| format!("failed to sync local PDF staging directory: {error}"))?;
        fs::rename(&staging, destination)
            .map_err(|error| format!("failed to publish local PDF revision: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn prepare_cloud_pdf_revision(
    library_root: &Path,
    revision_id: &str,
    claim: &KnowledgeJobClaim,
    extraction: &super::mineru::MineruExtraction,
    archive: &super::mineru::ResultArchiveManifest,
    config: &super::mineru::MineruConfig,
    chunk_target: usize,
    chunk_max: usize,
) -> Result<PreparedPdfRevision, String> {
    if extraction.preflight.sha256 != claim.source_hash {
        return Err("MinerU extraction source hash does not match the claimed PDF".to_string());
    }
    if !extraction.result_zip_sha256.is_empty() && extraction.result_zip_sha256 != archive.sha256 {
        return Err("MinerU result ZIP hash changed before normalization".to_string());
    }
    let root = revision_root(library_root, revision_id)?;
    let full_path = root.join(&archive.full_markdown_entry);
    let stored_full = fs::read_to_string(&full_path)
        .map_err(|error| format!("failed to read extracted MinerU full.md: {error}"))?;
    if stored_full != archive.full_markdown {
        return Err("extracted MinerU full.md differs from the validated archive".to_string());
    }
    let mut canonical_text = normalize_pdf_text(&archive.full_markdown)?;
    if canonical_text.trim().is_empty() {
        return Err("MinerU full.md contains no searchable text".to_string());
    }

    let geometries =
        super::mineru::parse_page_geometries(&archive.layout).map_err(mineru_error_string)?;
    let page_count = extraction.preflight.page_count;
    let mut warnings = Vec::new();
    if geometries.len() != usize::try_from(page_count).unwrap_or(usize::MAX) {
        warnings.push(format!(
            "MINERU_PAGE_GEOMETRY_COUNT_MISMATCH:{}:{}",
            geometries.len(),
            page_count
        ));
    }
    let archive_entries = archive
        .entries
        .iter()
        .filter(|entry| !entry.directory)
        .map(|entry| entry.name.clone())
        .collect::<HashSet<_>>();
    let (provider_elements, parser_warnings) = super::mineru::parse_content_elements(
        &archive.content_list,
        usize::try_from(page_count).unwrap_or(usize::MAX),
        &archive_entries,
    )
    .map_err(mineru_error_string)?;
    warnings.extend(parser_warnings);
    if provider_elements.is_empty() {
        return Err("MinerU content_list produced no elements".to_string());
    }

    let mut records = Vec::with_capacity(provider_elements.len());
    let mut source_cursor = 0usize;
    let mut provider_heading_stack = Vec::<(usize, String)>::new();
    for provider in provider_elements {
        let mut quality_flags = Vec::new();
        if provider.element_type == "unknown" {
            warnings.push(format!("MINERU_UNKNOWN_ELEMENT_TYPE:{}", provider.ordinal));
            quality_flags.push("unknownElementType".to_string());
        }
        let page_index = provider.page_index.filter(|page| *page < page_count);
        if provider.page_index.is_some() && page_index.is_none() {
            quality_flags.push("pageUnavailable".to_string());
        }
        let (page_width, page_height) = page_index
            .and_then(|page| geometries.get(usize::try_from(page).ok()?))
            .map(|geometry| (Some(geometry.width), Some(geometry.height)))
            .unwrap_or((None, None));
        if provider.bbox.is_none() {
            quality_flags.push("bboxUnavailable".to_string());
        }

        let heading_title = provider_heading_title(&provider);
        if provider.element_type == "title" && !heading_title.is_empty() {
            let level = provider_heading_level(&provider).max(1).min(6);
            while provider_heading_stack
                .last()
                .is_some_and(|(previous, _)| *previous >= level)
            {
                provider_heading_stack.pop();
            }
            provider_heading_stack.push((level, heading_title));
        }
        let fallback_heading_path = provider_heading_stack
            .iter()
            .map(|(_, title)| title.clone())
            .collect::<Vec<_>>();

        let semantic_text = pdf_element_semantic_text(&provider);
        let candidates = pdf_element_source_candidates(&provider);
        let mut span = locate_pdf_source_span(&canonical_text, source_cursor, &candidates);
        if span.is_none() && !semantic_text.trim().is_empty() {
            let appended =
                append_unmapped_pdf_element(&mut canonical_text, &semantic_text, provider.ordinal)?;
            span = Some(appended);
            warnings.push(format!(
                "MINERU_CANONICAL_SPAN_SYNTHESIZED:{}",
                provider.ordinal
            ));
            quality_flags.push("canonicalSpanSynthesized".to_string());
        }
        if let Some(found) = span {
            source_cursor = source_cursor.max(found.byte_end);
        } else if !semantic_text.trim().is_empty() {
            quality_flags.push("canonicalSpanUnavailable".to_string());
            warnings.push(format!(
                "MINERU_CANONICAL_SPAN_UNAVAILABLE:{}",
                provider.ordinal
            ));
        } else {
            quality_flags.push("contentUnavailable".to_string());
        }

        let (byte_start, byte_end, char_start, char_end, raw_text) = span
            .map(|found| {
                (
                    Some(found.byte_start),
                    Some(found.byte_end),
                    Some(found.char_start),
                    Some(found.char_end),
                    canonical_text[found.byte_start..found.byte_end].to_string(),
                )
            })
            .unwrap_or((None, None, None, None, String::new()));
        let text = if !raw_text.trim().is_empty() {
            raw_text.clone()
        } else {
            preferred_pdf_element_text(&provider)
        };
        let line_start = byte_start.and_then(|offset| line_number_at(&canonical_text, offset));
        let line_end =
            byte_end.and_then(|offset| line_number_at(&canonical_text, offset.saturating_sub(1)));
        let heading_path = fallback_heading_path;
        let metadata = pdf_element_metadata(&provider, page_index, page_width, page_height);
        records.push(PdfElementRecord {
            id: Uuid::new_v4().to_string(),
            ordinal: provider.ordinal,
            element_type: provider.element_type.clone(),
            block_kind: pdf_block_kind(&provider.element_type),
            provider_element_id: provider.provider_element_id.clone(),
            page_index,
            page_end: page_index,
            page_width,
            page_height,
            bbox: provider.bbox,
            text,
            raw_text,
            ocr_text: pdf_element_metadata_string(&provider.metadata, &["ocr_text", "ocrText"]),
            formula_latex: provider.formula_latex.clone(),
            table_html: provider.table_html.clone(),
            table_json: provider.table_json.clone(),
            caption: provider.caption.clone(),
            heading_path,
            line_start,
            line_end,
            byte_start,
            byte_end,
            char_start,
            char_end,
            asset_names: provider.asset_names.clone(),
            asset_ids: Vec::new(),
            metadata,
            quality_flags,
        });
    }

    let parsed_projection = parse_markdown(&canonical_text);
    for element in &mut records {
        if let Some(offset) = element.byte_start {
            let markdown_heading_path = markdown_heading_path_at(&parsed_projection, offset);
            if !markdown_heading_path.is_empty() {
                element.heading_path = markdown_heading_path;
            }
        }
    }

    let mut assets = Vec::new();
    let mut asset_by_name = HashMap::<String, String>::new();
    for entry in archive.entries.iter().filter(|entry| !entry.directory) {
        let Some(mime_type) = mime_type_for_path(&entry.name) else {
            continue;
        };
        let path = root.join(&entry.name);
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("failed to inspect MinerU asset {}: {error}", entry.name))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MARKDOWN_ASSET_BYTES {
            warnings.push(format!("MINERU_ASSET_SIZE_INVALID:{}", entry.name));
            continue;
        }
        let bytes = fs::read(&path)
            .map_err(|error| format!("failed to read MinerU asset {}: {error}", entry.name))?;
        if bytes.len() as u64 != entry.uncompressed_size {
            return Err(format!(
                "MinerU asset {} changed after archive extraction",
                entry.name
            ));
        }
        let references = records
            .iter()
            .filter(|element| element.asset_names.iter().any(|name| name == &entry.name))
            .collect::<Vec<_>>();
        let first_reference = references.first().copied();
        let asset_id = Uuid::new_v4().to_string();
        let (page_index, page_width, page_height, bbox, caption, alt_text) = first_reference
            .map(|element| {
                (
                    element.page_index,
                    element.page_width,
                    element.page_height,
                    element.bbox,
                    element.caption.clone(),
                    if !element.caption.trim().is_empty() {
                        element.caption.clone()
                    } else {
                        element.text.chars().take(500).collect()
                    },
                )
            })
            .unwrap_or((None, None, None, None, String::new(), String::new()));
        let metadata_json = serde_json::json!({
            "archiveName": entry.name,
            "compressedSize": entry.compressed_size,
            "uncompressedSize": entry.uncompressed_size,
            "referencedElementIds": references.iter().map(|element| element.id.clone()).collect::<Vec<_>>(),
        });
        let (width_px, height_px) = image_dimensions_from_bytes(&bytes);
        let asset_kind = pdf_asset_kind(&entry.name, &references);
        asset_by_name.insert(entry.name.clone(), asset_id.clone());
        assets.push(PdfAssetRecord {
            id: asset_id,
            archive_name: entry.name.clone(),
            relative_path: entry.name.clone(),
            asset_kind,
            mime_type: mime_type.to_string(),
            byte_size: bytes.len() as u64,
            sha256: super::markdown::sha256_hex(&bytes),
            width_px,
            height_px,
            page_index,
            page_width,
            page_height,
            bbox,
            alt_text,
            caption,
            source_asset_name: entry.name.clone(),
            metadata: metadata_json,
        });
    }
    for element in &mut records {
        for asset_name in element.asset_names.clone() {
            if let Some(asset_id) = asset_by_name.get(&asset_name) {
                if !element.asset_ids.contains(asset_id) {
                    element.asset_ids.push(asset_id.clone());
                }
            } else {
                element.quality_flags.push("assetUnavailable".to_string());
                warnings.push(format!("MINERU_ASSET_NOT_INDEXED:{asset_name}"));
            }
        }
        deduplicate_strings(&mut element.quality_flags);
    }

    let chunks = build_pdf_chunks(&records, &canonical_text, chunk_target, chunk_max);
    if chunks.is_empty() {
        return Err("MinerU extraction produced no searchable chunks".to_string());
    }
    write_canonical_projection(&root, &canonical_text)?;
    deduplicate_strings(&mut warnings);
    warnings.sort();
    let config_json = mineru_config_json(config);
    let config_text = config_json.to_string();
    let mut quality_flags = Vec::new();
    if geometries.len() != usize::try_from(page_count).unwrap_or(usize::MAX) {
        quality_flags.push("pageGeometryMismatch".to_string());
    }
    let extraction_quality = if warnings.is_empty() && quality_flags.is_empty() {
        "cloud_ready"
    } else {
        "cloud_partial"
    };
    Ok(PreparedPdfRevision {
        revision_id: revision_id.to_string(),
        source_hash: claim.source_hash.clone(),
        canonical_text,
        page_count,
        elements: records,
        assets,
        chunks,
        parser_id: PDF_PARSER_ID.to_string(),
        parser_version: PDF_PARSER_VERSION.to_string(),
        provider_id: super::mineru::MINERU_PROVIDER_ID.to_string(),
        parser_config_hash: super::markdown::sha256_hex(config_text.as_bytes()),
        parser_config_json: config_text,
        provider_task_id: extraction.provider_task_id.clone(),
        provider_batch_id: Some(extraction.batch_id.clone()),
        provider_result_hash: Some(extraction.result_zip_sha256.clone()),
        consent_id: claim.consent_id.clone(),
        remote_upload: true,
        normalization_version: PDF_NORMALIZATION_VERSION.to_string(),
        chunk_policy_version: PDF_CHUNK_POLICY_VERSION.to_string(),
        extraction_quality: extraction_quality.to_string(),
        quality_flags,
        warnings,
        content_path: revision_relative_path(revision_id, PDF_CANONICAL_CONTENT_FILE),
        manifest_path: revision_relative_path(revision_id, PDF_REVISION_MANIFEST_FILE),
        provider_archive_path: revision_relative_path(revision_id, ""),
    })
}

fn prepare_local_pdf_revision(
    revision_id: &str,
    claim: &KnowledgeJobClaim,
    fallback: &super::mineru::LocalPdfExtraction,
    cloud_error: Option<&str>,
    chunk_target: usize,
    chunk_max: usize,
) -> Result<PreparedPdfRevision, String> {
    if fallback.preflight.sha256 != claim.source_hash {
        return Err("local PDF fallback source hash does not match the claimed PDF".to_string());
    }
    let mut canonical_text = normalize_pdf_text(&fallback.full_markdown)?;
    let mut warnings = fallback.warnings.clone();
    if let Some(error) = cloud_error {
        let code = safe_error_code(error);
        if !code.is_empty() {
            warnings.push(format!("CLOUD_FALLBACK:{code}"));
        }
    }
    let mut records = Vec::new();
    let mut source_cursor = 0usize;
    for page in &fallback.pages {
        if page.text.trim().is_empty() {
            continue;
        }
        let span = locate_pdf_source_span(
            &canonical_text,
            source_cursor,
            std::slice::from_ref(&page.text),
        )
        .or_else(|| {
            append_unmapped_pdf_element(&mut canonical_text, &page.text, page.page_index as usize)
                .ok()
        });
        let Some(span) = span else {
            warnings.push(format!(
                "LOCAL_PDF_PAGE_SPAN_UNAVAILABLE:{}",
                page.page_index
            ));
            continue;
        };
        source_cursor = source_cursor.max(span.byte_end);
        let raw_text = canonical_text[span.byte_start..span.byte_end].to_string();
        let page_index =
            (page.page_index < fallback.preflight.page_count).then_some(page.page_index);
        let mut quality_flags = fallback.quality_flags.clone();
        if page_index.is_none() {
            quality_flags.push("pageUnavailable".to_string());
        }
        deduplicate_strings(&mut quality_flags);
        records.push(PdfElementRecord {
            id: Uuid::new_v4().to_string(),
            ordinal: records.len(),
            element_type: "paragraph".to_string(),
            block_kind: "paragraph".to_string(),
            provider_element_id: None,
            page_index,
            page_end: page_index,
            page_width: None,
            page_height: None,
            bbox: None,
            text: raw_text.clone(),
            raw_text,
            ocr_text: String::new(),
            formula_latex: String::new(),
            table_html: String::new(),
            table_json: String::new(),
            caption: String::new(),
            heading_path: Vec::new(),
            line_start: Some(line_number_at(&canonical_text, span.byte_start).unwrap_or(1)),
            line_end: Some(
                line_number_at(&canonical_text, span.byte_end.saturating_sub(1)).unwrap_or(1),
            ),
            byte_start: Some(span.byte_start),
            byte_end: Some(span.byte_end),
            char_start: Some(span.char_start),
            char_end: Some(span.char_end),
            asset_names: Vec::new(),
            asset_ids: Vec::new(),
            metadata: serde_json::json!({
                "origin": "lopdf-text-fallback",
                "pageIndex": page.page_index,
            }),
            quality_flags,
        });
    }
    if records.is_empty() {
        return Err("local PDF fallback produced no searchable pages".to_string());
    }
    let parsed_projection = parse_markdown(&canonical_text);
    for element in &mut records {
        if let Some(offset) = element.byte_start {
            element.heading_path = markdown_heading_path_at(&parsed_projection, offset);
        }
    }
    let chunks = build_pdf_chunks(&records, &canonical_text, chunk_target, chunk_max);
    if chunks.is_empty() {
        return Err("local PDF fallback produced no searchable chunks".to_string());
    }
    let mut quality_flags = fallback.quality_flags.clone();
    quality_flags.push("localTextOnly".to_string());
    deduplicate_strings(&mut quality_flags);
    deduplicate_strings(&mut warnings);
    warnings.sort();
    let config_json = serde_json::json!({
        "provider": "local",
        "parser": LOCAL_PDF_PARSER_ID,
        "version": LOCAL_PDF_PARSER_VERSION,
        "cloudFallbackCode": cloud_error.map(safe_error_code).filter(|value| !value.is_empty()),
    });
    let config_text = config_json.to_string();
    Ok(PreparedPdfRevision {
        revision_id: revision_id.to_string(),
        source_hash: claim.source_hash.clone(),
        canonical_text,
        page_count: fallback.preflight.page_count,
        elements: records,
        assets: Vec::new(),
        chunks,
        parser_id: LOCAL_PDF_PARSER_ID.to_string(),
        parser_version: LOCAL_PDF_PARSER_VERSION.to_string(),
        provider_id: "local".to_string(),
        parser_config_hash: super::markdown::sha256_hex(config_text.as_bytes()),
        parser_config_json: config_text,
        provider_task_id: None,
        provider_batch_id: None,
        provider_result_hash: None,
        consent_id: None,
        remote_upload: false,
        normalization_version: PDF_NORMALIZATION_VERSION.to_string(),
        chunk_policy_version: PDF_CHUNK_POLICY_VERSION.to_string(),
        extraction_quality: if cloud_error.is_some() {
            "cloud_failed_local_fallback".to_string()
        } else {
            "local_text_only".to_string()
        },
        quality_flags,
        warnings,
        content_path: revision_relative_path(revision_id, PDF_CANONICAL_CONTENT_FILE),
        manifest_path: revision_relative_path(revision_id, PDF_REVISION_MANIFEST_FILE),
        provider_archive_path: String::new(),
    })
}

fn normalize_pdf_text(value: &str) -> Result<String, String> {
    if value.chars().any(|character| character == '\0') {
        return Err("PDF extracted text contains a NUL character".to_string());
    }
    let normalized = super::markdown::normalize_newlines_and_bom(value);
    if normalized.as_bytes().len() > MAX_PDF_CANONICAL_BYTES {
        return Err("PDF canonical projection exceeds the local safety limit".to_string());
    }
    Ok(normalized)
}

#[derive(Debug, Clone, Copy)]
struct PdfSourceSpan {
    byte_start: usize,
    byte_end: usize,
    char_start: usize,
    char_end: usize,
}

fn locate_pdf_source_span(
    content: &str,
    from: usize,
    candidates: &[String],
) -> Option<PdfSourceSpan> {
    let mut ordered = candidates
        .iter()
        .map(|candidate| candidate.trim())
        .filter(|candidate| !candidate.is_empty())
        .collect::<Vec<_>>();
    ordered.sort_by_key(|candidate| std::cmp::Reverse(candidate.chars().count()));
    ordered.dedup();
    for start in [from.min(content.len()), 0] {
        for candidate in &ordered {
            if let Some(offset) = content[start..].find(candidate) {
                let byte_start = start + offset;
                let byte_end = byte_start + candidate.len();
                return Some(pdf_span(content, byte_start, byte_end));
            }
        }
        for candidate in &ordered {
            if let Some((byte_start, byte_end)) =
                find_flexible_whitespace(content, candidate, start)
            {
                return Some(pdf_span(content, byte_start, byte_end));
            }
        }
    }
    None
}

fn find_flexible_whitespace(content: &str, needle: &str, from: usize) -> Option<(usize, usize)> {
    let first = needle.split_whitespace().next()?;
    let mut search = from.min(content.len());
    while let Some(relative) = content[search..].find(first) {
        let start = search + relative;
        let mut haystack_cursor = start;
        let mut needle_cursor = 0usize;
        let needle_chars = needle.chars().collect::<Vec<_>>();
        let mut matched = true;
        while needle_cursor < needle_chars.len() {
            let needle_character = needle_chars[needle_cursor];
            if needle_character.is_whitespace() {
                if content[haystack_cursor..]
                    .chars()
                    .next()
                    .is_none_or(|character| !character.is_whitespace())
                {
                    matched = false;
                    break;
                }
                while let Some(character) = content[haystack_cursor..].chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    haystack_cursor += character.len_utf8();
                }
                while needle_cursor < needle_chars.len()
                    && needle_chars[needle_cursor].is_whitespace()
                {
                    needle_cursor += 1;
                }
            } else {
                let Some(character) = content[haystack_cursor..].chars().next() else {
                    matched = false;
                    break;
                };
                if character != needle_character {
                    matched = false;
                    break;
                }
                haystack_cursor += character.len_utf8();
                needle_cursor += 1;
            }
        }
        if matched {
            return Some((start, haystack_cursor));
        }
        let Some(character) = content[start..].chars().next() else {
            break;
        };
        search = start + character.len_utf8();
    }
    None
}

fn pdf_span(content: &str, byte_start: usize, byte_end: usize) -> PdfSourceSpan {
    PdfSourceSpan {
        byte_start,
        byte_end,
        char_start: content[..byte_start].chars().count(),
        char_end: content[..byte_end].chars().count(),
    }
}

fn append_unmapped_pdf_element(
    content: &mut String,
    text: &str,
    ordinal: usize,
) -> Result<PdfSourceSpan, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("cannot append an empty PDF element projection".to_string());
    }
    let separator = if content.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let additional = separator
        .len()
        .saturating_add(format!("<!-- mnemora:provider-element-{ordinal} -->\n").len())
        .saturating_add(text.len())
        .saturating_add(1);
    if content.len().saturating_add(additional) > MAX_PDF_CANONICAL_BYTES {
        return Err("PDF canonical projection exceeds the local safety limit".to_string());
    }
    content.push_str(separator);
    content.push_str(&format!("<!-- mnemora:provider-element-{ordinal} -->\n"));
    let byte_start = content.len();
    content.push_str(text);
    let byte_end = content.len();
    content.push('\n');
    Ok(pdf_span(content, byte_start, byte_end))
}

fn pdf_element_source_candidates(element: &super::mineru::MineruElement) -> Vec<String> {
    let mut candidates = Vec::new();
    let preferred = preferred_pdf_element_text(element);
    if !preferred.is_empty() {
        candidates.push(preferred);
    }
    for value in [
        element.text.clone(),
        element.caption.clone(),
        element.formula_latex.clone(),
        element.table_html.clone(),
        element.table_json.clone(),
    ] {
        if !value.trim().is_empty() && !candidates.iter().any(|candidate| candidate == &value) {
            candidates.push(value);
        }
    }
    candidates
}

fn preferred_pdf_element_text(element: &super::mineru::MineruElement) -> String {
    match element.element_type.as_str() {
        "table" if !element.table_html.trim().is_empty() => element.table_html.clone(),
        "formula" if !element.formula_latex.trim().is_empty() => element.formula_latex.clone(),
        "figure" | "chart" | "caption" if !element.caption.trim().is_empty() => {
            element.caption.clone()
        }
        _ if !element.text.trim().is_empty() => element.text.clone(),
        _ if !element.caption.trim().is_empty() => element.caption.clone(),
        _ if !element.formula_latex.trim().is_empty() => element.formula_latex.clone(),
        _ if !element.table_html.trim().is_empty() => element.table_html.clone(),
        _ => element.table_json.clone(),
    }
}

fn pdf_element_semantic_text(element: &super::mineru::MineruElement) -> String {
    let mut parts = Vec::new();
    for value in [
        element.text.clone(),
        element.caption.clone(),
        element.formula_latex.clone(),
        strip_html_tags(&element.table_html),
        if element.table_json.trim().is_empty() {
            String::new()
        } else {
            element.table_json.clone()
        },
    ] {
        let value = value.trim();
        if !value.is_empty() && !parts.iter().any(|part: &String| part == value) {
            parts.push(value.to_string());
        }
    }
    parts.join("\n")
}

fn strip_html_tags(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn provider_heading_level(element: &super::mineru::MineruElement) -> usize {
    element
        .metadata
        .get("raw")
        .and_then(|raw| raw.get("text_level").or_else(|| raw.get("textLevel")))
        .and_then(JsonValue::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn provider_heading_title(element: &super::mineru::MineruElement) -> String {
    let value = if !element.text.trim().is_empty() {
        element.text.trim()
    } else {
        element.caption.trim()
    };
    value
        .trim_start_matches('#')
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_string()
}

fn pdf_element_metadata(
    element: &super::mineru::MineruElement,
    page_index: Option<u32>,
    page_width: Option<f64>,
    page_height: Option<f64>,
) -> JsonValue {
    let mut metadata = element.metadata.clone();
    let source_ref = serde_json::json!({
        "pageIndex": page_index,
        "pageWidth": page_width,
        "pageHeight": page_height,
        "bbox": element.bbox,
    });
    if let Some(object) = metadata.as_object_mut() {
        object.insert("sourceRef".to_string(), source_ref);
    } else {
        metadata = serde_json::json!({ "provider": metadata, "sourceRef": source_ref });
    }
    metadata
}

fn pdf_element_metadata_string(metadata: &JsonValue, keys: &[&str]) -> String {
    metadata
        .get("raw")
        .and_then(|raw| {
            keys.iter().find_map(|key| {
                raw.get(*key)
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
        })
        .unwrap_or_default()
}

fn pdf_block_kind(element_type: &str) -> String {
    match element_type {
        "title" => "heading",
        "paragraph" | "text" => "paragraph",
        "figure" | "chart" | "page_image" => "figure",
        "formula" => "formula",
        "table" => "table",
        "code" | "algorithm" => "code",
        "list" => "list",
        "quote" => "quote",
        "caption" => "caption",
        "reference" => "reference",
        "header" => "header",
        "footer" => "footer",
        "footnote" => "footnote",
        _ => "unknown",
    }
    .to_string()
}

fn pdf_asset_kind(name: &str, references: &[&PdfElementRecord]) -> String {
    let lower = name.to_ascii_lowercase();
    if references
        .iter()
        .any(|element| element.element_type == "table")
        || lower.contains("table")
    {
        "table_crop".to_string()
    } else if references
        .iter()
        .any(|element| element.element_type == "formula")
        || lower.contains("formula")
        || lower.contains("equation")
    {
        "formula_crop".to_string()
    } else if references
        .iter()
        .any(|element| element.element_type == "chart")
        || lower.contains("chart")
    {
        "chart".to_string()
    } else if lower.contains("page") && lower.contains("image") {
        "page_render".to_string()
    } else if references
        .iter()
        .any(|element| element.element_type == "figure")
    {
        "figure".to_string()
    } else {
        "embedded_image".to_string()
    }
}

fn image_dimensions_from_bytes(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let Ok(reader) = ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {
        return (None, None);
    };
    let Ok((width, height)) = reader.into_dimensions() else {
        return (None, None);
    };
    if width == 0
        || height == 0
        || width > 100_000
        || height > 100_000
        || u64::from(width).saturating_mul(u64::from(height)) > 100_000_000
    {
        return (None, None);
    }
    (Some(width), Some(height))
}

fn markdown_heading_path_at(document: &MarkdownDocument, offset: usize) -> Vec<String> {
    document
        .blocks
        .iter()
        .take_while(|block| block.byte_start <= offset)
        .last()
        .map(|block| block.heading_path.clone())
        .unwrap_or_default()
}

fn build_pdf_chunks(
    elements: &[PdfElementRecord],
    canonical_text: &str,
    target: usize,
    max: usize,
) -> Vec<PdfChunkRecord> {
    let target = target.max(1);
    let max = max.max(target);
    let overlap = target
        .saturating_div(8)
        .min(200)
        .min(target.saturating_sub(1));
    let mut chunks = Vec::new();
    for element in elements {
        let source_text = if !element.raw_text.trim().is_empty() {
            element.raw_text.as_str()
        } else if !element.text.trim().is_empty() {
            element.text.as_str()
        } else if !element.caption.trim().is_empty() {
            element.caption.as_str()
        } else if !element.formula_latex.trim().is_empty() {
            element.formula_latex.as_str()
        } else {
            element.table_html.as_str()
        };
        if source_text.trim().is_empty() {
            continue;
        }
        let segments = split_pdf_text(
            source_text,
            element.byte_start,
            element.char_start,
            target,
            max,
            overlap,
        );
        for segment in segments {
            if segment.text.trim().is_empty() {
                continue;
            }
            let mut search_parts = vec![search_projection(&segment.text, &[])];
            for value in [
                element.text.as_str(),
                element.caption.as_str(),
                element.ocr_text.as_str(),
                element.formula_latex.as_str(),
                strip_html_tags(&element.table_html).as_str(),
                element.table_json.as_str(),
                element.heading_path.join(" ").as_str(),
                element.element_type.as_str(),
            ] {
                if !value.trim().is_empty() {
                    search_parts.push(value.trim().to_string());
                }
            }
            let search_text = search_parts
                .into_iter()
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let line_start = segment
                .byte_start
                .checked_sub(0)
                .and_then(|offset| line_number_at(canonical_text, offset));
            let line_end = segment
                .byte_end
                .checked_sub(1)
                .and_then(|offset| line_number_at(canonical_text, offset));
            let page_bboxes = element
                .bbox
                .map(|bbox| {
                    vec![serde_json::json!({
                        "pageIndex": element.page_index,
                        "pageWidth": element.page_width,
                        "pageHeight": element.page_height,
                        "bbox": bbox,
                    })]
                })
                .unwrap_or_default();
            chunks.push(PdfChunkRecord {
                id: Uuid::new_v4().to_string(),
                ordinal: chunks.len(),
                block_kind: element.block_kind.clone(),
                text: segment.text,
                search_text,
                heading_path: element.heading_path.clone(),
                element_ids: vec![element.id.clone()],
                asset_ids: element.asset_ids.clone(),
                page_start: element.page_index,
                page_end: element.page_end,
                line_start,
                line_end,
                byte_start: segment.byte_start,
                byte_end: segment.byte_end,
                char_start: segment.char_start,
                char_end: segment.char_end,
                page_bboxes,
                quality_flags: element.quality_flags.clone(),
                is_overlap: segment.is_overlap,
            });
        }
    }
    chunks
}

#[derive(Debug, Clone)]
struct PdfTextSegment {
    text: String,
    byte_start: usize,
    byte_end: usize,
    char_start: Option<usize>,
    char_end: Option<usize>,
    is_overlap: bool,
}

fn split_pdf_text(
    text: &str,
    base_byte: Option<usize>,
    base_char: Option<usize>,
    target: usize,
    max: usize,
    overlap: usize,
) -> Vec<PdfTextSegment> {
    let positions = text.char_indices().collect::<Vec<_>>();
    let total = positions.len();
    if total == 0 {
        return Vec::new();
    }
    let target = target.max(1);
    let max = max.max(target);
    if total <= max {
        return vec![PdfTextSegment {
            text: text.to_string(),
            byte_start: base_byte.unwrap_or(0),
            byte_end: base_byte
                .map(|base| base + text.len())
                .unwrap_or(text.len()),
            char_start: base_char,
            char_end: base_char.map(|base| base + total),
            is_overlap: false,
        }];
    }
    let mut output = Vec::new();
    let mut start = 0usize;
    while start < total {
        let desired_end = (start + target).min(total);
        let hard_end = (start + max).min(total);
        let mut end = desired_end;
        if desired_end < total {
            for candidate in (start + 1..=hard_end).rev() {
                let byte = positions[candidate.saturating_sub(1)].0;
                if text[byte..].chars().next().is_some_and(|character| {
                    character.is_whitespace() || ".,;:!?".contains(character)
                }) {
                    end = candidate;
                    break;
                }
            }
        } else {
            end = total;
        }
        end = end.max(start + 1).min(hard_end);
        let local_start = positions[start].0;
        let local_end = if end < total {
            positions[end].0
        } else {
            text.len()
        };
        output.push(PdfTextSegment {
            text: text[local_start..local_end].to_string(),
            byte_start: base_byte
                .map(|base| base + local_start)
                .unwrap_or(local_start),
            byte_end: base_byte.map(|base| base + local_end).unwrap_or(local_end),
            char_start: base_char.map(|base| base + start),
            char_end: base_char.map(|base| base + end),
            is_overlap: !output.is_empty(),
        });
        if end >= total {
            break;
        }
        let next_start = end.saturating_sub(overlap.min(end));
        start = if next_start <= start {
            start + 1
        } else {
            next_start
        };
    }
    output
}

fn mineru_config_json(config: &super::mineru::MineruConfig) -> JsonValue {
    serde_json::json!({
        "endpoint": config.endpoint,
        "model": config.model,
        "language": config.language,
        "ocrEnabled": config.ocr_enabled,
        "formulaEnabled": config.formula_enabled,
        "tableEnabled": config.table_enabled,
        "figureEnabled": config.figure_enabled,
        "requestTimeoutMs": config.request_timeout.as_millis(),
        "jobDeadlineMs": config.job_deadline.as_millis(),
        "pollIntervalMs": config.poll_interval.as_millis(),
        "maxAttempts": config.max_attempts,
    })
}

fn cloud_revision_manifest_json(
    claim: &KnowledgeJobClaim,
    extraction: &super::mineru::MineruExtraction,
    config: &super::mineru::MineruConfig,
    prepared: &PreparedPdfRevision,
) -> JsonValue {
    serde_json::json!({
        "schemaVersion": 1,
        "source": {
            "documentId": claim.document_id,
            "sourceId": claim.source_id,
            "sha256": claim.source_hash,
            "originalName": claim.original_name,
        },
        "provider": {
            "id": super::mineru::MINERU_PROVIDER_ID,
            "batchId": extraction.batch_id,
            "taskId": extraction.provider_task_id,
            "resultZipSha256": extraction.result_zip_sha256,
            "preflight": extraction.preflight,
        },
        "parser": {
            "id": prepared.parser_id,
            "version": prepared.parser_version,
            "config": mineru_config_json(config),
            "configHash": prepared.parser_config_hash,
        },
        "projection": {
            "fullMarkdown": prepared.content_path,
            "canonicalText": prepared.content_path,
            "manifest": prepared.manifest_path,
            "archiveRoot": prepared.provider_archive_path,
            "pageCount": prepared.page_count,
            "elementCount": prepared.elements.len(),
            "assetCount": prepared.assets.len(),
            "chunkCount": prepared.chunks.len(),
        },
        "quality": {
            "extraction": prepared.extraction_quality,
            "flags": prepared.quality_flags,
            "warnings": prepared.warnings,
        },
        "consentId": claim.consent_id,
    })
}

fn local_revision_manifest_json(
    claim: &KnowledgeJobClaim,
    fallback: &super::mineru::LocalPdfExtraction,
    cloud_error: Option<&str>,
    prepared: &PreparedPdfRevision,
) -> JsonValue {
    serde_json::json!({
        "schemaVersion": 1,
        "source": {
            "documentId": claim.document_id,
            "sourceId": claim.source_id,
            "sha256": claim.source_hash,
            "originalName": claim.original_name,
        },
        "provider": {
            "id": "local",
            "cloudFallbackCode": cloud_error.map(safe_error_code).filter(|value| !value.is_empty()),
            "preflight": fallback.preflight,
        },
        "parser": {
            "id": prepared.parser_id,
            "version": prepared.parser_version,
            "config": serde_json::from_str::<JsonValue>(&prepared.parser_config_json).unwrap_or_else(|_| serde_json::json!({})),
            "configHash": prepared.parser_config_hash,
        },
        "projection": {
            "fullMarkdown": format!("knowledge/revisions/{}/full.md", prepared.revision_id),
            "canonicalText": prepared.content_path,
            "manifest": prepared.manifest_path,
            "pageCount": prepared.page_count,
            "elementCount": prepared.elements.len(),
            "assetCount": prepared.assets.len(),
            "chunkCount": prepared.chunks.len(),
        },
        "quality": {
            "extraction": prepared.extraction_quality,
            "flags": prepared.quality_flags,
            "warnings": prepared.warnings,
        },
    })
}

fn safe_error_code(value: &str) -> String {
    value
        .split(':')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(96)
        .collect()
}

fn deduplicate_strings(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn set_claim_stage_tx(
    transaction: &Transaction<'_>,
    claim: &KnowledgeJobClaim,
    stage: &str,
    now: i64,
) -> Result<(), String> {
    let changed = transaction
        .execute(
            "UPDATE knowledge_index_jobs
             SET stage = ?, updated_at = ?, heartbeat_at = ?,
                 lease_expires_at = ?
             WHERE id = ? AND state = 'running'
               AND execution_version = ? AND state_version = ?
               AND runtime_instance_id = ? AND lease_token = ?
               AND cancel_requested_at IS NULL",
            params![
                stage,
                now,
                now,
                now.saturating_add(KNOWLEDGE_JOB_LEASE_MS),
                claim.job_id,
                claim.execution_version,
                claim.state_version,
                claim.runtime_instance_id,
                claim.lease_token,
            ],
        )
        .map_err(|error| format!("failed to update knowledge commit stage: {error}"))?;
    if changed != 1 {
        return Err("knowledge commit stage CAS was rejected".to_string());
    }
    Ok(())
}

fn finish_claimed_job_tx(
    transaction: &Transaction<'_>,
    claim: &KnowledgeJobClaim,
    revision_id: &str,
    partial: bool,
    now: i64,
) -> Result<(), String> {
    let current: Option<(
        String,
        i64,
        i64,
        Option<String>,
        Option<String>,
        Option<i64>,
    )> = transaction
        .query_row(
            "SELECT state, execution_version, state_version,
                    runtime_instance_id, lease_token, cancel_requested_at
             FROM knowledge_index_jobs WHERE id = ?",
            params![claim.job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("failed to read PDF completion lease: {error}"))?;
    let Some((
        state,
        execution_version,
        state_version,
        runtime_id,
        lease_token,
        cancel_requested_at,
    )) = current
    else {
        return Err("PDF completion lease no longer exists".to_string());
    };
    if state != "running"
        || cancel_requested_at.is_some()
        || execution_version != claim.execution_version
        || state_version != claim.state_version
        || runtime_id.as_deref() != Some(claim.runtime_instance_id.as_str())
        || lease_token.as_deref() != Some(claim.lease_token.as_str())
    {
        return Err("PDF completion lease is stale or cancelled".to_string());
    }
    let terminal = if partial { "partial" } else { "succeeded" };
    let changed = transaction
        .execute(
            "UPDATE knowledge_index_jobs
             SET revision_id = ?, state = ?, stage = 'done',
                 completed_units = total_units, finished_at = ?, updated_at = ?,
                 heartbeat_at = ?, lease_token = NULL, lease_owner = NULL,
                 lease_expires_at = NULL, runtime_instance_id = NULL,
                 state_version = state_version + 1
             WHERE id = ? AND state = 'running'
               AND execution_version = ? AND state_version = ?
               AND runtime_instance_id = ? AND lease_token = ?
               AND cancel_requested_at IS NULL",
            params![
                revision_id,
                terminal,
                now,
                now,
                now,
                claim.job_id,
                claim.execution_version,
                claim.state_version,
                claim.runtime_instance_id,
                claim.lease_token,
            ],
        )
        .map_err(|error| format!("failed to finish PDF knowledge job: {error}"))?;
    if changed != 1 {
        return Err("PDF knowledge job completion CAS was rejected".to_string());
    }
    insert_job_event_tx(
        transaction,
        &claim.job_id,
        if partial {
            "jobPartial"
        } else {
            "jobSucceeded"
        },
        Some("running"),
        Some(terminal),
        claim.execution_version,
        claim.state_version.saturating_add(1),
        if partial {
            r#"{"stage":"done","quality":"partial"}"#
        } else {
            r#"{"stage":"done","quality":"cloud_ready"}"#
        },
        None,
        Some(&claim.runtime_instance_id),
        now,
    )?;
    Ok(())
}

fn ensure_document_tx(
    transaction: &Transaction<'_>,
    source_class: &str,
    source_kind: &str,
    source_id: &str,
    title: &str,
    source_hash: &str,
    library_item_id: Option<&str>,
    note_id: Option<&str>,
    initial_state: &str,
    cloud_consent_state: &str,
    now: i64,
) -> Result<(SourceDocument, bool), String> {
    let existing = transaction
        .query_row(
                "SELECT id, source_id, title, current_source_hash, active_revision_id, state,
                        cloud_consent_state
                 FROM knowledge_documents WHERE source_class = ? AND source_kind = ? AND source_id = ?",
            params![source_class, source_kind, source_id],
            |row| {
                Ok(SourceDocument {
                    id: row.get(0)?,
                    source_id: row.get(1)?,
                    title: row.get(2)?,
                    source_hash: row.get(3)?,
                    active_revision_id: row.get(4)?,
                    state: row.get(5)?,
                    cloud_consent_state: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("查询知识文档失败：{error}"))?;
    if let Some(mut document) = existing {
        let hash_changed = document.source_hash != source_hash;
        let changed = hash_changed || document.title != title || document.state == "deleted";
        if changed {
            let previous_revision_id = document.active_revision_id.clone();
            let next_active_revision_id = if hash_changed {
                None
            } else {
                document.active_revision_id.clone()
            };
            transaction
                .execute(
                    "UPDATE knowledge_documents
                     SET title = ?, current_source_hash = ?, state = ?,
                         active_revision_id = ?,
                         cloud_consent_state = ?, include_in_default_scope = 1, updated_at = ?
                     WHERE id = ?",
                    params![
                        title,
                        source_hash,
                        initial_state,
                        next_active_revision_id,
                        cloud_consent_state,
                        now,
                        document.id
                    ],
                )
                .map_err(|error| format!("更新知识文档失败：{error}"))?;
            if hash_changed {
                if let Some(previous_revision_id) = previous_revision_id {
                    transaction
                        .execute(
                            "UPDATE knowledge_revisions SET status = 'stale', updated_at = ?
                             WHERE id = ? AND status NOT IN ('failed', 'cancelled', 'stale')",
                            params![now, previous_revision_id],
                        )
                        .map_err(|error| format!("标记旧知识 revision 失败：{error}"))?;
                }
                invalidate_jobs_for_source_hash_tx(transaction, &document.id, source_hash, now)?;
            }
            document.title = title.to_string();
            document.source_hash = source_hash.to_string();
            document.state = initial_state.to_string();
            document.active_revision_id = next_active_revision_id;
            document.cloud_consent_state = cloud_consent_state.to_string();
        } else if document.cloud_consent_state != cloud_consent_state {
            transaction
                .execute(
                    "UPDATE knowledge_documents
                     SET cloud_consent_state = ?, updated_at = ?
                     WHERE id = ?",
                    params![cloud_consent_state, now, document.id],
                )
                .map_err(|error| format!("更新知识文档云端同意状态失败：{error}"))?;
            document.cloud_consent_state = cloud_consent_state.to_string();
        }
        return Ok((document, changed));
    }
    let id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO knowledge_documents (
                id, source_class, source_kind, source_id, library_item_id, note_id,
                title, state, current_source_hash, cloud_consent_state,
                include_in_default_scope, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
            params![
                id,
                source_class,
                source_kind,
                source_id,
                library_item_id,
                note_id,
                title,
                initial_state,
                source_hash,
                cloud_consent_state,
                now,
                now,
            ],
        )
        .map_err(|error| format!("创建知识文档失败：{error}"))?;
    Ok((
        SourceDocument {
            id,
            source_id: source_id.to_string(),
            title: title.to_string(),
            source_hash: source_hash.to_string(),
            active_revision_id: None,
            state: initial_state.to_string(),
            cloud_consent_state: cloud_consent_state.to_string(),
        },
        true,
    ))
}

fn has_active_literature_consent(
    connection: &Connection,
    item_id: &str,
    source_hash: &str,
) -> Result<bool, String> {
    Ok(active_literature_consent_id(connection, item_id, source_hash)?.is_some())
}

/// Return the most specific currently valid consent for a PDF.  A document
/// consent wins over a global consent so the job audit trail records the
/// narrowest permission that authorized the upload.
fn active_literature_consent_id(
    connection: &Connection,
    item_id: &str,
    source_hash: &str,
) -> Result<Option<String>, String> {
    active_literature_consent_id_on(connection, item_id, source_hash)
}

fn active_literature_consent_id_on(
    connection: &Connection,
    item_id: &str,
    source_hash: &str,
) -> Result<Option<String>, String> {
    let document_id: Option<String> = connection
        .query_row(
            "SELECT id FROM knowledge_documents
             WHERE source_class = 'literature' AND source_id = ?",
            params![item_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("failed to resolve MinerU consent document: {error}"))?;

    if let Some(document_id) = document_id {
        let document_consent: Option<String> = connection
            .query_row(
                "SELECT id FROM knowledge_cloud_consents
                  WHERE provider_id = 'mineru-cloud' AND scope = 'document'
                    AND policy_version = ? AND scope_key = 'local-library'
                    AND document_id = ? AND source_hash = ? AND revoked_at IS NULL
                  ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
                params![MINERU_CONSENT_POLICY_VERSION, document_id, source_hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("failed to inspect document MinerU consent: {error}"))?;
        if document_consent.is_some() {
            return Ok(document_consent);
        }
    }

    connection
        .query_row(
            "SELECT id FROM knowledge_cloud_consents
              WHERE provider_id = 'mineru-cloud' AND scope = 'global'
                AND policy_version = ? AND scope_key = 'local-library'
                AND revoked_at IS NULL
              ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
            params![MINERU_CONSENT_POLICY_VERSION],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("failed to inspect global MinerU consent: {error}"))
}

fn active_document_literature_consent_id_on(
    connection: &Connection,
    document_id: &str,
    source_hash: &str,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT id FROM knowledge_cloud_consents
             WHERE provider_id = 'mineru-cloud' AND policy_version = ?
               AND scope_key = 'local-library' AND scope = 'document'
               AND document_id = ? AND source_hash = ? AND revoked_at IS NULL
             ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
            params![MINERU_CONSENT_POLICY_VERSION, document_id, source_hash],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("failed to inspect active document MinerU consent: {error}"))
}

fn active_global_literature_consent_id_on(
    connection: &Connection,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT id FROM knowledge_cloud_consents
             WHERE provider_id = 'mineru-cloud' AND policy_version = ?
               AND scope_key = 'local-library' AND scope = 'global'
               AND revoked_at IS NULL
             ORDER BY granted_at DESC, created_at DESC, id DESC LIMIT 1",
            params![MINERU_CONSENT_POLICY_VERSION],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("failed to inspect active global MinerU consent: {error}"))
}

fn load_active_pdf_sources(
    connection: &Connection,
) -> Result<Vec<(String, String, String)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT i.id, i.title, f.file_hash
             FROM library_items i
             JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
             WHERE i.item_type = 'pdf' AND i.deleted_at IS NULL
             ORDER BY i.updated_at DESC, i.id ASC",
        )
        .map_err(|error| format!("failed to prepare active PDF source list: {error}"))?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|error| format!("failed to query active PDF sources: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read active PDF sources: {error}"))
}

fn load_literature_documents(connection: &Connection) -> Result<Vec<(String, String)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, current_source_hash
             FROM knowledge_documents
             WHERE source_class = 'literature' AND source_kind = 'pdf'
               AND state <> 'deleted'",
        )
        .map_err(|error| format!("failed to prepare literature document list: {error}"))?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|error| format!("failed to query literature documents: {error}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read literature documents: {error}"))
}

fn invalidate_jobs_for_source_hash_tx(
    transaction: &Transaction<'_>,
    document_id: &str,
    current_source_hash: &str,
    now: i64,
) -> Result<(), String> {
    let mut statement = transaction
        .prepare(
            "SELECT id, state, execution_version, state_version
             FROM knowledge_index_jobs
             WHERE document_id = ? AND requested_source_hash <> ?
               AND state IN ('queued', 'running', 'cancelling', 'paused')",
        )
        .map_err(|error| format!("failed to prepare stale source job list: {error}"))?;
    let rows = statement
        .query_map(params![document_id, current_source_hash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| format!("failed to query stale source jobs: {error}"))?;
    let jobs = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read stale source jobs: {error}"))?;
    drop(statement);
    for (job_id, state, execution_version, state_version) in jobs {
        let next_execution_version = execution_version.saturating_add(1);
        let next_state_version = state_version.saturating_add(1);
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'stale', stage = 'cleaning',
                     execution_version = ?, state_version = ?,
                     error_code = 'SOURCE_CHANGED',
                     error_message = '源文件内容已变化，旧任务结果已作废。',
                     finished_at = ?, updated_at = ?, heartbeat_at = ?,
                     lease_token = NULL, lease_owner = NULL,
                     lease_expires_at = NULL, runtime_instance_id = NULL
                 WHERE id = ? AND state IN ('queued', 'running', 'cancelling', 'paused')
                   AND execution_version = ? AND state_version = ?",
                params![
                    next_execution_version,
                    next_state_version,
                    now,
                    now,
                    now,
                    job_id,
                    execution_version,
                    state_version,
                ],
            )
            .map_err(|error| format!("failed to invalidate stale source job: {error}"))?;
        if changed == 1 {
            insert_job_event_tx(
                transaction,
                &job_id,
                "sourceChanged",
                Some(&state),
                Some("stale"),
                next_execution_version,
                next_state_version,
                r#"{"reason":"sourceHashChanged"}"#,
                None,
                None,
                now,
            )?;
        }
    }
    Ok(())
}

fn bind_extract_job_tx(
    transaction: &Transaction<'_>,
    job_id: &str,
    consent_id: Option<&str>,
    now: i64,
) -> Result<(), String> {
    let changed = if let Some(consent_id) = consent_id {
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'queued', stage = 'queued', consent_id = ?,
                     cancel_requested_at = NULL, error_code = NULL,
                     error_message = NULL, finished_at = NULL, updated_at = ?
                 WHERE id = ? AND state IN ('queued', 'paused')",
                params![consent_id, now, job_id],
            )
            .map_err(|error| format!("failed to bind queued extraction consent: {error}"))?
    } else {
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'queued', stage = 'awaiting_consent', consent_id = NULL,
                     updated_at = ?
                 WHERE id = ? AND state IN ('queued', 'paused')",
                params![now, job_id],
            )
            .map_err(|error| format!("failed to clear queued extraction consent: {error}"))?
    };
    if changed > 1 {
        return Err("multiple knowledge jobs matched one extraction ID".to_string());
    }
    Ok(())
}

fn bind_extract_jobs_for_document_tx(
    transaction: &Transaction<'_>,
    document_id: &str,
    consent_id: Option<&str>,
    now: i64,
) -> Result<(), String> {
    if let Some(consent_id) = consent_id {
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'queued', stage = 'queued', consent_id = ?,
                     cancel_requested_at = NULL, error_code = NULL,
                     error_message = NULL, finished_at = NULL, updated_at = ?
                 WHERE document_id = ? AND job_kind = 'extract'
                   AND state IN ('queued', 'paused')
                   AND requested_source_hash = (
                       SELECT current_source_hash FROM knowledge_documents WHERE id = ?
                   )",
                params![consent_id, now, document_id, document_id],
            )
            .map_err(|error| format!("failed to bind document extraction consent: {error}"))?;
    } else {
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'queued', stage = 'awaiting_consent', consent_id = NULL,
                     updated_at = ?
                 WHERE document_id = ? AND job_kind = 'extract'
                   AND state IN ('queued', 'paused')
                   AND requested_source_hash = (
                       SELECT current_source_hash FROM knowledge_documents WHERE id = ?
                   )",
                params![now, document_id, document_id],
            )
            .map_err(|error| format!("failed to clear document extraction consent: {error}"))?;
    }
    Ok(())
}

fn ensure_job_tx(
    transaction: &Transaction<'_>,
    job_kind: &str,
    document_id: &str,
    source_hash: &str,
    stage: &str,
    priority: i64,
    now: i64,
) -> Result<(String, bool), String> {
    let base_idempotency_key = format!("{job_kind}:{document_id}:{source_hash}");
    let existing = transaction
        .query_row(
            "SELECT id, state FROM knowledge_index_jobs
             WHERE idempotency_key = ? OR idempotency_key LIKE ?
             ORDER BY created_at DESC, id DESC LIMIT 1",
            params![
                base_idempotency_key,
                format!("{base_idempotency_key}:retry:%"),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| format!("查询知识库幂等任务失败：{error}"))?;
    let idempotency_key = if let Some((id, state)) = existing {
        // Active and successful jobs are safe to reuse.  A failed/cancelled/
        // stale job must be retryable even when the source hash is unchanged;
        // give the retry a distinct durable key while retaining the original
        // attempt for diagnostics.
        if matches!(
            KnowledgeJobState::parse(&state),
            Some(KnowledgeJobState::Succeeded)
                | Some(KnowledgeJobState::Queued)
                | Some(KnowledgeJobState::Running)
                | Some(KnowledgeJobState::Cancelling)
                | Some(KnowledgeJobState::Paused)
        ) {
            if job_kind == "extract" {
                transaction
                    .execute(
                        "UPDATE knowledge_index_jobs SET fallback_allowed = 1 WHERE id = ?",
                        params![id],
                    )
                    .map_err(|error| {
                        format!("failed to enable PDF local fallback policy: {error}")
                    })?;
            }
            return Ok((id, false));
        }
        format!("{base_idempotency_key}:retry:{}", Uuid::new_v4())
    } else {
        base_idempotency_key
    };
    let id = Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO knowledge_index_jobs (
                id, job_kind, document_id, requested_source_hash, state, stage,
                priority, created_at, updated_at, idempotency_key, fallback_allowed
             ) VALUES (?, ?, ?, ?, 'queued', ?, ?, ?, ?, ?, ?)",
            params![
                id,
                job_kind,
                document_id,
                source_hash,
                stage,
                priority,
                now,
                now,
                idempotency_key,
                if job_kind == "extract" { 1 } else { 0 }
            ],
        )
        .map_err(|error| format!("创建知识库任务失败：{error}"))?;
    insert_job_event_tx(
        transaction,
        &id,
        "jobQueued",
        None,
        Some("queued"),
        1,
        0,
        r#"{"source":"library"}"#,
        None,
        None,
        now,
    )?;
    Ok((id, true))
}

fn finish_job_tx(
    transaction: &Transaction<'_>,
    job_id: &str,
    revision_id: Option<&str>,
    terminal_state: KnowledgeJobState,
    now: i64,
) -> Result<(), String> {
    if !matches!(
        terminal_state,
        KnowledgeJobState::Succeeded | KnowledgeJobState::Partial
    ) {
        return Err("知识库任务完成状态无效。".to_string());
    }
    let current = transaction
        .query_row(
            "SELECT state, execution_version, state_version FROM knowledge_index_jobs WHERE id = ?",
            params![job_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取知识库完成任务失败：{error}"))?;
    let Some((state, execution_version, state_version)) = current else {
        return Err("知识库任务不存在。".to_string());
    };
    if state == terminal_state.as_str() {
        if let Some(revision_id) = revision_id {
            let changed = transaction
                .execute(
                    "UPDATE knowledge_index_jobs SET revision_id = ?, updated_at = ? WHERE id = ?",
                    params![revision_id, now, job_id],
                )
                .map_err(|error| format!("补写知识库任务 revision 失败：{error}"))?;
            if changed == 0 {
                return Err("知识库任务完成记录已失效。".to_string());
            }
        }
        return Ok(());
    }
    let from =
        KnowledgeJobState::parse(&state).ok_or_else(|| "知识库任务状态无效。".to_string())?;
    if !matches!(from, KnowledgeJobState::Queued | KnowledgeJobState::Running) {
        return Err("知识库任务不能从当前状态完成。".to_string());
    }
    let mut running_state_version = state_version;
    if from == KnowledgeJobState::Queued {
        let changed = transaction
            .execute(
                "UPDATE knowledge_index_jobs SET state = 'running', stage = 'indexing',
                     started_at = COALESCE(started_at, ?), heartbeat_at = ?,
                     state_version = state_version + 1, updated_at = ?
                 WHERE id = ? AND state = 'queued'
                   AND execution_version = ? AND state_version = ?",
                params![now, now, now, job_id, execution_version, state_version],
            )
            .map_err(|error| format!("推进知识库任务失败：{error}"))?;
        if changed == 0 {
            return Err("知识库任务状态已被其他执行实例改变。".to_string());
        }
        running_state_version = running_state_version.saturating_add(1);
        insert_job_event_tx(
            transaction,
            job_id,
            "jobStarted",
            Some("queued"),
            Some("running"),
            execution_version,
            state_version + 1,
            r#"{"stage":"indexing"}"#,
            None,
            None,
            now,
        )?;
    }
    let completed_state_version = running_state_version.saturating_add(1);
    let terminal_state_name = terminal_state.as_str();
    let changed = if let Some(revision_id) = revision_id {
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET revision_id = ?, state = ?, stage = 'done',
                     completed_units = total_units, finished_at = ?, updated_at = ?,
                     heartbeat_at = ?, lease_token = NULL, lease_owner = NULL,
                     lease_expires_at = NULL, runtime_instance_id = NULL,
                     state_version = state_version + 1
                 WHERE id = ? AND state = 'running'
                   AND execution_version = ? AND state_version = ?",
                params![
                    revision_id,
                    terminal_state_name,
                    now,
                    now,
                    now,
                    job_id,
                    execution_version,
                    running_state_version
                ],
            )
            .map_err(|error| format!("完成知识库任务失败：{error}"))?
    } else {
        transaction
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = ?, stage = 'done',
                     completed_units = total_units, finished_at = ?, updated_at = ?,
                     heartbeat_at = ?, lease_token = NULL, lease_owner = NULL,
                     lease_expires_at = NULL, runtime_instance_id = NULL,
                     state_version = state_version + 1
                 WHERE id = ? AND state = 'running'
                   AND execution_version = ? AND state_version = ?",
                params![
                    terminal_state_name,
                    now,
                    now,
                    now,
                    job_id,
                    execution_version,
                    running_state_version
                ],
            )
            .map_err(|error| format!("完成知识库任务失败：{error}"))?
    };
    if changed == 0 {
        return Err("知识库任务完成 CAS 校验失败，已拒绝过期结果。".to_string());
    }
    insert_job_event_tx(
        transaction,
        job_id,
        if terminal_state == KnowledgeJobState::Partial {
            "jobPartial"
        } else {
            "jobSucceeded"
        },
        Some("running"),
        Some(terminal_state_name),
        execution_version,
        completed_state_version,
        r#"{"stage":"done"}"#,
        None,
        None,
        now,
    )?;
    Ok(())
}

fn insert_job_event_tx(
    transaction: &Transaction<'_>,
    job_id: &str,
    event_type: &str,
    from_state: Option<&str>,
    to_state: Option<&str>,
    execution_version: i64,
    state_version: i64,
    payload_json: &str,
    command_id: Option<&str>,
    runtime_instance_id: Option<&str>,
    now: i64,
) -> Result<(), String> {
    let (current_execution_version, current_state_version, last_sequence): (i64, i64, i64) =
        transaction
            .query_row(
                "SELECT execution_version, state_version, COALESCE(last_event_sequence, 0)
             FROM knowledge_index_jobs WHERE id = ?",
                params![job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| format!("读取知识库事件游标失败：{error}"))?;
    if current_execution_version != execution_version || current_state_version != state_version {
        return Err("知识库事件版本与任务状态不一致，拒绝写入。".to_string());
    }
    let sequence = last_sequence
        .checked_add(1)
        .ok_or_else(|| "知识库事件序号已耗尽。".to_string())?;
    transaction
        .execute(
            "INSERT INTO knowledge_index_events (
                event_id, job_id, sequence, event_type, from_state, to_state,
                execution_version, state_version, payload_json, command_id,
                runtime_instance_id, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                Uuid::new_v4().to_string(),
                job_id,
                sequence,
                event_type,
                from_state,
                to_state,
                execution_version,
                state_version,
                payload_json,
                command_id,
                runtime_instance_id,
                now,
            ],
        )
        .map_err(|error| format!("写入知识库任务事件失败：{error}"))?;
    let changed = transaction
        .execute(
            "UPDATE knowledge_index_jobs
             SET last_event_sequence = ?, updated_at = ?
             WHERE id = ? AND last_event_sequence = ?
               AND execution_version = ? AND state_version = ?",
            params![
                sequence,
                now,
                job_id,
                last_sequence,
                execution_version,
                state_version
            ],
        )
        .map_err(|error| format!("更新知识库事件序号失败：{error}"))?;
    if changed != 1 {
        return Err("知识库事件游标发生并发冲突，拒绝写入。".to_string());
    }
    Ok(())
}

fn limit_with_document_diversity(
    hits: Vec<KnowledgeSearchHit>,
    limit: usize,
) -> Vec<KnowledgeSearchHit> {
    let mut per_document = HashMap::<String, usize>::new();
    let mut output = Vec::with_capacity(limit.min(hits.len()));
    for hit in hits {
        let count = per_document.entry(hit.document_id.clone()).or_default();
        if *count >= 4 {
            continue;
        }
        *count += 1;
        output.push(hit);
        if output.len() >= limit {
            break;
        }
    }
    output
}

fn count_documents(connection: &Connection, filter: &str, values: &[Value]) -> Result<i64, String> {
    let sql = format!("SELECT COUNT(*) FROM knowledge_documents WHERE {filter}");
    connection
        .query_row(&sql, params_from_iter(values.iter()), |row| row.get(0))
        .map_err(|error| format!("统计知识文档失败：{error}"))
}

fn build_fts_match_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn escaped_like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn make_snippet(text: &str, query: &str) -> String {
    const MAX_SNIPPET_CHARS: usize = 320;
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX_SNIPPET_CHARS {
        return normalized;
    }
    let start = query
        .split_whitespace()
        .find_map(|term| normalized.find(term))
        .unwrap_or(0);
    let prefix = normalized[..start]
        .chars()
        .rev()
        .take(80)
        .collect::<Vec<_>>();
    let prefix = prefix.into_iter().rev().collect::<String>();
    let suffix = normalized[start..]
        .chars()
        .take(MAX_SNIPPET_CHARS.saturating_sub(prefix.chars().count()))
        .collect::<String>();
    format!(
        "{}{}{}",
        if start > prefix.len() { "…" } else { "" },
        prefix,
        suffix
    )
}

fn parse_json_vec(value: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&value).unwrap_or_default()
}

fn parse_csv_vec(value: String) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn has_warning_entries(value: &str) -> bool {
    serde_json::from_str::<Vec<serde_json::Value>>(value)
        .map(|items| !items.is_empty())
        .unwrap_or(true)
}

fn line_number_at(content: &str, byte_offset: usize) -> Option<usize> {
    if content.is_empty() {
        return None;
    }
    let offset = byte_offset.min(content.len());
    Some(
        content[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1,
    )
}

#[derive(Debug, Clone)]
struct BlockSegment {
    text: String,
    byte_start: usize,
    byte_end: usize,
    char_start: usize,
    char_end: usize,
    is_overlap: bool,
}

fn split_block(
    block: &MarkdownBlock,
    target: usize,
    max: usize,
    overlap: usize,
) -> Vec<BlockSegment> {
    let chars = block.text.char_indices().collect::<Vec<_>>();
    let total_chars = block.text.chars().count();
    if total_chars <= max || chars.is_empty() {
        return vec![BlockSegment {
            text: block.text.clone(),
            byte_start: block.byte_start,
            byte_end: block.byte_end,
            char_start: block.char_start,
            char_end: block.char_end,
            is_overlap: false,
        }];
    }
    let mut segments = Vec::new();
    let mut start_char = 0usize;
    while start_char < total_chars {
        let end_char = (start_char + target.max(1)).min(total_chars);
        let hard_end_char = (start_char + max.max(target.max(1))).min(total_chars);
        let chosen_end = if end_char < total_chars {
            // Prefer a paragraph boundary near target, but never exceed max.
            let mut candidate = end_char;
            for index in (start_char..hard_end_char).rev() {
                let byte = chars[index].0;
                if block.text[byte..].chars().next().is_some_and(|character| {
                    matches!(character, '\n' | ' ' | '。' | '！' | '？' | '.' | '!' | '?')
                }) {
                    candidate = index.saturating_add(1);
                    break;
                }
            }
            candidate.max(start_char + 1).min(hard_end_char)
        } else {
            total_chars
        };
        let local_start = chars[start_char].0;
        let local_end = if chosen_end < chars.len() {
            chars[chosen_end].0
        } else {
            block.text.len()
        };
        segments.push(BlockSegment {
            text: block.text[local_start..local_end].to_string(),
            byte_start: block.byte_start + local_start,
            byte_end: block.byte_start + local_end,
            char_start: block.char_start + start_char,
            char_end: block.char_start + chosen_end,
            is_overlap: !segments.is_empty(),
        });
        if chosen_end >= total_chars {
            break;
        }
        start_char = chosen_end.saturating_sub(overlap.min(chosen_end));
    }
    segments
}

fn mime_type_for_path(path: &str) -> Option<&'static str> {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

fn to_usize(value: i64) -> usize {
    usize::try_from(value).unwrap_or(0)
}

fn bounded_error(value: &str) -> String {
    value.chars().take(1_000).collect()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use rusqlite::params;
    use uuid::Uuid;

    use super::{build_fts_match_query, escaped_like_pattern, split_block, KnowledgeRepository};
    use crate::knowledge::markdown::{parse_markdown, safe_relative_asset_path};
    use crate::knowledge::types::{KnowledgeQueryScope, KnowledgeSearchRequest};
    use crate::library::{
        types::{LibraryNoteCreate, LibraryNoteUpdate},
        LibraryRepository,
    };
    use crate::settings::app_types::KnowledgeRetrievalMode;

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mnemora-knowledge-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn search_request(query: &str) -> KnowledgeSearchRequest {
        KnowledgeSearchRequest {
            query: query.to_string(),
            scope: KnowledgeQueryScope::Library,
            current_literature_id: None,
            current_note_id: None,
            selected_document_ids: Vec::new(),
            element_types: Vec::new(),
            limit: 20,
        }
    }

    fn scalar_i64(connection: &rusqlite::Connection, sql: &str) -> i64 {
        connection.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    fn import_test_pdf(
        library: &LibraryRepository,
        root: &std::path::Path,
        label: &str,
        marker: &str,
    ) -> crate::library::types::LibraryItem {
        let source = root.join(format!("{label}.pdf"));
        fs::write(&source, format!("%PDF-1.7\n{marker}\n%%EOF\n").as_bytes()).unwrap();
        library
            .import_pdfs(vec![source.to_string_lossy().into_owned()], None)
            .unwrap()
            .imported
            .into_iter()
            .next()
            .unwrap()
    }

    fn install_active_test_revision(
        knowledge: &KnowledgeRepository,
        document_id: &str,
        source_hash: &str,
        marker: &str,
    ) -> String {
        let revision_id = Uuid::new_v4().to_string();
        let now = super::now_ms();
        let connection = knowledge.open_connection().unwrap();
        connection
            .execute(
                "INSERT INTO knowledge_revisions (
                    id, document_id, source_hash, canonical_hash, parser_id,
                    parser_version, provider_id, parser_config_hash,
                    normalization_version, chunk_policy_version, status,
                    extraction_quality, created_at, updated_at, completed_at
                 ) VALUES (?, ?, ?, ?, 'test-parser', '1', 'local', '',
                           'test-normalization', 'test-chunk', 'lexical_ready',
                           'local_text_only', ?, ?, ?)",
                params![
                    revision_id,
                    document_id,
                    source_hash,
                    format!("canonical-{marker}"),
                    now,
                    now,
                    now,
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE knowledge_documents
                 SET active_revision_id = ?, state = 'lexical_ready', updated_at = ?
                 WHERE id = ?",
                params![revision_id, now, document_id],
            )
            .unwrap();
        revision_id
    }

    fn literature_status(
        knowledge: &KnowledgeRepository,
        source_id: &str,
    ) -> crate::knowledge::types::KnowledgeDocumentStatus {
        let connection = knowledge.open_connection().unwrap();
        let document_id: String = connection
            .query_row(
                "SELECT id FROM knowledge_documents
                 WHERE source_class = 'literature' AND source_kind = 'pdf' AND source_id = ?",
                [source_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        knowledge.get_document_status(&document_id).unwrap()
    }

    #[test]
    fn query_builders_escape_user_operators() {
        assert_eq!(build_fts_match_query("a \"b\""), "\"a\" AND \"\"\"b\"\"\"");
        assert_eq!(escaped_like_pattern("a_%"), "%a\\_\\%%");
    }

    #[test]
    fn long_markdown_blocks_keep_overlap_and_byte_ranges() {
        let document = parse_markdown(&format!("{}\n", "字".repeat(5_000)));
        let block = &document.blocks[0];
        let chunks = split_block(block, 100, 160, 20);
        assert!(chunks.len() > 20);
        assert!(chunks.iter().skip(1).all(|chunk| chunk.is_overlap));
        assert!(chunks.iter().all(|chunk| chunk.byte_start < chunk.byte_end));
    }

    #[test]
    fn asset_path_gate_is_independent_of_search() {
        assert!(safe_relative_asset_path("attachments/a.png").is_some());
        assert!(safe_relative_asset_path("../../secret").is_none());
    }

    #[test]
    fn global_consent_discovers_every_active_pdf_and_queues_extraction() {
        let root = test_directory("global-consent-discovery");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let first = import_test_pdf(&library, &root, "first", "alpha");
        let second = import_test_pdf(&library, &root, "second", "beta");
        let knowledge = KnowledgeRepository::new(&library);

        assert_eq!(
            scalar_i64(
                &knowledge.open_connection().unwrap(),
                "SELECT COUNT(*) FROM knowledge_documents"
            ),
            0
        );
        assert_eq!(knowledge.grant_global_literature_consent().unwrap(), 2);

        let connection = knowledge.open_connection().unwrap();
        assert_eq!(
            scalar_i64(
                &connection,
                "SELECT COUNT(*) FROM knowledge_documents
                 WHERE source_class = 'literature' AND source_kind = 'pdf'
                   AND cloud_consent_state = 'granted' AND state = 'pending'",
            ),
            2
        );
        assert_eq!(
            scalar_i64(
                &connection,
                "SELECT COUNT(*) FROM knowledge_index_jobs
                 WHERE job_kind = 'extract' AND state = 'queued' AND stage = 'queued'
                   AND consent_id IS NOT NULL",
            ),
            2
        );
        let source_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM knowledge_documents WHERE source_id IN (?, ?)",
                params![first.id, second.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_count, 2);
        assert!(
            knowledge
                .global_literature_consent_status()
                .unwrap()
                .granted
        );
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn document_revoke_does_not_revoke_global_consent() {
        let root = test_directory("document-revoke-global");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let item = import_test_pdf(&library, &root, "paper", "global-stays-active");
        let knowledge = KnowledgeRepository::new(&library);

        knowledge.grant_global_literature_consent().unwrap();
        knowledge
            .grant_literature_consent(&item.id, "document")
            .unwrap();
        assert!(knowledge.revoke_literature_consent(&item.id).unwrap());

        let global = knowledge.global_literature_consent_status().unwrap();
        let document = knowledge
            .literature_consent_status(&item.id)
            .unwrap()
            .unwrap();
        assert!(global.granted);
        assert!(document.granted);
        assert!(!document.document_granted);
        assert!(document.global_granted);
        assert_eq!(document.effective_scope.as_deref(), Some("global"));
        assert_eq!(
            literature_status(&knowledge, &item.id).cloud_consent_state,
            "granted"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn global_grant_keeps_document_consent_precedence_on_existing_job() {
        let root = test_directory("global-grant-document-precedence");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let item = import_test_pdf(&library, &root, "paper", "document-precedence");
        let knowledge = KnowledgeRepository::new(&library);
        let document = knowledge
            .grant_literature_consent(&item.id, "document")
            .unwrap();
        let connection = knowledge.open_connection().unwrap();
        let document_consent_id: String = connection
            .query_row(
                "SELECT id FROM knowledge_cloud_consents
                 WHERE document_id = ? AND scope = 'document' AND revoked_at IS NULL",
                [&document.id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);

        knowledge.grant_global_literature_consent().unwrap();
        let connection = knowledge.open_connection().unwrap();
        let job_consent_id: String = connection
            .query_row(
                "SELECT consent_id FROM knowledge_index_jobs
                 WHERE document_id = ? AND job_kind = 'extract'
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                [&document.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(job_consent_id, document_consent_id);
        drop(connection);
        let status = knowledge
            .literature_consent_status(&item.id)
            .unwrap()
            .unwrap();
        assert_eq!(status.effective_scope.as_deref(), Some("document"));
        assert!(status.document_granted && status.global_granted);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn global_revoke_preserves_document_consent_and_active_revisions() {
        let root = test_directory("global-revoke-scopes");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let retained = import_test_pdf(&library, &root, "retained", "document-consent");
        let waiting = import_test_pdf(&library, &root, "waiting", "global-only");
        let knowledge = KnowledgeRepository::new(&library);

        knowledge.grant_global_literature_consent().unwrap();
        knowledge
            .grant_literature_consent(&retained.id, "document")
            .unwrap();
        let retained_document = literature_status(&knowledge, &retained.id);
        let waiting_document = literature_status(&knowledge, &waiting.id);
        let retained_revision = install_active_test_revision(
            &knowledge,
            &retained_document.id,
            &retained_document.source_hash,
            "retained",
        );
        let waiting_revision = install_active_test_revision(
            &knowledge,
            &waiting_document.id,
            &waiting_document.source_hash,
            "waiting",
        );

        assert!(knowledge.revoke_global_literature_consent().unwrap());
        let retained_after = literature_status(&knowledge, &retained.id);
        let waiting_after = literature_status(&knowledge, &waiting.id);
        assert_eq!(retained_after.cloud_consent_state, "granted");
        assert_eq!(
            retained_after.active_revision_id.as_deref(),
            Some(retained_revision.as_str())
        );
        assert_eq!(retained_after.state, "lexical_ready");
        assert_eq!(waiting_after.cloud_consent_state, "revoked");
        assert_eq!(
            waiting_after.active_revision_id.as_deref(),
            Some(waiting_revision.as_str())
        );
        assert_eq!(waiting_after.state, "lexical_ready");
        assert_eq!(
            knowledge
                .literature_consent_status(&retained.id)
                .unwrap()
                .unwrap()
                .effective_scope
                .as_deref(),
            Some("document")
        );
        assert!(
            !knowledge
                .literature_consent_status(&waiting.id)
                .unwrap()
                .unwrap()
                .granted
        );

        let connection = knowledge.open_connection().unwrap();
        let waiting_job: (String, String, Option<String>) = connection
            .query_row(
                "SELECT state, stage, consent_id FROM knowledge_index_jobs
                 WHERE document_id = ? AND job_kind = 'extract'
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                [&waiting_document.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(waiting_job.0, "queued");
        assert_eq!(waiting_job.1, "awaiting_consent");
        assert!(waiting_job.2.is_none());
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_hash_change_invalidates_document_consent_and_old_job() {
        let root = test_directory("consent-source-change");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let item = import_test_pdf(&library, &root, "paper", "old-source");
        let knowledge = KnowledgeRepository::new(&library);
        let granted = knowledge
            .grant_literature_consent(&item.id, "document")
            .unwrap();

        let new_hash = "f".repeat(64);
        let connection = knowledge.open_connection().unwrap();
        connection
            .execute(
                "UPDATE library_files SET file_hash = ? WHERE item_id = ? AND is_primary = 1",
                params![new_hash, item.id],
            )
            .unwrap();
        drop(connection);

        let refreshed = knowledge.register_literature(&item.id).unwrap();
        assert_eq!(refreshed.source_hash, new_hash);
        assert_eq!(refreshed.cloud_consent_state, "awaiting");
        assert_eq!(refreshed.state, "awaiting_consent");
        let consent = knowledge
            .literature_consent_status(&item.id)
            .unwrap()
            .unwrap();
        assert!(!consent.granted);
        assert_eq!(consent.document_consent_state, "stale");
        let connection = knowledge.open_connection().unwrap();
        let old_job_state: String = connection
            .query_row(
                "SELECT state FROM knowledge_index_jobs
                 WHERE document_id = ? AND requested_source_hash = ?",
                params![granted.id, granted.source_hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_job_state, "stale");
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_fallback_flag_cannot_bypass_missing_cloud_consent() {
        let root = test_directory("fallback-consent-gate");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let item = import_test_pdf(&library, &root, "paper", "no-cloud-consent");
        let knowledge = KnowledgeRepository::new(&library);
        let document = knowledge.enqueue_literature(&item.id).unwrap();
        let connection = knowledge.open_connection().unwrap();
        connection
            .execute(
                "UPDATE knowledge_index_jobs
                 SET stage = 'queued', fallback_allowed = 1
                 WHERE document_id = ? AND job_kind = 'extract'",
                [&document.id],
            )
            .unwrap();
        drop(connection);

        assert!(knowledge
            .claim_next_extract_job_with_fallback("fallback-worker", true)
            .unwrap()
            .is_none());
        let connection = knowledge.open_connection().unwrap();
        let state: (String, i64) = connection
            .query_row(
                "SELECT state, attempt FROM knowledge_index_jobs
                 WHERE document_id = ? AND job_kind = 'extract'",
                [&document.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state.0, "queued");
        assert_eq!(state.1, 0);
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mark_claim_stale_accepts_only_the_current_lease() {
        let root = test_directory("stale-claim-cas");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let item = import_test_pdf(&library, &root, "paper", "lease-cas");
        let knowledge = KnowledgeRepository::new(&library);
        knowledge
            .grant_literature_consent(&item.id, "document")
            .unwrap();
        let current_claim = knowledge
            .claim_next_extract_job("runtime-current")
            .unwrap()
            .unwrap();
        let stale_claim = super::KnowledgeJobClaim {
            lease_token: "replaced-token".to_string(),
            ..current_claim.clone()
        };

        assert!(!knowledge
            .mark_claim_stale(&stale_claim, "WORKER_PANIC", "late panic")
            .unwrap());
        assert!(knowledge
            .mark_claim_stale(&current_claim, "WORKER_PANIC", "current panic")
            .unwrap());
        assert!(!knowledge
            .mark_claim_stale(&current_claim, "WORKER_PANIC", "duplicate panic")
            .unwrap());
        let connection = knowledge.open_connection().unwrap();
        let row: (String, i64, i64, Option<String>) = connection
            .query_row(
                "SELECT state, execution_version, state_version, lease_token
                 FROM knowledge_index_jobs WHERE id = ?",
                [&current_claim.job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "stale");
        assert_eq!(row.1, current_claim.execution_version + 1);
        assert_eq!(row.2, current_claim.state_version + 1);
        assert!(row.3.is_none());
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pdf_revision_elements_assets_chunks_and_fts_commit_atomically() {
        let root = test_directory("pdf-atomic-commit");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let item = import_test_pdf(&library, &root, "paper", "atomic-pdf");
        let knowledge = KnowledgeRepository::new(&library);
        knowledge
            .grant_literature_consent(&item.id, "document")
            .unwrap();
        let claim = knowledge
            .claim_next_extract_job("atomic-worker")
            .unwrap()
            .unwrap();

        let revision_id = Uuid::new_v4().to_string();
        let asset_id = Uuid::new_v4().to_string();
        let element_id = Uuid::new_v4().to_string();
        let duplicate_chunk_id = Uuid::new_v4().to_string();
        let base_chunk = super::PdfChunkRecord {
            id: duplicate_chunk_id.clone(),
            ordinal: 0,
            block_kind: "paragraph".to_string(),
            text: "atomic-evidence".to_string(),
            search_text: "atomic-evidence".to_string(),
            heading_path: vec!["Atomic".to_string()],
            element_ids: vec![element_id.clone()],
            asset_ids: vec![asset_id.clone()],
            page_start: Some(0),
            page_end: Some(0),
            line_start: None,
            line_end: None,
            byte_start: 0,
            byte_end: "atomic-evidence".len(),
            char_start: Some(0),
            char_end: Some("atomic-evidence".chars().count()),
            page_bboxes: vec![serde_json::json!({"pageIndex": 0, "bbox": [0.1, 0.1, 0.8, 0.3]})],
            quality_flags: Vec::new(),
            is_overlap: false,
        };
        let prepared_base = super::PreparedPdfRevision {
            revision_id: revision_id.clone(),
            source_hash: claim.source_hash.clone(),
            canonical_text: "atomic-evidence".to_string(),
            page_count: 1,
            elements: vec![super::PdfElementRecord {
                id: element_id.clone(),
                ordinal: 0,
                element_type: "figure".to_string(),
                block_kind: "figure".to_string(),
                provider_element_id: Some("provider-element-1".to_string()),
                page_index: Some(0),
                page_end: Some(0),
                page_width: Some(1000.0),
                page_height: Some(1400.0),
                bbox: Some([0.1, 0.1, 0.8, 0.3]),
                text: "atomic-evidence".to_string(),
                raw_text: "atomic-evidence".to_string(),
                ocr_text: String::new(),
                formula_latex: String::new(),
                table_html: String::new(),
                table_json: String::new(),
                caption: "atomic figure".to_string(),
                heading_path: vec!["Atomic".to_string()],
                line_start: None,
                line_end: None,
                byte_start: Some(0),
                byte_end: Some("atomic-evidence".len()),
                char_start: Some(0),
                char_end: Some("atomic-evidence".chars().count()),
                asset_names: vec!["figure.png".to_string()],
                asset_ids: vec![asset_id.clone()],
                metadata: serde_json::json!({"sourceRef": {"pageIndex": 0}}),
                quality_flags: Vec::new(),
            }],
            assets: vec![super::PdfAssetRecord {
                id: asset_id.clone(),
                archive_name: "images/figure.png".to_string(),
                relative_path: "images/figure.png".to_string(),
                asset_kind: "figure".to_string(),
                mime_type: "image/png".to_string(),
                byte_size: 8,
                sha256: "asset-hash".to_string(),
                width_px: Some(64),
                height_px: Some(32),
                page_index: Some(0),
                page_width: Some(1000.0),
                page_height: Some(1400.0),
                bbox: Some([0.1, 0.1, 0.8, 0.3]),
                alt_text: "atomic figure".to_string(),
                caption: "atomic figure".to_string(),
                source_asset_name: "figure.png".to_string(),
                metadata: serde_json::json!({}),
            }],
            chunks: vec![base_chunk.clone()],
            parser_id: "atomic-test-parser".to_string(),
            parser_version: "1".to_string(),
            provider_id: "local".to_string(),
            parser_config_hash: "atomic-config".to_string(),
            parser_config_json: "{}".to_string(),
            provider_task_id: None,
            provider_batch_id: None,
            provider_result_hash: None,
            consent_id: None,
            remote_upload: false,
            normalization_version: "atomic-normalization".to_string(),
            chunk_policy_version: "atomic-chunk-policy".to_string(),
            extraction_quality: "local_text_only".to_string(),
            quality_flags: Vec::new(),
            warnings: Vec::new(),
            content_path: "content.txt".to_string(),
            manifest_path: "mnemora_manifest.json".to_string(),
            provider_archive_path: String::new(),
        };

        let mut invalid = prepared_base.clone();
        let mut duplicate_chunk = base_chunk.clone();
        duplicate_chunk.ordinal = 1;
        invalid.chunks.push(duplicate_chunk);
        let error = knowledge
            .commit_prepared_pdf_revision(&claim, &invalid)
            .unwrap_err();
        assert!(error.contains("PDF chunk"));

        let connection = knowledge.open_connection().unwrap();
        for table in [
            "knowledge_revisions",
            "knowledge_assets",
            "knowledge_elements",
            "knowledge_element_assets",
            "knowledge_chunks",
            "knowledge_fts_source",
        ] {
            let count: i64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} must roll back with the failed revision");
        }
        let job_after_failure: (String, String, Option<String>) = connection
            .query_row(
                "SELECT state, stage, revision_id FROM knowledge_index_jobs WHERE id = ?",
                [&claim.job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(job_after_failure.0, "running");
        assert_eq!(job_after_failure.1, "validating");
        assert!(job_after_failure.2.is_none());
        drop(connection);

        let committed = knowledge
            .commit_prepared_pdf_revision(&claim, &prepared_base)
            .unwrap();
        assert_eq!(committed.revision_id, revision_id);
        assert!(!committed.partial);
        let connection = knowledge.open_connection().unwrap();
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_revisions"),
            1
        );
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_assets"),
            1
        );
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_elements"),
            1
        );
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_element_assets"),
            1
        );
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_chunks"),
            1
        );
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_fts_source"),
            1
        );
        let active_revision: String = connection
            .query_row(
                "SELECT active_revision_id FROM knowledge_documents WHERE id = ?",
                [&claim.document_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_revision, revision_id);
        drop(connection);
        assert!(!knowledge
            .search(search_request("atomic-evidence"))
            .unwrap()
            .hits
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn library_note_sync_persists_structure_assets_fts_and_citations() {
        let root = test_directory("sync-note");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "实验记录".to_string(),
                content: "# 研究方法\n\n本文验证 retrieval 检索基线。\n\n![结果图](attachments/figure%20one.png \"趋势图\")\n\n| 指标 | 值 |\n| --- | --- |\n| recall | 0.8 |\n\n```rust\nlet answer = true;\n```\n".to_string(),
                group_name: None,
            })
            .unwrap();
        let note_directory = root.join(note.directory_path.as_deref().unwrap());
        fs::create_dir_all(note_directory.join("attachments")).unwrap();
        fs::write(
            note_directory.join("attachments").join("figure one.png"),
            b"\x89PNG\r\n\x1a\nasset",
        )
        .unwrap();

        let knowledge = KnowledgeRepository::new(&library);
        let status = knowledge.sync_note(&note.id).unwrap();
        assert_eq!(status.source_class, "note");
        assert_eq!(status.source_kind, "markdown_note");
        assert_eq!(status.state, "ready");
        assert_eq!(status.asset_count, 1);
        assert_eq!(status.warning_count, 0);

        let connection = knowledge.open_connection().unwrap();
        assert!(scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_revisions") == 1);
        assert!(scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_elements") >= 4);
        assert!(scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_chunks") >= 4);
        assert!(scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_fts_source") >= 4);

        let lexical = knowledge.search(search_request("retrieval")).unwrap();
        assert!(!lexical.hits.is_empty());
        assert_eq!(lexical.hits[0].source_class, "note");
        assert!(lexical.hits[0].line_start.is_some());
        assert!(lexical.hits[0]
            .element_types
            .iter()
            .any(|value| value == "paragraph"));

        // Two-character Chinese input deliberately takes the LIKE fallback;
        // it must still return the same source and a usable line citation.
        let short = knowledge.search(search_request("检索")).unwrap();
        assert!(!short.hits.is_empty());
        assert!(short.lexical_degraded);
        let chunk = knowledge.get_chunk(&short.hits[0].chunk_id).unwrap();
        assert!(chunk.line_start.unwrap_or_default() >= 1);
        assert!(chunk.byte_end > chunk.byte_start);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn note_revision_switch_hides_old_content_and_source_delete_clears_fts() {
        let root = test_directory("revision-delete");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "版本测试".to_string(),
                content: "旧术语 alpha-only".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        let first = knowledge.sync_note(&note.id).unwrap();
        let old_hit = knowledge
            .search(search_request("alpha-only"))
            .unwrap()
            .hits
            .into_iter()
            .next()
            .unwrap();

        let updated = library
            .update_note(LibraryNoteUpdate {
                note_id: note.id.clone(),
                title: "版本测试".to_string(),
                content: "新术语 beta-only".to_string(),
            })
            .unwrap();
        let second = knowledge.sync_note(&updated.id).unwrap();
        assert_ne!(first.active_revision_id, second.active_revision_id);
        assert!(knowledge
            .search(search_request("alpha-only"))
            .unwrap()
            .hits
            .is_empty());
        assert!(!knowledge
            .search(search_request("beta-only"))
            .unwrap()
            .hits
            .is_empty());
        assert!(knowledge.get_chunk(&old_hit.chunk_id).is_err());

        assert!(knowledge.mark_source_deleted("note", &note.id).unwrap());
        let connection = knowledge.open_connection().unwrap();
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_fts_source"),
            0
        );
        let fts_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'knowledge_fts')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        if fts_exists {
            assert_eq!(
                scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_fts"),
                0
            );
        }
        assert!(knowledge
            .search(search_request("beta-only"))
            .unwrap()
            .hits
            .is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_and_cancelled_note_jobs_are_retryable_without_unbounded_duplicates() {
        let root = test_directory("job-retry");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "任务重试".to_string(),
                content: "retryable content".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        knowledge.sync_note(&note.id).unwrap();
        let connection = knowledge.open_connection().unwrap();
        let first_job: String = connection
            .query_row(
                "SELECT id FROM knowledge_index_jobs ORDER BY created_at ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'failed', finished_at = 1, updated_at = 1
                 WHERE id = ?",
                params![first_job],
            )
            .unwrap();
        drop(connection);

        knowledge.sync_note(&note.id).unwrap();
        let jobs = knowledge.list_jobs(20).unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].state, "succeeded");

        let latest_job = jobs[0].id.clone();
        let connection = knowledge.open_connection().unwrap();
        connection
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'cancelled', finished_at = 2, updated_at = 2
                 WHERE id = ?",
                params![latest_job],
            )
            .unwrap();
        drop(connection);
        knowledge.sync_note(&note.id).unwrap();
        assert_eq!(knowledge.list_jobs(20).unwrap().len(), 3);

        // The latest successful retry is reused; a stable source must not
        // create another attempt on every background notification.
        knowledge.sync_note(&note.id).unwrap();
        assert_eq!(knowledge.list_jobs(20).unwrap().len(), 3);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn queued_cancellation_closes_lease_and_appends_a_consistent_event() {
        let root = test_directory("queued-cancel");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "排队取消".to_string(),
                content: "queued cancellation".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        knowledge.sync_note(&note.id).unwrap();

        let connection = knowledge.open_connection().unwrap();
        let job_id: String = connection
            .query_row("SELECT id FROM knowledge_index_jobs LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        connection
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'queued', stage = 'queued', finished_at = NULL,
                     lease_token = 'stale-token', lease_owner = 'worker-a',
                     lease_expires_at = 123, runtime_instance_id = 'runtime-a'",
                [],
            )
            .unwrap();
        let before_sequence: i64 = connection
            .query_row(
                "SELECT last_event_sequence FROM knowledge_index_jobs WHERE id = ?",
                [&job_id],
                |row| row.get(0),
            )
            .unwrap();
        let before_execution: i64 = connection
            .query_row(
                "SELECT execution_version FROM knowledge_index_jobs WHERE id = ?",
                [&job_id],
                |row| row.get(0),
            )
            .unwrap();
        let before_state_version: i64 = connection
            .query_row(
                "SELECT state_version FROM knowledge_index_jobs WHERE id = ?",
                [&job_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);

        assert!(knowledge.cancel_job(&job_id).unwrap());
        let connection = knowledge.open_connection().unwrap();
        let row: (
            String,
            String,
            i64,
            i64,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            i64,
        ) = connection
            .query_row(
                "SELECT state, stage, execution_version, state_version, finished_at,
                        lease_token, lease_owner, lease_expires_at, runtime_instance_id,
                        last_event_sequence
                 FROM knowledge_index_jobs WHERE id = ?",
                [&job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "cancelled");
        assert_eq!(row.1, "done");
        assert_eq!(row.2, before_execution + 1);
        assert_eq!(row.3, before_state_version + 1);
        assert!(row.4.is_some());
        assert!(row.5.is_none() && row.6.is_none() && row.7.is_none() && row.8.is_none());
        assert_eq!(row.9, before_sequence + 1);

        let event: (String, String, i64, i64, i64) = connection
            .query_row(
                "SELECT event_type, to_state, execution_version, state_version, sequence
                 FROM knowledge_index_events WHERE job_id = ? ORDER BY sequence DESC LIMIT 1",
                [&job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(event.0, "jobCancelled");
        assert_eq!(event.1, "cancelled");
        assert_eq!(event.2, row.2);
        assert_eq!(event.3, row.3);
        assert_eq!(event.4, row.9);
        assert!(!knowledge.cancel_job(&job_id).unwrap());
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn running_cancellation_waits_for_worker_and_then_invalidates_old_execution() {
        let root = test_directory("running-cancel");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "运行取消".to_string(),
                content: "running cancellation".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        knowledge.sync_note(&note.id).unwrap();
        let connection = knowledge.open_connection().unwrap();
        let job_id: String = connection
            .query_row("SELECT id FROM knowledge_index_jobs LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        connection
            .execute(
                "UPDATE knowledge_index_jobs
                 SET state = 'running', stage = 'remote_running',
                     finished_at = NULL, error_code = NULL, error_message = NULL,
                     lease_token = 'worker-token', lease_owner = 'worker-a',
                     lease_expires_at = 999999999, runtime_instance_id = 'runtime-a'",
                [],
            )
            .unwrap();
        let before: (i64, i64) = connection
            .query_row(
                "SELECT execution_version, state_version FROM knowledge_index_jobs WHERE id = ?",
                [&job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        drop(connection);

        assert!(knowledge.cancel_job(&job_id).unwrap());
        let connection = knowledge.open_connection().unwrap();
        let during: (String, Option<i64>, Option<String>, i64, i64) = connection
            .query_row(
                "SELECT state, finished_at, lease_token, execution_version, state_version
                 FROM knowledge_index_jobs WHERE id = ?",
                [&job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(during.0, "cancelling");
        assert!(during.1.is_none());
        assert_eq!(during.2.as_deref(), Some("worker-token"));
        assert_eq!(during.3, before.0);
        assert_eq!(during.4, before.1 + 1);
        drop(connection);

        assert!(knowledge
            .finalize_cancelled_job(&job_id, "worker-stopped")
            .unwrap());
        let connection = knowledge.open_connection().unwrap();
        let after: (String, i64, i64, Option<i64>, Option<String>) = connection
            .query_row(
                "SELECT state, execution_version, state_version, finished_at, lease_token
                 FROM knowledge_index_jobs WHERE id = ?",
                [&job_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(after.0, "cancelled");
        assert_eq!(after.1, before.0 + 1);
        assert_eq!(after.2, before.1 + 2);
        assert!(after.3.is_some() && after.4.is_none());
        assert!(knowledge
            .finalize_cancelled_job(&job_id, "duplicate")
            .unwrap());
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_deletion_cancels_active_jobs_with_audit_events_and_execution_fences() {
        let root = test_directory("delete-jobs");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "删除任务".to_string(),
                content: "first revision".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        knowledge.sync_note(&note.id).unwrap();
        let updated = library
            .update_note(LibraryNoteUpdate {
                note_id: note.id.clone(),
                title: "删除任务".to_string(),
                content: "second revision".to_string(),
            })
            .unwrap();
        knowledge.sync_note(&updated.id).unwrap();

        let connection = knowledge.open_connection().unwrap();
        let document_id: String = connection
            .query_row(
                "SELECT id FROM knowledge_documents WHERE source_class = 'note' AND source_id = ?",
                [&note.id],
                |row| row.get(0),
            )
            .unwrap();
        let job_ids: Vec<String> = {
            let mut statement = connection
                .prepare("SELECT id FROM knowledge_index_jobs WHERE document_id = ? ORDER BY id")
                .unwrap();
            statement
                .query_map([&document_id], |row| row.get(0))
                .unwrap()
                .collect::<Result<Vec<String>, _>>()
                .unwrap()
        };
        assert_eq!(job_ids.len(), 2);
        for (index, _job_id) in job_ids.iter().enumerate() {
            connection
                .execute(
                    "UPDATE knowledge_index_jobs
                     SET state = ?, stage = 'remote_running',
                         lease_token = ?, lease_owner = ?, lease_expires_at = 999999999,
                         runtime_instance_id = ?",
                    rusqlite::params![
                        if index == 0 { "queued" } else { "running" },
                        format!("token-{index}"),
                        format!("owner-{index}"),
                        format!("runtime-{index}"),
                    ],
                )
                .unwrap();
        }
        let before_versions: Vec<(String, i64, i64, i64)> = {
            let mut statement = connection
                .prepare(
                    "SELECT id, execution_version, state_version, last_event_sequence
                     FROM knowledge_index_jobs WHERE document_id = ?",
                )
                .unwrap();
            statement
                .query_map([&document_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        drop(connection);

        assert!(knowledge.mark_source_deleted("note", &note.id).unwrap());
        let connection = knowledge.open_connection().unwrap();
        let document_state: (String, Option<String>, i64) = connection
            .query_row(
                "SELECT state, active_revision_id, include_in_default_scope
                 FROM knowledge_documents WHERE id = ?",
                [&document_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(document_state.0, "deleted");
        assert!(document_state.1.is_none());
        assert_eq!(document_state.2, 0);
        for (job_id, execution, state_version, sequence) in before_versions {
            let current: (String, i64, i64, i64, Option<String>, Option<String>) = connection
                .query_row(
                    "SELECT state, execution_version, state_version, last_event_sequence,
                            lease_token, runtime_instance_id
                     FROM knowledge_index_jobs WHERE id = ?",
                    [&job_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(current.0, "cancelled");
            assert_eq!(current.1, execution + 1);
            assert_eq!(current.2, state_version + 1);
            assert_eq!(current.3, sequence + 1);
            assert!(current.4.is_none() && current.5.is_none());
            let event: (String, String) = connection
                .query_row(
                    "SELECT event_type, json_extract(payload_json, '$.reason')
                     FROM knowledge_index_events WHERE job_id = ? ORDER BY sequence DESC LIMIT 1",
                    [&job_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(event.0, "jobCancelled");
            assert_eq!(event.1, "sourceDeleted");
        }
        drop(connection);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejected_markdown_assets_are_visible_as_partial_quality() {
        let root = test_directory("asset-quality");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "资产质量".to_string(),
                content: "![remote](https://example.com/a.png)\n\n![escape](../secret.png)\n\n![missing](attachments/missing.png)".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        let status = knowledge.sync_note(&note.id).unwrap();
        assert_eq!(status.state, "partial");
        assert_eq!(status.asset_count, 0);
        assert!(status.warning_count >= 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn source_whitelist_rejects_chat_like_rows() {
        let root = test_directory("source-whitelist");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let connection = library.open_connection().unwrap();
        let error = connection
            .execute(
                "INSERT INTO knowledge_documents (
                    id, source_class, source_kind, source_id, title, state,
                    current_source_hash, cloud_consent_state, created_at, updated_at
                 ) VALUES (?, 'conversation', 'chat', 'chat-1', '', 'pending', '', 'not_required', 1, 1)",
                params![Uuid::new_v4().to_string()],
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("KNOWLEDGE_SOURCE_CLASS_NOT_ALLOWED"));
        let _ = fs::remove_dir_all(root);
    }

    fn test_embedding_spec() -> crate::knowledge::embedding::EmbeddingProviderSpec {
        crate::knowledge::embedding::EmbeddingProviderSpec {
            provider_id: "test-openai".to_string(),
            model_id: "test-embedding".to_string(),
            model_revision: "embedding-v1:test".to_string(),
            base_url: "https://example.test/v1".to_string(),
            protocol: crate::settings::types::ApiProtocol::OpenAiChatCompletions,
            auth_scheme: crate::settings::types::AuthScheme::Bearer,
            credential_revision: 0,
            expected_dimensions: Some(2),
            embedding_key: "sha256:test-embedding-route".to_string(),
        }
    }

    fn finish_test_embeddings(
        knowledge: &KnowledgeRepository,
        document_id: &str,
        spec: &crate::knowledge::embedding::EmbeddingProviderSpec,
    ) -> Vec<super::EmbeddingChunkInput> {
        let summary = knowledge
            .enqueue_embedding_jobs(spec, Some(document_id), false)
            .unwrap();
        assert_eq!(summary.queued_job_count, 1);
        let claim = knowledge
            .claim_next_embedding_job("embedding-test-worker", &spec.embedding_key)
            .unwrap()
            .unwrap();
        let chunks = knowledge.embedding_chunks_for_claim(&claim).unwrap();
        assert!(!chunks.is_empty());
        let writes = chunks
            .iter()
            .enumerate()
            .map(|(index, chunk)| super::EmbeddingWrite {
                chunk_id: chunk.chunk_id.clone(),
                content_hash: chunk.content_hash.clone(),
                vector: if index == 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                },
            })
            .collect::<Vec<_>>();
        knowledge
            .write_embedding_batch(&claim, spec, writes, chunks.len(), chunks.len(), 1)
            .unwrap();
        assert!(knowledge.complete_embedding_claim(&claim).unwrap());
        chunks
    }

    #[test]
    fn embedding_job_persists_normalized_blobs_and_enables_vector_and_hybrid_search() {
        let root = test_directory("embedding-vector-hybrid");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "向量检索测试".to_string(),
                content: "# Semantic retrieval\n\nVector evidence lives in this paragraph.\n\nAnother independent paragraph.".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        let document = knowledge.sync_note(&note.id).unwrap();
        let spec = test_embedding_spec();
        let chunks = finish_test_embeddings(&knowledge, &document.id, &spec);
        let overview = knowledge.overview().unwrap();
        assert_eq!(overview.embedding_ready_count, chunks.len());
        assert_eq!(overview.embedding_pending_count, 0);
        assert_eq!(overview.embedding_failed_count, 0);

        let connection = knowledge.open_connection().unwrap();
        let stored: (i64, i64, String, i64) = connection
            .query_row(
                "SELECT COUNT(*), MIN(length(vector_blob)), MIN(normalization),
                        MIN(retry_count)
                 FROM knowledge_embeddings WHERE embedding_key = ? AND status = 'ready'",
                [&spec.embedding_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(stored.0 as usize, chunks.len());
        assert_eq!(stored.1, 8);
        assert_eq!(stored.2, "l2");
        assert_eq!(stored.3, 1);
        drop(connection);

        let vector = knowledge
            .search_with_vector(
                search_request("concept absent from lexical text"),
                KnowledgeRetrievalMode::Vector,
                &spec.embedding_key,
                vec![10.0, 0.0],
            )
            .unwrap();
        assert_eq!(vector.actual_mode, "vector");
        assert_eq!(vector.vector_dimensions, Some(2));
        assert_eq!(vector.hits[0].chunk_id, chunks[0].chunk_id);
        assert_eq!(vector.hits[0].vector_rank, Some(1));
        assert!(vector.hits[0].vector_score.is_some());

        let hybrid = knowledge
            .search_with_vector(
                search_request("retrieval"),
                KnowledgeRetrievalMode::Hybrid,
                &spec.embedding_key,
                vec![1.0, 0.0],
            )
            .unwrap();
        assert_eq!(hybrid.actual_mode, "hybrid");
        assert!(hybrid.hits.iter().any(|hit| hit.fused_score.is_some()));
        assert!(hybrid.hits.iter().any(|hit| hit.lexical_rank.is_some()));
        assert!(hybrid.hits.iter().any(|hit| hit.vector_rank.is_some()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn overview_derives_pending_and_failed_embedding_counts_from_durable_jobs() {
        let root = test_directory("embedding-overview-jobs");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "向量状态统计".to_string(),
                content: "first pending paragraph\n\nsecond pending paragraph".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        let document = knowledge.sync_note(&note.id).unwrap();
        let spec = test_embedding_spec();
        let summary = knowledge
            .enqueue_embedding_jobs(&spec, Some(&document.id), false)
            .unwrap();
        assert!(summary.pending_chunk_count > 0);
        assert!(knowledge
            .active_embedding_job_ids_except(Some(&spec.embedding_key))
            .unwrap()
            .is_empty());
        assert_eq!(
            knowledge
                .active_embedding_job_ids_except(Some("sha256:other-route"))
                .unwrap()
                .len(),
            1
        );

        let pending = knowledge.overview().unwrap();
        assert_eq!(pending.embedding_ready_count, 0);
        assert_eq!(pending.embedding_pending_count, summary.pending_chunk_count);
        assert_eq!(pending.embedding_failed_count, 0);

        let job_id: String = knowledge
            .open_connection()
            .unwrap()
            .query_row(
                "SELECT id FROM knowledge_index_jobs WHERE job_kind = 'embed' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(knowledge.cancel_job(&job_id).unwrap());
        let failed = knowledge.overview().unwrap();
        assert_eq!(failed.embedding_pending_count, 0);
        assert!(failed.embedding_failed_count >= summary.pending_chunk_count);

        // Retrying the same idempotent job makes the latest state live again;
        // the old cancelled diagnostic must not be counted as an additional
        // failure while work is pending.
        knowledge
            .enqueue_embedding_jobs(&spec, Some(&document.id), false)
            .unwrap();
        let retried = knowledge.overview().unwrap();
        assert_eq!(retried.embedding_pending_count, summary.pending_chunk_count);
        assert_eq!(retried.embedding_failed_count, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_dimension_mismatch_rolls_back_the_whole_batch_and_keeps_lexical() {
        let root = test_directory("embedding-dimension-rollback");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let note = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "维度回滚".to_string(),
                content: "first searchable paragraph\n\nsecond searchable paragraph".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        let document = knowledge.sync_note(&note.id).unwrap();
        let spec = test_embedding_spec();
        knowledge
            .enqueue_embedding_jobs(&spec, Some(&document.id), false)
            .unwrap();
        let claim = knowledge
            .claim_next_embedding_job("dimension-worker", &spec.embedding_key)
            .unwrap()
            .unwrap();
        let chunks = knowledge.embedding_chunks_for_claim(&claim).unwrap();
        assert!(chunks.len() >= 2);
        let error = knowledge
            .write_embedding_batch(
                &claim,
                &spec,
                vec![
                    super::EmbeddingWrite {
                        chunk_id: chunks[0].chunk_id.clone(),
                        content_hash: chunks[0].content_hash.clone(),
                        vector: vec![1.0, 0.0],
                    },
                    super::EmbeddingWrite {
                        chunk_id: chunks[1].chunk_id.clone(),
                        content_hash: chunks[1].content_hash.clone(),
                        vector: vec![1.0, 0.0, 0.0],
                    },
                ],
                2,
                chunks.len(),
                0,
            )
            .unwrap_err();
        assert!(error.contains("EMBEDDING_DIMENSION_MISMATCH"));
        let connection = knowledge.open_connection().unwrap();
        assert_eq!(
            scalar_i64(&connection, "SELECT COUNT(*) FROM knowledge_embeddings"),
            0
        );
        drop(connection);
        assert!(!knowledge
            .search(search_request("searchable"))
            .unwrap()
            .hits
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn identical_chunk_content_reuses_embedding_cache_without_a_remote_job() {
        let root = test_directory("embedding-cache");
        let library = LibraryRepository::new(root.clone());
        library.initialize().unwrap();
        let first = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "Shared title".to_string(),
                content: "# Shared heading\n\nIdentical reusable content.".to_string(),
                group_name: None,
            })
            .unwrap();
        let second = library
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "Shared title".to_string(),
                content: "# Shared heading\n\nIdentical reusable content.".to_string(),
                group_name: None,
            })
            .unwrap();
        let knowledge = KnowledgeRepository::new(&library);
        let first_document = knowledge.sync_note(&first.id).unwrap();
        let second_document = knowledge.sync_note(&second.id).unwrap();
        let spec = test_embedding_spec();
        let first_chunks = finish_test_embeddings(&knowledge, &first_document.id, &spec);
        let summary = knowledge
            .enqueue_embedding_jobs(&spec, Some(&second_document.id), false)
            .unwrap();
        assert_eq!(summary.queued_job_count, 0);
        assert_eq!(summary.pending_chunk_count, 0);
        assert_eq!(summary.cached_chunk_count, first_chunks.len());

        let response = knowledge
            .search_with_vector(
                search_request("not a lexical match"),
                KnowledgeRetrievalMode::Vector,
                &spec.embedding_key,
                vec![1.0, 0.0],
            )
            .unwrap();
        assert!(response
            .hits
            .iter()
            .any(|hit| hit.document_id == second_document.id));
        let _ = fs::remove_dir_all(root);
    }
}
