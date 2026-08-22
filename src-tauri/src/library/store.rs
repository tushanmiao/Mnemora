//! SQLite 文献库仓库。
//!
//! 每个方法短暂打开连接并立即关闭；写操作由 Tauri `AppState::library_operations`
//! 串行化。数据库不保存 PDF 二进制，只保存应用内快照文件名和校验信息。

use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Row};
use uuid::Uuid;

use crate::chat::note_pipeline::types::{
    DeepNoteEvidenceArtifact, DeepNoteEvidenceStatus, DeepNoteInputSnapshot, DeepNoteLedger,
    DeepNoteSourceChunk, DeepNoteSourceKind, DeepNoteSourceUnit, DeepNoteSourceUnitKind,
    DeepNoteSourceUnitStatus, DeepNoteSupportLevel,
};

use super::{
    import::{import_pdf, ImportOutcome},
    types::{
        normalize_collection_name, normalize_identifier, normalize_note_group_name,
        LibraryAnnotation, LibraryAnnotationColor, LibraryAnnotationCreate, LibraryAnnotationKind,
        LibraryAnnotationRect, LibraryAnnotationUpdate, LibraryCollection, LibraryImportFailure,
        LibraryImportResult, LibraryItem, LibraryItemUpdate, LibraryListPage, LibraryListRequest,
        LibraryNote, LibraryNoteCreate, LibraryNoteGroup, LibraryNoteImportFailure,
        LibraryNoteImportResult, LibraryNoteRename, LibraryNoteSummary, LibraryNoteUpdate,
        LibraryReadingState, LibraryReadingStateUpdate, LibrarySort, LibraryView, NoteEditProposal,
        NoteEditProposalCreate, NotePipelinePhase, NotePipelineRun, NotePipelineRunCreate,
        NotePipelineSection, NotePipelineSectionCreate, NotePipelineSectionStatus, NoteSource,
        NoteSourceCreate, NoteSourceOrigin, MAX_NOTE_IMPORT_BYTES, MAX_NOTE_IMPORT_FILES,
        MAX_NOTE_PIPELINE_JSON_BYTES, MAX_NOTE_PIPELINE_SECTIONS, MAX_NOTE_SOURCES,
        MAX_PDF_RANGE_BYTES,
    },
};

const LIBRARY_SCHEMA_VERSION: i64 = 10;
const LIBRARY_DIRECTORY_NAME: &str = "library";
const LIBRARY_DATABASE_NAME: &str = "library.sqlite3";
const LIBRARY_FILES_DIRECTORY_NAME: &str = "files";

const ITEM_COLUMNS: &str = "
    i.id,
    i.title,
    i.authors_json,
    i.publication_year,
    i.publication_title,
    i.doi,
    i.abstract_text,
    i.favorite,
    i.created_at,
    i.updated_at,
    i.last_opened_at,
    i.deleted_at,
    f.id,
    f.original_name,
    f.stored_name,
    f.file_size,
    f.file_hash,
    f.mime_type,
    f.created_at
";

#[derive(Clone)]
pub struct LibraryRepository {
    pub(crate) root_directory: PathBuf,
    pub(crate) database_path: PathBuf,
    pub(crate) files_directory: PathBuf,
}

struct RawLibraryItem {
    id: String,
    title: String,
    authors_json: String,
    publication_year: Option<i32>,
    publication_title: String,
    doi: String,
    abstract_text: String,
    favorite: bool,
    created_at: i64,
    updated_at: i64,
    last_opened_at: Option<i64>,
    deleted_at: Option<i64>,
    file_id: String,
    original_name: String,
    stored_name: String,
    file_size: i64,
    file_hash: String,
    mime_type: String,
    file_created_at: i64,
}

impl LibraryRepository {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let root_directory = app_data_dir.join(LIBRARY_DIRECTORY_NAME);
        Self {
            database_path: root_directory.join(LIBRARY_DATABASE_NAME),
            files_directory: root_directory.join(LIBRARY_FILES_DIRECTORY_NAME),
            root_directory,
        }
    }

    pub fn list_items(&self, request: LibraryListRequest) -> Result<LibraryListPage, String> {
        let request = request.normalize_and_validate()?;
        let connection = self.open_connection()?;
        let (where_clause, query_values) = build_item_filters(&request);
        let count_sql = format!("SELECT COUNT(*) FROM library_items i WHERE {where_clause}");
        let total: i64 = connection
            .query_row(&count_sql, params_from_iter(query_values.iter()), |row| {
                row.get(0)
            })
            .map_err(|error| format!("读取文献总数失败：{error}"))?;

        let order_by = item_order_by(request.sort, request.view);
        let list_sql = format!(
            "SELECT {ITEM_COLUMNS}
             FROM library_items i
             JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
             WHERE {where_clause}
             ORDER BY {order_by}
             LIMIT ? OFFSET ?"
        );
        let mut list_values = query_values;
        list_values.push(Value::Integer(request.limit as i64));
        list_values.push(Value::Integer(request.offset as i64));
        let mut statement = connection
            .prepare(&list_sql)
            .map_err(|error| format!("准备文献列表查询失败：{error}"))?;
        let rows = statement
            .query_map(params_from_iter(list_values.iter()), raw_item_from_row)
            .map_err(|error| format!("查询文献列表失败：{error}"))?;
        let mut raw_items = Vec::new();
        for row in rows {
            raw_items.push(row.map_err(|error| format!("读取文献记录失败：{error}"))?);
        }
        drop(statement);

        let mut items = Vec::with_capacity(raw_items.len());
        for raw in raw_items {
            items.push(self.hydrate_item(&connection, raw)?);
        }
        let total = usize::try_from(total).unwrap_or(usize::MAX);
        let has_more = request.offset.saturating_add(items.len()) < total;
        Ok(LibraryListPage {
            items,
            offset: request.offset,
            total,
            has_more,
        })
    }

    pub fn get_item(&self, item_id: &str) -> Result<LibraryItem, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        self.get_item_with_connection(&connection, &item_id)?
            .ok_or_else(|| "文献不存在。".to_string())
    }

    pub fn import_pdfs(
        &self,
        paths: Vec<String>,
        collection_id: Option<String>,
    ) -> Result<LibraryImportResult, String> {
        if paths.is_empty() {
            return Err("没有选择需要导入的 PDF。".to_string());
        }
        if paths.len() > 100 {
            return Err("单次最多导入 100 个 PDF。".to_string());
        }
        let collection_id = collection_id
            .as_deref()
            .map(|value| normalize_identifier("分类 ID", value))
            .transpose()?;
        let mut result = LibraryImportResult {
            imported: Vec::new(),
            duplicates: Vec::new(),
            failed: Vec::new(),
        };
        for path in paths {
            match import_pdf(self, &path, collection_id.as_deref()) {
                Ok(ImportOutcome::Imported(item)) => result.imported.push(item),
                Ok(ImportOutcome::Duplicate(item)) => result.duplicates.push(item),
                Err(error) => {
                    let source = Path::new(&path);
                    result.failed.push(LibraryImportFailure {
                        file_name: source
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.clone()),
                        path,
                        error,
                    });
                }
            }
        }
        Ok(result)
    }

    pub fn update_item(&self, update: LibraryItemUpdate) -> Result<LibraryItem, String> {
        let update = update.normalize_and_validate()?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始文献更新事务失败：{error}"))?;
        ensure_item_exists(&transaction, &update.item_id)?;
        ensure_collections_exist(&transaction, &update.collection_ids)?;
        let now = now_millis_i64();
        let authors_json = serde_json::to_string(&update.authors)
            .map_err(|error| format!("序列化作者信息失败：{error}"))?;
        transaction
            .execute(
                "UPDATE library_items
                 SET title = ?, authors_json = ?, publication_year = ?, publication_title = ?,
                     doi = ?, abstract_text = ?, favorite = ?, updated_at = ?
                 WHERE id = ?",
                params![
                    update.title,
                    authors_json,
                    update.publication_year,
                    update.publication_title,
                    update.doi,
                    update.abstract_text,
                    bool_to_i64(update.favorite),
                    now,
                    update.item_id,
                ],
            )
            .map_err(|error| format!("更新文献元数据失败：{error}"))?;
        replace_item_collections(&transaction, &update.item_id, &update.collection_ids)?;
        replace_item_tags(&transaction, &update.item_id, &update.tags, now)?;
        transaction
            .commit()
            .map_err(|error| format!("保存文献更新失败：{error}"))?;
        self.get_item_with_connection(&connection, &update.item_id)?
            .ok_or_else(|| "更新后的文献不存在。".to_string())
    }

    pub fn set_favorite(&self, item_id: &str, favorite: bool) -> Result<LibraryItem, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE library_items SET favorite = ?, updated_at = ? WHERE id = ?",
                params![bool_to_i64(favorite), now_millis_i64(), item_id],
            )
            .map_err(|error| format!("更新收藏状态失败：{error}"))?;
        if changed == 0 {
            return Err("文献不存在。".to_string());
        }
        self.get_item_with_connection(&connection, &item_id)?
            .ok_or_else(|| "文献不存在。".to_string())
    }

    pub fn move_to_trash(&self, item_id: &str) -> Result<LibraryItem, String> {
        self.set_deleted_at(item_id, Some(now_millis_i64()))
    }

    pub fn restore_from_trash(&self, item_id: &str) -> Result<LibraryItem, String> {
        self.set_deleted_at(item_id, None)
    }

    pub fn delete_permanently(&self, item_id: &str) -> Result<bool, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let mut connection = self.open_connection()?;
        let stored_names = {
            let mut statement = connection
                .prepare(
                    "SELECT f.stored_name
                     FROM library_files f
                     JOIN library_items i ON i.id = f.item_id
                     WHERE f.item_id = ? AND i.deleted_at IS NOT NULL",
                )
                .map_err(|error| format!("准备文献文件查询失败：{error}"))?;
            let rows = statement
                .query_map(params![item_id], |row| row.get::<_, String>(0))
                .map_err(|error| format!("查询文献文件失败：{error}"))?;
            let mut names = Vec::new();
            for row in rows {
                names.push(row.map_err(|error| format!("读取文献文件失败：{error}"))?);
            }
            names
        };
        if stored_names.is_empty() {
            return Ok(false);
        }
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始永久删除事务失败：{error}"))?;
        let removed = transaction
            .execute(
                "DELETE FROM library_items WHERE id = ? AND deleted_at IS NOT NULL",
                params![item_id],
            )
            .map_err(|error| format!("永久删除文献记录失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("提交永久删除失败：{error}"))?;
        for stored_name in stored_names {
            let path = self.resolve_stored_file_name(&stored_name)?;
            if path.exists() {
                fs::remove_file(&path)
                    .map_err(|error| format!("文献记录已删除，但清理 PDF 快照失败：{error}"))?;
            }
        }
        Ok(removed > 0)
    }

    pub fn mark_opened(&self, item_id: &str) -> Result<LibraryItem, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        let now = now_millis_i64();
        let changed = connection
            .execute(
                "UPDATE library_items SET last_opened_at = ?, updated_at = ?
                 WHERE id = ? AND deleted_at IS NULL",
                params![now, now, item_id],
            )
            .map_err(|error| format!("更新最近阅读时间失败：{error}"))?;
        if changed == 0 {
            return Err("文献不存在或位于回收站。".to_string());
        }
        self.get_item_with_connection(&connection, &item_id)?
            .ok_or_else(|| "文献不存在。".to_string())
    }

    pub fn primary_file_path(&self, item_id: &str) -> Result<PathBuf, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        let stored_name = connection
            .query_row(
                "SELECT f.stored_name
                 FROM library_files f
                 JOIN library_items i ON i.id = f.item_id
                 WHERE f.item_id = ? AND f.is_primary = 1 AND i.deleted_at IS NULL",
                params![item_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取文献文件路径失败：{error}"))?
            .ok_or_else(|| "文献文件不存在或位于回收站。".to_string())?;
        let path = self.resolve_stored_file_name(&stored_name)?;
        if !path.is_file() {
            return Err("PDF 快照文件不存在。".to_string());
        }
        Ok(path)
    }

    pub fn read_pdf_range(&self, item_id: &str, start: u64, end: u64) -> Result<Vec<u8>, String> {
        if end <= start {
            return Err("PDF 数据区间无效。".to_string());
        }
        if end.saturating_sub(start) > MAX_PDF_RANGE_BYTES {
            return Err("单次 PDF 数据读取不能超过 1 MB。".to_string());
        }
        let path = self.primary_file_path(item_id)?;
        let mut file =
            fs::File::open(&path).map_err(|error| format!("打开 PDF 快照失败：{error}"))?;
        let file_length = file
            .metadata()
            .map_err(|error| format!("读取 PDF 快照大小失败：{error}"))?
            .len();
        if start >= file_length {
            return Err("PDF 数据起始位置超出文件范围。".to_string());
        }
        let end = end.min(file_length);
        let length = usize::try_from(end - start).map_err(|_| "PDF 数据区间过大。".to_string())?;
        file.seek(SeekFrom::Start(start))
            .map_err(|error| format!("定位 PDF 数据失败：{error}"))?;
        let mut bytes = vec![0u8; length];
        file.read_exact(&mut bytes)
            .map_err(|error| format!("读取 PDF 数据失败：{error}"))?;
        Ok(bytes)
    }

    pub fn get_reading_state(&self, item_id: &str) -> Result<LibraryReadingState, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        ensure_active_item_exists(&connection, &item_id)?;
        let state = connection
            .query_row(
                "SELECT page_index, scroll_offset, zoom, updated_at
                 FROM library_reading_state WHERE item_id = ?",
                params![item_id],
                |row| {
                    Ok(LibraryReadingState {
                        item_id: item_id.clone(),
                        page_index: row.get::<_, i64>(0)?.try_into().unwrap_or(0),
                        scroll_offset: row.get(1)?,
                        zoom: row.get(2)?,
                        updated_at: i64_to_u64(row.get(3)?),
                    })
                },
            )
            .optional()
            .map_err(|error| format!("读取 PDF 阅读位置失败：{error}"))?;
        Ok(state.unwrap_or(LibraryReadingState {
            item_id,
            page_index: 0,
            scroll_offset: 0.0,
            zoom: 1.0,
            updated_at: 0,
        }))
    }

    pub fn save_reading_state(
        &self,
        update: LibraryReadingStateUpdate,
    ) -> Result<LibraryReadingState, String> {
        let update = update.normalize_and_validate()?;
        let connection = self.open_connection()?;
        ensure_active_item_exists(&connection, &update.item_id)?;
        let now = now_millis_i64();
        connection
            .execute(
                "INSERT INTO library_reading_state (
                    item_id, page_index, scroll_offset, zoom, updated_at
                 ) VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(item_id) DO UPDATE SET
                    page_index = excluded.page_index,
                    scroll_offset = excluded.scroll_offset,
                    zoom = excluded.zoom,
                    updated_at = excluded.updated_at",
                params![
                    update.item_id,
                    i64::from(update.page_index),
                    update.scroll_offset,
                    update.zoom,
                    now,
                ],
            )
            .map_err(|error| format!("保存 PDF 阅读位置失败：{error}"))?;
        Ok(LibraryReadingState {
            item_id: update.item_id,
            page_index: update.page_index,
            scroll_offset: update.scroll_offset,
            zoom: update.zoom,
            updated_at: i64_to_u64(now),
        })
    }

    pub fn list_annotations(&self, item_id: &str) -> Result<Vec<LibraryAnnotation>, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        ensure_active_item_exists(&connection, &item_id)?;
        let mut statement = connection
            .prepare(
                "SELECT id, item_id, kind, page_index, color, text, comment, rects_json,
                        created_at, updated_at
                 FROM library_annotations
                 WHERE item_id = ?
                 ORDER BY page_index ASC, created_at ASC",
            )
            .map_err(|error| format!("准备批注列表查询失败：{error}"))?;
        let rows = statement
            .query_map(params![item_id], annotation_from_row)
            .map_err(|error| format!("查询批注列表失败：{error}"))?;
        let mut annotations = Vec::new();
        for row in rows {
            annotations.push(row.map_err(|error| format!("读取批注记录失败：{error}"))??);
        }
        Ok(annotations)
    }

    pub fn create_annotation(
        &self,
        create: LibraryAnnotationCreate,
    ) -> Result<LibraryAnnotation, String> {
        let create = create.normalize_and_validate()?;
        let connection = self.open_connection()?;
        ensure_active_item_exists(&connection, &create.item_id)?;
        let id = Uuid::new_v4().to_string();
        let now = now_millis_i64();
        let rects_json = serde_json::to_string(&create.rects)
            .map_err(|error| format!("序列化批注区域失败：{error}"))?;
        connection
            .execute(
                "INSERT INTO library_annotations (
                    id, item_id, kind, page_index, color, text, comment, rects_json,
                    created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    id,
                    create.item_id,
                    create.kind.as_str(),
                    i64::from(create.page_index),
                    create.color.as_str(),
                    create.text,
                    create.comment,
                    rects_json,
                    now,
                    now,
                ],
            )
            .map_err(|error| format!("创建 PDF 批注失败：{error}"))?;
        self.get_annotation_with_connection(&connection, &id)?
            .ok_or_else(|| "创建后的批注不存在。".to_string())
    }

    pub fn update_annotation(
        &self,
        update: LibraryAnnotationUpdate,
    ) -> Result<LibraryAnnotation, String> {
        let update = update.normalize_and_validate()?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE library_annotations
                 SET color = ?, comment = ?, updated_at = ?
                 WHERE id = ? AND EXISTS (
                    SELECT 1 FROM library_items i
                    WHERE i.id = library_annotations.item_id AND i.deleted_at IS NULL
                 )",
                params![
                    update.color.as_str(),
                    update.comment,
                    now_millis_i64(),
                    update.annotation_id,
                ],
            )
            .map_err(|error| format!("更新 PDF 批注失败：{error}"))?;
        if changed == 0 {
            return Err("批注不存在或所属文献位于回收站。".to_string());
        }
        self.get_annotation_with_connection(&connection, &update.annotation_id)?
            .ok_or_else(|| "更新后的批注不存在。".to_string())
    }

    pub fn delete_annotation(&self, annotation_id: &str) -> Result<bool, String> {
        let annotation_id = normalize_identifier("批注 ID", annotation_id)?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "DELETE FROM library_annotations
                 WHERE id = ? AND EXISTS (
                    SELECT 1 FROM library_items i
                    WHERE i.id = library_annotations.item_id AND i.deleted_at IS NULL
                 )",
                params![annotation_id],
            )
            .map_err(|error| format!("删除 PDF 批注失败：{error}"))?;
        Ok(changed > 0)
    }

    pub fn list_notes(&self, item_id: Option<&str>) -> Result<Vec<LibraryNoteSummary>, String> {
        let item_id = item_id
            .map(|value| normalize_identifier("文献 ID", value))
            .transpose()?;
        let connection = self.open_connection()?;
        if let Some(item_id) = item_id.as_deref() {
            ensure_active_item_exists(&connection, item_id)?;
        }
        let (sql, values) = if let Some(item_id) = item_id {
            (
                "SELECT n.id, n.item_id, i.title, n.title, substr(n.content, 1, 600),
                        length(n.content), n.group_name, n.created_at, n.updated_at,
                        length(CAST(n.content AS BLOB))
                 FROM library_notes n
                 LEFT JOIN library_items i ON i.id = n.item_id
                 WHERE n.item_id = ? AND i.deleted_at IS NULL
                 ORDER BY n.updated_at DESC",
                vec![Value::Text(item_id)],
            )
        } else {
            (
                "SELECT n.id, n.item_id, i.title, n.title, substr(n.content, 1, 600),
                        length(n.content), n.group_name, n.created_at, n.updated_at,
                        length(CAST(n.content AS BLOB))
                 FROM library_notes n
                 LEFT JOIN library_items i ON i.id = n.item_id
                 WHERE n.item_id IS NULL OR i.deleted_at IS NULL
                 ORDER BY n.updated_at DESC",
                Vec::new(),
            )
        };
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| format!("准备笔记列表查询失败：{error}"))?;
        let rows = statement
            .query_map(params_from_iter(values.iter()), note_summary_from_row)
            .map_err(|error| format!("查询笔记列表失败：{error}"))?;
        let mut notes = Vec::new();
        for row in rows {
            notes.push(row.map_err(|error| format!("读取笔记记录失败：{error}"))?);
        }
        Ok(notes)
    }

    /// 同步等批处理只读取 ID，避免为每篇笔记预取正文预览。
    pub fn list_note_ids(&self) -> Result<Vec<String>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT n.id
                 FROM library_notes n
                 LEFT JOIN library_items i ON i.id = n.item_id
                 WHERE n.item_id IS NULL OR i.deleted_at IS NULL
                 ORDER BY n.updated_at DESC",
            )
            .map_err(|error| format!("准备笔记 ID 查询失败：{error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("查询笔记 ID 失败：{error}"))?;
        let mut note_ids = Vec::new();
        for row in rows {
            note_ids.push(row.map_err(|error| format!("读取笔记 ID 失败：{error}"))?);
        }
        Ok(note_ids)
    }

    /// Agent 只读工具使用的有界笔记目录。正文仍由 `get_note` 按需读取，
    /// 避免为了目录或搜索一次性把全部笔记内容载入内存。
    pub fn list_notes_page_for_agent(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<LibraryNoteSummary>, usize), String> {
        let query = query.trim();
        if query.chars().count() > 500 {
            return Err("笔记搜索内容过长。".to_string());
        }
        if !(1..=200).contains(&limit) || offset > 100_000 {
            return Err("笔记目录分页参数超出允许范围。".to_string());
        }
        let connection = self.open_connection()?;
        let pattern = format!("%{query}%");
        let filter = if query.is_empty() {
            "(n.item_id IS NULL OR i.deleted_at IS NULL)"
        } else {
            "(n.item_id IS NULL OR i.deleted_at IS NULL) AND (n.title LIKE ? OR n.content LIKE ?)"
        };
        let count_sql = format!(
            "SELECT COUNT(*) FROM library_notes n LEFT JOIN library_items i ON i.id = n.item_id WHERE {filter}"
        );
        let total: i64 = if query.is_empty() {
            connection.query_row(&count_sql, [], |row| row.get(0))
        } else {
            connection.query_row(&count_sql, params![pattern, pattern], |row| row.get(0))
        }
        .map_err(|error| format!("读取笔记总数失败：{error}"))?;
        let list_sql = format!(
            "SELECT n.id, n.item_id, i.title, n.title, substr(n.content, 1, 600),
                    length(n.content), n.group_name, n.created_at, n.updated_at,
                    length(CAST(n.content AS BLOB))
             FROM library_notes n
             LEFT JOIN library_items i ON i.id = n.item_id
             WHERE {filter}
             ORDER BY n.updated_at DESC, n.id ASC
             LIMIT ? OFFSET ?"
        );
        let mut statement = connection
            .prepare(&list_sql)
            .map_err(|error| format!("准备笔记目录查询失败：{error}"))?;
        let mut notes = Vec::new();
        if query.is_empty() {
            let rows = statement
                .query_map(params![limit as i64, offset as i64], note_summary_from_row)
                .map_err(|error| format!("查询笔记目录失败：{error}"))?;
            for row in rows {
                notes.push(row.map_err(|error| format!("读取笔记目录失败：{error}"))?);
            }
        } else {
            let rows = statement
                .query_map(
                    params![pattern, pattern, limit as i64, offset as i64],
                    note_summary_from_row,
                )
                .map_err(|error| format!("查询笔记目录失败：{error}"))?;
            for row in rows {
                notes.push(row.map_err(|error| format!("读取笔记目录失败：{error}"))?);
            }
        }
        Ok((notes, usize::try_from(total).unwrap_or(usize::MAX)))
    }

    pub fn get_note(&self, note_id: &str) -> Result<LibraryNote, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let connection = self.open_connection()?;
        self.get_note_with_connection(&connection, &note_id)?
            .ok_or_else(|| "笔记不存在或所属文献位于回收站。".to_string())
    }

    pub fn create_note(&self, create: LibraryNoteCreate) -> Result<LibraryNote, String> {
        let create = create.normalize_and_validate()?;
        let connection = self.open_connection()?;
        if let Some(item_id) = create.item_id.as_deref() {
            ensure_active_item_exists(&connection, item_id)?;
        }
        let id = Uuid::new_v4().to_string();
        let now = now_millis_i64();
        if let Some(group_name) = create.group_name.as_deref() {
            register_note_group(&connection, group_name, now)?;
        }
        connection
            .execute(
                "INSERT INTO library_notes (id, item_id, title, content, group_name, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, create.item_id, create.title, create.content, create.group_name, now, now],
            )
            .map_err(|error| format!("创建文献笔记失败：{error}"))?;
        self.get_note_with_connection(&connection, &id)?
            .ok_or_else(|| "创建后的笔记不存在。".to_string())
    }

    /// 原子创建笔记及其章节级来源。任一来源写入失败时整篇笔记回滚。
    pub fn create_note_with_sources(
        &self,
        create: LibraryNoteCreate,
        sources: Vec<NoteSourceCreate>,
    ) -> Result<LibraryNote, String> {
        let create = create.normalize_and_validate()?;
        let sources = normalize_note_sources(sources)?;
        let mut connection = self.open_connection()?;
        if let Some(item_id) = create.item_id.as_deref() {
            ensure_active_item_exists(&connection, item_id)?;
        }
        let id = Uuid::new_v4().to_string();
        let now = now_millis_i64();
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始创建深度笔记失败：{error}"))?;
        if let Some(group_name) = create.group_name.as_deref() {
            register_note_group(&transaction, group_name, now)?;
        }
        transaction
            .execute(
                "INSERT INTO library_notes (id, item_id, title, content, group_name, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, create.item_id, create.title, create.content, create.group_name, now, now],
            )
            .map_err(|error| format!("创建深度笔记失败：{error}"))?;
        insert_note_sources(&transaction, &id, sources, now)?;
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记失败：{error}"))?;
        self.get_note_with_connection(&connection, &id)?
            .ok_or_else(|| "创建后的深度笔记不存在。".to_string())
    }

    /// 原子创建深度笔记、章节来源和覆盖快照。覆盖快照保存逐消息与附件内容 Hash，
    /// 后续增量更新前必须先验证它，避免把已编辑、删除或重排的旧来源混入新笔记。
    pub fn create_note_with_sources_and_coverage(
        &self,
        create: LibraryNoteCreate,
        sources: Vec<NoteSourceCreate>,
        conversation_id: &str,
        snapshot: &DeepNoteInputSnapshot,
    ) -> Result<LibraryNote, String> {
        let create = create.normalize_and_validate()?;
        let sources = normalize_note_sources(sources)?;
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let snapshot_json = normalize_coverage_snapshot(snapshot)?;
        let mut connection = self.open_connection()?;
        if let Some(item_id) = create.item_id.as_deref() {
            ensure_active_item_exists(&connection, item_id)?;
        }
        let id = Uuid::new_v4().to_string();
        let now = now_millis_i64();
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始创建深度笔记失败：{error}"))?;
        if let Some(group_name) = create.group_name.as_deref() {
            register_note_group(&transaction, group_name, now)?;
        }
        transaction
            .execute(
                "INSERT INTO library_notes (id, item_id, title, content, group_name, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, create.item_id, create.title, create.content, create.group_name, now, now],
            )
            .map_err(|error| format!("创建深度笔记失败：{error}"))?;
        insert_note_sources(&transaction, &id, sources, now)?;
        upsert_deep_note_coverage_snapshot(
            &transaction,
            &id,
            &conversation_id,
            &snapshot_json,
            now,
        )?;
        let units = source_units_from_snapshot(&id, &conversation_id, snapshot, i64_to_u64(now));
        insert_deep_note_source_units(&transaction, &id, &conversation_id, &units)?;
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记失败：{error}"))?;
        self.get_note_with_connection(&connection, &id)?
            .ok_or_else(|| "创建后的深度笔记不存在。".to_string())
    }

    /// A full rebuild creates a new immutable note but makes it the latest
    /// update target for this conversation. Older notes keep their historical
    /// message-level citations; only the moving summarized-until anchor is
    /// cleared so future update inspection cannot select the stale generation.
    pub fn create_rebuilt_note_with_sources_and_coverage(
        &self,
        create: LibraryNoteCreate,
        sources: Vec<NoteSourceCreate>,
        conversation_id: &str,
        snapshot: &DeepNoteInputSnapshot,
    ) -> Result<LibraryNote, String> {
        let create = create.normalize_and_validate()?;
        let sources = normalize_note_sources(sources)?;
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let snapshot_json = normalize_coverage_snapshot(snapshot)?;
        let mut connection = self.open_connection()?;
        if let Some(item_id) = create.item_id.as_deref() {
            ensure_active_item_exists(&connection, item_id)?;
        }
        let id = Uuid::new_v4().to_string();
        let now = now_millis_i64();
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始重建深度笔记失败：{error}"))?;
        if let Some(group_name) = create.group_name.as_deref() {
            register_note_group(&transaction, group_name, now)?;
        }
        transaction
            .execute(
                "UPDATE note_sources SET summarized_until_message_id = NULL
                 WHERE conversation_id = ? AND summarized_until_message_id IS NOT NULL",
                params![conversation_id],
            )
            .map_err(|error| format!("切换深度笔记更新锚点失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO library_notes (id, item_id, title, content, group_name, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, create.item_id, create.title, create.content, create.group_name, now, now],
            )
            .map_err(|error| format!("创建重建深度笔记失败：{error}"))?;
        insert_note_sources(&transaction, &id, sources, now)?;
        upsert_deep_note_coverage_snapshot(
            &transaction,
            &id,
            &conversation_id,
            &snapshot_json,
            now,
        )?;
        let units = source_units_from_snapshot(&id, &conversation_id, snapshot, i64_to_u64(now));
        insert_deep_note_source_units(&transaction, &id, &conversation_id, &units)?;
        transaction
            .commit()
            .map_err(|error| format!("提交重建深度笔记失败：{error}"))?;
        self.get_note_with_connection(&connection, &id)?
            .ok_or_else(|| "创建后的重建深度笔记不存在。".to_string())
    }

    pub fn deep_note_coverage_snapshot(
        &self,
        note_id: &str,
        conversation_id: &str,
    ) -> Result<Option<DeepNoteInputSnapshot>, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let connection = self.open_connection()?;
        let snapshot_json = connection
            .query_row(
                "SELECT snapshot_json FROM deep_note_coverage_snapshots
                 WHERE note_id = ? AND conversation_id = ?",
                params![note_id, conversation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取深度笔记覆盖快照失败：{error}"))?;
        snapshot_json
            .map(|json| {
                serde_json::from_str::<DeepNoteInputSnapshot>(&json)
                    .map_err(|error| format!("解析深度笔记覆盖快照失败：{error}"))
            })
            .transpose()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_deep_note_source_units(
        &self,
        note_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<DeepNoteSourceUnit>, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT unit_id, message_id, kind, attachment_id, content_hash, parser_id,
                        parser_version, status, chunk_ids_json, evidence_ids_json,
                        error_message, created_at, updated_at
                 FROM deep_note_source_units
                 WHERE note_id = ? AND conversation_id = ?
                 ORDER BY created_at ASC, unit_id ASC",
            )
            .map_err(|error| format!("准备深度笔记来源单元查询失败：{error}"))?;
        let rows = statement
            .query_map(params![note_id, conversation_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                ))
            })
            .map_err(|error| format!("查询深度笔记来源单元失败：{error}"))?;
        rows.map(|row| {
            let raw = row.map_err(|error| format!("读取深度笔记来源单元失败：{error}"))?;
            Ok(DeepNoteSourceUnit {
                unit_id: raw.0,
                note_id: note_id.clone(),
                conversation_id: conversation_id.clone(),
                message_id: raw.1,
                kind: DeepNoteSourceUnitKind::parse(&raw.2)?,
                attachment_id: raw.3,
                content_hash: raw.4,
                parser_id: raw.5,
                parser_version: raw.6,
                status: DeepNoteSourceUnitStatus::parse(&raw.7)?,
                chunk_ids: serde_json::from_str(&raw.8)
                    .map_err(|error| format!("解析来源单元 Chunk 引用失败：{error}"))?,
                evidence_ids: serde_json::from_str(&raw.9)
                    .map_err(|error| format!("解析来源单元 Evidence 引用失败：{error}"))?,
                error_message: raw.10,
                created_at: i64_to_u64(raw.11),
                updated_at: i64_to_u64(raw.12),
            })
        })
        .collect()
    }

    pub fn list_note_sources(&self, note_id: &str) -> Result<Vec<NoteSource>, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let connection = self.open_connection()?;
        self.get_note_with_connection(&connection, &note_id)?
            .ok_or_else(|| "笔记不存在或所属文献位于回收站。".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT id, note_id, section_id, origin, conversation_id, message_id,
                        summarized_until_message_id, created_at
                 FROM note_sources
                 WHERE note_id = ?
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|error| format!("准备笔记来源查询失败：{error}"))?;
        let rows = statement
            .query_map(params![note_id], note_source_from_row)
            .map_err(|error| format!("查询笔记来源失败：{error}"))?;
        let mut sources = Vec::new();
        for row in rows {
            sources.push(row.map_err(|error| format!("读取笔记来源失败：{error}"))??);
        }
        Ok(sources)
    }

    /// 删除单个会话时只断开来源锚点，不删除来源记录或笔记正文。
    pub fn detach_note_sources_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<usize, String> {
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let connection = self.open_connection()?;
        connection
            .execute(
                "UPDATE note_sources
                 SET conversation_id = NULL,
                     message_id = NULL,
                     summarized_until_message_id = NULL
                 WHERE conversation_id = ?",
                params![conversation_id],
            )
            .map_err(|error| format!("断开笔记会话来源失败：{error}"))
    }

    /// 清空会话前断开全部会话来源；AI 补充来源不受影响。
    pub fn detach_all_note_conversation_sources(&self) -> Result<usize, String> {
        let connection = self.open_connection()?;
        connection
            .execute(
                "UPDATE note_sources
                 SET conversation_id = NULL,
                     message_id = NULL,
                     summarized_until_message_id = NULL
                 WHERE conversation_id IS NOT NULL",
                [],
            )
            .map_err(|error| format!("断开全部笔记会话来源失败：{error}"))
    }

    pub fn create_note_pipeline_run(
        &self,
        create: NotePipelineRunCreate,
    ) -> Result<NotePipelineRun, String> {
        let id = normalize_identifier("任务 ID", &create.id)?;
        let conversation_id = normalize_identifier("会话 ID", &create.conversation_id)?;
        let provider_id = normalize_identifier("供应商 ID", &create.provider_id)?;
        let model_id = normalize_identifier("模型 ID", &create.model_id)?;
        if !(256..=131_072).contains(&create.max_output_tokens) {
            return Err("深度笔记输出 Token 上限无效。".to_string());
        }
        if create.retry_attempts > 5 {
            return Err("深度笔记重试次数无效。".to_string());
        }
        let connection = self.open_connection()?;
        let now = now_millis_i64();
        let inserted = connection.execute(
            "INSERT INTO note_pipeline_runs (
                id, conversation_id, phase, outline_json, selected_section_ids_json,
                provider_id, model_id, max_output_tokens, thinking_enabled, retry_attempts,
                input_snapshot_hash, current_plan_version, execution_version,
                budget_json, preflight_json, sidecar_json, idempotency_key,
                warnings_json, created_at, updated_at
             ) VALUES (?, ?, 'preflight', '', '[]', ?, ?, ?, ?, ?, ?, 0, 1, ?, ?, '', ?, '[]', ?, ?)",
            params![
                id,
                conversation_id,
                provider_id,
                model_id,
                i64::from(create.max_output_tokens),
                bool_to_i64(create.thinking_enabled),
                i64::from(create.retry_attempts),
                create.input_snapshot_hash,
                create.budget_json,
                create.preflight_json,
                create.idempotency_key,
                now,
                now,
            ],
        );
        match inserted {
            Ok(_) => get_note_pipeline_run_with_connection(&connection, &id)?
                .ok_or_else(|| "创建后的深度笔记任务不存在。".to_string()),
            Err(error) if is_unique_constraint(&error) => {
                Err("该会话已有一个可恢复的深度笔记任务。".to_string())
            }
            Err(error) => Err(format!("创建深度笔记任务失败：{error}")),
        }
    }

    pub fn get_note_pipeline_run(&self, run_id: &str) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        get_note_pipeline_run_with_connection(&connection, &run_id)?
            .ok_or_else(|| "深度笔记任务不存在。".to_string())
    }

    pub fn request_note_pipeline_cancellation(
        &self,
        run_id: &str,
    ) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始记录深度笔记停止请求失败：{error}"))?;
        let phase: String = transaction
            .query_row(
                "SELECT phase FROM note_pipeline_runs WHERE id = ?",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取深度笔记停止状态失败：{error}"))?
            .ok_or_else(|| "深度笔记任务不存在。".to_string())?;
        if !matches!(phase.as_str(), "done" | "cancelled" | "cancelling") {
            let now = now_millis_i64();
            transaction
                .execute(
                    "UPDATE note_pipeline_runs
                     SET phase = 'cancelling', error_message = NULL, updated_at = ? WHERE id = ?",
                    params![now, run_id],
                )
                .map_err(|error| format!("记录深度笔记停止状态失败：{error}"))?;
            let sequence: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(sequence), 0) + 1 FROM note_pipeline_events WHERE run_id = ?",
                    params![run_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("读取深度笔记停止事件序号失败：{error}"))?;
            transaction
                .execute(
                    "INSERT INTO note_pipeline_events (
                        run_id, sequence, event_type, node_id, payload_json, created_at
                     ) VALUES (?, ?, 'runCancellationRequested', NULL, '{}', ?)",
                    params![run_id, sequence, now],
                )
                .map_err(|error| format!("保存深度笔记停止事件失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记停止请求失败：{error}"))?;
        self.get_note_pipeline_run(&run_id)
    }

    pub fn finalize_note_pipeline_cancellation(
        &self,
        run_id: &str,
        forced: bool,
        reason: &str,
        diagnostic_path: Option<&str>,
    ) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始完成深度笔记停止状态失败：{error}"))?;
        let phase: String = transaction
            .query_row(
                "SELECT phase FROM note_pipeline_runs WHERE id = ?",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取深度笔记最终停止状态失败：{error}"))?
            .ok_or_else(|| "深度笔记任务不存在。".to_string())?;
        if phase != "done" && phase != "cancelled" {
            let now = now_millis_i64();
            transaction
                .execute(
                    "UPDATE note_pipeline_runs
                     SET phase = 'cancelled', error_message = NULL, updated_at = ? WHERE id = ?",
                    params![now, run_id],
                )
                .map_err(|error| format!("完成深度笔记停止状态失败：{error}"))?;
            let sequence: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(sequence), 0) + 1 FROM note_pipeline_events WHERE run_id = ?",
                    params![run_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("读取深度笔记最终停止事件序号失败：{error}"))?;
            let payload = serde_json::json!({
                "forced": forced,
                "reason": reason,
                "diagnosticPath": diagnostic_path,
            })
            .to_string();
            transaction
                .execute(
                    "INSERT INTO note_pipeline_events (
                        run_id, sequence, event_type, node_id, payload_json, created_at
                     ) VALUES (?, ?, 'runCancelled', NULL, ?, ?)",
                    params![run_id, sequence, payload, now],
                )
                .map_err(|error| format!("保存深度笔记最终停止事件失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记最终停止状态失败：{error}"))?;
        self.get_note_pipeline_run(&run_id)
    }

    pub fn fail_note_pipeline_task(
        &self,
        run_id: &str,
        message: &str,
        diagnostic_path: &str,
    ) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let current = self.get_note_pipeline_run(&run_id)?;
        if matches!(
            current.phase,
            NotePipelinePhase::Done | NotePipelinePhase::Cancelled
        ) {
            return Ok(current);
        }
        let failed = self.update_note_pipeline_phase(
            &run_id,
            NotePipelinePhase::Error,
            None,
            &current.warnings,
            Some(message),
        )?;
        self.append_note_pipeline_event(
            &run_id,
            "runPanicked",
            None,
            &serde_json::json!({
                "message": message,
                "diagnosticPath": diagnostic_path,
            })
            .to_string(),
        )?;
        Ok(failed)
    }

    pub fn recover_stale_cancelling_runs(&self) -> Result<usize, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare("SELECT id FROM note_pipeline_runs WHERE phase = 'cancelling'")
            .map_err(|error| format!("准备恢复停止中任务失败：{error}"))?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("查询停止中任务失败：{error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取停止中任务失败：{error}"))?;
        drop(statement);
        drop(connection);
        for id in &ids {
            self.finalize_note_pipeline_cancellation(
                id,
                true,
                "application-restart-recovery",
                None,
            )?;
        }
        Ok(ids.len())
    }

    /// 将任务标记为不可恢复的“已遗弃”。保留事件与诊断记录，但后续恢复扫描、
    /// 重试和重新生成都不会再把它视为当前任务。
    pub fn abandon_note_pipeline_run(&self, run_id: &str) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("开始遗弃深度笔记任务失败：{error}"))?;
        let current_phase: Option<String> = transaction
            .query_row(
                "SELECT phase FROM note_pipeline_runs WHERE id = ?",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取待遗弃深度笔记任务失败：{error}"))?;
        let Some(current_phase) = current_phase else {
            return Err("深度笔记任务不存在。".to_string());
        };
        if current_phase == NotePipelinePhase::Done.as_str() {
            return Err("已完成的深度笔记不能遗弃。".to_string());
        }
        {
            let next_sequence: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(sequence), 0) + 1 FROM note_pipeline_events WHERE run_id = ?",
                    params![run_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("读取深度笔记事件序号失败：{error}"))?;
            let now = now_millis_i64();
            transaction
                .execute(
                    "UPDATE note_pipeline_runs
                     SET phase = 'cancelled', error_message = 'mnemora:abandoned', updated_at = ?
                     WHERE id = ?",
                    params![now, run_id],
                )
                .map_err(|error| format!("遗弃深度笔记任务失败：{error}"))?;
            transaction
                .execute(
                    "INSERT INTO note_pipeline_events (
                        run_id, sequence, event_type, node_id, payload_json, created_at
                     ) VALUES (?, ?, 'runAbandoned', NULL, '{}', ?)",
                    params![run_id, next_sequence, now],
                )
                .map_err(|error| format!("记录深度笔记遗弃事件失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记遗弃状态失败：{error}"))?;
        self.get_note_pipeline_run(&run_id)
    }

    pub fn list_resumable_note_pipeline_runs(&self) -> Result<Vec<NotePipelineRun>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT candidate.id FROM note_pipeline_runs AS candidate
                 WHERE (
                    candidate.phase IN (
                       'preflight', 'analyzing', 'awaiting_outline', 'compiling', 'queued',
                       'drafting', 'validating', 'replanning', 'assembling', 'persisting',
                       'paused', 'blocked', 'error'
                    )
                    OR (candidate.phase = 'cancelled' AND candidate.note_id IS NULL
                        AND (candidate.error_message IS NULL OR candidate.error_message <> 'mnemora:abandoned'))
                 )
                 AND NOT EXISTS (
                    SELECT 1 FROM note_pipeline_runs AS newer
                    WHERE newer.conversation_id = candidate.conversation_id
                      AND (
                         newer.created_at > candidate.created_at
                         OR (newer.created_at = candidate.created_at AND newer.rowid > candidate.rowid)
                      )
                 )
                 ORDER BY candidate.updated_at DESC, candidate.created_at DESC",
            )
            .map_err(|error| format!("准备深度笔记任务查询失败：{error}"))?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("查询深度笔记任务失败：{error}"))?;
        let mut runs = Vec::new();
        for id in ids {
            let id = id.map_err(|error| format!("读取深度笔记任务失败：{error}"))?;
            if let Some(run) = get_note_pipeline_run_with_connection(&connection, &id)? {
                runs.push(run);
            }
        }
        Ok(runs)
    }

    pub fn list_note_pipeline_runs_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<NotePipelineRun>, String> {
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id FROM note_pipeline_runs
                 WHERE conversation_id = ? AND phase NOT IN ('done')
                   AND (error_message IS NULL OR error_message <> 'mnemora:abandoned')
                 ORDER BY updated_at DESC",
            )
            .map_err(|error| format!("准备会话深度笔记任务查询失败：{error}"))?;
        let ids = statement
            .query_map(params![conversation_id], |row| row.get::<_, String>(0))
            .map_err(|error| format!("查询会话深度笔记任务失败：{error}"))?;
        ids.map(|id| {
            let id = id.map_err(|error| format!("读取会话深度笔记任务失败：{error}"))?;
            get_note_pipeline_run_with_connection(&connection, &id)?
                .ok_or_else(|| "会话深度笔记任务不存在。".to_string())
        })
        .collect()
    }

    pub fn save_note_pipeline_outline(
        &self,
        run_id: &str,
        outline_json: &str,
        sections: Vec<NotePipelineSectionCreate>,
    ) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        if outline_json.is_empty() || outline_json.len() > MAX_NOTE_PIPELINE_JSON_BYTES {
            return Err("深度笔记提纲为空或过长。".to_string());
        }
        serde_json::from_str::<serde_json::Value>(outline_json)
            .map_err(|error| format!("深度笔记提纲 JSON 无效：{error}"))?;
        if sections.is_empty() || sections.len() > MAX_NOTE_PIPELINE_SECTIONS {
            return Err(format!(
                "深度笔记提纲必须包含 1 到 {MAX_NOTE_PIPELINE_SECTIONS} 个章节。"
            ));
        }
        let mut normalized = Vec::with_capacity(sections.len());
        let mut ids = std::collections::HashSet::new();
        for section in sections {
            let section_id = normalize_identifier("章节 ID", &section.section_id)?;
            if !ids.insert(section_id.clone()) {
                return Err("深度笔记提纲包含重复章节 ID。".to_string());
            }
            if section.section_json.is_empty()
                || section.section_json.len() > MAX_NOTE_PIPELINE_JSON_BYTES
            {
                return Err("深度笔记章节 JSON 为空或过长。".to_string());
            }
            serde_json::from_str::<serde_json::Value>(&section.section_json)
                .map_err(|error| format!("深度笔记章节 JSON 无效：{error}"))?;
            normalized.push(NotePipelineSectionCreate {
                section_id,
                ..section
            });
        }
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始保存深度笔记提纲失败：{error}"))?;
        let now = now_millis_i64();
        let changed = transaction
            .execute(
                "UPDATE note_pipeline_runs
                 SET phase = 'awaiting_outline', outline_json = ?, error_message = NULL, updated_at = ?
                 WHERE id = ? AND phase NOT IN ('cancelling', 'cancelled')",
                params![outline_json, now, run_id],
            )
            .map_err(|error| format!("保存深度笔记提纲失败：{error}"))?;
        if changed == 0 {
            return Err("深度笔记任务不存在。".to_string());
        }
        transaction
            .execute(
                "DELETE FROM note_pipeline_sections WHERE run_id = ?",
                params![run_id],
            )
            .map_err(|error| format!("重置深度笔记章节失败：{error}"))?;
        for section in normalized {
            transaction
                .execute(
                    "INSERT INTO note_pipeline_sections (
                        run_id, section_id, position, section_json, markdown, status,
                        attempt_count, revision_count, evidence_ids_json, validation_json,
                        input_hash, updated_at
                     ) VALUES (?, ?, ?, ?, '', 'pending', 0, 0, '[]', '', ?, ?)",
                    params![
                        run_id,
                        section.section_id,
                        section.position as i64,
                        section.section_json,
                        section.input_hash,
                        now
                    ],
                )
                .map_err(|error| format!("保存深度笔记章节失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记提纲失败：{error}"))?;
        self.get_note_pipeline_run(&run_id)
    }

    pub fn save_note_pipeline_plan_version(
        &self,
        run_id: &str,
        version: u32,
        plan_id: &str,
        plan_json: &str,
        compiled_dag_json: &str,
        plan_hash: &str,
        revision_reason: &str,
        confirmed_at: Option<u64>,
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let plan_id = normalize_identifier("计划 ID", plan_id)?;
        if plan_json.is_empty()
            || plan_json.len() > MAX_NOTE_PIPELINE_JSON_BYTES
            || compiled_dag_json.is_empty()
            || compiled_dag_json.len() > MAX_NOTE_PIPELINE_JSON_BYTES
        {
            return Err("深度笔记计划或 DAG 为空或过长。".to_string());
        }
        let connection = self.open_connection()?;
        let now = now_millis_i64();
        connection
            .execute(
                "INSERT INTO note_pipeline_plan_versions (
                    run_id, version, plan_id, plan_json, compiled_dag_json, plan_hash,
                    revision_reason, confirmed_at, created_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(run_id, version) DO UPDATE SET
                    plan_id = excluded.plan_id,
                    plan_json = excluded.plan_json,
                    compiled_dag_json = excluded.compiled_dag_json,
                    plan_hash = excluded.plan_hash,
                    revision_reason = excluded.revision_reason,
                    confirmed_at = excluded.confirmed_at",
                params![
                    run_id,
                    i64::from(version),
                    plan_id,
                    plan_json,
                    compiled_dag_json,
                    plan_hash,
                    revision_reason,
                    confirmed_at
                        .map(|value| i64::try_from(value).map_err(|_| "确认时间无效。".to_string()))
                        .transpose()?,
                    now,
                ],
            )
            .map_err(|error| format!("保存深度笔记计划版本失败：{error}"))?;
        connection
            .execute(
                "UPDATE note_pipeline_runs
                 SET current_plan_version = ?, updated_at = ? WHERE id = ?",
                params![i64::from(version), now, run_id],
            )
            .map_err(|error| format!("更新深度笔记计划版本失败：{error}"))?;
        Ok(())
    }

    pub fn replace_note_pipeline_nodes(
        &self,
        run_id: &str,
        plan_version: u32,
        nodes_json: &[(String, String, Option<String>, String, String, String)],
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始保存深度笔记 DAG 失败：{error}"))?;
        transaction
            .execute(
                "DELETE FROM note_pipeline_nodes WHERE run_id = ? AND plan_version = ?",
                params![run_id, i64::from(plan_version)],
            )
            .map_err(|error| format!("重置深度笔记 DAG 失败：{error}"))?;
        let now = now_millis_i64();
        for (node_id, node_type, section_id, depends_on_json, status, input_hash) in nodes_json {
            transaction
                .execute(
                    "INSERT INTO note_pipeline_nodes (
                        run_id, plan_version, node_id, node_type, section_id, depends_on_json,
                        status, attempt_count, evidence_ids_json, input_hash, validation_json, updated_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, 0, '[]', ?, '', ?)",
                    params![
                        run_id,
                        i64::from(plan_version),
                        node_id,
                        node_type,
                        section_id,
                        depends_on_json,
                        status,
                        input_hash,
                        now,
                    ],
                )
                .map_err(|error| format!("保存深度笔记 DAG 节点失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记 DAG 失败：{error}"))
    }

    pub fn update_note_pipeline_node_state(
        &self,
        run_id: &str,
        plan_version: u32,
        node_id: &str,
        status: &str,
        attempt_count: u8,
        evidence_ids: &[String],
        output_ref: Option<&str>,
        validation_json: &str,
        error_message: Option<&str>,
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let node_id = normalize_identifier("DAG 节点 ID", node_id)?;
        let evidence_ids_json = serde_json::to_string(evidence_ids)
            .map_err(|error| format!("序列化深度笔记 DAG 证据失败：{error}"))?;
        let connection = self.open_connection()?;
        let now = now_millis_i64();
        let changed = connection
            .execute(
                "UPDATE note_pipeline_nodes
                 SET status = ?, attempt_count = ?, evidence_ids_json = ?, output_ref = ?,
                     validation_json = ?, error_message = ?, updated_at = ?
                 WHERE run_id = ? AND plan_version = ? AND node_id = ?",
                params![
                    status,
                    i64::from(attempt_count),
                    evidence_ids_json,
                    output_ref,
                    validation_json,
                    error_message,
                    now,
                    run_id,
                    i64::from(plan_version),
                    node_id,
                ],
            )
            .map_err(|error| format!("更新深度笔记 DAG 节点失败：{error}"))?;
        if changed == 0 {
            return Err(format!("深度笔记 DAG 节点不存在：{node_id}"));
        }
        connection
            .execute(
                "UPDATE note_pipeline_runs SET updated_at = ? WHERE id = ?",
                params![now, run_id],
            )
            .map_err(|error| format!("更新深度笔记任务时间失败：{error}"))?;
        Ok(())
    }

    pub fn replace_note_pipeline_source_chunks(
        &self,
        run_id: &str,
        chunks: &[DeepNoteSourceChunk],
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始保存深度笔记来源分块失败：{error}"))?;
        transaction
            .execute(
                "DELETE FROM note_pipeline_source_chunks WHERE run_id = ?",
                params![run_id],
            )
            .map_err(|error| format!("清理深度笔记旧来源分块失败：{error}"))?;
        let now = now_millis_i64();
        for chunk in chunks {
            transaction
                .execute(
                    "INSERT INTO note_pipeline_source_chunks (
                        run_id, chunk_id, source_kind, source_id, message_id, attachment_id,
                        library_item_id, location, excerpt, content_hash, ocr_confidence, created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        run_id,
                        chunk.chunk_id,
                        chunk.source_kind.as_str(),
                        chunk.source_id,
                        chunk.message_id,
                        chunk.attachment_id,
                        chunk.library_item_id,
                        chunk.location,
                        chunk.excerpt,
                        chunk.content_hash,
                        chunk.ocr_confidence,
                        now,
                    ],
                )
                .map_err(|error| format!("保存深度笔记来源分块失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记来源分块失败：{error}"))
    }

    pub fn list_note_pipeline_source_chunks(
        &self,
        run_id: &str,
    ) -> Result<Vec<DeepNoteSourceChunk>, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT chunk_id, source_kind, source_id, message_id, attachment_id,
                        library_item_id, location, excerpt, content_hash, ocr_confidence
                 FROM note_pipeline_source_chunks WHERE run_id = ?
                 ORDER BY created_at ASC, chunk_id ASC",
            )
            .map_err(|error| format!("准备深度笔记来源分块查询失败：{error}"))?;
        let rows = statement
            .query_map(params![run_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<f32>>(9)?,
                ))
            })
            .map_err(|error| format!("查询深度笔记来源分块失败：{error}"))?;
        rows.map(|row| {
            let raw = row.map_err(|error| format!("读取深度笔记来源分块失败：{error}"))?;
            Ok(DeepNoteSourceChunk {
                chunk_id: raw.0,
                source_kind: DeepNoteSourceKind::parse(&raw.1)?,
                source_id: raw.2,
                message_id: raw.3,
                attachment_id: raw.4,
                library_item_id: raw.5,
                location: raw.6,
                excerpt: raw.7,
                content_hash: raw.8,
                ocr_confidence: raw.9,
            })
        })
        .collect()
    }

    pub fn replace_note_pipeline_evidence(
        &self,
        run_id: &str,
        evidence: &[DeepNoteEvidenceArtifact],
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始保存深度笔记证据失败：{error}"))?;
        transaction
            .execute(
                "DELETE FROM note_pipeline_evidence WHERE run_id = ?",
                params![run_id],
            )
            .map_err(|error| format!("清理深度笔记旧证据失败：{error}"))?;
        for item in evidence {
            transaction
                .execute(
                    "INSERT INTO note_pipeline_evidence (
                        run_id, evidence_id, section_id, source_chunk_ids_json, claim_text,
                        model_synthesis, source_excerpt, support_level, status, content_hash,
                        created_at
                     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        run_id,
                        item.evidence_id,
                        item.section_id,
                        serde_json::to_string(&item.source_chunk_ids)
                            .map_err(|error| format!("序列化证据来源失败：{error}"))?,
                        item.claim,
                        item.model_synthesis,
                        item.source_excerpt,
                        item.support_level.as_str(),
                        item.status.as_str(),
                        item.content_hash,
                        i64::try_from(item.created_at).unwrap_or(i64::MAX),
                    ],
                )
                .map_err(|error| format!("保存深度笔记证据失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记证据失败：{error}"))
    }

    pub fn list_note_pipeline_evidence(
        &self,
        run_id: &str,
    ) -> Result<Vec<DeepNoteEvidenceArtifact>, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT evidence_id, section_id, source_chunk_ids_json, claim_text,
                        model_synthesis, source_excerpt, support_level, status, content_hash,
                        created_at
                 FROM note_pipeline_evidence WHERE run_id = ?
                 ORDER BY created_at ASC, evidence_id ASC",
            )
            .map_err(|error| format!("准备深度笔记证据查询失败：{error}"))?;
        let rows = statement
            .query_map(params![run_id], |row| {
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
                    row.get::<_, i64>(9)?,
                ))
            })
            .map_err(|error| format!("查询深度笔记证据失败：{error}"))?;
        rows.map(|row| {
            let raw = row.map_err(|error| format!("读取深度笔记证据失败：{error}"))?;
            Ok(DeepNoteEvidenceArtifact {
                evidence_id: raw.0,
                section_id: raw.1,
                source_chunk_ids: serde_json::from_str(&raw.2)
                    .map_err(|error| format!("解析证据来源失败：{error}"))?,
                claim: raw.3,
                model_synthesis: raw.4,
                source_excerpt: raw.5,
                support_level: DeepNoteSupportLevel::parse(&raw.6)?,
                status: DeepNoteEvidenceStatus::parse(&raw.7)?,
                content_hash: raw.8,
                created_at: i64_to_u64(raw.9),
            })
        })
        .collect()
    }

    pub fn save_note_pipeline_ledger(
        &self,
        run_id: &str,
        version: u32,
        ledger: &DeepNoteLedger,
        patch_json: &str,
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let ledger_json = serde_json::to_string(ledger)
            .map_err(|error| format!("序列化深度笔记账本失败：{error}"))?;
        let connection = self.open_connection()?;
        connection
            .execute(
                "INSERT INTO note_pipeline_ledgers (
                    run_id, version, ledger_json, patch_json, created_at
                 ) VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(run_id, version) DO UPDATE SET
                    ledger_json = excluded.ledger_json,
                    patch_json = excluded.patch_json",
                params![
                    run_id,
                    i64::from(version),
                    ledger_json,
                    patch_json,
                    now_millis_i64(),
                ],
            )
            .map_err(|error| format!("保存深度笔记账本失败：{error}"))?;
        Ok(())
    }

    pub fn latest_note_pipeline_ledger(
        &self,
        run_id: &str,
    ) -> Result<Option<DeepNoteLedger>, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        let raw = connection
            .query_row(
                "SELECT ledger_json FROM note_pipeline_ledgers
                 WHERE run_id = ? ORDER BY version DESC LIMIT 1",
                params![run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取深度笔记账本失败：{error}"))?;
        raw.map(|value| {
            serde_json::from_str(&value).map_err(|error| format!("解析深度笔记账本失败：{error}"))
        })
        .transpose()
    }

    pub fn append_note_pipeline_event(
        &self,
        run_id: &str,
        event_type: &str,
        node_id: Option<&str>,
        payload_json: &str,
    ) -> Result<u64, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        let next: i64 = connection
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM note_pipeline_events WHERE run_id = ?",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("读取深度笔记事件序号失败：{error}"))?;
        connection
            .execute(
                "INSERT INTO note_pipeline_events (
                    run_id, sequence, event_type, node_id, payload_json, created_at
                 ) VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    run_id,
                    next,
                    event_type,
                    node_id,
                    payload_json,
                    now_millis_i64()
                ],
            )
            .map_err(|error| format!("保存深度笔记事件失败：{error}"))?;
        u64::try_from(next).map_err(|_| "深度笔记事件序号无效。".to_string())
    }

    pub fn list_note_pipeline_events(
        &self,
        run_id: &str,
        limit: usize,
    ) -> Result<Vec<(u64, String, Option<String>, String, u64)>, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let limit = limit.clamp(1, 500);
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT sequence, event_type, node_id, payload_json, created_at
                 FROM note_pipeline_events WHERE run_id = ?
                 ORDER BY sequence DESC LIMIT ?",
            )
            .map_err(|error| format!("准备深度笔记事件查询失败：{error}"))?;
        let rows = statement
            .query_map(params![run_id, limit as i64], |row| {
                Ok((
                    i64_to_u64(row.get(0)?),
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    i64_to_u64(row.get(4)?),
                ))
            })
            .map_err(|error| format!("查询深度笔记事件失败：{error}"))?;
        let mut events = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("读取深度笔记事件失败：{error}"))?;
        events.reverse();
        Ok(events)
    }

    pub fn update_note_pipeline_runtime_json(
        &self,
        run_id: &str,
        budget_json: &str,
        preflight_json: &str,
        sidecar_json: Option<&str>,
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        connection
            .execute(
                "UPDATE note_pipeline_runs
                 SET budget_json = ?, preflight_json = ?, sidecar_json = COALESCE(?, sidecar_json),
                     updated_at = ? WHERE id = ?",
                params![
                    budget_json,
                    preflight_json,
                    sidecar_json,
                    now_millis_i64(),
                    run_id
                ],
            )
            .map_err(|error| format!("保存深度笔记运行状态失败：{error}"))?;
        Ok(())
    }

    pub fn prepare_note_pipeline_retry(
        &self,
        run_id: &str,
        reset_failed_sections: bool,
    ) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始准备深度笔记恢复失败：{error}"))?;
        let (phase, execution_version, outline_json, selected_json): (String, i64, String, String) =
            transaction
                .query_row(
                    "SELECT phase, execution_version, outline_json, selected_section_ids_json
                 FROM note_pipeline_runs WHERE id = ?",
                    params![run_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(|error| format!("读取深度笔记恢复状态失败：{error}"))?
                .ok_or_else(|| "深度笔记任务不存在。".to_string())?;
        if !matches!(phase.as_str(), "error" | "blocked" | "cancelled") {
            return Err("当前深度笔记任务不需要人工恢复。".to_string());
        }
        if execution_version >= 6 {
            return Err("该深度笔记任务已达到 5 次人工恢复上限，请重新生成。".to_string());
        }
        let now = now_millis_i64();
        let selected_ids: Vec<String> = serde_json::from_str(&selected_json)
            .map_err(|error| format!("解析恢复章节选择失败：{error}"))?;
        let resume_phase = if outline_json.trim().is_empty() {
            NotePipelinePhase::Analyzing
        } else if selected_ids.is_empty() {
            NotePipelinePhase::AwaitingOutline
        } else {
            NotePipelinePhase::Drafting
        };
        transaction
            .execute(
                "UPDATE note_pipeline_runs
                 SET phase = ?, execution_version = execution_version + 1,
                     error_message = NULL, updated_at = ?
                 WHERE id = ?",
                params![resume_phase.as_str(), now, run_id],
            )
            .map_err(|error| format!("更新深度笔记恢复版本失败：{error}"))?;
        if reset_failed_sections {
            transaction
                .execute(
                    "UPDATE note_pipeline_sections
                     SET markdown = '', status = 'pending', attempt_count = 0, revision_count = 0,
                         evidence_ids_json = '[]', validation_json = '', error_message = NULL,
                         updated_at = ?
                     WHERE run_id = ? AND status IN (
                        'failed', 'blocked', 'needs_review', 'needs_revision', 'interrupted'
                     )",
                    params![now, run_id],
                )
                .map_err(|error| format!("重置失败章节检查点失败：{error}"))?;
            transaction
                .execute(
                    "UPDATE note_pipeline_nodes
                     SET status = 'pending', attempt_count = 0, evidence_ids_json = '[]',
                         output_ref = NULL, validation_json = '', error_message = NULL,
                         updated_at = ?
                     WHERE run_id = ? AND status IN (
                        'failed', 'blocked', 'needs_review', 'needs_revision', 'interrupted',
                        'needsReview', 'needsRevision'
                     )",
                    params![now, run_id],
                )
                .map_err(|error| format!("重置失败执行节点失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交深度笔记恢复状态失败：{error}"))?;
        self.get_note_pipeline_run(&run_id)
    }

    pub fn select_note_pipeline_sections(
        &self,
        run_id: &str,
        selected_section_ids: Vec<String>,
    ) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        if selected_section_ids.is_empty()
            || selected_section_ids.len() > MAX_NOTE_PIPELINE_SECTIONS
        {
            return Err("请至少保留一个深度笔记章节。".to_string());
        }
        let selected_section_ids = selected_section_ids
            .into_iter()
            .map(|id| normalize_identifier("章节 ID", &id))
            .collect::<Result<Vec<_>, _>>()?;
        let selected_json = serde_json::to_string(&selected_section_ids)
            .map_err(|error| format!("序列化章节选择失败：{error}"))?;
        let connection = self.open_connection()?;
        let available = get_note_pipeline_sections_with_connection(&connection, &run_id)?;
        let available_ids = available
            .iter()
            .map(|section| section.section_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if selected_section_ids
            .iter()
            .any(|section_id| !available_ids.contains(section_id.as_str()))
        {
            return Err("章节选择包含提纲中不存在的 ID。".to_string());
        }
        let changed = connection
            .execute(
                "UPDATE note_pipeline_runs
                 SET phase = 'compiling', selected_section_ids_json = ?, error_message = NULL, updated_at = ?
                 WHERE id = ? AND phase NOT IN ('cancelling', 'cancelled')",
                params![selected_json, now_millis_i64(), run_id],
            )
            .map_err(|error| format!("保存章节选择失败：{error}"))?;
        if changed == 0 {
            return Err("深度笔记任务不存在。".to_string());
        }
        self.get_note_pipeline_run(&run_id)
    }

    pub fn list_note_pipeline_sections(
        &self,
        run_id: &str,
    ) -> Result<Vec<NotePipelineSection>, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let connection = self.open_connection()?;
        get_note_pipeline_sections_with_connection(&connection, &run_id)
    }

    pub fn save_note_pipeline_section(
        &self,
        run_id: &str,
        section_id: &str,
        markdown: &str,
        status: NotePipelineSectionStatus,
        error_message: Option<&str>,
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let section_id = normalize_identifier("章节 ID", section_id)?;
        if markdown.len() > MAX_NOTE_PIPELINE_JSON_BYTES {
            return Err("深度笔记章节正文过长。".to_string());
        }
        let connection = self.open_connection()?;
        let now = now_millis_i64();
        let changed = connection
            .execute(
                "UPDATE note_pipeline_sections
                 SET markdown = ?, status = ?, error_message = ?, updated_at = ?
                 WHERE run_id = ? AND section_id = ?",
                params![
                    markdown,
                    status.as_str(),
                    error_message,
                    now,
                    run_id,
                    section_id
                ],
            )
            .map_err(|error| format!("保存深度笔记章节状态失败：{error}"))?;
        if changed == 0 {
            return Err("深度笔记章节不存在。".to_string());
        }
        connection
            .execute(
                "UPDATE note_pipeline_runs SET updated_at = ? WHERE id = ?",
                params![now, run_id],
            )
            .map_err(|error| format!("更新深度笔记任务时间失败：{error}"))?;
        Ok(())
    }

    pub fn save_note_pipeline_section_checkpoint(
        &self,
        run_id: &str,
        section_id: &str,
        markdown: &str,
        status: NotePipelineSectionStatus,
        attempt_count: u8,
        revision_count: u8,
        evidence_ids: &[String],
        validation_json: &str,
        error_message: Option<&str>,
    ) -> Result<(), String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let section_id = normalize_identifier("章节 ID", section_id)?;
        if markdown.len() > MAX_NOTE_PIPELINE_JSON_BYTES
            || validation_json.len() > MAX_NOTE_PIPELINE_JSON_BYTES
        {
            return Err("深度笔记章节检查点过长。".to_string());
        }
        let evidence_ids_json = serde_json::to_string(evidence_ids)
            .map_err(|error| format!("序列化章节证据失败：{error}"))?;
        let connection = self.open_connection()?;
        let now = now_millis_i64();
        let changed = connection
            .execute(
                "UPDATE note_pipeline_sections
                 SET markdown = ?, status = ?, attempt_count = ?, revision_count = ?,
                     evidence_ids_json = ?, validation_json = ?, error_message = ?, updated_at = ?
                 WHERE run_id = ? AND section_id = ?",
                params![
                    markdown,
                    status.as_str(),
                    i64::from(attempt_count),
                    i64::from(revision_count),
                    evidence_ids_json,
                    validation_json,
                    error_message,
                    now,
                    run_id,
                    section_id,
                ],
            )
            .map_err(|error| format!("保存深度笔记章节检查点失败：{error}"))?;
        if changed == 0 {
            return Err("深度笔记章节不存在。".to_string());
        }
        connection
            .execute(
                "UPDATE note_pipeline_runs SET updated_at = ? WHERE id = ?",
                params![now, run_id],
            )
            .map_err(|error| format!("更新深度笔记任务时间失败：{error}"))?;
        Ok(())
    }

    pub fn update_note_pipeline_phase(
        &self,
        run_id: &str,
        phase: NotePipelinePhase,
        note_id: Option<&str>,
        warnings: &[String],
        error_message: Option<&str>,
    ) -> Result<NotePipelineRun, String> {
        let run_id = normalize_identifier("任务 ID", run_id)?;
        let note_id = note_id
            .map(|id| normalize_identifier("笔记 ID", id))
            .transpose()?;
        let warnings_json = serde_json::to_string(warnings)
            .map_err(|error| format!("序列化深度笔记检查提示失败：{error}"))?;
        let connection = self.open_connection()?;
        let target_phase = phase.as_str();
        let changed = connection
            .execute(
                "UPDATE note_pipeline_runs
                 SET phase = ?, note_id = COALESCE(?, note_id), warnings_json = ?,
                      error_message = ?, updated_at = ?
                 WHERE id = ?
                   AND (
                     phase NOT IN ('cancelling', 'cancelled')
                     OR ? IN ('cancelling', 'cancelled')
                   )",
                params![
                    target_phase,
                    note_id,
                    warnings_json,
                    error_message,
                    now_millis_i64(),
                    run_id,
                    target_phase,
                ],
            )
            .map_err(|error| format!("更新深度笔记任务状态失败：{error}"))?;
        if changed == 0 {
            let current = self.get_note_pipeline_run(&run_id)?;
            if matches!(
                current.phase,
                NotePipelinePhase::Cancelling | NotePipelinePhase::Cancelled
            ) && !matches!(
                phase,
                NotePipelinePhase::Cancelling | NotePipelinePhase::Cancelled
            ) {
                return Err("深度笔记任务已经进入停止状态，拒绝继续推进阶段。".to_string());
            }
            return Err("深度笔记任务不存在。".to_string());
        }
        self.get_note_pipeline_run(&run_id)
    }

    pub fn latest_summarized_message_id(
        &self,
        note_id: &str,
        conversation_id: &str,
    ) -> Result<Option<String>, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let connection = self.open_connection()?;
        connection
            .query_row(
                "SELECT summarized_until_message_id FROM note_sources
                 WHERE note_id = ? AND conversation_id = ? AND summarized_until_message_id IS NOT NULL
                 ORDER BY created_at DESC LIMIT 1",
                params![note_id, conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取笔记增量锚点失败：{error}"))
    }

    /// 查找某个会话最近一次由深度笔记管线写入的笔记及其增量锚点。
    /// 只有带 summarized_until_message_id 的来源才参与匹配，避免把普通“保存消息为笔记”
    /// 误判成可增量更新的深度笔记。
    pub fn latest_deep_note_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<(LibraryNote, Option<String>)>, String> {
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let connection = self.open_connection()?;
        let note_id = connection
            .query_row(
                "SELECT note_id FROM note_sources
                 WHERE conversation_id = ? AND summarized_until_message_id IS NOT NULL
                 ORDER BY created_at DESC LIMIT 1",
                params![conversation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("读取会话已有深度笔记失败：{error}"))?;
        let Some(note_id) = note_id else {
            return Ok(None);
        };
        let note = self
            .get_note_with_connection(&connection, &note_id)?
            .ok_or_else(|| "会话关联的深度笔记已不存在。".to_string())?;
        let anchor = connection
            .query_row(
                "SELECT summarized_until_message_id FROM note_sources
                 WHERE note_id = ? AND conversation_id = ?
                   AND summarized_until_message_id IS NOT NULL
                 ORDER BY created_at DESC LIMIT 1",
                params![note_id, conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取会话深度笔记锚点失败：{error}"))?;
        Ok(Some((note, anchor)))
    }

    pub fn latest_completed_deep_note_runtime_json(
        &self,
        note_id: &str,
        conversation_id: &str,
    ) -> Result<Option<String>, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let conversation_id = normalize_identifier("会话 ID", conversation_id)?;
        let connection = self.open_connection()?;
        connection
            .query_row(
                "SELECT preflight_json FROM note_pipeline_runs
                 WHERE note_id = ? AND conversation_id = ? AND phase = 'done'
                 ORDER BY updated_at DESC LIMIT 1",
                params![note_id, conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("读取已完成深度笔记运行快照失败：{error}"))
    }

    pub fn create_note_edit_proposal(
        &self,
        create: NoteEditProposalCreate,
    ) -> Result<NoteEditProposal, String> {
        let id = normalize_identifier("修改提案 ID", &create.id)?;
        let note_id = normalize_identifier("笔记 ID", &create.note_id)?;
        let conversation_id = normalize_identifier("会话 ID", &create.conversation_id)?;
        let source_message_id = create
            .source_message_id
            .as_deref()
            .map(|id| normalize_identifier("消息 ID", id))
            .transpose()?;
        let normalized = LibraryNoteUpdate {
            note_id: note_id.clone(),
            title: create.new_title,
            content: create.new_content,
        }
        .normalize_and_validate()?;
        let sources = normalize_note_sources(create.sources)?;
        if create.diff.is_empty() || create.diff.len() > MAX_NOTE_PIPELINE_JSON_BYTES {
            return Err("修改提案 diff 为空或过长。".to_string());
        }
        let sources_json = serde_json::to_string(&sources)
            .map_err(|error| format!("序列化修改来源失败：{error}"))?;
        let coverage_snapshot_json =
            normalize_coverage_snapshot_json(&create.coverage_snapshot_json)?;
        let mut connection = self.open_connection()?;
        let note = self
            .get_note_with_connection(&connection, &note_id)?
            .ok_or_else(|| "目标笔记不存在。".to_string())?;
        if note.updated_at != create.expected_note_updated_at {
            return Err("目标笔记已发生变化，请重新生成修改提案。".to_string());
        }
        let now = now_millis_i64();
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始保存笔记修改提案失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO note_edit_proposals (
                    id, note_id, conversation_id, source_message_id, expected_note_updated_at,
                    old_title, new_title, old_content, new_content, diff_text, sources_json,
                    coverage_snapshot_json, status, created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
                params![
                    id,
                    note_id,
                    conversation_id,
                    source_message_id,
                    i64::try_from(create.expected_note_updated_at).unwrap_or(i64::MAX),
                    create.old_title,
                    normalized.title,
                    create.old_content,
                    normalized.content,
                    create.diff,
                    sources_json,
                    coverage_snapshot_json,
                    now,
                    now,
                ],
            )
            .map_err(|error| format!("保存笔记修改提案失败：{error}"))?;
        for unit in &create.source_units {
            let unit_json = serde_json::to_string(unit)
                .map_err(|error| format!("序列化笔记增量来源单元失败：{error}"))?;
            transaction
                .execute(
                    "INSERT INTO note_edit_source_units (proposal_id, unit_json, created_at)
                     VALUES (?, ?, ?)",
                    params![id, unit_json, now],
                )
                .map_err(|error| format!("保存笔记增量来源单元失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交笔记修改提案失败：{error}"))?;
        get_note_edit_proposal_with_connection(&connection, &id)?
            .ok_or_else(|| "创建后的笔记修改提案不存在。".to_string())
    }

    pub fn pending_note_edit_coverage_snapshot(
        &self,
        proposal_id: &str,
    ) -> Result<Option<(String, String)>, String> {
        let proposal_id = normalize_identifier("修改提案 ID", proposal_id)?;
        let connection = self.open_connection()?;
        connection
            .query_row(
                "SELECT conversation_id, coverage_snapshot_json
                 FROM note_edit_proposals
                 WHERE id = ? AND status = 'pending'",
                params![proposal_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("读取笔记修改提案覆盖快照失败：{error}"))
    }

    pub fn resolve_note_edit_proposal(
        &self,
        proposal_id: &str,
        accepted: bool,
    ) -> Result<Option<LibraryNote>, String> {
        self.resolve_note_edit_proposal_with_content(proposal_id, accepted, None)
    }

    pub fn resolve_note_edit_proposal_with_content(
        &self,
        proposal_id: &str,
        accepted: bool,
        replacement: Option<(String, String, String)>,
    ) -> Result<Option<LibraryNote>, String> {
        let proposal_id = normalize_identifier("修改提案 ID", proposal_id)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始应用笔记修改失败：{error}"))?;
        let raw = transaction
            .query_row(
                "SELECT note_id, expected_note_updated_at, old_title, new_title,
                        old_content, new_content, sources_json, coverage_snapshot_json, status,
                        conversation_id
                 FROM note_edit_proposals WHERE id = ?",
                params![proposal_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("读取笔记修改提案失败：{error}"))?
            .ok_or_else(|| "笔记修改提案不存在。".to_string())?;
        if raw.8 != "pending" {
            return Err("笔记修改提案已经处理。".to_string());
        }
        let now = now_millis_i64();
        if !accepted {
            transaction
                .execute(
                    "UPDATE note_edit_proposals SET status = 'rejected', updated_at = ? WHERE id = ?",
                    params![now, proposal_id],
                )
                .map_err(|error| format!("拒绝笔记修改提案失败：{error}"))?;
            transaction
                .execute(
                    "DELETE FROM note_edit_source_units WHERE proposal_id = ?",
                    params![proposal_id],
                )
                .map_err(|error| format!("清理已拒绝的附件增量来源失败：{error}"))?;
            transaction
                .commit()
                .map_err(|error| format!("提交拒绝结果失败：{error}"))?;
            return Ok(None);
        }
        let partial_replacement = replacement.is_some();
        let (new_title, new_content, applied_diff) =
            if let Some((title, content, diff)) = replacement {
                let normalized = LibraryNoteUpdate {
                    note_id: raw.0.clone(),
                    title,
                    content,
                }
                .normalize_and_validate()?;
                if diff.is_empty() || diff.len() > MAX_NOTE_PIPELINE_JSON_BYTES {
                    return Err("部分修改提案 diff 为空或过长。".to_string());
                }
                (normalized.title, normalized.content, diff)
            } else {
                (raw.3.clone(), raw.5.clone(), String::new())
            };
        let current = self
            .get_note_with_connection(&transaction, &raw.0)?
            .ok_or_else(|| "目标笔记不存在。".to_string())?;
        if current.updated_at != i64_to_u64(raw.1) {
            return Err("目标笔记已发生变化，请重新生成修改提案。".to_string());
        }
        let version_id = Uuid::new_v4().to_string();
        transaction
            .execute(
                "INSERT INTO library_note_versions (id, note_id, title, content, reason, created_at)
                 VALUES (?, ?, ?, ?, 'noteEdit', ?)",
                params![version_id, raw.0, raw.2, raw.4, now],
            )
            .map_err(|error| format!("备份旧笔记版本失败：{error}"))?;
        let updated_at = now.max(raw.1.saturating_add(1));
        transaction
            .execute(
                "UPDATE library_notes SET title = ?, content = ?, updated_at = ? WHERE id = ?",
                params![new_title, new_content, updated_at, raw.0],
            )
            .map_err(|error| format!("应用笔记修改失败：{error}"))?;
        let sources = if partial_replacement {
            vec![NoteSourceCreate {
                section_id: "partial-edit".to_string(),
                origin: NoteSourceOrigin::Conversation,
                conversation_id: Some(raw.9.clone()),
                message_id: None,
                summarized_until_message_id: None,
            }]
        } else {
            serde_json::from_str::<Vec<NoteSourceCreate>>(&raw.6)
                .map_err(|error| format!("读取修改来源失败：{error}"))?
        };
        insert_note_sources(&transaction, &raw.0, sources, updated_at)?;
        let source_units = pending_note_edit_source_units(&transaction, &proposal_id)?;
        if !source_units.is_empty() {
            let current_units = load_deep_note_source_units(&transaction, &raw.0, &raw.9)?;
            let mut merged = current_units
                .into_iter()
                .map(|unit| (unit.unit_id.clone(), unit))
                .collect::<std::collections::BTreeMap<_, _>>();
            for unit in source_units {
                if unit.note_id == raw.0 && unit.conversation_id == raw.9 {
                    merged.insert(unit.unit_id.clone(), unit);
                }
            }
            transaction
                .execute(
                    "DELETE FROM deep_note_source_units WHERE note_id = ? AND conversation_id = ?",
                    params![raw.0, raw.9],
                )
                .map_err(|error| format!("替换深度笔记来源单元失败：{error}"))?;
            insert_deep_note_source_units(
                &transaction,
                &raw.0,
                &raw.9,
                &merged.into_values().collect::<Vec<_>>(),
            )?;
        }
        if !raw.7.is_empty() {
            let coverage_snapshot_json = normalize_coverage_snapshot_json(&raw.7)?;
            upsert_deep_note_coverage_snapshot(
                &transaction,
                &raw.0,
                &raw.9,
                &coverage_snapshot_json,
                updated_at,
            )?;
        }
        transaction
            .execute(
                "UPDATE note_edit_proposals
                 SET status = 'applied',
                     diff_text = CASE WHEN ? = '' THEN diff_text ELSE ? END,
                     updated_at = ?
                 WHERE id = ?",
                params![applied_diff, applied_diff, updated_at, proposal_id],
            )
            .map_err(|error| format!("完成笔记修改提案失败：{error}"))?;
        transaction
            .execute(
                "DELETE FROM note_edit_source_units WHERE proposal_id = ?",
                params![proposal_id],
            )
            .map_err(|error| format!("清理已应用的附件增量来源失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("提交笔记修改失败：{error}"))?;
        Ok(Some(
            self.get_note_with_connection(&connection, &raw.0)?
                .ok_or_else(|| "更新后的笔记不存在。".to_string())?,
        ))
    }

    /// 列出全部笔记分组（含空分组）；计数只统计独立笔记。
    pub fn list_note_groups(&self) -> Result<Vec<LibraryNoteGroup>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT g.name, g.created_at,
                        COUNT(CASE WHEN n.item_id IS NULL THEN 1 END) AS note_count
                 FROM library_note_groups g
                 LEFT JOIN library_notes n ON n.group_name = g.name COLLATE NOCASE
                 GROUP BY g.name
                 ORDER BY g.name COLLATE NOCASE ASC",
            )
            .map_err(|error| format!("准备笔记分组查询失败：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok(LibraryNoteGroup {
                    name: row.get(0)?,
                    created_at: i64_to_u64(row.get(1)?),
                    note_count: usize::try_from(row.get::<_, i64>(2)?).unwrap_or(usize::MAX),
                })
            })
            .map_err(|error| format!("查询笔记分组失败：{error}"))?;
        let mut groups = Vec::new();
        for row in rows {
            groups.push(row.map_err(|error| format!("读取笔记分组失败：{error}"))?);
        }
        Ok(groups)
    }

    pub fn create_note_group(&self, name: &str) -> Result<LibraryNoteGroup, String> {
        let name = normalize_note_group_name(name)?;
        let connection = self.open_connection()?;
        let now = now_millis_i64();
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO library_note_groups (name, created_at) VALUES (?, ?)",
                params![name, now],
            )
            .map_err(|error| format!("创建笔记分组失败：{error}"))?;
        if inserted == 0 {
            return Err(format!("分组“{name}”已存在。"));
        }
        Ok(LibraryNoteGroup {
            name,
            note_count: 0,
            created_at: i64_to_u64(now),
        })
    }

    /// 删除分组并把其中的笔记恢复为未分类。
    pub fn delete_note_group(&self, name: &str) -> Result<bool, String> {
        let name = normalize_note_group_name(name)?;
        let mut connection = self.open_connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始删除笔记分组失败：{error}"))?;
        transaction
            .execute(
                "UPDATE library_notes SET group_name = NULL WHERE group_name = ? COLLATE NOCASE",
                params![name],
            )
            .map_err(|error| format!("清空分组内笔记失败：{error}"))?;
        let removed = transaction
            .execute(
                "DELETE FROM library_note_groups WHERE name = ? COLLATE NOCASE",
                params![name],
            )
            .map_err(|error| format!("删除笔记分组失败：{error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("提交删除笔记分组失败：{error}"))?;
        Ok(removed > 0)
    }

    /// 调整笔记所属分组；传入 None 恢复未分类。目标分组不存在时自动注册。
    pub fn set_note_group(
        &self,
        note_id: &str,
        group_name: Option<&str>,
    ) -> Result<LibraryNote, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let group_name = group_name
            .filter(|value| !value.trim().is_empty())
            .map(normalize_note_group_name)
            .transpose()?;
        let connection = self.open_connection()?;
        if let Some(name) = group_name.as_deref() {
            register_note_group(&connection, name, now_millis_i64())?;
        }
        // 分组调整不修改 updated_at：归档整理不应把笔记顶到最近编辑列表顶部。
        let changed = connection
            .execute(
                "UPDATE library_notes
                 SET group_name = ?
                 WHERE id = ? AND (
                    item_id IS NULL OR EXISTS (
                        SELECT 1 FROM library_items i
                        WHERE i.id = library_notes.item_id AND i.deleted_at IS NULL
                    )
                 )",
                params![group_name, note_id],
            )
            .map_err(|error| format!("调整笔记分组失败：{error}"))?;
        if changed == 0 {
            return Err("笔记不存在或所属文献位于回收站。".to_string());
        }
        self.get_note_with_connection(&connection, &note_id)?
            .ok_or_else(|| "调整分组后的笔记不存在。".to_string())
    }

    pub fn import_markdown_notes(
        &self,
        paths: Vec<String>,
    ) -> Result<LibraryNoteImportResult, String> {
        if paths.is_empty() {
            return Err("没有选择需要导入的 Markdown 文件。".to_string());
        }
        if paths.len() > MAX_NOTE_IMPORT_FILES {
            return Err(format!(
                "单次最多导入 {MAX_NOTE_IMPORT_FILES} 个 Markdown 文件。"
            ));
        }
        let mut result = LibraryNoteImportResult {
            imported: Vec::new(),
            failed: Vec::new(),
        };
        for path in paths {
            let file_name = Path::new(&path)
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            let import = (|| {
                let extension = Path::new(&path)
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if !extension.eq_ignore_ascii_case("md")
                    && !extension.eq_ignore_ascii_case("markdown")
                {
                    return Err("仅支持 .md 或 .markdown 文件。".to_string());
                }
                let metadata =
                    fs::metadata(&path).map_err(|error| format!("读取文件信息失败：{error}"))?;
                if metadata.len() > MAX_NOTE_IMPORT_BYTES {
                    return Err("单篇 Markdown 笔记不能超过 2 MB。".to_string());
                }
                let content = fs::read_to_string(&path)
                    .map_err(|error| format!("读取 UTF-8 文件失败：{error}"))?;
                let content = content
                    .strip_prefix('\u{feff}')
                    .unwrap_or(&content)
                    .to_string();
                let title = Path::new(&path)
                    .file_stem()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "导入笔记".to_string());
                self.create_note(LibraryNoteCreate {
                    item_id: None,
                    title,
                    content,
                    group_name: None,
                })
            })();
            match import {
                Ok(note) => result.imported.push(note),
                Err(error) => result.failed.push(LibraryNoteImportFailure {
                    path,
                    file_name,
                    error,
                }),
            }
        }
        Ok(result)
    }

    pub fn update_note(&self, update: LibraryNoteUpdate) -> Result<LibraryNote, String> {
        let update = update.normalize_and_validate()?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE library_notes
                 SET title = ?, content = ?, updated_at = ?
                 WHERE id = ? AND (
                    item_id IS NULL OR EXISTS (
                        SELECT 1 FROM library_items i
                        WHERE i.id = library_notes.item_id AND i.deleted_at IS NULL
                    )
                 )",
                params![
                    update.title,
                    update.content,
                    now_millis_i64(),
                    update.note_id,
                ],
            )
            .map_err(|error| format!("更新文献笔记失败：{error}"))?;
        if changed == 0 {
            return Err("笔记不存在或所属文献位于回收站。".to_string());
        }
        self.get_note_with_connection(&connection, &update.note_id)?
            .ok_or_else(|| "更新后的笔记不存在。".to_string())
    }

    pub fn rename_note(&self, rename: LibraryNoteRename) -> Result<LibraryNote, String> {
        let rename = rename.normalize_and_validate()?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE library_notes
                 SET title = ?, updated_at = ?
                 WHERE id = ? AND (
                    item_id IS NULL OR EXISTS (
                        SELECT 1 FROM library_items i
                        WHERE i.id = library_notes.item_id AND i.deleted_at IS NULL
                    )
                 )",
                params![rename.title, now_millis_i64(), rename.note_id],
            )
            .map_err(|error| format!("重命名文献笔记失败：{error}"))?;
        if changed == 0 {
            return Err("笔记不存在或所属文献位于回收站。".to_string());
        }
        self.get_note_with_connection(&connection, &rename.note_id)?
            .ok_or_else(|| "重命名后的笔记不存在。".to_string())
    }

    pub fn delete_note(&self, note_id: &str) -> Result<bool, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "DELETE FROM library_notes
                 WHERE id = ? AND (
                    item_id IS NULL OR EXISTS (
                        SELECT 1 FROM library_items i
                        WHERE i.id = library_notes.item_id AND i.deleted_at IS NULL
                    )
                 )",
                params![note_id],
            )
            .map_err(|error| format!("删除文献笔记失败：{error}"))?;
        Ok(changed > 0)
    }

    pub fn list_collections(&self) -> Result<Vec<LibraryCollection>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT c.id, c.name, c.created_at, c.updated_at,
                        COUNT(CASE WHEN i.deleted_at IS NULL THEN 1 END) AS item_count
                 FROM library_collections c
                 LEFT JOIN library_item_collections ic ON ic.collection_id = c.id
                 LEFT JOIN library_items i ON i.id = ic.item_id
                 GROUP BY c.id
                 ORDER BY c.name COLLATE NOCASE ASC",
            )
            .map_err(|error| format!("准备分类列表查询失败：{error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok(LibraryCollection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: i64_to_u64(row.get(2)?),
                    updated_at: i64_to_u64(row.get(3)?),
                    item_count: usize::try_from(row.get::<_, i64>(4)?).unwrap_or(usize::MAX),
                })
            })
            .map_err(|error| format!("查询分类列表失败：{error}"))?;
        let mut collections = Vec::new();
        for row in rows {
            collections.push(row.map_err(|error| format!("读取分类记录失败：{error}"))?);
        }
        Ok(collections)
    }

    pub fn create_collection(&self, name: &str) -> Result<LibraryCollection, String> {
        let name = normalize_collection_name(name)?;
        let connection = self.open_connection()?;
        let id = Uuid::new_v4().to_string();
        let now = now_millis_i64();
        connection
            .execute(
                "INSERT INTO library_collections (id, name, created_at, updated_at)
                 VALUES (?, ?, ?, ?)",
                params![id, name, now, now],
            )
            .map_err(|error| {
                if is_unique_constraint(&error) {
                    "已经存在同名分类。".to_string()
                } else {
                    format!("创建分类失败：{error}")
                }
            })?;
        Ok(LibraryCollection {
            id,
            name,
            item_count: 0,
            created_at: i64_to_u64(now),
            updated_at: i64_to_u64(now),
        })
    }

    pub fn rename_collection(&self, collection_id: &str, name: &str) -> Result<(), String> {
        let collection_id = normalize_identifier("分类 ID", collection_id)?;
        let name = normalize_collection_name(name)?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE library_collections SET name = ?, updated_at = ? WHERE id = ?",
                params![name, now_millis_i64(), collection_id],
            )
            .map_err(|error| {
                if is_unique_constraint(&error) {
                    "已经存在同名分类。".to_string()
                } else {
                    format!("重命名分类失败：{error}")
                }
            })?;
        if changed == 0 {
            return Err("分类不存在。".to_string());
        }
        Ok(())
    }

    pub fn delete_collection(&self, collection_id: &str) -> Result<bool, String> {
        let collection_id = normalize_identifier("分类 ID", collection_id)?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "DELETE FROM library_collections WHERE id = ?",
                params![collection_id],
            )
            .map_err(|error| format!("删除分类失败：{error}"))?;
        Ok(changed > 0)
    }

    pub(crate) fn open_connection(&self) -> Result<Connection, String> {
        fs::create_dir_all(&self.root_directory)
            .map_err(|error| format!("创建文献库目录失败：{error}"))?;
        fs::create_dir_all(&self.files_directory)
            .map_err(|error| format!("创建文献文件目录失败：{error}"))?;
        let connection = Connection::open(&self.database_path)
            .map_err(|error| format!("打开文献库数据库失败：{error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("设置文献库等待时间失败：{error}"))?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| format!("启用文献库外键失败：{error}"))?;
        migrate(&connection)?;
        Ok(connection)
    }

    pub(crate) fn find_by_hash_with_connection(
        &self,
        connection: &Connection,
        file_hash: &str,
    ) -> Result<Option<LibraryItem>, String> {
        let sql = format!(
            "SELECT {ITEM_COLUMNS}
             FROM library_items i
             JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
             WHERE f.file_hash = ?"
        );
        let raw = connection
            .query_row(&sql, params![file_hash], raw_item_from_row)
            .optional()
            .map_err(|error| format!("检查重复 PDF 失败：{error}"))?;
        raw.map(|raw| self.hydrate_item(connection, raw))
            .transpose()
    }

    pub(crate) fn insert_imported_item(
        &self,
        connection: &mut Connection,
        item_id: &str,
        file_id: &str,
        title: &str,
        original_name: &str,
        stored_name: &str,
        source_path: &str,
        file_size: u64,
        file_hash: &str,
        collection_id: Option<&str>,
        now: i64,
    ) -> Result<LibraryItem, String> {
        let transaction = connection
            .transaction()
            .map_err(|error| format!("开始 PDF 导入事务失败：{error}"))?;
        if let Some(collection_id) = collection_id {
            ensure_collections_exist(&transaction, &[collection_id.to_string()])?;
        }
        transaction
            .execute(
                "INSERT INTO library_items (
                    id, item_type, title, authors_json, publication_title, doi, abstract_text,
                    favorite, created_at, updated_at
                 ) VALUES (?, 'pdf', ?, '[]', '', '', '', 0, ?, ?)",
                params![item_id, title, now, now],
            )
            .map_err(|error| format!("创建文献记录失败：{error}"))?;
        transaction
            .execute(
                "INSERT INTO library_files (
                    id, item_id, original_name, stored_name, source_path, file_size, file_hash,
                    mime_type, is_primary, created_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, 'application/pdf', 1, ?)",
                params![
                    file_id,
                    item_id,
                    original_name,
                    stored_name,
                    source_path,
                    i64::try_from(file_size).map_err(|_| "PDF 文件过大。".to_string())?,
                    file_hash,
                    now,
                ],
            )
            .map_err(|error| format!("创建 PDF 快照记录失败：{error}"))?;
        if let Some(collection_id) = collection_id {
            transaction
                .execute(
                    "INSERT INTO library_item_collections (item_id, collection_id) VALUES (?, ?)",
                    params![item_id, collection_id],
                )
                .map_err(|error| format!("把文献加入分类失败：{error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("提交 PDF 导入失败：{error}"))?;
        self.get_item_with_connection(connection, item_id)?
            .ok_or_else(|| "导入后的文献不存在。".to_string())
    }

    pub(crate) fn attach_collection_if_needed(
        &self,
        connection: &Connection,
        item_id: &str,
        collection_id: Option<&str>,
    ) -> Result<(), String> {
        let Some(collection_id) = collection_id else {
            return Ok(());
        };
        ensure_collections_exist(connection, &[collection_id.to_string()])?;
        connection
            .execute(
                "INSERT OR IGNORE INTO library_item_collections (item_id, collection_id)
                 VALUES (?, ?)",
                params![item_id, collection_id],
            )
            .map_err(|error| format!("把已有文献加入分类失败：{error}"))?;
        Ok(())
    }

    pub(crate) fn resolve_stored_file_name(&self, stored_name: &str) -> Result<PathBuf, String> {
        let path = Path::new(stored_name);
        let mut components = path.components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            return Err("文献快照文件名无效。".to_string());
        }
        Ok(self.files_directory.join(stored_name))
    }

    fn set_deleted_at(
        &self,
        item_id: &str,
        deleted_at: Option<i64>,
    ) -> Result<LibraryItem, String> {
        let item_id = normalize_identifier("文献 ID", item_id)?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE library_items SET deleted_at = ?, updated_at = ? WHERE id = ?",
                params![deleted_at, now_millis_i64(), item_id],
            )
            .map_err(|error| format!("更新回收站状态失败：{error}"))?;
        if changed == 0 {
            return Err("文献不存在。".to_string());
        }
        self.get_item_with_connection(&connection, &item_id)?
            .ok_or_else(|| "文献不存在。".to_string())
    }

    fn get_item_with_connection(
        &self,
        connection: &Connection,
        item_id: &str,
    ) -> Result<Option<LibraryItem>, String> {
        let sql = format!(
            "SELECT {ITEM_COLUMNS}
             FROM library_items i
             JOIN library_files f ON f.item_id = i.id AND f.is_primary = 1
             WHERE i.id = ?"
        );
        let raw = connection
            .query_row(&sql, params![item_id], raw_item_from_row)
            .optional()
            .map_err(|error| format!("读取文献详情失败：{error}"))?;
        raw.map(|raw| self.hydrate_item(connection, raw))
            .transpose()
    }

    fn hydrate_item(
        &self,
        connection: &Connection,
        raw: RawLibraryItem,
    ) -> Result<LibraryItem, String> {
        let authors = serde_json::from_str::<Vec<String>>(&raw.authors_json)
            .map_err(|error| format!("解析文献作者失败：{error}"))?;
        let mut tag_statement = connection
            .prepare(
                "SELECT t.name
                 FROM library_tags t
                 JOIN library_item_tags it ON it.tag_id = t.id
                 WHERE it.item_id = ?
                 ORDER BY t.name COLLATE NOCASE ASC",
            )
            .map_err(|error| format!("准备标签查询失败：{error}"))?;
        let tag_rows = tag_statement
            .query_map(params![raw.id], |row| row.get::<_, String>(0))
            .map_err(|error| format!("查询文献标签失败：{error}"))?;
        let mut tags = Vec::new();
        for row in tag_rows {
            tags.push(row.map_err(|error| format!("读取文献标签失败：{error}"))?);
        }
        drop(tag_statement);

        let mut collection_statement = connection
            .prepare(
                "SELECT c.id, c.name
                 FROM library_collections c
                 JOIN library_item_collections ic ON ic.collection_id = c.id
                 WHERE ic.item_id = ?
                 ORDER BY c.name COLLATE NOCASE ASC",
            )
            .map_err(|error| format!("准备分类关联查询失败：{error}"))?;
        let collection_rows = collection_statement
            .query_map(params![raw.id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("查询文献分类失败：{error}"))?;
        let mut collection_ids = Vec::new();
        let mut collection_names = Vec::new();
        for row in collection_rows {
            let (id, name) = row.map_err(|error| format!("读取文献分类失败：{error}"))?;
            collection_ids.push(id);
            collection_names.push(name);
        }

        let file_path = self.resolve_stored_file_name(&raw.stored_name)?;
        Ok(LibraryItem {
            id: raw.id,
            title: raw.title,
            authors,
            publication_year: raw.publication_year,
            publication_title: raw.publication_title,
            doi: raw.doi,
            abstract_text: raw.abstract_text,
            favorite: raw.favorite,
            tags,
            collection_ids,
            collection_names,
            file: super::types::LibraryFileSummary {
                id: raw.file_id,
                original_name: raw.original_name,
                file_size: u64::try_from(raw.file_size).unwrap_or(0),
                file_hash: raw.file_hash,
                mime_type: raw.mime_type,
                created_at: i64_to_u64(raw.file_created_at),
                available: file_path.is_file(),
            },
            created_at: i64_to_u64(raw.created_at),
            updated_at: i64_to_u64(raw.updated_at),
            last_opened_at: raw.last_opened_at.map(i64_to_u64),
            deleted_at: raw.deleted_at.map(i64_to_u64),
        })
    }

    fn get_annotation_with_connection(
        &self,
        connection: &Connection,
        annotation_id: &str,
    ) -> Result<Option<LibraryAnnotation>, String> {
        connection
            .query_row(
                "SELECT a.id, a.item_id, a.kind, a.page_index, a.color, a.text, a.comment,
                        a.rects_json, a.created_at, a.updated_at
                 FROM library_annotations a
                 JOIN library_items i ON i.id = a.item_id
                 WHERE a.id = ? AND i.deleted_at IS NULL",
                params![annotation_id],
                annotation_from_row,
            )
            .optional()
            .map_err(|error| format!("读取 PDF 批注失败：{error}"))?
            .transpose()
    }

    fn get_note_with_connection(
        &self,
        connection: &Connection,
        note_id: &str,
    ) -> Result<Option<LibraryNote>, String> {
        connection
            .query_row(
                "SELECT n.id, n.item_id, i.title, n.title, n.content, n.group_name,
                        n.created_at, n.updated_at
                 FROM library_notes n
                 LEFT JOIN library_items i ON i.id = n.item_id
                 WHERE n.id = ? AND (n.item_id IS NULL OR i.deleted_at IS NULL)",
                params![note_id],
                note_from_row,
            )
            .optional()
            .map_err(|error| format!("读取文献笔记失败：{error}"))
    }
}

fn migrate(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| format!("读取文献库版本失败：{error}"))?;
    if version > LIBRARY_SCHEMA_VERSION {
        return Err("文献库版本高于当前应用支持的版本。".to_string());
    }
    if version == 0 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS library_items (
                    id TEXT PRIMARY KEY,
                    item_type TEXT NOT NULL CHECK (item_type = 'pdf'),
                    title TEXT NOT NULL,
                    authors_json TEXT NOT NULL DEFAULT '[]',
                    publication_year INTEGER,
                    publication_title TEXT NOT NULL DEFAULT '',
                    doi TEXT NOT NULL DEFAULT '',
                    abstract_text TEXT NOT NULL DEFAULT '',
                    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    last_opened_at INTEGER,
                    deleted_at INTEGER
                 );
                 CREATE TABLE IF NOT EXISTS library_files (
                    id TEXT PRIMARY KEY,
                    item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                    original_name TEXT NOT NULL,
                    stored_name TEXT NOT NULL UNIQUE,
                    source_path TEXT NOT NULL DEFAULT '',
                    file_size INTEGER NOT NULL CHECK (file_size >= 0),
                    file_hash TEXT NOT NULL UNIQUE,
                    mime_type TEXT NOT NULL,
                    is_primary INTEGER NOT NULL DEFAULT 0 CHECK (is_primary IN (0, 1)),
                    created_at INTEGER NOT NULL
                 );
                 CREATE UNIQUE INDEX IF NOT EXISTS library_primary_file_per_item
                    ON library_files(item_id) WHERE is_primary = 1;
                 CREATE TABLE IF NOT EXISTS library_collections (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS library_item_collections (
                    item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                    collection_id TEXT NOT NULL REFERENCES library_collections(id) ON DELETE CASCADE,
                    PRIMARY KEY (item_id, collection_id)
                 );
                 CREATE TABLE IF NOT EXISTS library_tags (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                    created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS library_item_tags (
                    item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                    tag_id TEXT NOT NULL REFERENCES library_tags(id) ON DELETE CASCADE,
                    PRIMARY KEY (item_id, tag_id)
                 );
                 CREATE TABLE IF NOT EXISTS library_reading_state (
                    item_id TEXT PRIMARY KEY REFERENCES library_items(id) ON DELETE CASCADE,
                    page_index INTEGER NOT NULL DEFAULT 0,
                    scroll_offset REAL NOT NULL DEFAULT 0,
                    zoom REAL NOT NULL DEFAULT 1,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS library_annotations (
                    id TEXT PRIMARY KEY,
                    item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                    kind TEXT NOT NULL CHECK (kind IN ('highlight', 'underline', 'area')),
                    page_index INTEGER NOT NULL CHECK (page_index >= 0),
                    color TEXT NOT NULL CHECK (color IN ('yellow', 'green', 'blue', 'pink', 'purple')),
                    text TEXT NOT NULL DEFAULT '',
                    comment TEXT NOT NULL DEFAULT '',
                    rects_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS library_notes (
                    id TEXT PRIMARY KEY,
                    item_id TEXT REFERENCES library_items(id) ON DELETE CASCADE,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS library_items_updated_at ON library_items(updated_at DESC);
                 CREATE INDEX IF NOT EXISTS library_items_last_opened_at ON library_items(last_opened_at DESC);
                 CREATE INDEX IF NOT EXISTS library_items_deleted_at ON library_items(deleted_at);
                 CREATE INDEX IF NOT EXISTS library_items_favorite ON library_items(favorite);
                 CREATE INDEX IF NOT EXISTS library_annotations_item_page
                    ON library_annotations(item_id, page_index, created_at);
                 CREATE INDEX IF NOT EXISTS library_notes_item_updated
                    ON library_notes(item_id, updated_at DESC);
                  PRAGMA user_version = 3;
                 COMMIT;",
            )
            .map_err(|error| format!("创建文献库结构失败：{error}"))?;
    }
    if version == 1 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS library_annotations (
                    id TEXT PRIMARY KEY,
                    item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                    kind TEXT NOT NULL CHECK (kind IN ('highlight', 'underline', 'area')),
                    page_index INTEGER NOT NULL CHECK (page_index >= 0),
                    color TEXT NOT NULL CHECK (color IN ('yellow', 'green', 'blue', 'pink', 'purple')),
                    text TEXT NOT NULL DEFAULT '',
                    comment TEXT NOT NULL DEFAULT '',
                    rects_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS library_notes (
                    id TEXT PRIMARY KEY,
                    item_id TEXT REFERENCES library_items(id) ON DELETE CASCADE,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS library_annotations_item_page
                    ON library_annotations(item_id, page_index, created_at);
                 CREATE INDEX IF NOT EXISTS library_notes_item_updated
                    ON library_notes(item_id, updated_at DESC);
                  PRAGMA user_version = 3;
                 COMMIT;",
            )
            .map_err(|error| format!("升级文献库批注与笔记结构失败：{error}"))?;
    }
    if version == 2 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE library_notes RENAME TO library_notes_v2;
                 CREATE TABLE library_notes (
                    id TEXT PRIMARY KEY,
                    item_id TEXT REFERENCES library_items(id) ON DELETE CASCADE,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 INSERT INTO library_notes (id, item_id, title, content, created_at, updated_at)
                    SELECT id, item_id, title, content, created_at, updated_at FROM library_notes_v2;
                 DROP TABLE library_notes_v2;
                 CREATE INDEX library_notes_item_updated
                    ON library_notes(item_id, updated_at DESC);
                 PRAGMA user_version = 3;
                 COMMIT;",
            )
            .map_err(|error| format!("升级全局 Markdown 笔记结构失败：{error}"))?;
    }
    // v4：笔记分组从前端 localStorage 迁入 SQLite（列 + 分组注册表）。
    // 前面各分支都把版本推进到 3，因此这里用旧读数 <= 3 统一收口。
    if version <= 3 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE library_notes ADD COLUMN group_name TEXT;
                 CREATE TABLE IF NOT EXISTS library_note_groups (
                    name TEXT PRIMARY KEY COLLATE NOCASE,
                    created_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 4;
                 COMMIT;",
            )
            .map_err(|error| format!("升级笔记分组结构失败：{error}"))?;
    }
    // v5：新增章节级笔记来源表 note_sources（Chat 深度笔记管线的溯源锚点）。
    // note_id 对笔记 ON DELETE CASCADE；conversation_id / message_id 是普通可空列，
    // 绝不加外键、绝不 CASCADE——对话与笔记分属两库，断链在应用层维护。
    if version <= 4 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS note_sources (
                    id TEXT PRIMARY KEY,
                    note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
                    section_id TEXT NOT NULL,
                    origin TEXT NOT NULL CHECK (origin IN ('conversation', 'ai_supplement')),
                    conversation_id TEXT,
                    message_id TEXT,
                    summarized_until_message_id TEXT,
                    created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS note_sources_note
                    ON note_sources(note_id);
                 CREATE INDEX IF NOT EXISTS note_sources_conversation
                    ON note_sources(conversation_id);
                 PRAGMA user_version = 5;
                 COMMIT;",
            )
            .map_err(|error| format!("升级笔记来源结构失败：{error}"))?;
    }
    // v6：M2 后台任务恢复、笔记版本与必须确认的 noteEdit 提案。
    if version <= 5 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS note_pipeline_runs (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL,
                    note_id TEXT REFERENCES library_notes(id) ON DELETE SET NULL,
                    phase TEXT NOT NULL CHECK (phase IN (
                        'analyzing', 'awaiting_outline', 'drafting', 'assembling',
                        'persisting', 'done', 'cancelled', 'error'
                    )),
                    outline_json TEXT NOT NULL DEFAULT '',
                    selected_section_ids_json TEXT NOT NULL DEFAULT '[]',
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    max_output_tokens INTEGER NOT NULL,
                    thinking_enabled INTEGER NOT NULL DEFAULT 0 CHECK (thinking_enabled IN (0, 1)),
                    retry_attempts INTEGER NOT NULL DEFAULT 1,
                    warnings_json TEXT NOT NULL DEFAULT '[]',
                    error_message TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE UNIQUE INDEX IF NOT EXISTS note_pipeline_active_conversation
                    ON note_pipeline_runs(conversation_id)
                    WHERE phase IN (
                        'analyzing', 'awaiting_outline', 'drafting', 'assembling',
                        'persisting', 'error'
                    );
                 CREATE INDEX IF NOT EXISTS note_pipeline_runs_updated
                    ON note_pipeline_runs(updated_at DESC);
                 CREATE TABLE IF NOT EXISTS note_pipeline_sections (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    section_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    section_json TEXT NOT NULL,
                    markdown TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'completed', 'failed')),
                    error_message TEXT,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, section_id)
                 );
                 CREATE INDEX IF NOT EXISTS note_pipeline_sections_order
                    ON note_pipeline_sections(run_id, position);
                 CREATE TABLE IF NOT EXISTS library_note_versions (
                    id TEXT PRIMARY KEY,
                    note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS library_note_versions_note
                    ON library_note_versions(note_id, created_at DESC);
                 CREATE TABLE IF NOT EXISTS note_edit_proposals (
                    id TEXT PRIMARY KEY,
                    note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
                    conversation_id TEXT NOT NULL,
                    source_message_id TEXT,
                    expected_note_updated_at INTEGER NOT NULL,
                    old_title TEXT NOT NULL,
                    new_title TEXT NOT NULL,
                    old_content TEXT NOT NULL,
                    new_content TEXT NOT NULL,
                    diff_text TEXT NOT NULL,
                    sources_json TEXT NOT NULL DEFAULT '[]',
                    status TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'applied', 'rejected')),
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS note_edit_proposals_note
                    ON note_edit_proposals(note_id, created_at DESC);
                 PRAGMA user_version = 6;
                 COMMIT;",
            )
            .map_err(|error| format!("升级深度笔记任务与版本结构失败：{error}"))?;
    }
    // v7：深度笔记第一版正式 Plan-and-Execute / DAG 运行结构。
    if version <= 6 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 DROP INDEX IF EXISTS note_pipeline_active_conversation;
                 DROP INDEX IF EXISTS note_pipeline_runs_updated;
                 DROP INDEX IF EXISTS note_pipeline_sections_order;
                 ALTER TABLE note_pipeline_sections RENAME TO note_pipeline_sections_v6;
                 ALTER TABLE note_pipeline_runs RENAME TO note_pipeline_runs_v6;
                 CREATE TABLE note_pipeline_runs (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL,
                    note_id TEXT REFERENCES library_notes(id) ON DELETE SET NULL,
                    phase TEXT NOT NULL CHECK (phase IN (
                        'preflight', 'analyzing', 'awaiting_outline', 'compiling', 'queued',
                        'drafting', 'validating', 'replanning', 'assembling', 'persisting',
                        'paused', 'blocked', 'done', 'cancelled', 'error'
                    )),
                    outline_json TEXT NOT NULL DEFAULT '',
                    selected_section_ids_json TEXT NOT NULL DEFAULT '[]',
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    max_output_tokens INTEGER NOT NULL,
                    thinking_enabled INTEGER NOT NULL DEFAULT 0 CHECK (thinking_enabled IN (0, 1)),
                    retry_attempts INTEGER NOT NULL DEFAULT 1,
                    input_snapshot_hash TEXT NOT NULL DEFAULT '',
                    current_plan_version INTEGER NOT NULL DEFAULT 0,
                    execution_version INTEGER NOT NULL DEFAULT 1,
                    budget_json TEXT NOT NULL DEFAULT '{}',
                    preflight_json TEXT NOT NULL DEFAULT '{}',
                    sidecar_json TEXT NOT NULL DEFAULT '',
                    idempotency_key TEXT NOT NULL DEFAULT '',
                    warnings_json TEXT NOT NULL DEFAULT '[]',
                    error_message TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 INSERT INTO note_pipeline_runs (
                    id, conversation_id, note_id, phase, outline_json, selected_section_ids_json,
                    provider_id, model_id, max_output_tokens, thinking_enabled, retry_attempts,
                    warnings_json, error_message, created_at, updated_at
                 ) SELECT id, conversation_id, note_id, phase, outline_json, selected_section_ids_json,
                    provider_id, model_id, max_output_tokens, thinking_enabled, retry_attempts,
                    warnings_json, error_message, created_at, updated_at
                   FROM note_pipeline_runs_v6;
                 CREATE TABLE note_pipeline_sections (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    section_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    section_json TEXT NOT NULL,
                    markdown TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
                        'pending', 'ready', 'in_progress', 'completed', 'needs_review',
                        'needs_revision', 'failed', 'blocked', 'skipped', 'interrupted'
                    )),
                    attempt_count INTEGER NOT NULL DEFAULT 0,
                    revision_count INTEGER NOT NULL DEFAULT 0,
                    evidence_ids_json TEXT NOT NULL DEFAULT '[]',
                    validation_json TEXT NOT NULL DEFAULT '',
                    input_hash TEXT NOT NULL DEFAULT '',
                    error_message TEXT,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, section_id)
                 );
                 INSERT INTO note_pipeline_sections (
                    run_id, section_id, position, section_json, markdown, status,
                    error_message, updated_at
                 ) SELECT run_id, section_id, position, section_json, markdown, status,
                    error_message, updated_at FROM note_pipeline_sections_v6;
                 DROP TABLE note_pipeline_sections_v6;
                 DROP TABLE note_pipeline_runs_v6;
                 CREATE UNIQUE INDEX IF NOT EXISTS note_pipeline_active_conversation
                    ON note_pipeline_runs(conversation_id)
                    WHERE phase IN (
                        'preflight', 'analyzing', 'awaiting_outline', 'compiling', 'queued',
                        'drafting', 'validating', 'replanning', 'assembling', 'persisting',
                        'paused', 'blocked', 'error'
                    );
                 CREATE UNIQUE INDEX IF NOT EXISTS note_pipeline_output_idempotency
                    ON note_pipeline_runs(idempotency_key) WHERE idempotency_key <> '';
                 CREATE INDEX IF NOT EXISTS note_pipeline_runs_updated
                    ON note_pipeline_runs(updated_at DESC);
                 CREATE INDEX IF NOT EXISTS note_pipeline_sections_order
                    ON note_pipeline_sections(run_id, position);
                 CREATE TABLE IF NOT EXISTS note_pipeline_plan_versions (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    version INTEGER NOT NULL,
                    plan_id TEXT NOT NULL,
                    plan_json TEXT NOT NULL,
                    compiled_dag_json TEXT NOT NULL,
                    plan_hash TEXT NOT NULL,
                    revision_reason TEXT NOT NULL DEFAULT '',
                    confirmed_at INTEGER,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, version)
                 );
                 CREATE INDEX IF NOT EXISTS note_pipeline_plan_hash
                    ON note_pipeline_plan_versions(run_id, plan_hash);
                 CREATE TABLE IF NOT EXISTS note_pipeline_nodes (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    plan_version INTEGER NOT NULL,
                    node_id TEXT NOT NULL,
                    node_type TEXT NOT NULL,
                    section_id TEXT,
                    depends_on_json TEXT NOT NULL DEFAULT '[]',
                    status TEXT NOT NULL,
                    attempt_count INTEGER NOT NULL DEFAULT 0,
                    evidence_ids_json TEXT NOT NULL DEFAULT '[]',
                    input_hash TEXT NOT NULL DEFAULT '',
                    output_ref TEXT,
                    validation_json TEXT NOT NULL DEFAULT '',
                    error_message TEXT,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, plan_version, node_id)
                 );
                 CREATE INDEX IF NOT EXISTS note_pipeline_nodes_ready
                    ON note_pipeline_nodes(run_id, plan_version, status);
                 CREATE TABLE IF NOT EXISTS note_pipeline_source_chunks (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    chunk_id TEXT NOT NULL,
                    source_kind TEXT NOT NULL,
                    source_id TEXT NOT NULL,
                    message_id TEXT,
                    attachment_id TEXT,
                    library_item_id TEXT,
                    location TEXT NOT NULL,
                    excerpt TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    ocr_confidence REAL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, chunk_id)
                 );
                 CREATE INDEX IF NOT EXISTS note_pipeline_chunks_source
                    ON note_pipeline_source_chunks(run_id, source_id);
                 CREATE TABLE IF NOT EXISTS note_pipeline_evidence (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    evidence_id TEXT NOT NULL,
                    section_id TEXT NOT NULL,
                    source_chunk_ids_json TEXT NOT NULL,
                    claim_text TEXT NOT NULL,
                    model_synthesis TEXT NOT NULL DEFAULT '',
                    source_excerpt TEXT NOT NULL,
                    support_level TEXT NOT NULL,
                    status TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, evidence_id)
                 );
                 CREATE INDEX IF NOT EXISTS note_pipeline_evidence_section
                    ON note_pipeline_evidence(run_id, section_id);
                 CREATE TABLE IF NOT EXISTS note_pipeline_ledgers (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    version INTEGER NOT NULL,
                    ledger_json TEXT NOT NULL,
                    patch_json TEXT NOT NULL DEFAULT '',
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, version)
                 );
                 CREATE TABLE IF NOT EXISTS note_pipeline_events (
                    run_id TEXT NOT NULL REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    sequence INTEGER NOT NULL,
                    event_type TEXT NOT NULL,
                    node_id TEXT,
                    payload_json TEXT NOT NULL DEFAULT '{}',
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (run_id, sequence)
                 );
                 CREATE TABLE IF NOT EXISTS note_pipeline_outputs (
                    run_id TEXT PRIMARY KEY REFERENCES note_pipeline_runs(id) ON DELETE CASCADE,
                    note_id TEXT REFERENCES library_notes(id) ON DELETE SET NULL,
                    markdown TEXT NOT NULL,
                    sidecar_json TEXT NOT NULL,
                    idempotency_key TEXT NOT NULL UNIQUE,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 7;
                 COMMIT;",
            )
            .map_err(|error| format!("升级深度笔记 Plan-and-Execute 结构失败：{error}"))?;
    }
    // v8：保存深度笔记完整覆盖快照，并让增量编辑提案在应用时原子推进快照。
    // 快照包含有序消息 ID、逐消息 Hash 和附件真实字节 Hash，用于拒绝编辑、删除、
    // 重排或附件替换后的不安全恢复与增量合并。
    if version <= 7 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS deep_note_coverage_snapshots (
                    note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
                    conversation_id TEXT NOT NULL,
                    snapshot_json TEXT NOT NULL,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY (note_id, conversation_id)
                 );
                 CREATE INDEX IF NOT EXISTS deep_note_coverage_conversation
                    ON deep_note_coverage_snapshots(conversation_id, updated_at DESC);
                 ALTER TABLE note_edit_proposals
                    ADD COLUMN coverage_snapshot_json TEXT NOT NULL DEFAULT '';
                 PRAGMA user_version = 8;
                 COMMIT;",
            )
            .map_err(|error| format!("升级深度笔记覆盖快照结构失败：{error}"))?;
    }
    // v9：附件级增量更新的 Source Unit 与提案暂存。
    if version <= 8 {
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS deep_note_source_units (
                    unit_id TEXT PRIMARY KEY,
                    note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
                    conversation_id TEXT NOT NULL,
                    message_id TEXT NOT NULL,
                    kind TEXT NOT NULL CHECK (kind IN (
                        'body', 'attachment', 'literatureSelection', 'noteSelection'
                    )),
                    attachment_id TEXT,
                    content_hash TEXT NOT NULL,
                    parser_id TEXT NOT NULL,
                    parser_version TEXT NOT NULL,
                    status TEXT NOT NULL CHECK (status IN (
                        'pending', 'extracted', 'covered', 'failed', 'unsupported'
                    )),
                    chunk_ids_json TEXT NOT NULL DEFAULT '[]',
                    evidence_ids_json TEXT NOT NULL DEFAULT '[]',
                    error_message TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS deep_note_source_units_note
                    ON deep_note_source_units(note_id, conversation_id);
                 CREATE UNIQUE INDEX IF NOT EXISTS deep_note_source_units_attachment
                    ON deep_note_source_units(note_id, conversation_id, attachment_id)
                    WHERE attachment_id IS NOT NULL;
                 CREATE TABLE IF NOT EXISTS note_edit_source_units (
                    proposal_id TEXT NOT NULL REFERENCES note_edit_proposals(id) ON DELETE CASCADE,
                    unit_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY (proposal_id, unit_json)
                 );
                 PRAGMA user_version = 9;
                 COMMIT;",
            )
            .map_err(|error| format!("升级深度笔记附件增量结构失败：{error}"))?;
    }
    // v10：增加可观测的 cancelling 阶段。取消命令先持久化该阶段，再等待
    // 后台任务协作退出；超时后由任务监督器强制终止并收敛到 cancelled。
    if version <= 9 {
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 PRAGMA legacy_alter_table = ON;
                 BEGIN IMMEDIATE;
                 CREATE TABLE note_pipeline_runs_v10 (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL,
                    note_id TEXT REFERENCES library_notes(id) ON DELETE SET NULL,
                    phase TEXT NOT NULL CHECK (phase IN (
                        'preflight', 'analyzing', 'awaiting_outline', 'compiling', 'queued',
                        'drafting', 'validating', 'replanning', 'assembling', 'persisting',
                        'cancelling', 'paused', 'blocked', 'done', 'cancelled', 'error'
                    )),
                    outline_json TEXT NOT NULL DEFAULT '',
                    selected_section_ids_json TEXT NOT NULL DEFAULT '[]',
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    max_output_tokens INTEGER NOT NULL,
                    thinking_enabled INTEGER NOT NULL DEFAULT 0 CHECK (thinking_enabled IN (0, 1)),
                    retry_attempts INTEGER NOT NULL DEFAULT 1,
                    input_snapshot_hash TEXT NOT NULL DEFAULT '',
                    current_plan_version INTEGER NOT NULL DEFAULT 0,
                    execution_version INTEGER NOT NULL DEFAULT 1,
                    budget_json TEXT NOT NULL DEFAULT '{}',
                    preflight_json TEXT NOT NULL DEFAULT '{}',
                    sidecar_json TEXT NOT NULL DEFAULT '',
                    idempotency_key TEXT NOT NULL DEFAULT '',
                    warnings_json TEXT NOT NULL DEFAULT '[]',
                    error_message TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 INSERT INTO note_pipeline_runs_v10 (
                    id, conversation_id, note_id, phase, outline_json, selected_section_ids_json,
                    provider_id, model_id, max_output_tokens, thinking_enabled, retry_attempts,
                    input_snapshot_hash, current_plan_version, execution_version, budget_json,
                    preflight_json, sidecar_json, idempotency_key, warnings_json, error_message,
                    created_at, updated_at
                 ) SELECT
                    id, conversation_id, note_id, phase, outline_json, selected_section_ids_json,
                    provider_id, model_id, max_output_tokens, thinking_enabled, retry_attempts,
                    input_snapshot_hash, current_plan_version, execution_version, budget_json,
                    preflight_json, sidecar_json, idempotency_key, warnings_json, error_message,
                    created_at, updated_at
                 FROM note_pipeline_runs;
                 DROP TABLE note_pipeline_runs;
                 ALTER TABLE note_pipeline_runs_v10 RENAME TO note_pipeline_runs;
                 CREATE UNIQUE INDEX note_pipeline_active_conversation
                    ON note_pipeline_runs(conversation_id)
                    WHERE phase IN (
                        'preflight', 'analyzing', 'awaiting_outline', 'compiling', 'queued',
                        'drafting', 'validating', 'replanning', 'assembling', 'persisting',
                        'cancelling', 'paused', 'blocked', 'error'
                    );
                 CREATE UNIQUE INDEX note_pipeline_output_idempotency
                    ON note_pipeline_runs(idempotency_key) WHERE idempotency_key <> '';
                 CREATE INDEX note_pipeline_runs_updated
                    ON note_pipeline_runs(updated_at DESC);
                 PRAGMA user_version = 10;
                 COMMIT;
                 PRAGMA legacy_alter_table = OFF;
                 PRAGMA foreign_keys = ON;",
            )
            .map_err(|error| format!("升级深度笔记取消状态结构失败：{error}"))?;
    }
    Ok(())
}

fn raw_item_from_row(row: &Row<'_>) -> rusqlite::Result<RawLibraryItem> {
    Ok(RawLibraryItem {
        id: row.get(0)?,
        title: row.get(1)?,
        authors_json: row.get(2)?,
        publication_year: row.get(3)?,
        publication_title: row.get(4)?,
        doi: row.get(5)?,
        abstract_text: row.get(6)?,
        favorite: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_opened_at: row.get(10)?,
        deleted_at: row.get(11)?,
        file_id: row.get(12)?,
        original_name: row.get(13)?,
        stored_name: row.get(14)?,
        file_size: row.get(15)?,
        file_hash: row.get(16)?,
        mime_type: row.get(17)?,
        file_created_at: row.get(18)?,
    })
}

fn annotation_from_row(row: &Row<'_>) -> rusqlite::Result<Result<LibraryAnnotation, String>> {
    let id = row.get::<_, String>(0)?;
    let item_id = row.get::<_, String>(1)?;
    let kind = row.get::<_, String>(2)?;
    let page_index = row.get::<_, i64>(3)?;
    let color = row.get::<_, String>(4)?;
    let text = row.get::<_, String>(5)?;
    let comment = row.get::<_, String>(6)?;
    let rects_json = row.get::<_, String>(7)?;
    let created_at = row.get::<_, i64>(8)?;
    let updated_at = row.get::<_, i64>(9)?;
    Ok((|| {
        Ok(LibraryAnnotation {
            id,
            item_id,
            kind: LibraryAnnotationKind::parse(&kind)?,
            page_index: u32::try_from(page_index)
                .map_err(|_| "数据库中的批注页码无效。".to_string())?,
            color: LibraryAnnotationColor::parse(&color)?,
            text,
            comment,
            rects: serde_json::from_str::<Vec<LibraryAnnotationRect>>(&rects_json)
                .map_err(|error| format!("解析批注区域失败：{error}"))?,
            created_at: i64_to_u64(created_at),
            updated_at: i64_to_u64(updated_at),
        })
    })())
}

/// 分组注册幂等：INSERT OR IGNORE，同名（不区分大小写）分组直接复用。
fn register_note_group(connection: &Connection, name: &str, now: i64) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO library_note_groups (name, created_at) VALUES (?, ?)",
            params![name, now],
        )
        .map_err(|error| format!("注册笔记分组失败：{error}"))?;
    Ok(())
}

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<LibraryNote> {
    Ok(LibraryNote {
        id: row.get(0)?,
        item_id: row.get(1)?,
        item_title: row.get(2)?,
        title: row.get(3)?,
        content: row.get(4)?,
        group_name: row.get(5)?,
        created_at: i64_to_u64(row.get(6)?),
        updated_at: i64_to_u64(row.get(7)?),
    })
}

fn note_summary_from_row(row: &Row<'_>) -> rusqlite::Result<LibraryNoteSummary> {
    Ok(LibraryNoteSummary {
        id: row.get(0)?,
        item_id: row.get(1)?,
        item_title: row.get(2)?,
        title: row.get(3)?,
        content_preview: row.get(4)?,
        content_chars: usize::try_from(row.get::<_, i64>(5)?).unwrap_or(usize::MAX),
        group_name: row.get(6)?,
        created_at: i64_to_u64(row.get(7)?),
        updated_at: i64_to_u64(row.get(8)?),
        content_bytes: usize::try_from(row.get::<_, i64>(9)?).unwrap_or(usize::MAX),
    })
}

fn note_source_from_row(row: &Row<'_>) -> rusqlite::Result<Result<NoteSource, String>> {
    let origin = row.get::<_, String>(3)?;
    let id = row.get(0)?;
    let note_id = row.get(1)?;
    let section_id = row.get(2)?;
    let conversation_id = row.get(4)?;
    let message_id = row.get(5)?;
    let summarized_until_message_id = row.get(6)?;
    let created_at = row.get::<_, i64>(7)?;
    Ok(NoteSourceOrigin::parse(&origin).map(|origin| NoteSource {
        id,
        note_id,
        section_id,
        origin,
        conversation_id,
        message_id,
        summarized_until_message_id,
        created_at: i64_to_u64(created_at),
    }))
}

fn get_note_pipeline_run_with_connection(
    connection: &Connection,
    run_id: &str,
) -> Result<Option<NotePipelineRun>, String> {
    let raw = connection
        .query_row(
            "SELECT id, conversation_id, note_id, phase, outline_json,
                    selected_section_ids_json, provider_id, model_id, max_output_tokens,
                    thinking_enabled, retry_attempts, warnings_json, error_message,
                    created_at, updated_at, input_snapshot_hash, current_plan_version,
                    execution_version, budget_json, preflight_json, sidecar_json, idempotency_key
             FROM note_pipeline_runs WHERE id = ?",
            params![run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, i64>(16)?,
                    row.get::<_, i64>(17)?,
                    row.get::<_, String>(18)?,
                    row.get::<_, String>(19)?,
                    row.get::<_, String>(20)?,
                    row.get::<_, String>(21)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("读取深度笔记任务失败：{error}"))?;
    let Some(raw) = raw else { return Ok(None) };
    let sections = get_note_pipeline_sections_with_connection(connection, &raw.0)?;
    let completed_section_ids = sections
        .iter()
        .filter(|section| section.status == NotePipelineSectionStatus::Completed)
        .map(|section| section.section_id.clone())
        .collect();
    let failed_section_ids = sections
        .iter()
        .filter(|section| section.status == NotePipelineSectionStatus::Failed)
        .map(|section| section.section_id.clone())
        .collect();
    Ok(Some(NotePipelineRun {
        id: raw.0,
        conversation_id: raw.1,
        note_id: raw.2,
        phase: NotePipelinePhase::parse(&raw.3)?,
        outline_json: raw.4,
        selected_section_ids: serde_json::from_str(&raw.5)
            .map_err(|error| format!("解析章节选择失败：{error}"))?,
        provider_id: raw.6,
        model_id: raw.7,
        max_output_tokens: u32::try_from(raw.8)
            .map_err(|_| "深度笔记 Token 上限无效。".to_string())?,
        thinking_enabled: raw.9 != 0,
        retry_attempts: u8::try_from(raw.10).map_err(|_| "深度笔记重试次数无效。".to_string())?,
        input_snapshot_hash: raw.15,
        current_plan_version: u32::try_from(raw.16)
            .map_err(|_| "深度笔记计划版本无效。".to_string())?,
        execution_version: u32::try_from(raw.17)
            .map_err(|_| "深度笔记执行版本无效。".to_string())?,
        budget_json: raw.18,
        preflight_json: raw.19,
        sidecar_json: raw.20,
        idempotency_key: raw.21,
        completed_section_ids,
        failed_section_ids,
        warnings: serde_json::from_str(&raw.11)
            .map_err(|error| format!("解析深度笔记检查提示失败：{error}"))?,
        abandoned: raw.12.as_deref() == Some("mnemora:abandoned"),
        error_message: raw.12,
        created_at: i64_to_u64(raw.13),
        updated_at: i64_to_u64(raw.14),
    }))
}

fn get_note_pipeline_sections_with_connection(
    connection: &Connection,
    run_id: &str,
) -> Result<Vec<NotePipelineSection>, String> {
    let mut statement = connection
        .prepare(
            "SELECT run_id, section_id, position, section_json, markdown, status,
                    error_message, updated_at, attempt_count, revision_count,
                    evidence_ids_json, validation_json, input_hash
             FROM note_pipeline_sections WHERE run_id = ? ORDER BY position ASC",
        )
        .map_err(|error| format!("准备深度笔记章节查询失败：{error}"))?;
    let rows = statement
        .query_map(params![run_id], |row| {
            let status = row.get::<_, String>(5)?;
            let position = row.get::<_, i64>(2)?;
            Ok(
                NotePipelineSectionStatus::parse(&status).and_then(|status| {
                    Ok(NotePipelineSection {
                        run_id: row.get(0).map_err(|error| error.to_string())?,
                        section_id: row.get(1).map_err(|error| error.to_string())?,
                        position: usize::try_from(position)
                            .map_err(|_| "深度笔记章节位置无效。".to_string())?,
                        section_json: row.get(3).map_err(|error| error.to_string())?,
                        markdown: row.get(4).map_err(|error| error.to_string())?,
                        status,
                        attempt_count: u8::try_from(
                            row.get::<_, i64>(8).map_err(|error| error.to_string())?,
                        )
                        .map_err(|_| "深度笔记章节尝试次数无效。".to_string())?,
                        revision_count: u8::try_from(
                            row.get::<_, i64>(9).map_err(|error| error.to_string())?,
                        )
                        .map_err(|_| "深度笔记章节修订次数无效。".to_string())?,
                        evidence_ids: serde_json::from_str(
                            &row.get::<_, String>(10)
                                .map_err(|error| error.to_string())?,
                        )
                        .map_err(|error| format!("解析章节证据失败：{error}"))?,
                        validation_json: row.get(11).map_err(|error| error.to_string())?,
                        input_hash: row.get(12).map_err(|error| error.to_string())?,
                        error_message: row.get(6).map_err(|error| error.to_string())?,
                        updated_at: i64_to_u64(row.get(7).map_err(|error| error.to_string())?),
                    })
                }),
            )
        })
        .map_err(|error| format!("查询深度笔记章节失败：{error}"))?;
    let mut sections = Vec::new();
    for row in rows {
        sections.push(row.map_err(|error| format!("读取深度笔记章节失败：{error}"))??);
    }
    Ok(sections)
}

fn get_note_edit_proposal_with_connection(
    connection: &Connection,
    proposal_id: &str,
) -> Result<Option<NoteEditProposal>, String> {
    connection
        .query_row(
            "SELECT id, note_id, conversation_id, source_message_id,
                    expected_note_updated_at, old_title, new_title, old_content,
                    new_content, diff_text, created_at
             FROM note_edit_proposals WHERE id = ? AND status = 'pending'",
            params![proposal_id],
            |row| {
                Ok(NoteEditProposal {
                    id: row.get(0)?,
                    note_id: row.get(1)?,
                    conversation_id: row.get(2)?,
                    source_message_id: row.get(3)?,
                    expected_note_updated_at: i64_to_u64(row.get(4)?),
                    old_title: row.get(5)?,
                    new_title: row.get(6)?,
                    old_content: row.get(7)?,
                    new_content: row.get(8)?,
                    diff: row.get(9)?,
                    created_at: i64_to_u64(row.get(10)?),
                })
            },
        )
        .optional()
        .map_err(|error| format!("读取笔记修改提案失败：{error}"))
}

fn normalize_note_sources(sources: Vec<NoteSourceCreate>) -> Result<Vec<NoteSourceCreate>, String> {
    if sources.len() > MAX_NOTE_SOURCES {
        return Err(format!("单篇笔记最多允许 {MAX_NOTE_SOURCES} 条来源记录。"));
    }
    sources
        .into_iter()
        .map(NoteSourceCreate::normalize_and_validate)
        .collect()
}

fn normalize_coverage_snapshot(snapshot: &DeepNoteInputSnapshot) -> Result<String, String> {
    if snapshot.message_ids.is_empty()
        || snapshot.message_ids.len() != snapshot.message_content_hashes.len()
        || snapshot.attachment_ids.len() != snapshot.attachment_content_hashes.len()
    {
        return Err("深度笔记覆盖快照缺少完整的消息或附件 Hash。".to_string());
    }
    let json = serde_json::to_string(snapshot)
        .map_err(|error| format!("序列化深度笔记覆盖快照失败：{error}"))?;
    if json.len() > MAX_NOTE_PIPELINE_JSON_BYTES {
        return Err("深度笔记覆盖快照过长。".to_string());
    }
    Ok(json)
}

fn normalize_coverage_snapshot_json(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Ok(String::new());
    }
    let snapshot = serde_json::from_str::<DeepNoteInputSnapshot>(value)
        .map_err(|error| format!("解析深度笔记覆盖快照失败：{error}"))?;
    normalize_coverage_snapshot(&snapshot)
}

fn upsert_deep_note_coverage_snapshot(
    connection: &Connection,
    note_id: &str,
    conversation_id: &str,
    snapshot_json: &str,
    updated_at: i64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO deep_note_coverage_snapshots (
                note_id, conversation_id, snapshot_json, updated_at
             ) VALUES (?, ?, ?, ?)
             ON CONFLICT(note_id, conversation_id) DO UPDATE SET
                snapshot_json = excluded.snapshot_json,
                updated_at = excluded.updated_at",
            params![note_id, conversation_id, snapshot_json, updated_at],
        )
        .map_err(|error| format!("保存深度笔记覆盖快照失败：{error}"))?;
    Ok(())
}

fn insert_note_sources(
    connection: &Connection,
    note_id: &str,
    sources: Vec<NoteSourceCreate>,
    created_at: i64,
) -> Result<(), String> {
    for source in sources {
        connection
            .execute(
                "INSERT INTO note_sources (
                    id, note_id, section_id, origin, conversation_id, message_id,
                    summarized_until_message_id, created_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    Uuid::new_v4().to_string(),
                    note_id,
                    source.section_id,
                    source.origin.as_str(),
                    source.conversation_id,
                    source.message_id,
                    source.summarized_until_message_id,
                    created_at,
                ],
            )
            .map_err(|error| format!("写入笔记来源失败：{error}"))?;
    }
    Ok(())
}

fn insert_deep_note_source_units(
    connection: &Connection,
    note_id: &str,
    conversation_id: &str,
    units: &[DeepNoteSourceUnit],
) -> Result<(), String> {
    for unit in units {
        connection
            .execute(
                "INSERT INTO deep_note_source_units (
                    unit_id, note_id, conversation_id, message_id, kind, attachment_id,
                    content_hash, parser_id, parser_version, status, chunk_ids_json,
                    evidence_ids_json, error_message, created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    unit.unit_id,
                    note_id,
                    conversation_id,
                    unit.message_id,
                    unit.kind.as_str(),
                    unit.attachment_id,
                    unit.content_hash,
                    unit.parser_id,
                    unit.parser_version,
                    unit.status.as_str(),
                    serde_json::to_string(&unit.chunk_ids)
                        .map_err(|error| format!("序列化来源单元 Chunk 引用失败：{error}"))?,
                    serde_json::to_string(&unit.evidence_ids)
                        .map_err(|error| format!("序列化来源单元 Evidence 引用失败：{error}"))?,
                    unit.error_message,
                    i64::try_from(unit.created_at).unwrap_or(i64::MAX),
                    i64::try_from(unit.updated_at).unwrap_or(i64::MAX),
                ],
            )
            .map_err(|error| format!("写入深度笔记来源单元失败：{error}"))?;
    }
    Ok(())
}

fn source_units_from_snapshot(
    note_id: &str,
    conversation_id: &str,
    snapshot: &DeepNoteInputSnapshot,
    created_at: u64,
) -> Vec<DeepNoteSourceUnit> {
    let mut units = Vec::new();
    for (index, message_id) in snapshot.message_ids.iter().enumerate() {
        if let Some(content_hash) = snapshot.message_content_hashes.get(index) {
            units.push(DeepNoteSourceUnit {
                unit_id: format!("{}:body:{message_id}", note_id),
                note_id: note_id.to_string(),
                conversation_id: conversation_id.to_string(),
                message_id: message_id.clone(),
                kind: DeepNoteSourceUnitKind::Body,
                attachment_id: None,
                content_hash: content_hash.clone(),
                parser_id: "conversation-body".to_string(),
                parser_version: "1".to_string(),
                status: DeepNoteSourceUnitStatus::Covered,
                chunk_ids: Vec::new(),
                evidence_ids: Vec::new(),
                error_message: None,
                created_at,
                updated_at: created_at,
            });
        }
    }
    for (index, attachment_id) in snapshot.attachment_ids.iter().enumerate() {
        if let Some(content_hash) = snapshot.attachment_content_hashes.get(index) {
            units.push(DeepNoteSourceUnit {
                unit_id: format!("{}:attachment:{attachment_id}", note_id),
                note_id: note_id.to_string(),
                conversation_id: conversation_id.to_string(),
                message_id: snapshot
                    .attachment_message_ids
                    .get(index)
                    .cloned()
                    .or_else(|| snapshot.message_ids.last().cloned())
                    .unwrap_or_default(),
                kind: DeepNoteSourceUnitKind::Attachment,
                attachment_id: Some(attachment_id.clone()),
                content_hash: content_hash.clone(),
                parser_id: "deep-note-reader".to_string(),
                parser_version: "1".to_string(),
                status: DeepNoteSourceUnitStatus::Covered,
                chunk_ids: Vec::new(),
                evidence_ids: Vec::new(),
                error_message: None,
                created_at,
                updated_at: created_at,
            });
        }
    }
    units
}

fn load_deep_note_source_units(
    connection: &Connection,
    note_id: &str,
    conversation_id: &str,
) -> Result<Vec<DeepNoteSourceUnit>, String> {
    let mut statement = connection
        .prepare(
            "SELECT unit_id, message_id, kind, attachment_id, content_hash, parser_id,
                    parser_version, status, chunk_ids_json, evidence_ids_json,
                    error_message, created_at, updated_at
             FROM deep_note_source_units
             WHERE note_id = ? AND conversation_id = ?",
        )
        .map_err(|error| format!("准备深度笔记来源单元读取失败：{error}"))?;
    let rows = statement
        .query_map(params![note_id, conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, i64>(12)?,
            ))
        })
        .map_err(|error| format!("读取深度笔记来源单元失败：{error}"))?;
    rows.map(|row| {
        let raw = row.map_err(|error| format!("读取深度笔记来源单元失败：{error}"))?;
        Ok(DeepNoteSourceUnit {
            unit_id: raw.0,
            note_id: note_id.to_string(),
            conversation_id: conversation_id.to_string(),
            message_id: raw.1,
            kind: DeepNoteSourceUnitKind::parse(&raw.2)?,
            attachment_id: raw.3,
            content_hash: raw.4,
            parser_id: raw.5,
            parser_version: raw.6,
            status: DeepNoteSourceUnitStatus::parse(&raw.7)?,
            chunk_ids: serde_json::from_str(&raw.8)
                .map_err(|error| format!("解析来源单元 Chunk 引用失败：{error}"))?,
            evidence_ids: serde_json::from_str(&raw.9)
                .map_err(|error| format!("解析来源单元 Evidence 引用失败：{error}"))?,
            error_message: raw.10,
            created_at: i64_to_u64(raw.11),
            updated_at: i64_to_u64(raw.12),
        })
    })
    .collect()
}

fn pending_note_edit_source_units(
    connection: &Connection,
    proposal_id: &str,
) -> Result<Vec<DeepNoteSourceUnit>, String> {
    let mut statement = connection
        .prepare(
            "SELECT unit_json FROM note_edit_source_units
             WHERE proposal_id = ? ORDER BY created_at ASC",
        )
        .map_err(|error| format!("准备笔记增量来源单元查询失败：{error}"))?;
    let rows = statement
        .query_map(params![proposal_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("查询笔记增量来源单元失败：{error}"))?;
    rows.map(|row| {
        let value = row.map_err(|error| format!("读取笔记增量来源单元失败：{error}"))?;
        serde_json::from_str(&value).map_err(|error| format!("解析笔记增量来源单元失败：{error}"))
    })
    .collect()
}

fn build_item_filters(request: &LibraryListRequest) -> (String, Vec<Value>) {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    match request.view {
        LibraryView::Trash => clauses.push("i.deleted_at IS NOT NULL".to_string()),
        _ => clauses.push("i.deleted_at IS NULL".to_string()),
    }
    match request.view {
        LibraryView::Recent => clauses.push("i.last_opened_at IS NOT NULL".to_string()),
        LibraryView::Favorites => clauses.push("i.favorite = 1".to_string()),
        LibraryView::Unfiled => clauses.push(
            "NOT EXISTS (
                SELECT 1 FROM library_item_collections unfiled
                WHERE unfiled.item_id = i.id
             )"
            .to_string(),
        ),
        _ => {}
    }
    if let Some(collection_id) = &request.collection_id {
        clauses.push(
            "EXISTS (
                SELECT 1 FROM library_item_collections selected_collection
                WHERE selected_collection.item_id = i.id
                  AND selected_collection.collection_id = ?
             )"
            .to_string(),
        );
        values.push(Value::Text(collection_id.clone()));
    }
    if !request.search_query.is_empty() {
        let pattern = format!("%{}%", escape_like(&request.search_query));
        clauses.push(
            "(
                i.title LIKE ? ESCAPE '\\' COLLATE NOCASE OR
                i.authors_json LIKE ? ESCAPE '\\' COLLATE NOCASE OR
                i.publication_title LIKE ? ESCAPE '\\' COLLATE NOCASE OR
                i.doi LIKE ? ESCAPE '\\' COLLATE NOCASE OR
                EXISTS (
                    SELECT 1
                    FROM library_item_tags search_item_tags
                    JOIN library_tags search_tags ON search_tags.id = search_item_tags.tag_id
                    WHERE search_item_tags.item_id = i.id
                      AND search_tags.name LIKE ? ESCAPE '\\' COLLATE NOCASE
                ) OR
                EXISTS (
                    SELECT 1
                    FROM library_item_collections search_item_collections
                    JOIN library_collections search_collections
                      ON search_collections.id = search_item_collections.collection_id
                    WHERE search_item_collections.item_id = i.id
                      AND search_collections.name LIKE ? ESCAPE '\\' COLLATE NOCASE
                )
             )"
            .to_string(),
        );
        for _ in 0..6 {
            values.push(Value::Text(pattern.clone()));
        }
    }
    (clauses.join(" AND "), values)
}

fn item_order_by(sort: LibrarySort, view: LibraryView) -> &'static str {
    if view == LibraryView::Recent {
        return "i.last_opened_at DESC, i.title COLLATE NOCASE ASC";
    }
    match sort {
        LibrarySort::Updated => "i.updated_at DESC, i.title COLLATE NOCASE ASC",
        LibrarySort::Title => "i.title COLLATE NOCASE ASC, i.updated_at DESC",
        LibrarySort::Year => {
            "i.publication_year IS NULL ASC, i.publication_year DESC, i.title COLLATE NOCASE ASC"
        }
        LibrarySort::Imported => "i.created_at DESC, i.title COLLATE NOCASE ASC",
    }
}

fn ensure_item_exists(connection: &Connection, item_id: &str) -> Result<(), String> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM library_items WHERE id = ?",
            params![item_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| format!("检查文献记录失败：{error}"))?
        .is_some();
    if !exists {
        return Err("文献不存在。".to_string());
    }
    Ok(())
}

fn ensure_active_item_exists(connection: &Connection, item_id: &str) -> Result<(), String> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM library_items WHERE id = ? AND deleted_at IS NULL",
            params![item_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| format!("检查活动文献记录失败：{error}"))?
        .is_some();
    if !exists {
        return Err("文献不存在或位于回收站。".to_string());
    }
    Ok(())
}

fn ensure_collections_exist(
    connection: &Connection,
    collection_ids: &[String],
) -> Result<(), String> {
    for collection_id in collection_ids {
        let exists = connection
            .query_row(
                "SELECT 1 FROM library_collections WHERE id = ?",
                params![collection_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|error| format!("检查分类失败：{error}"))?
            .is_some();
        if !exists {
            return Err(format!("分类 {collection_id} 不存在。"));
        }
    }
    Ok(())
}

fn replace_item_collections(
    connection: &Connection,
    item_id: &str,
    collection_ids: &[String],
) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM library_item_collections WHERE item_id = ?",
            params![item_id],
        )
        .map_err(|error| format!("清理旧分类关联失败：{error}"))?;
    for collection_id in collection_ids {
        connection
            .execute(
                "INSERT INTO library_item_collections (item_id, collection_id) VALUES (?, ?)",
                params![item_id, collection_id],
            )
            .map_err(|error| format!("保存分类关联失败：{error}"))?;
    }
    Ok(())
}

fn replace_item_tags(
    connection: &Connection,
    item_id: &str,
    tags: &[String],
    now: i64,
) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM library_item_tags WHERE item_id = ?",
            params![item_id],
        )
        .map_err(|error| format!("清理旧标签关联失败：{error}"))?;
    for tag in tags {
        let tag_id = connection
            .query_row(
                "SELECT id FROM library_tags WHERE name = ? COLLATE NOCASE",
                params![tag],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("查询标签失败：{error}"))?
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        connection
            .execute(
                "INSERT OR IGNORE INTO library_tags (id, name, created_at) VALUES (?, ?, ?)",
                params![tag_id, tag, now],
            )
            .map_err(|error| format!("保存标签失败：{error}"))?;
        connection
            .execute(
                "INSERT INTO library_item_tags (item_id, tag_id) VALUES (?, ?)",
                params![item_id, tag_id],
            )
            .map_err(|error| format!("保存文献标签关联失败：{error}"))?;
    }
    connection
        .execute(
            "DELETE FROM library_tags
             WHERE NOT EXISTS (
                SELECT 1 FROM library_item_tags it WHERE it.tag_id = library_tags.id
             )",
            [],
        )
        .map_err(|error| format!("清理未使用标签失败：{error}"))?;
    Ok(())
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn now_millis_i64() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX)
}

pub(crate) fn library_now_millis() -> i64 {
    now_millis_i64()
}

fn i64_to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn is_unique_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                || code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use rusqlite::Connection;

    use super::LibraryRepository;
    use crate::chat::note_pipeline::types::{
        DeepNoteCapabilities, DeepNoteInputSnapshot, DeepNoteModelSnapshot, DeepNoteSourceUnit,
        DeepNoteSourceUnitKind, DeepNoteSourceUnitStatus,
    };
    use crate::library::types::{
        LibraryAnnotationColor, LibraryAnnotationCreate, LibraryAnnotationKind,
        LibraryAnnotationRect, LibraryAnnotationUpdate, LibraryItemUpdate, LibraryListRequest,
        LibraryNoteCreate, LibraryNoteRename, LibraryNoteUpdate, LibraryReadingStateUpdate,
        LibraryView, NoteEditProposalCreate, NotePipelinePhase, NotePipelineRunCreate,
        NotePipelineSectionCreate, NotePipelineSectionStatus, NoteSourceCreate, NoteSourceOrigin,
        MAX_PDF_RANGE_BYTES,
    };

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mnemora-library-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn coverage_snapshot(message_ids: &[&str]) -> DeepNoteInputSnapshot {
        DeepNoteInputSnapshot {
            conversation_revision: message_ids.len() as u64,
            message_ids: message_ids
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            message_content_hashes: message_ids
                .iter()
                .map(|value| format!("hash-{value}"))
                .collect(),
            attachment_ids: Vec::new(),
            attachment_content_hashes: Vec::new(),
            attachment_message_ids: Vec::new(),
            selected_literature_ids: Vec::new(),
            selected_note_ids: Vec::new(),
            model: DeepNoteModelSnapshot {
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                api_model: "model-1".to_string(),
                context_window_tokens: Some(128_000),
                capabilities: DeepNoteCapabilities {
                    tools: Some(true),
                    vision: Some(true),
                    reasoning: Some(true),
                    structured_outputs: true,
                },
            },
            permission_mode: "askSensitive".to_string(),
            created_at: 1,
        }
    }

    #[test]
    fn creates_collections_and_keeps_deleted_items_out_of_normal_views() {
        let directory = test_directory("schema");
        let repository = LibraryRepository::new(directory.clone());
        let collection = repository.create_collection("研究资料").unwrap();
        assert_eq!(collection.item_count, 0);
        assert_eq!(repository.list_collections().unwrap().len(), 1);
        assert!(repository.rename_collection(&collection.id, "论文").is_ok());
        assert!(repository.delete_collection(&collection.id).unwrap());

        let page = repository
            .list_items(serde_json::from_value(serde_json::json!({ "view": "all" })).unwrap())
            .unwrap();
        assert_eq!(page.total, 0);
        let trash = repository
            .list_items(LibraryListRequest {
                view: LibraryView::Trash,
                ..serde_json::from_str("{}").unwrap()
            })
            .unwrap();
        assert_eq!(trash.total, 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn imports_deduplicates_updates_searches_and_deletes_pdf_snapshots() {
        let directory = test_directory("roundtrip");
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("研究论文.pdf");
        fs::write(&source, b"%PDF-1.7\nminimal test content").unwrap();
        let repository = LibraryRepository::new(directory.clone());
        let collection = repository.create_collection("论文").unwrap();

        let imported = repository
            .import_pdfs(
                vec![source.to_string_lossy().into_owned()],
                Some(collection.id.clone()),
            )
            .unwrap();
        assert_eq!(imported.imported.len(), 1);
        let item = &imported.imported[0];
        assert_eq!(item.collection_names, vec!["论文"]);
        assert!(item.file.available);
        let snapshot = directory
            .join("library")
            .join("files")
            .join(format!("{}.pdf", item.file.id));
        assert!(snapshot.is_file());

        let duplicate = repository
            .import_pdfs(vec![source.to_string_lossy().into_owned()], None)
            .unwrap();
        assert_eq!(duplicate.duplicates.len(), 1);
        assert!(duplicate.imported.is_empty());

        let update: LibraryItemUpdate = serde_json::from_value(serde_json::json!({
            "itemId": item.id,
            "title": "Updated Research Paper",
            "authors": ["Alice"],
            "publicationYear": 2026,
            "publicationTitle": "Mnemora Journal",
            "doi": "10.1/example",
            "abstractText": "Abstract",
            "favorite": true,
            "tags": ["Agent", "PDF"],
            "collectionIds": [collection.id]
        }))
        .unwrap();
        let updated = repository.update_item(update).unwrap();
        assert_eq!(updated.authors, vec!["Alice"]);
        assert_eq!(updated.tags, vec!["Agent", "PDF"]);

        let search: LibraryListRequest = serde_json::from_value(serde_json::json!({
            "view": "all",
            "searchQuery": "Agent"
        }))
        .unwrap();
        assert_eq!(repository.list_items(search).unwrap().total, 1);
        let favorites: LibraryListRequest =
            serde_json::from_value(serde_json::json!({ "view": "favorites" })).unwrap();
        assert_eq!(repository.list_items(favorites).unwrap().total, 1);

        repository.mark_opened(&item.id).unwrap();
        let recent: LibraryListRequest =
            serde_json::from_value(serde_json::json!({ "view": "recent" })).unwrap();
        assert_eq!(repository.list_items(recent).unwrap().total, 1);

        repository.move_to_trash(&item.id).unwrap();
        let all: LibraryListRequest =
            serde_json::from_value(serde_json::json!({ "view": "all" })).unwrap();
        assert_eq!(repository.list_items(all.clone()).unwrap().total, 0);
        let trash: LibraryListRequest =
            serde_json::from_value(serde_json::json!({ "view": "trash" })).unwrap();
        assert_eq!(repository.list_items(trash).unwrap().total, 1);
        repository.restore_from_trash(&item.id).unwrap();
        assert_eq!(repository.list_items(all).unwrap().total, 1);

        let reopened_repository = LibraryRepository::new(directory.clone());
        assert_eq!(
            reopened_repository.get_item(&item.id).unwrap().title,
            "Updated Research Paper"
        );
        assert!(!repository.delete_permanently(&item.id).unwrap());
        assert!(snapshot.exists());
        repository.move_to_trash(&item.id).unwrap();
        assert!(repository.delete_permanently(&item.id).unwrap());
        assert!(!snapshot.exists());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn persists_reading_state_and_reads_bounded_pdf_ranges() {
        let directory = test_directory("reading-state");
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("reading.pdf");
        fs::write(&source, b"%PDF-1.7\n0123456789abcdef").unwrap();
        let repository = LibraryRepository::new(directory.clone());
        let imported = repository
            .import_pdfs(vec![source.to_string_lossy().into_owned()], None)
            .unwrap();
        let item = &imported.imported[0];

        assert_eq!(
            repository.read_pdf_range(&item.id, 5, 11).unwrap(),
            b"1.7\n01"
        );
        let state = repository.get_reading_state(&item.id).unwrap();
        assert_eq!(state.page_index, 0);
        let saved = repository
            .save_reading_state(LibraryReadingStateUpdate {
                item_id: item.id.clone(),
                page_index: 4,
                scroll_offset: 0.25,
                zoom: 1.5,
            })
            .unwrap();
        assert_eq!(saved.page_index, 4);
        assert_eq!(repository.get_reading_state(&item.id).unwrap().zoom, 1.5);
        assert!(repository
            .read_pdf_range(&item.id, 0, MAX_PDF_RANGE_BYTES + 1)
            .is_err());

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn creates_updates_lists_and_cascades_annotations_and_notes() {
        let directory = test_directory("annotations-notes");
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("annotated.pdf");
        fs::write(&source, b"%PDF-1.7\nannotation test").unwrap();
        let repository = LibraryRepository::new(directory.clone());
        let item = repository
            .import_pdfs(vec![source.to_string_lossy().into_owned()], None)
            .unwrap()
            .imported
            .remove(0);

        let annotation = repository
            .create_annotation(LibraryAnnotationCreate {
                item_id: item.id.clone(),
                kind: LibraryAnnotationKind::Highlight,
                page_index: 3,
                color: LibraryAnnotationColor::Yellow,
                text: "selected passage".to_string(),
                comment: String::new(),
                rects: vec![LibraryAnnotationRect {
                    x: 0.1,
                    y: 0.2,
                    width: 0.3,
                    height: 0.04,
                }],
            })
            .unwrap();
        assert_eq!(repository.list_annotations(&item.id).unwrap().len(), 1);
        let annotation = repository
            .update_annotation(LibraryAnnotationUpdate {
                annotation_id: annotation.id.clone(),
                color: LibraryAnnotationColor::Blue,
                comment: "important".to_string(),
            })
            .unwrap();
        assert_eq!(annotation.comment, "important");
        assert_eq!(annotation.color, LibraryAnnotationColor::Blue);

        let note = repository
            .create_note(LibraryNoteCreate {
                item_id: Some(item.id.clone()),
                title: "Reading note".to_string(),
                content: "Initial content".to_string(),
                group_name: None,
            })
            .unwrap();
        assert_eq!(repository.list_notes(Some(&item.id)).unwrap().len(), 1);
        assert_eq!(
            repository.list_notes(None).unwrap()[0].item_title,
            Some(item.title.clone())
        );
        let note = repository
            .update_note(LibraryNoteUpdate {
                note_id: note.id.clone(),
                title: "Updated note".to_string(),
                content: "x".repeat(700),
            })
            .unwrap();
        assert_eq!(repository.get_note(&note.id).unwrap().title, "Updated note");
        let summaries = repository.list_notes(None).unwrap();
        assert_eq!(summaries[0].content_preview.chars().count(), 600);
        assert_eq!(summaries[0].content_chars, 700);
        assert_eq!(summaries[0].content_bytes, 700);
        let renamed = repository
            .rename_note(LibraryNoteRename {
                note_id: note.id.clone(),
                title: "Renamed without loading content".to_string(),
            })
            .unwrap();
        assert_eq!(renamed.title, "Renamed without loading content");
        assert_eq!(renamed.content, "x".repeat(700));

        let global_note = repository
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "Global markdown".to_string(),
                content: "# 全局\n\n独立笔记".to_string(),
                group_name: None,
            })
            .unwrap();
        assert!(global_note.item_id.is_none());
        assert!(global_note.item_title.is_none());
        assert_eq!(
            repository.get_note(&global_note.id).unwrap().title,
            "Global markdown"
        );
        let global_summary = repository
            .list_notes(None)
            .unwrap()
            .into_iter()
            .find(|summary| summary.id == global_note.id)
            .unwrap();
        assert_eq!(
            global_summary.content_bytes,
            "# 全局\n\n独立笔记".as_bytes().len()
        );

        assert!(repository.delete_annotation(&annotation.id).unwrap());
        assert!(repository.delete_note(&note.id).unwrap());
        assert!(repository.delete_note(&global_note.id).unwrap());
        assert!(repository.list_annotations(&item.id).unwrap().is_empty());
        assert!(repository.list_notes(Some(&item.id)).unwrap().is_empty());

        repository
            .create_annotation(LibraryAnnotationCreate {
                item_id: item.id.clone(),
                kind: LibraryAnnotationKind::Area,
                page_index: 0,
                color: LibraryAnnotationColor::Pink,
                text: String::new(),
                comment: String::new(),
                rects: vec![LibraryAnnotationRect {
                    x: 0.2,
                    y: 0.2,
                    width: 0.2,
                    height: 0.2,
                }],
            })
            .unwrap();
        repository
            .create_note(LibraryNoteCreate {
                item_id: Some(item.id.clone()),
                title: "Cascade note".to_string(),
                content: String::new(),
                group_name: None,
            })
            .unwrap();
        repository.move_to_trash(&item.id).unwrap();
        assert!(repository.delete_permanently(&item.id).unwrap());
        let connection = repository.open_connection().unwrap();
        let remaining: i64 = connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM library_annotations) +
                    (SELECT COUNT(*) FROM library_notes)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn imports_markdown_files_as_global_notes() {
        let root = test_directory("import-markdown-notes");
        let repository = LibraryRepository::new(root.join("app-data"));
        let source = root.join("research.md");
        fs::create_dir_all(&root).unwrap();
        fs::write(&source, "# Research\n\nEvidence").unwrap();

        let result = repository
            .import_markdown_notes(vec![source.to_string_lossy().into_owned()])
            .unwrap();

        assert_eq!(result.imported.len(), 1);
        assert!(result.failed.is_empty());
        assert_eq!(result.imported[0].title, "research");
        assert_eq!(result.imported[0].content, "# Research\n\nEvidence");
        assert!(result.imported[0].item_id.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migrates_version_one_databases_to_annotation_and_note_schema() {
        let directory = test_directory("migration-v2");
        let library_directory = directory.join("library");
        fs::create_dir_all(&library_directory).unwrap();
        let database_path = library_directory.join("library.sqlite3");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_items (id TEXT PRIMARY KEY);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let repository = LibraryRepository::new(directory.clone());
        let connection = repository.open_connection().unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10);
        let event_parent: String = connection
            .query_row("PRAGMA foreign_key_list(note_pipeline_events)", [], |row| {
                row.get(2)
            })
            .unwrap();
        assert_eq!(event_parent, "note_pipeline_runs");
        let foreign_key_violations: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(foreign_key_violations, 0);
        let tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table'
                   AND name IN (
                     'library_annotations', 'library_notes', 'library_note_groups', 'note_sources'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 4);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn creates_lists_detaches_and_cascades_note_sources() {
        let directory = test_directory("note-sources");
        let repository = LibraryRepository::new(directory.clone());
        let note = repository
            .create_note_with_sources(
                LibraryNoteCreate {
                    item_id: None,
                    title: "MVCC 深度笔记".to_string(),
                    content: "# MVCC\n\n正文".to_string(),
                    group_name: None,
                },
                vec![
                    NoteSourceCreate {
                        section_id: "sec-1".to_string(),
                        origin: NoteSourceOrigin::Conversation,
                        conversation_id: Some("conversation-1".to_string()),
                        message_id: Some("message-1".to_string()),
                        summarized_until_message_id: Some("message-1".to_string()),
                    },
                    NoteSourceCreate {
                        section_id: "sec-2".to_string(),
                        origin: NoteSourceOrigin::AiSupplement,
                        conversation_id: None,
                        message_id: None,
                        summarized_until_message_id: None,
                    },
                ],
            )
            .unwrap();

        let sources = repository.list_note_sources(&note.id).unwrap();
        assert_eq!(sources.len(), 2);
        let conversation_source = sources
            .iter()
            .find(|source| source.origin == NoteSourceOrigin::Conversation)
            .unwrap();
        assert_eq!(
            conversation_source.conversation_id.as_deref(),
            Some("conversation-1")
        );
        assert!(sources
            .iter()
            .any(|source| source.origin == NoteSourceOrigin::AiSupplement));

        assert_eq!(
            repository
                .detach_note_sources_for_conversation("conversation-1")
                .unwrap(),
            1
        );
        let detached = repository.list_note_sources(&note.id).unwrap();
        let detached_source = detached
            .iter()
            .find(|source| source.origin == NoteSourceOrigin::Conversation)
            .unwrap();
        assert!(detached_source.conversation_id.is_none());
        assert!(detached_source.message_id.is_none());
        assert!(detached_source.summarized_until_message_id.is_none());
        assert_eq!(
            repository.get_note(&note.id).unwrap().content,
            "# MVCC\n\n正文"
        );

        assert!(repository.delete_note(&note.id).unwrap());
        let connection = repository.open_connection().unwrap();
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM note_sources", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_invalid_note_sources_without_persisting_note() {
        let directory = test_directory("invalid-note-sources");
        let repository = LibraryRepository::new(directory.clone());
        let result = repository.create_note_with_sources(
            LibraryNoteCreate {
                item_id: None,
                title: "Invalid".to_string(),
                content: "# Invalid".to_string(),
                group_name: None,
            },
            vec![NoteSourceCreate {
                section_id: "sec-1".to_string(),
                origin: NoteSourceOrigin::Conversation,
                conversation_id: None,
                message_id: Some("message-1".to_string()),
                summarized_until_message_id: None,
            }],
        );
        assert!(result.is_err());
        assert!(repository.list_notes(None).unwrap().is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn note_groups_cover_assignment_and_cleanup() {
        let directory = test_directory("note-groups");
        let repository = LibraryRepository::new(directory.clone());

        // 空分组可以先创建并保留；重名（含大小写差异）被拒绝。
        let group = repository.create_note_group("数据库").unwrap();
        assert_eq!(group.note_count, 0);
        assert!(repository.create_note_group("数据库").is_err());

        let note = repository
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "MVCC 笔记".to_string(),
                content: "# MVCC".to_string(),
                group_name: Some("数据库".to_string()),
            })
            .unwrap();
        assert_eq!(note.group_name.as_deref(), Some("数据库"));
        assert_eq!(repository.list_note_groups().unwrap()[0].note_count, 1);
        assert_eq!(
            repository.list_notes(None).unwrap()[0]
                .group_name
                .as_deref(),
            Some("数据库"),
        );

        // set_note_group 自动注册新分组；调整分组不改变 updated_at 排序语义。
        let updated_before = repository.get_note(&note.id).unwrap().updated_at;
        let moved = repository.set_note_group(&note.id, Some("英语")).unwrap();
        assert_eq!(moved.group_name.as_deref(), Some("英语"));
        assert_eq!(moved.updated_at, updated_before);
        assert_eq!(repository.list_note_groups().unwrap().len(), 2);

        // 传 None 回到未分类；删除分组把残留笔记恢复为未分类。
        let cleared = repository.set_note_group(&note.id, None).unwrap();
        assert!(cleared.group_name.is_none());
        repository.set_note_group(&note.id, Some("英语")).unwrap();
        assert!(repository.delete_note_group("英语").unwrap());
        assert!(repository.get_note(&note.id).unwrap().group_name.is_none());
        assert!(!repository.delete_note_group("英语").unwrap());

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn note_pipeline_run_accepts_zero_retries_when_auto_retry_is_disabled() {
        let directory = test_directory("note-pipeline-zero-retries");
        let repository = LibraryRepository::new(directory.clone());

        let run = repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "run-zero-retries".to_string(),
                conversation_id: "conversation-zero-retries".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: false,
                retry_attempts: 0,
                input_snapshot_hash: "snapshot-zero-retries".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-zero-retries".to_string(),
            })
            .unwrap();

        assert_eq!(run.retry_attempts, 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn cancellation_state_rejects_stale_progress_and_recovers_after_restart() {
        let directory = test_directory("note-pipeline-cancelling");
        let repository = LibraryRepository::new(directory.clone());
        let run = repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "cancelling-run".to_string(),
                conversation_id: "conversation-cancelling".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: false,
                retry_attempts: 1,
                input_snapshot_hash: "snapshot-cancelling".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-cancelling".to_string(),
            })
            .unwrap();

        let stopping = repository
            .request_note_pipeline_cancellation(&run.id)
            .unwrap();
        assert_eq!(stopping.phase, NotePipelinePhase::Cancelling);
        assert!(repository
            .update_note_pipeline_phase(&run.id, NotePipelinePhase::Analyzing, None, &[], None,)
            .is_err());

        let recovered = LibraryRepository::new(directory.clone());
        assert_eq!(recovered.recover_stale_cancelling_runs().unwrap(), 1);
        assert_eq!(
            recovered.get_note_pipeline_run(&run.id).unwrap().phase,
            NotePipelinePhase::Cancelled
        );
        let events = recovered.list_note_pipeline_events(&run.id, 10).unwrap();
        assert!(events
            .iter()
            .any(|event| event.1 == "runCancellationRequested"));
        assert!(events.iter().any(|event| event.1 == "runCancelled"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn persists_and_resumes_note_pipeline_sections() {
        let directory = test_directory("note-pipeline");
        let repository = LibraryRepository::new(directory.clone());
        let run = repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "run-1".to_string(),
                conversation_id: "conversation-1".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: true,
                retry_attempts: 2,
                input_snapshot_hash: "snapshot-1".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-1".to_string(),
            })
            .unwrap();
        assert_eq!(run.phase, NotePipelinePhase::Preflight);
        assert!(repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "run-duplicate".to_string(),
                conversation_id: "conversation-1".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: false,
                retry_attempts: 1,
                input_snapshot_hash: "snapshot-2".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-2".to_string(),
            })
            .is_err());

        let outline = serde_json::json!({
            "title": "T",
            "summary": "S",
            "weakPoints": [],
            "sections": [
                { "id": "sec-1", "heading": "A", "kind": "concept", "brief": "A brief" },
                { "id": "sec-2", "heading": "B", "kind": "summary", "brief": "B brief" }
            ]
        })
        .to_string();
        let awaiting = repository
            .save_note_pipeline_outline(
                &run.id,
                &outline,
                vec![
                    NotePipelineSectionCreate {
                        section_id: "sec-1".to_string(),
                        position: 0,
                        section_json: serde_json::json!({ "id": "sec-1" }).to_string(),
                        input_hash: "sec-1-input".to_string(),
                    },
                    NotePipelineSectionCreate {
                        section_id: "sec-2".to_string(),
                        position: 1,
                        section_json: serde_json::json!({ "id": "sec-2" }).to_string(),
                        input_hash: "sec-2-input".to_string(),
                    },
                ],
            )
            .unwrap();
        assert_eq!(awaiting.phase, NotePipelinePhase::AwaitingOutline);
        repository
            .select_note_pipeline_sections(&run.id, vec!["sec-1".to_string()])
            .unwrap();
        repository
            .save_note_pipeline_section(
                &run.id,
                "sec-1",
                "## A\n\n正文",
                NotePipelineSectionStatus::Completed,
                None,
            )
            .unwrap();

        let reopened = LibraryRepository::new(directory.clone());
        let persisted = reopened.get_note_pipeline_run(&run.id).unwrap();
        assert_eq!(persisted.phase, NotePipelinePhase::Compiling);
        assert_eq!(persisted.selected_section_ids, vec!["sec-1"]);
        assert_eq!(persisted.completed_section_ids, vec!["sec-1"]);
        assert_eq!(
            reopened.list_resumable_note_pipeline_runs().unwrap().len(),
            1
        );

        reopened
            .update_note_pipeline_phase(&run.id, NotePipelinePhase::Done, None, &[], None)
            .unwrap();
        assert!(reopened
            .list_resumable_note_pipeline_runs()
            .unwrap()
            .is_empty());
        assert!(reopened
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "run-2".to_string(),
                conversation_id: "conversation-1".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: false,
                retry_attempts: 1,
                input_snapshot_hash: "snapshot-3".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-3".to_string(),
            })
            .is_ok());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn discovers_only_the_latest_recoverable_run_for_each_conversation() {
        let directory = test_directory("note-pipeline-recovery-discovery");
        let repository = LibraryRepository::new(directory.clone());
        let first = repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "cancelled-run".to_string(),
                conversation_id: "conversation-1".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: false,
                retry_attempts: 5,
                input_snapshot_hash: "snapshot-1".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-1".to_string(),
            })
            .unwrap();
        repository
            .update_note_pipeline_phase(&first.id, NotePipelinePhase::Cancelled, None, &[], None)
            .unwrap();

        let recoverable = repository.list_resumable_note_pipeline_runs().unwrap();
        assert_eq!(recoverable.len(), 1);
        assert_eq!(recoverable[0].id, first.id);
        assert!(NotePipelinePhase::Cancelled.is_resumable());

        let newer = repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "completed-run".to_string(),
                conversation_id: "conversation-1".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: false,
                retry_attempts: 5,
                input_snapshot_hash: "snapshot-2".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-2".to_string(),
            })
            .unwrap();
        repository
            .update_note_pipeline_phase(&newer.id, NotePipelinePhase::Done, None, &[], None)
            .unwrap();

        assert!(repository
            .list_resumable_note_pipeline_runs()
            .unwrap()
            .is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn abandoned_runs_are_persisted_and_excluded_from_recovery() {
        let directory = test_directory("note-pipeline-abandoned");
        let repository = LibraryRepository::new(directory.clone());
        let run = repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "abandoned-run".to_string(),
                conversation_id: "conversation-abandoned".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: false,
                retry_attempts: 5,
                input_snapshot_hash: "snapshot-abandoned".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-abandoned".to_string(),
            })
            .unwrap();

        let abandoned = repository.abandon_note_pipeline_run(&run.id).unwrap();
        assert!(abandoned.abandoned);
        assert_eq!(abandoned.phase, NotePipelinePhase::Cancelled);
        assert!(repository
            .list_resumable_note_pipeline_runs()
            .unwrap()
            .is_empty());
        assert!(repository
            .list_note_pipeline_runs_for_conversation("conversation-abandoned")
            .unwrap()
            .is_empty());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn retry_preserves_completed_checkpoints_and_resets_failed_work() {
        let directory = test_directory("note-pipeline-retry");
        let repository = LibraryRepository::new(directory.clone());
        let run = repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: "retry-run".to_string(),
                conversation_id: "conversation-1".to_string(),
                provider_id: "provider-1".to_string(),
                model_id: "model-1".to_string(),
                max_output_tokens: 4_096,
                thinking_enabled: true,
                retry_attempts: 5,
                input_snapshot_hash: "snapshot-1".to_string(),
                budget_json: "{}".to_string(),
                preflight_json: "{}".to_string(),
                idempotency_key: "output-1".to_string(),
            })
            .unwrap();
        let outline = serde_json::json!({
            "title": "T",
            "summary": "S",
            "weakPoints": [],
            "sections": [
                { "id": "completed", "heading": "A", "kind": "concept", "brief": "A" },
                { "id": "failed", "heading": "B", "kind": "summary", "brief": "B" }
            ]
        })
        .to_string();
        repository
            .save_note_pipeline_outline(
                &run.id,
                &outline,
                vec![
                    NotePipelineSectionCreate {
                        section_id: "completed".to_string(),
                        position: 0,
                        section_json: serde_json::json!({ "id": "completed" }).to_string(),
                        input_hash: "completed-input".to_string(),
                    },
                    NotePipelineSectionCreate {
                        section_id: "failed".to_string(),
                        position: 1,
                        section_json: serde_json::json!({ "id": "failed" }).to_string(),
                        input_hash: "failed-input".to_string(),
                    },
                ],
            )
            .unwrap();
        repository
            .select_note_pipeline_sections(
                &run.id,
                vec!["completed".to_string(), "failed".to_string()],
            )
            .unwrap();
        repository
            .save_note_pipeline_section_checkpoint(
                &run.id,
                "completed",
                "## Completed",
                NotePipelineSectionStatus::Completed,
                2,
                1,
                &["evidence-1".to_string()],
                "{\"valid\":true}",
                None,
            )
            .unwrap();
        repository
            .save_note_pipeline_section_checkpoint(
                &run.id,
                "failed",
                "partial draft",
                NotePipelineSectionStatus::Failed,
                5,
                5,
                &["evidence-2".to_string()],
                "{\"valid\":false}",
                Some("timeout"),
            )
            .unwrap();
        repository
            .replace_note_pipeline_nodes(
                &run.id,
                1,
                &[
                    (
                        "node-completed".to_string(),
                        "draftSection".to_string(),
                        Some("completed".to_string()),
                        "[]".to_string(),
                        "completed".to_string(),
                        "completed-input".to_string(),
                    ),
                    (
                        "node-review".to_string(),
                        "validateSection".to_string(),
                        Some("failed".to_string()),
                        "[]".to_string(),
                        "needsReview".to_string(),
                        "failed-input".to_string(),
                    ),
                ],
            )
            .unwrap();
        repository
            .update_note_pipeline_node_state(
                &run.id,
                1,
                "node-completed",
                "completed",
                2,
                &["evidence-1".to_string()],
                Some("section:completed"),
                "{\"valid\":true}",
                None,
            )
            .unwrap();
        {
            let connection = repository.open_connection().unwrap();
            connection
                .execute(
                    "UPDATE note_pipeline_nodes SET attempt_count = 5, error_message = 'timeout' WHERE node_id = 'node-review'",
                    [],
                )
                .unwrap();
        }
        repository
            .update_note_pipeline_phase(
                &run.id,
                NotePipelinePhase::Error,
                None,
                &[],
                Some("timeout"),
            )
            .unwrap();

        let recovered = repository
            .prepare_note_pipeline_retry(&run.id, true)
            .unwrap();
        assert_eq!(recovered.execution_version, 2);
        assert!(recovered.error_message.is_none());
        let sections = repository.list_note_pipeline_sections(&run.id).unwrap();
        let completed = sections
            .iter()
            .find(|section| section.section_id == "completed")
            .unwrap();
        assert_eq!(completed.status, NotePipelineSectionStatus::Completed);
        assert_eq!(completed.markdown, "## Completed");
        assert_eq!(completed.attempt_count, 2);
        let failed = sections
            .iter()
            .find(|section| section.section_id == "failed")
            .unwrap();
        assert_eq!(failed.status, NotePipelineSectionStatus::Pending);
        assert!(failed.markdown.is_empty());
        assert_eq!(failed.attempt_count, 0);
        assert_eq!(failed.revision_count, 0);

        let connection = repository.open_connection().unwrap();
        let completed_node: (String, i64, String) = connection
            .query_row(
                "SELECT status, attempt_count, evidence_ids_json
                 FROM note_pipeline_nodes WHERE node_id = 'node-completed'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            completed_node,
            ("completed".to_string(), 2, "[\"evidence-1\"]".to_string())
        );
        let retried_node: (String, i64, Option<String>) = connection
            .query_row(
                "SELECT status, attempt_count, error_message FROM note_pipeline_nodes WHERE node_id = 'node-review'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(retried_node, ("pending".to_string(), 0, None));
        drop(connection);

        for _ in 0..4 {
            repository
                .update_note_pipeline_phase(
                    &run.id,
                    NotePipelinePhase::Error,
                    None,
                    &[],
                    Some("retry test"),
                )
                .unwrap();
            repository
                .prepare_note_pipeline_retry(&run.id, false)
                .unwrap();
        }
        assert_eq!(
            repository
                .get_note_pipeline_run(&run.id)
                .unwrap()
                .execution_version,
            6
        );
        repository
            .update_note_pipeline_phase(
                &run.id,
                NotePipelinePhase::Error,
                None,
                &[],
                Some("retry test"),
            )
            .unwrap();
        assert!(repository
            .prepare_note_pipeline_retry(&run.id, false)
            .unwrap_err()
            .contains("5 次人工恢复上限"));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn deep_note_coverage_snapshot_advances_only_when_update_is_applied() {
        let directory = test_directory("deep-note-coverage");
        let repository = LibraryRepository::new(directory.clone());
        let initial_snapshot = coverage_snapshot(&["message-a", "message-b"]);
        let note = repository
            .create_note_with_sources_and_coverage(
                LibraryNoteCreate {
                    item_id: None,
                    title: "Deep note".to_string(),
                    content: "# Deep note\n\nInitial".to_string(),
                    group_name: None,
                },
                vec![NoteSourceCreate {
                    section_id: "sec-1".to_string(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some("conversation-1".to_string()),
                    message_id: Some("message-b".to_string()),
                    summarized_until_message_id: Some("message-b".to_string()),
                }],
                "conversation-1",
                &initial_snapshot,
            )
            .unwrap();
        assert_eq!(
            repository
                .deep_note_coverage_snapshot(&note.id, "conversation-1")
                .unwrap(),
            Some(initial_snapshot.clone())
        );

        let updated_snapshot = coverage_snapshot(&["message-a", "message-b", "message-c"]);
        let proposal = repository
            .create_note_edit_proposal(NoteEditProposalCreate {
                id: "proposal-coverage".to_string(),
                note_id: note.id.clone(),
                conversation_id: "conversation-1".to_string(),
                source_message_id: Some("message-c".to_string()),
                expected_note_updated_at: note.updated_at,
                old_title: note.title.clone(),
                new_title: note.title.clone(),
                old_content: note.content.clone(),
                new_content: "# Deep note\n\nUpdated".to_string(),
                diff: "update message-c".to_string(),
                sources: vec![NoteSourceCreate {
                    section_id: "edit-1".to_string(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some("conversation-1".to_string()),
                    message_id: Some("message-c".to_string()),
                    summarized_until_message_id: Some("message-c".to_string()),
                }],
                coverage_snapshot_json: serde_json::to_string(&updated_snapshot).unwrap(),
                source_units: Vec::new(),
            })
            .unwrap();

        assert_eq!(
            repository
                .deep_note_coverage_snapshot(&note.id, "conversation-1")
                .unwrap(),
            Some(initial_snapshot)
        );
        repository
            .resolve_note_edit_proposal(&proposal.id, true)
            .unwrap();
        assert_eq!(
            repository
                .deep_note_coverage_snapshot(&note.id, "conversation-1")
                .unwrap(),
            Some(updated_snapshot)
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rebuilt_note_becomes_the_only_future_update_anchor() {
        let directory = test_directory("deep-note-rebuild-anchor");
        let repository = LibraryRepository::new(directory.clone());
        let initial_snapshot = coverage_snapshot(&["message-a", "message-b"]);
        let old = repository
            .create_note_with_sources_and_coverage(
                LibraryNoteCreate {
                    item_id: None,
                    title: "Old deep note".to_string(),
                    content: "# Old".to_string(),
                    group_name: None,
                },
                vec![NoteSourceCreate {
                    section_id: "sec-old".to_string(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some("conversation-1".to_string()),
                    message_id: Some("message-b".to_string()),
                    summarized_until_message_id: Some("message-b".to_string()),
                }],
                "conversation-1",
                &initial_snapshot,
            )
            .unwrap();
        let rebuilt_snapshot = coverage_snapshot(&["message-a", "message-b", "message-c"]);
        let rebuilt = repository
            .create_rebuilt_note_with_sources_and_coverage(
                LibraryNoteCreate {
                    item_id: None,
                    title: "Rebuilt deep note".to_string(),
                    content: "# Rebuilt".to_string(),
                    group_name: None,
                },
                vec![NoteSourceCreate {
                    section_id: "sec-new".to_string(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some("conversation-1".to_string()),
                    message_id: Some("message-c".to_string()),
                    summarized_until_message_id: Some("message-c".to_string()),
                }],
                "conversation-1",
                &rebuilt_snapshot,
            )
            .unwrap();

        let latest = repository
            .latest_deep_note_for_conversation("conversation-1")
            .unwrap()
            .unwrap();
        assert_eq!(latest.0.id, rebuilt.id);
        assert_eq!(latest.1.as_deref(), Some("message-c"));
        assert_eq!(
            repository
                .deep_note_coverage_snapshot(&rebuilt.id, "conversation-1")
                .unwrap(),
            Some(rebuilt_snapshot)
        );
        assert!(repository
            .list_note_sources(&old.id)
            .unwrap()
            .iter()
            .all(|source| source.summarized_until_message_id.is_none()));
        assert!(repository.get_note(&old.id).is_ok());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn attachment_source_units_advance_only_after_the_update_is_applied() {
        let directory = test_directory("deep-note-source-units");
        let repository = LibraryRepository::new(directory.clone());
        let initial_snapshot = coverage_snapshot(&["message-a"]);
        let note = repository
            .create_note_with_sources_and_coverage(
                LibraryNoteCreate {
                    item_id: None,
                    title: "Deep note".to_string(),
                    content: "# Deep note\n\nInitial".to_string(),
                    group_name: None,
                },
                vec![NoteSourceCreate {
                    section_id: "sec-1".to_string(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some("conversation-1".to_string()),
                    message_id: Some("message-a".to_string()),
                    summarized_until_message_id: Some("message-a".to_string()),
                }],
                "conversation-1",
                &initial_snapshot,
            )
            .unwrap();
        let existing = repository
            .list_deep_note_source_units(&note.id, "conversation-1")
            .unwrap();
        assert_eq!(existing.len(), 1);

        let mut updated_snapshot = coverage_snapshot(&["message-a", "message-b"]);
        updated_snapshot.attachment_ids = vec!["attachment-b".to_string()];
        updated_snapshot.attachment_content_hashes = vec!["hash-attachment-b".to_string()];
        let attachment_unit = DeepNoteSourceUnit {
            unit_id: format!("{}:attachment:attachment-b", note.id),
            note_id: note.id.clone(),
            conversation_id: "conversation-1".to_string(),
            message_id: "message-b".to_string(),
            kind: DeepNoteSourceUnitKind::Attachment,
            attachment_id: Some("attachment-b".to_string()),
            content_hash: "hash-attachment-b".to_string(),
            parser_id: "read_attachment_text".to_string(),
            parser_version: "1".to_string(),
            status: DeepNoteSourceUnitStatus::Covered,
            chunk_ids: vec!["chunk-b".to_string()],
            evidence_ids: Vec::new(),
            error_message: None,
            created_at: 2,
            updated_at: 2,
        };
        let proposal = repository
            .create_note_edit_proposal(NoteEditProposalCreate {
                id: "proposal-source-unit".to_string(),
                note_id: note.id.clone(),
                conversation_id: "conversation-1".to_string(),
                source_message_id: Some("message-b".to_string()),
                expected_note_updated_at: note.updated_at,
                old_title: note.title.clone(),
                new_title: note.title.clone(),
                old_content: note.content.clone(),
                new_content: "# Deep note\n\nUpdated".to_string(),
                diff: "attachment update".to_string(),
                sources: vec![NoteSourceCreate {
                    section_id: "source-unit".to_string(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some("conversation-1".to_string()),
                    message_id: Some("message-b".to_string()),
                    summarized_until_message_id: Some("message-b".to_string()),
                }],
                coverage_snapshot_json: serde_json::to_string(&updated_snapshot).unwrap(),
                source_units: vec![attachment_unit],
            })
            .unwrap();

        assert_eq!(
            repository
                .list_deep_note_source_units(&note.id, "conversation-1")
                .unwrap()
                .len(),
            1
        );
        repository
            .resolve_note_edit_proposal(&proposal.id, true)
            .unwrap();
        let applied = repository
            .list_deep_note_source_units(&note.id, "conversation-1")
            .unwrap();
        assert_eq!(applied.len(), 2);
        assert!(applied.iter().any(|unit| {
            unit.attachment_id.as_deref() == Some("attachment-b")
                && unit.status == DeepNoteSourceUnitStatus::Covered
        }));
        assert_eq!(
            repository
                .deep_note_coverage_snapshot(&note.id, "conversation-1")
                .unwrap(),
            Some(updated_snapshot)
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn note_edit_requires_confirmation_backs_up_and_rejects_stale_edits() {
        let directory = test_directory("note-edit");
        let repository = LibraryRepository::new(directory.clone());
        let note = repository
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "Old title".to_string(),
                content: "# Old title\n\nOld body".to_string(),
                group_name: None,
            })
            .unwrap();
        let source = NoteSourceCreate {
            section_id: "edit-1".to_string(),
            origin: NoteSourceOrigin::Conversation,
            conversation_id: Some("conversation-1".to_string()),
            message_id: Some("message-2".to_string()),
            summarized_until_message_id: Some("message-2".to_string()),
        };
        let proposal =
            |id: &str, current: &crate::library::types::LibraryNote| NoteEditProposalCreate {
                id: id.to_string(),
                note_id: current.id.clone(),
                conversation_id: "conversation-1".to_string(),
                source_message_id: Some("message-2".to_string()),
                expected_note_updated_at: current.updated_at,
                old_title: current.title.clone(),
                new_title: "New title".to_string(),
                old_content: current.content.clone(),
                new_content: "# New title\n\nNew body".to_string(),
                diff: "--- old\n+++ new".to_string(),
                sources: vec![source.clone()],
                coverage_snapshot_json: String::new(),
                source_units: Vec::new(),
            };

        repository
            .create_note_edit_proposal(proposal("proposal-reject", &note))
            .unwrap();
        assert!(repository
            .resolve_note_edit_proposal("proposal-reject", false)
            .unwrap()
            .is_none());
        assert_eq!(repository.get_note(&note.id).unwrap().content, note.content);

        repository
            .create_note_edit_proposal(proposal("proposal-apply", &note))
            .unwrap();
        let updated = repository
            .resolve_note_edit_proposal("proposal-apply", true)
            .unwrap()
            .unwrap();
        assert_eq!(updated.title, "New title");
        assert_eq!(updated.content, "# New title\n\nNew body");
        assert_eq!(repository.list_note_sources(&note.id).unwrap().len(), 1);
        let connection = repository.open_connection().unwrap();
        let version: (String, String, String) = connection
            .query_row(
                "SELECT title, content, reason FROM library_note_versions WHERE note_id = ?",
                rusqlite::params![note.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(version.0, "Old title");
        assert_eq!(version.1, "# Old title\n\nOld body");
        assert_eq!(version.2, "noteEdit");
        drop(connection);

        let partial = repository
            .create_note_edit_proposal(proposal("proposal-partial", &updated))
            .unwrap();
        let partially_updated = repository
            .resolve_note_edit_proposal_with_content(
                &partial.id,
                true,
                Some((
                    updated.title.clone(),
                    "# New title\n\nPartially accepted body".to_string(),
                    "--- selected\n+++ selected".to_string(),
                )),
            )
            .unwrap()
            .unwrap();
        assert_eq!(partially_updated.title, "New title");
        assert_eq!(
            partially_updated.content,
            "# New title\n\nPartially accepted body"
        );
        assert!(repository
            .list_note_sources(&note.id)
            .unwrap()
            .iter()
            .any(|source| source.section_id == "partial-edit"));

        repository
            .create_note_edit_proposal(proposal("proposal-stale", &partially_updated))
            .unwrap();
        repository
            .update_note(LibraryNoteUpdate {
                note_id: note.id.clone(),
                title: "Manual title".to_string(),
                content: "Manual edit".to_string(),
            })
            .unwrap();
        assert!(repository
            .resolve_note_edit_proposal("proposal-stale", true)
            .is_err());
        assert_eq!(repository.get_note(&note.id).unwrap().title, "Manual title");
        let connection = repository.open_connection().unwrap();
        let versions: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM library_note_versions WHERE note_id = ?",
                rusqlite::params![note.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(versions, 2);
        let _ = fs::remove_dir_all(directory);
    }
}
