//! Knowledge schema v19/v20。
//!
//! 这里的表全部是 library 数据库中的派生表。迁移只建空表和空 FTS，不做
//! 网络请求、不上传文件，也不主动创建全库索引。

use rusqlite::Connection;

pub const KNOWLEDGE_SCHEMA_VERSION: i64 = 20;
const KNOWLEDGE_SCHEMA_V19_VERSION: i64 = 19;

fn knowledge_elements_exists(connection: &Connection) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'knowledge_elements'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| format!("检查知识元素表失败：{error}"))
}

fn knowledge_elements_has_column(
    connection: &Connection,
    column_name: &str,
) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pragma_table_info('knowledge_elements')
                WHERE name = ?
             )",
            [column_name],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| format!("检查知识元素列 {column_name} 失败：{error}"))
}

/// Repairs the short-lived v19 schema that shipped without Markdown character
/// offsets. The schema version remains 19 because this is a compatible
/// completion of v19, not the embedding migration reserved for the next
/// schema version.
fn ensure_v19_element_character_offsets(connection: &Connection) -> Result<(), String> {
    if !knowledge_elements_exists(connection)? {
        return Ok(());
    }

    let has_char_start = knowledge_elements_has_column(connection, "char_start")?;
    let has_char_end = knowledge_elements_has_column(connection, "char_end")?;
    let mut ddl = String::from("BEGIN IMMEDIATE;\n");
    if !has_char_start {
        ddl.push_str("ALTER TABLE knowledge_elements ADD COLUMN char_start INTEGER;\n");
    }
    if !has_char_end {
        ddl.push_str("ALTER TABLE knowledge_elements ADD COLUMN char_end INTEGER;\n");
    }
    // ALTER TABLE cannot retrofit the table-level CHECK used by fresh v19
    // databases. Equivalent triggers preserve the same invariant for an
    // upgraded database without rebuilding citation data.
    ddl.push_str(
        r#"
CREATE TRIGGER IF NOT EXISTS knowledge_elements_char_range_insert
BEFORE INSERT ON knowledge_elements
WHEN ((NEW.char_start IS NULL) <> (NEW.char_end IS NULL))
     OR NEW.char_start < 0
     OR NEW.char_end < NEW.char_start
BEGIN
    SELECT RAISE(ABORT, 'KNOWLEDGE_ELEMENT_CHAR_RANGE_INVALID');
END;

CREATE TRIGGER IF NOT EXISTS knowledge_elements_char_range_update
BEFORE UPDATE OF char_start, char_end ON knowledge_elements
WHEN ((NEW.char_start IS NULL) <> (NEW.char_end IS NULL))
     OR NEW.char_start < 0
     OR NEW.char_end < NEW.char_start
BEGIN
    SELECT RAISE(ABORT, 'KNOWLEDGE_ELEMENT_CHAR_RANGE_INVALID');
END;

COMMIT;
"#,
    );
    connection.execute_batch(&ddl).map_err(|error| {
        let _ = connection.execute_batch("ROLLBACK;");
        format!("修复知识库 v19 字符坐标结构失败：{error}")
    })
}

fn probe_tokenizer(connection: &Connection, tokenizer: &str) -> bool {
    let _ = connection.execute_batch("DROP TABLE IF EXISTS temp.knowledge_fts_probe;");
    let sql = format!(
        "CREATE VIRTUAL TABLE temp.knowledge_fts_probe USING fts5(value, tokenize='{tokenizer}');\nDROP TABLE temp.knowledge_fts_probe;"
    );
    connection.execute_batch(&sql).is_ok()
}

/// 创建 v19 知识表。FTS5/trigram 是能力增强而不是迁移硬依赖：如果当前
/// SQLite 没有 FTS5 或不支持 trigram，仍保留 source 表，repository 会使用
///受控 LIKE fallback，并在 overview 中标记 degraded。
pub fn migrate_v19(connection: &Connection) -> Result<(), String> {
    let tokenizer = if probe_tokenizer(connection, "trigram") {
        "trigram"
    } else if probe_tokenizer(connection, "unicode61") {
        "unicode61"
    } else {
        "none"
    };
    let fts_available = tokenizer != "none";

    let fts_ddl = if fts_available {
        format!(
            r#"
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
    title,
    heading_path,
    element_types,
    body,
    multimodal_text,
    search_text,
    content='knowledge_fts_source',
    content_rowid='rowid',
    tokenize='{tokenizer}'
);

CREATE TRIGGER IF NOT EXISTS knowledge_fts_source_ai
AFTER INSERT ON knowledge_fts_source
BEGIN
    INSERT INTO knowledge_fts(
        rowid, title, heading_path, element_types,
        body, multimodal_text, search_text
    ) VALUES (
        NEW.rowid, NEW.title, NEW.heading_path, NEW.element_types,
        NEW.body, NEW.multimodal_text, NEW.search_text
    );
END;

CREATE TRIGGER IF NOT EXISTS knowledge_fts_source_ad
AFTER DELETE ON knowledge_fts_source
BEGIN
    INSERT INTO knowledge_fts(
        knowledge_fts, rowid, title, heading_path, element_types,
        body, multimodal_text, search_text
    ) VALUES (
        'delete', OLD.rowid, OLD.title, OLD.heading_path, OLD.element_types,
        OLD.body, OLD.multimodal_text, OLD.search_text
    );
END;

CREATE TRIGGER IF NOT EXISTS knowledge_fts_source_au
AFTER UPDATE ON knowledge_fts_source
BEGIN
    INSERT INTO knowledge_fts(
        knowledge_fts, rowid, title, heading_path, element_types,
        body, multimodal_text, search_text
    ) VALUES (
        'delete', OLD.rowid, OLD.title, OLD.heading_path, OLD.element_types,
        OLD.body, OLD.multimodal_text, OLD.search_text
    );
    INSERT INTO knowledge_fts(
        rowid, title, heading_path, element_types,
        body, multimodal_text, search_text
    ) VALUES (
        NEW.rowid, NEW.title, NEW.heading_path, NEW.element_types,
        NEW.body, NEW.multimodal_text, NEW.search_text
    );
END;
"#
        )
    } else {
        String::new()
    };

    // SQLite 把 execute_batch 中的显式事务完整地视为一个迁移单元；如果任一
    // CREATE/trigger 失败，调用方不会看到 user_version=19。
    let ddl = format!(
        r#"
BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS knowledge_index_capabilities (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    fts5_available INTEGER NOT NULL CHECK (fts5_available IN (0, 1)),
    tokenizer TEXT NOT NULL CHECK (tokenizer IN ('trigram', 'unicode61', 'none')),
    lexical_degraded INTEGER NOT NULL CHECK (lexical_degraded IN (0, 1)),
    checked_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS knowledge_documents (
    id TEXT PRIMARY KEY,
    source_class TEXT NOT NULL CHECK (source_class IN ('literature', 'note')),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('pdf', 'markdown_note')),
    source_id TEXT NOT NULL,
    library_item_id TEXT REFERENCES library_items(id) ON DELETE CASCADE,
    note_id TEXT REFERENCES library_notes(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    active_revision_id TEXT,
    state TEXT NOT NULL CHECK (state IN (
        'pending', 'awaiting_consent', 'remote_pending', 'remote_running',
        'normalizing', 'lexical_ready', 'ready', 'partial', 'degraded',
        'failed', 'stale', 'deleted'
    )),
    current_source_hash TEXT NOT NULL DEFAULT '',
    cloud_consent_state TEXT NOT NULL DEFAULT 'not_required' CHECK (
        cloud_consent_state IN ('not_required', 'awaiting', 'granted', 'revoked')
    ),
    include_in_default_scope INTEGER NOT NULL DEFAULT 1 CHECK (
        include_in_default_scope IN (0, 1)
    ),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(source_class, source_kind, source_id),
    CHECK (length(trim(source_id)) > 0),
    CHECK (updated_at >= created_at)
);

CREATE TABLE IF NOT EXISTS knowledge_cloud_consents (
    id TEXT PRIMARY KEY,
    scope_key TEXT NOT NULL DEFAULT 'local-library',
    document_id TEXT REFERENCES knowledge_documents(id) ON DELETE CASCADE,
    source_hash TEXT NOT NULL DEFAULT '',
    provider_id TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('document', 'global')),
    granted_at INTEGER NOT NULL,
    revoked_at INTEGER,
    token_fingerprint TEXT NOT NULL DEFAULT '',
    pages_estimate INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (
        (scope = 'document' AND document_id IS NOT NULL AND length(source_hash) > 0)
        OR (scope = 'global' AND document_id IS NULL AND source_hash = '')
    ),
    CHECK (length(trim(scope_key)) > 0),
    CHECK (length(trim(provider_id)) > 0),
    CHECK (length(trim(policy_version)) > 0),
    CHECK (pages_estimate >= 0),
    CHECK (revoked_at IS NULL OR revoked_at >= granted_at)
);

CREATE TABLE IF NOT EXISTS knowledge_revisions (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES knowledge_documents(id) ON DELETE CASCADE,
    source_hash TEXT NOT NULL,
    canonical_hash TEXT NOT NULL,
    parser_id TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    provider_id TEXT NOT NULL DEFAULT 'local',
    parser_config_hash TEXT NOT NULL DEFAULT '',
    provider_task_id TEXT,
    provider_batch_id TEXT,
    batch_group_id TEXT,
    batch_count INTEGER NOT NULL DEFAULT 1,
    provider_result_hash TEXT,
    parser_config_json TEXT NOT NULL DEFAULT '{{}}',
    consent_id TEXT REFERENCES knowledge_cloud_consents(id),
    remote_upload INTEGER NOT NULL DEFAULT 0 CHECK (remote_upload IN (0, 1)),
    normalization_version TEXT NOT NULL,
    chunk_policy_version TEXT NOT NULL,
    content_path TEXT NOT NULL DEFAULT '',
    manifest_path TEXT NOT NULL DEFAULT '',
    provider_archive_path TEXT NOT NULL DEFAULT '',
    page_count INTEGER NOT NULL DEFAULT 0,
    source_page_offset INTEGER NOT NULL DEFAULT 0,
    line_count INTEGER NOT NULL DEFAULT 0,
    byte_count INTEGER NOT NULL DEFAULT 0,
    element_count INTEGER NOT NULL DEFAULT 0,
    asset_count INTEGER NOT NULL DEFAULT 0,
    chunk_count INTEGER NOT NULL DEFAULT 0,
    extraction_quality TEXT NOT NULL DEFAULT 'unknown' CHECK (
        extraction_quality IN (
            'unknown', 'cloud_ready', 'cloud_partial',
            'cloud_failed_local_fallback', 'local_text_only',
            'awaiting_consent', 'degraded', 'failed'
        )
    ),
    quality_flags TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK (status IN (
        'building', 'lexical_ready', 'embedding_pending', 'ready',
        'partial', 'failed', 'cancelled', 'stale'
    )),
    warning_json TEXT NOT NULL DEFAULT '[]',
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    UNIQUE(document_id, source_hash, parser_id, parser_version,
           parser_config_hash, normalization_version, chunk_policy_version),
    CHECK (batch_count > 0),
    CHECK (page_count >= 0 AND source_page_offset >= 0),
    CHECK (line_count >= 0 AND byte_count >= 0),
    CHECK (element_count >= 0 AND asset_count >= 0 AND chunk_count >= 0),
    CHECK (completed_at IS NULL OR completed_at >= created_at)
);

CREATE TABLE IF NOT EXISTS knowledge_elements (
    id TEXT PRIMARY KEY,
    revision_id TEXT NOT NULL REFERENCES knowledge_revisions(id) ON DELETE CASCADE,
    parent_element_id TEXT REFERENCES knowledge_elements(id) ON DELETE SET NULL,
    element_type TEXT NOT NULL CHECK (element_type IN (
        'text', 'title', 'list', 'table', 'figure', 'chart', 'formula',
        'algorithm', 'code', 'caption', 'reference', 'header', 'footer',
        'paragraph', 'quote', 'footnote', 'page_image', 'unknown'
    )),
    ordinal INTEGER NOT NULL,
    provider_element_id TEXT,
    page_index INTEGER,
    page_end INTEGER,
    page_width REAL,
    page_height REAL,
    norm_x1 REAL,
    norm_y1 REAL,
    norm_x2 REAL,
    norm_y2 REAL,
    page_x1 REAL,
    page_y1 REAL,
    page_x2 REAL,
    page_y2 REAL,
    reading_order INTEGER NOT NULL DEFAULT 0,
    line_start INTEGER,
    line_end INTEGER,
    byte_start INTEGER,
    byte_end INTEGER,
    char_start INTEGER,
    char_end INTEGER,
    heading_path_json TEXT NOT NULL DEFAULT '[]',
    text TEXT NOT NULL DEFAULT '',
    raw_text TEXT NOT NULL DEFAULT '',
    ocr_text TEXT NOT NULL DEFAULT '',
    formula_latex TEXT NOT NULL DEFAULT '',
    table_html TEXT NOT NULL DEFAULT '',
    table_json TEXT NOT NULL DEFAULT '',
    caption TEXT NOT NULL DEFAULT '',
    language TEXT NOT NULL DEFAULT '',
    confidence REAL,
    source_ref_json TEXT NOT NULL DEFAULT '{{}}',
    metadata_json TEXT NOT NULL DEFAULT '{{}}',
    content_hash TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    UNIQUE(revision_id, ordinal),
    CHECK (ordinal >= 0),
    CHECK (page_index IS NULL OR page_index >= 0),
    CHECK (page_end IS NULL OR (page_index IS NOT NULL AND page_end >= page_index)),
    CHECK (reading_order >= 0),
    CHECK ((line_start IS NULL AND line_end IS NULL)
        OR (line_start IS NOT NULL AND line_end IS NOT NULL
            AND line_start >= 1 AND line_end >= line_start)),
    CHECK ((byte_start IS NULL AND byte_end IS NULL)
        OR (byte_start IS NOT NULL AND byte_end IS NOT NULL
            AND byte_start >= 0 AND byte_end >= byte_start)),
    CHECK ((char_start IS NULL AND char_end IS NULL)
        OR (char_start IS NOT NULL AND char_end IS NOT NULL
            AND char_start >= 0 AND char_end >= char_start)),
    CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1))
);

CREATE TABLE IF NOT EXISTS knowledge_assets (
    id TEXT PRIMARY KEY,
    revision_id TEXT NOT NULL REFERENCES knowledge_revisions(id) ON DELETE CASCADE,
    asset_kind TEXT NOT NULL CHECK (asset_kind IN (
        'figure', 'chart', 'table_crop', 'formula_crop', 'page_render', 'embedded_image'
    )),
    relative_path TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    sha256 TEXT NOT NULL,
    width_px INTEGER,
    height_px INTEGER,
    page_index INTEGER,
    page_width REAL,
    page_height REAL,
    norm_x1 REAL,
    norm_y1 REAL,
    norm_x2 REAL,
    norm_y2 REAL,
    page_x1 REAL,
    page_y1 REAL,
    page_x2 REAL,
    page_y2 REAL,
    alt_text TEXT NOT NULL DEFAULT '',
    caption TEXT NOT NULL DEFAULT '',
    ocr_status TEXT NOT NULL DEFAULT 'not_applicable' CHECK (
        ocr_status IN ('not_applicable', 'pending', 'ready', 'failed')
    ),
    source_asset_name TEXT NOT NULL DEFAULT '',
    metadata_json TEXT NOT NULL DEFAULT '{{}}',
    created_at INTEGER NOT NULL,
    UNIQUE(revision_id, relative_path),
    CHECK (length(trim(relative_path)) > 0),
    CHECK (length(trim(mime_type)) > 0),
    CHECK ((width_px IS NULL AND height_px IS NULL)
        OR (width_px > 0 AND height_px > 0)),
    CHECK (page_index IS NULL OR page_index >= 0),
    CHECK (byte_size > 0 OR ocr_status <> 'ready')
);

CREATE TABLE IF NOT EXISTS knowledge_element_assets (
    element_id TEXT NOT NULL REFERENCES knowledge_elements(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES knowledge_assets(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('primary', 'caption_source', 'page_context', 'derived_crop')),
    ordinal INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(element_id, asset_id, role)
);

CREATE TABLE IF NOT EXISTS knowledge_pdf_batches (
    id TEXT PRIMARY KEY,
    revision_id TEXT NOT NULL REFERENCES knowledge_revisions(id) ON DELETE CASCADE,
    batch_group_id TEXT NOT NULL,
    batch_index INTEGER NOT NULL,
    batch_total INTEGER NOT NULL,
    source_page_start INTEGER NOT NULL,
    source_page_end INTEGER NOT NULL,
    split_relative_path TEXT NOT NULL DEFAULT '',
    split_hash TEXT NOT NULL DEFAULT '',
    byte_size INTEGER NOT NULL DEFAULT 0,
    provider_task_id TEXT,
    provider_batch_id TEXT,
    provider_state TEXT NOT NULL DEFAULT 'pending',
    extracted_pages INTEGER NOT NULL DEFAULT 0,
    total_pages INTEGER NOT NULL DEFAULT 0,
    result_zip_hash TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN (
        'planned', 'uploading', 'remote_pending', 'remote_running',
        'downloading', 'validating', 'ready', 'partial', 'failed',
        'cancelled', 'stale'
    )),
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(revision_id, batch_index),
    CHECK (batch_total > 0 AND batch_index >= 0 AND batch_index < batch_total),
    CHECK (source_page_start >= 0 AND source_page_end >= source_page_start),
    CHECK (byte_size >= 0 AND extracted_pages >= 0 AND total_pages >= 0),
    CHECK (total_pages = 0 OR extracted_pages <= total_pages),
    CHECK (provider_state IN ('pending', 'running', 'converting', 'done', 'failed', 'cancelled', 'unknown'))
);

CREATE TABLE IF NOT EXISTS knowledge_asset_analyses (
    id TEXT PRIMARY KEY,
    asset_id TEXT NOT NULL REFERENCES knowledge_assets(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    prompt_version TEXT NOT NULL DEFAULT '',
    input_hash TEXT NOT NULL,
    consent_id TEXT REFERENCES knowledge_cloud_consents(id),
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'ready', 'failed', 'cancelled', 'stale')),
    ocr_text TEXT NOT NULL DEFAULT '',
    caption TEXT NOT NULL DEFAULT '',
    structured_json TEXT NOT NULL DEFAULT '{{}}',
    quality_json TEXT NOT NULL DEFAULT '{{}}',
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(asset_id, provider_id, model_id, prompt_version, input_hash)
);

CREATE TABLE IF NOT EXISTS knowledge_chunks (
    id TEXT PRIMARY KEY,
    revision_id TEXT NOT NULL REFERENCES knowledge_revisions(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    block_kind TEXT NOT NULL,
    text TEXT NOT NULL,
    search_text TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    page_start INTEGER,
    page_end INTEGER,
    line_start INTEGER,
    line_end INTEGER,
    byte_start INTEGER NOT NULL,
    byte_end INTEGER NOT NULL,
    char_start INTEGER,
    char_end INTEGER,
    heading_path_json TEXT NOT NULL DEFAULT '[]',
    element_ids_json TEXT NOT NULL DEFAULT '[]',
    asset_ids_json TEXT NOT NULL DEFAULT '[]',
    page_bbox_json TEXT NOT NULL DEFAULT '[]',
    quality_flags TEXT NOT NULL DEFAULT '[]',
    metadata_json TEXT NOT NULL DEFAULT '{{}}',
    prev_chunk_id TEXT,
    next_chunk_id TEXT,
    fts_rowid INTEGER NOT NULL UNIQUE,
    token_estimate INTEGER NOT NULL DEFAULT 0,
    is_overlap INTEGER NOT NULL DEFAULT 0 CHECK (is_overlap IN (0, 1)),
    created_at INTEGER NOT NULL,
    UNIQUE(revision_id, ordinal),
    CHECK (ordinal >= 0 AND byte_start >= 0 AND byte_end >= byte_start),
    CHECK ((char_start IS NULL AND char_end IS NULL)
        OR (char_start IS NOT NULL AND char_end IS NOT NULL AND char_start >= 0 AND char_end >= char_start)),
    CHECK ((page_start IS NULL AND page_end IS NULL)
        OR (page_start IS NOT NULL AND page_end IS NOT NULL AND page_start >= 0 AND page_end >= page_start)),
    CHECK ((line_start IS NULL AND line_end IS NULL)
        OR (line_start IS NOT NULL AND line_end IS NOT NULL AND line_start >= 1 AND line_end >= line_start)),
    CHECK (token_estimate >= 0 AND fts_rowid > 0)
);

CREATE TABLE IF NOT EXISTS knowledge_index_jobs (
    id TEXT PRIMARY KEY,
    job_kind TEXT NOT NULL CHECK (job_kind IN ('extract', 'asset_analyze', 'chunk', 'lexical', 'embed', 'rebuild', 'cleanup', 'reconcile')),
    document_id TEXT REFERENCES knowledge_documents(id) ON DELETE CASCADE,
    revision_id TEXT REFERENCES knowledge_revisions(id) ON DELETE CASCADE,
    requested_source_hash TEXT NOT NULL DEFAULT '',
    requested_config_hash TEXT NOT NULL DEFAULT '',
    consent_id TEXT REFERENCES knowledge_cloud_consents(id),
    batch_group_id TEXT,
    batch_index INTEGER,
    batch_total INTEGER,
    provider_task_id TEXT,
    provider_batch_id TEXT,
    provider_state TEXT,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'cancelling', 'paused', 'succeeded', 'partial', 'failed', 'cancelled', 'stale')),
    stage TEXT NOT NULL DEFAULT 'queued' CHECK (stage IN (
        'queued', 'validating', 'awaiting_consent', 'planning_batches',
        'requesting_upload_url', 'uploading', 'remote_pending', 'remote_running',
        'downloading', 'validating_archive', 'normalizing_elements',
        'analyzing_asset', 'cloud_failed_local_fallback', 'local_text_fallback',
        'chunking', 'indexing', 'writing_revision', 'building_fts',
        'waiting_embedding', 'embedding', 'committing', 'cleaning', 'done'
    )),
    priority INTEGER NOT NULL DEFAULT 0,
    execution_version INTEGER NOT NULL DEFAULT 1,
    state_version INTEGER NOT NULL DEFAULT 0,
    attempt INTEGER NOT NULL DEFAULT 0,
    total_units INTEGER NOT NULL DEFAULT 0,
    completed_units INTEGER NOT NULL DEFAULT 0,
    estimated_pages INTEGER NOT NULL DEFAULT 0,
    uploaded_bytes INTEGER NOT NULL DEFAULT 0,
    last_provider_update_at INTEGER,
    cancel_requested_at INTEGER,
    fallback_allowed INTEGER NOT NULL DEFAULT 0 CHECK (fallback_allowed IN (0, 1)),
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    heartbeat_at INTEGER,
    finished_at INTEGER,
    updated_at INTEGER NOT NULL,
    runtime_instance_id TEXT,
    lease_token TEXT,
    lease_owner TEXT,
    lease_expires_at INTEGER,
    last_event_sequence INTEGER NOT NULL DEFAULT 0,
    idempotency_key TEXT NOT NULL UNIQUE,
    CHECK ((batch_index IS NULL AND batch_total IS NULL)
        OR (batch_index IS NOT NULL AND batch_total IS NOT NULL AND batch_total > 0 AND batch_index >= 0 AND batch_index < batch_total)),
    CHECK (total_units >= 0 AND completed_units >= 0 AND completed_units <= total_units),
    CHECK (estimated_pages >= 0 AND uploaded_bytes >= 0),
    CHECK (attempt >= 0 AND execution_version > 0 AND state_version >= 0 AND last_event_sequence >= 0)
);

CREATE TABLE IF NOT EXISTS knowledge_index_events (
    event_id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES knowledge_index_jobs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    from_state TEXT,
    to_state TEXT,
    execution_version INTEGER NOT NULL,
    state_version INTEGER NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{{}}',
    command_id TEXT,
    runtime_instance_id TEXT,
    created_at INTEGER NOT NULL,
    UNIQUE(job_id, sequence),
    CHECK (sequence >= 0 AND execution_version > 0 AND state_version >= 0),
    CHECK (length(trim(event_type)) > 0)
);

CREATE TABLE IF NOT EXISTS knowledge_fts_source (
    rowid INTEGER PRIMARY KEY,
    chunk_id TEXT NOT NULL UNIQUE REFERENCES knowledge_chunks(id) ON DELETE CASCADE,
    revision_id TEXT NOT NULL REFERENCES knowledge_revisions(id) ON DELETE CASCADE,
    document_id TEXT NOT NULL REFERENCES knowledge_documents(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    heading_path TEXT NOT NULL DEFAULT '',
    element_types TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    multimodal_text TEXT NOT NULL DEFAULT '',
    search_text TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS knowledge_documents_source
    ON knowledge_documents(source_class, source_kind, source_id);
CREATE INDEX IF NOT EXISTS knowledge_documents_state
    ON knowledge_documents(state, include_in_default_scope, updated_at DESC);

-- Extraction jobs created by the v19 queue are eligible for the explicitly
-- configured local text fallback.  This backfill only repairs rows from the
-- short-lived v19 schema whose default was zero; it never grants cloud
-- consent and therefore cannot authorize an upload.
UPDATE knowledge_index_jobs
SET fallback_allowed = 1
WHERE job_kind = 'extract' AND fallback_allowed = 0;
CREATE INDEX IF NOT EXISTS knowledge_revisions_document
    ON knowledge_revisions(document_id, created_at DESC);
CREATE INDEX IF NOT EXISTS knowledge_revisions_status
    ON knowledge_revisions(status, updated_at DESC);
CREATE INDEX IF NOT EXISTS knowledge_elements_revision_order
    ON knowledge_elements(revision_id, reading_order, ordinal);
CREATE INDEX IF NOT EXISTS knowledge_elements_page
    ON knowledge_elements(revision_id, page_index, reading_order);
CREATE INDEX IF NOT EXISTS knowledge_elements_type
    ON knowledge_elements(revision_id, element_type);
CREATE INDEX IF NOT EXISTS knowledge_assets_revision
    ON knowledge_assets(revision_id, page_index, asset_kind);
CREATE INDEX IF NOT EXISTS knowledge_assets_hash
    ON knowledge_assets(sha256);
CREATE INDEX IF NOT EXISTS knowledge_element_assets_asset
    ON knowledge_element_assets(asset_id);
CREATE INDEX IF NOT EXISTS knowledge_pdf_batches_active
    ON knowledge_pdf_batches(status, updated_at);
CREATE INDEX IF NOT EXISTS knowledge_pdf_batches_group
    ON knowledge_pdf_batches(batch_group_id, batch_index);
CREATE INDEX IF NOT EXISTS knowledge_asset_analyses_asset
    ON knowledge_asset_analyses(asset_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS knowledge_chunks_revision
    ON knowledge_chunks(revision_id, ordinal);
CREATE INDEX IF NOT EXISTS knowledge_chunks_content_hash
    ON knowledge_chunks(content_hash);
CREATE INDEX IF NOT EXISTS knowledge_chunks_markdown_line
    ON knowledge_chunks(line_start, line_end);
CREATE INDEX IF NOT EXISTS knowledge_jobs_active
    ON knowledge_index_jobs(state, priority DESC, created_at);
CREATE INDEX IF NOT EXISTS knowledge_jobs_document
    ON knowledge_index_jobs(document_id, created_at DESC);
CREATE INDEX IF NOT EXISTS knowledge_index_events_job
    ON knowledge_index_events(job_id, sequence);
CREATE UNIQUE INDEX IF NOT EXISTS knowledge_index_events_command
    ON knowledge_index_events(job_id, command_id) WHERE command_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS knowledge_consents_document
    ON knowledge_cloud_consents(document_id, granted_at DESC);
CREATE INDEX IF NOT EXISTS knowledge_consents_scope
    ON knowledge_cloud_consents(scope_key, scope, granted_at DESC);

CREATE TRIGGER IF NOT EXISTS knowledge_documents_source_whitelist_insert
BEFORE INSERT ON knowledge_documents
BEGIN
    SELECT CASE
        WHEN NEW.source_class NOT IN ('literature', 'note')
            THEN RAISE(ABORT, 'KNOWLEDGE_SOURCE_CLASS_NOT_ALLOWED')
        WHEN NEW.source_class = 'literature'
             AND (NEW.source_kind <> 'pdf' OR NEW.library_item_id IS NULL OR NEW.note_id IS NOT NULL
                  OR NOT EXISTS (SELECT 1 FROM library_items i WHERE i.id = NEW.library_item_id AND i.item_type = 'pdf'))
            THEN RAISE(ABORT, 'KNOWLEDGE_LITERATURE_SOURCE_MISMATCH')
        WHEN NEW.source_class = 'note'
             AND (NEW.source_kind <> 'markdown_note' OR NEW.note_id IS NULL OR NEW.library_item_id IS NOT NULL
                  OR NOT EXISTS (SELECT 1 FROM library_notes n WHERE n.id = NEW.note_id))
            THEN RAISE(ABORT, 'KNOWLEDGE_NOTE_SOURCE_MISMATCH')
        WHEN NEW.source_class = 'note' AND NEW.cloud_consent_state <> 'not_required'
            THEN RAISE(ABORT, 'KNOWLEDGE_NOTE_CLOUD_CONSENT_INVALID')
    END;
END;

CREATE TRIGGER IF NOT EXISTS knowledge_documents_source_whitelist_update
BEFORE UPDATE OF source_class, source_kind, library_item_id, note_id, cloud_consent_state
ON knowledge_documents
BEGIN
    SELECT CASE
        WHEN NEW.source_class NOT IN ('literature', 'note')
            THEN RAISE(ABORT, 'KNOWLEDGE_SOURCE_CLASS_NOT_ALLOWED')
        WHEN NEW.source_class = 'literature'
             AND (NEW.source_kind <> 'pdf' OR NEW.library_item_id IS NULL OR NEW.note_id IS NOT NULL
                  OR NOT EXISTS (SELECT 1 FROM library_items i WHERE i.id = NEW.library_item_id AND i.item_type = 'pdf'))
            THEN RAISE(ABORT, 'KNOWLEDGE_LITERATURE_SOURCE_MISMATCH')
        WHEN NEW.source_class = 'note'
             AND (NEW.source_kind <> 'markdown_note' OR NEW.note_id IS NULL OR NEW.library_item_id IS NOT NULL
                  OR NOT EXISTS (SELECT 1 FROM library_notes n WHERE n.id = NEW.note_id))
            THEN RAISE(ABORT, 'KNOWLEDGE_NOTE_SOURCE_MISMATCH')
        WHEN NEW.source_class = 'note' AND NEW.cloud_consent_state <> 'not_required'
            THEN RAISE(ABORT, 'KNOWLEDGE_NOTE_CLOUD_CONSENT_INVALID')
    END;
END;

INSERT INTO knowledge_index_capabilities(id, fts5_available, tokenizer, lexical_degraded, checked_at)
VALUES (1, {fts}, '{tokenizer}', {degraded}, CAST(strftime('%s','now') AS INTEGER) * 1000)
ON CONFLICT(id) DO UPDATE SET
    fts5_available = excluded.fts5_available,
    tokenizer = excluded.tokenizer,
    lexical_degraded = excluded.lexical_degraded,
    checked_at = excluded.checked_at;

{fts_ddl}

PRAGMA user_version = {version};
COMMIT;
"#,
        fts = if fts_available { 1 } else { 0 },
        degraded = if tokenizer == "trigram" { 0 } else { 1 },
        version = KNOWLEDGE_SCHEMA_V19_VERSION,
        fts_ddl = fts_ddl,
    );

    connection.execute_batch(&ddl).map_err(|error| {
        let _ = connection.execute_batch("ROLLBACK;");
        format!("知识库 v19 迁移失败：{error}")
    })?;
    ensure_v19_element_character_offsets(connection)
}

/// v20 only adds rebuildable embedding rows.  It deliberately does not start
/// network requests or backfill vectors during migration: indexing remains an
/// explicit, cancellable background operation after startup.
pub fn migrate_v20(connection: &Connection) -> Result<(), String> {
    let ddl = format!(
        r#"
BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS knowledge_embeddings (
    chunk_id TEXT NOT NULL REFERENCES knowledge_chunks(id) ON DELETE CASCADE,
    embedding_key TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    model_revision TEXT NOT NULL DEFAULT '',
    dimensions INTEGER NOT NULL,
    normalization TEXT NOT NULL CHECK (normalization IN ('none', 'l2')),
    content_hash TEXT NOT NULL,
    vector_blob BLOB,
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'ready', 'failed', 'cancelled', 'stale')
    ),
    error_code TEXT,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(chunk_id, embedding_key),
    CHECK (length(trim(embedding_key)) > 0),
    CHECK (length(trim(provider_id)) > 0),
    CHECK (length(trim(model_id)) > 0),
    CHECK (length(trim(content_hash)) > 0),
    CHECK (dimensions > 0 AND dimensions <= 65536),
    CHECK (retry_count >= 0),
    CHECK (updated_at >= created_at),
    CHECK (
        (status = 'ready'
            AND vector_blob IS NOT NULL
            AND length(vector_blob) = dimensions * 4)
        OR (status <> 'ready' AND vector_blob IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS knowledge_embeddings_lookup
    ON knowledge_embeddings(embedding_key, status, content_hash);
CREATE INDEX IF NOT EXISTS knowledge_embeddings_status
    ON knowledge_embeddings(status, updated_at DESC);

PRAGMA user_version = {version};
COMMIT;
"#,
        version = KNOWLEDGE_SCHEMA_VERSION,
    );

    connection.execute_batch(&ddl).map_err(|error| {
        let _ = connection.execute_batch("ROLLBACK;");
        format!("知识库 v20 embedding 迁移失败：{error}")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_v19_element_character_offsets, migrate_v19, migrate_v20,
        KNOWLEDGE_SCHEMA_V19_VERSION, KNOWLEDGE_SCHEMA_VERSION,
    };

    fn has_column(connection: &rusqlite::Connection, name: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM pragma_table_info('knowledge_elements') WHERE name = ?
                 )",
                [name],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn creates_knowledge_schema_and_is_idempotent() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_items (id TEXT PRIMARY KEY, item_type TEXT NOT NULL);\n\
                 CREATE TABLE library_notes (id TEXT PRIMARY KEY);",
            )
            .unwrap();
        migrate_v19(&connection).unwrap();
        migrate_v19(&connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, KNOWLEDGE_SCHEMA_V19_VERSION);
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('knowledge_documents', 'knowledge_chunks', 'knowledge_fts_source')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
        assert!(has_column(&connection, "char_start"));
        assert!(has_column(&connection, "char_end"));
    }

    #[test]
    fn creates_v20_embedding_schema_and_is_idempotent() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_items (id TEXT PRIMARY KEY, item_type TEXT NOT NULL);\n\
                 CREATE TABLE library_notes (id TEXT PRIMARY KEY);",
            )
            .unwrap();
        migrate_v19(&connection).unwrap();
        migrate_v20(&connection).unwrap();
        migrate_v20(&connection).unwrap();

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, KNOWLEDGE_SCHEMA_VERSION);
        let table_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'knowledge_embeddings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1);
    }

    #[test]
    fn repairs_legacy_v19_element_offsets_idempotently() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE knowledge_elements (
                    id TEXT PRIMARY KEY,
                    char_placeholder INTEGER
                 );
                 PRAGMA user_version = 19;",
            )
            .unwrap();

        ensure_v19_element_character_offsets(&connection).unwrap();
        ensure_v19_element_character_offsets(&connection).unwrap();

        assert!(has_column(&connection, "char_start"));
        assert!(has_column(&connection, "char_end"));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 19);

        connection
            .execute(
                "INSERT INTO knowledge_elements (id, char_start, char_end)
                 VALUES ('valid', 2, 8)",
                [],
            )
            .unwrap();
        let error = connection
            .execute(
                "INSERT INTO knowledge_elements (id, char_start, char_end)
                 VALUES ('invalid', 8, 2)",
                [],
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("KNOWLEDGE_ELEMENT_CHAR_RANGE_INVALID"));
    }

    #[test]
    fn whitelist_rejects_non_library_sources() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_items (id TEXT PRIMARY KEY, item_type TEXT NOT NULL);\n\
                 CREATE TABLE library_notes (id TEXT PRIMARY KEY);",
            )
            .unwrap();
        migrate_v19(&connection).unwrap();
        let error = connection
            .execute(
                "INSERT INTO knowledge_documents (id, source_class, source_kind, source_id, title, state, current_source_hash, created_at, updated_at) VALUES ('x', 'chat', 'message', 'm', '', 'pending', '', 1, 1)",
                [],
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("KNOWLEDGE_SOURCE_CLASS_NOT_ALLOWED"));
    }
}
