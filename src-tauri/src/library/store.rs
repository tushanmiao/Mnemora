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

use super::{
    import::{import_pdf, ImportOutcome},
    types::{
        normalize_collection_name, normalize_identifier, LibraryAnnotation, LibraryAnnotationColor,
        LibraryAnnotationCreate, LibraryAnnotationKind, LibraryAnnotationRect,
        LibraryAnnotationUpdate, LibraryCollection, LibraryImportFailure, LibraryImportResult,
        LibraryItem, LibraryItemUpdate, LibraryListPage, LibraryListRequest, LibraryNote,
        LibraryNoteCreate, LibraryNoteSummary, LibraryNoteUpdate, LibraryReadingState,
        LibraryReadingStateUpdate, LibrarySort, LibraryView, MAX_PDF_RANGE_BYTES,
    },
};

const LIBRARY_SCHEMA_VERSION: i64 = 2;
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
                        length(n.content), n.created_at, n.updated_at
                 FROM library_notes n
                 JOIN library_items i ON i.id = n.item_id
                 WHERE n.item_id = ? AND i.deleted_at IS NULL
                 ORDER BY n.updated_at DESC",
                vec![Value::Text(item_id)],
            )
        } else {
            (
                "SELECT n.id, n.item_id, i.title, n.title, substr(n.content, 1, 600),
                        length(n.content), n.created_at, n.updated_at
                 FROM library_notes n
                 JOIN library_items i ON i.id = n.item_id
                 WHERE i.deleted_at IS NULL
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

    pub fn get_note(&self, note_id: &str) -> Result<LibraryNote, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let connection = self.open_connection()?;
        self.get_note_with_connection(&connection, &note_id)?
            .ok_or_else(|| "笔记不存在或所属文献位于回收站。".to_string())
    }

    pub fn create_note(&self, create: LibraryNoteCreate) -> Result<LibraryNote, String> {
        let create = create.normalize_and_validate()?;
        let connection = self.open_connection()?;
        ensure_active_item_exists(&connection, &create.item_id)?;
        let id = Uuid::new_v4().to_string();
        let now = now_millis_i64();
        connection
            .execute(
                "INSERT INTO library_notes (id, item_id, title, content, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![id, create.item_id, create.title, create.content, now, now],
            )
            .map_err(|error| format!("创建文献笔记失败：{error}"))?;
        self.get_note_with_connection(&connection, &id)?
            .ok_or_else(|| "创建后的笔记不存在。".to_string())
    }

    pub fn update_note(&self, update: LibraryNoteUpdate) -> Result<LibraryNote, String> {
        let update = update.normalize_and_validate()?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "UPDATE library_notes
                 SET title = ?, content = ?, updated_at = ?
                 WHERE id = ? AND EXISTS (
                    SELECT 1 FROM library_items i
                    WHERE i.id = library_notes.item_id AND i.deleted_at IS NULL
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

    pub fn delete_note(&self, note_id: &str) -> Result<bool, String> {
        let note_id = normalize_identifier("笔记 ID", note_id)?;
        let connection = self.open_connection()?;
        let changed = connection
            .execute(
                "DELETE FROM library_notes
                 WHERE id = ? AND EXISTS (
                    SELECT 1 FROM library_items i
                    WHERE i.id = library_notes.item_id AND i.deleted_at IS NULL
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
                "SELECT n.id, n.item_id, i.title, n.title, n.content, n.created_at, n.updated_at
                 FROM library_notes n
                 JOIN library_items i ON i.id = n.item_id
                 WHERE n.id = ? AND i.deleted_at IS NULL",
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
                    item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
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
                 PRAGMA user_version = 2;
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
                    item_id TEXT NOT NULL REFERENCES library_items(id) ON DELETE CASCADE,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS library_annotations_item_page
                    ON library_annotations(item_id, page_index, created_at);
                 CREATE INDEX IF NOT EXISTS library_notes_item_updated
                    ON library_notes(item_id, updated_at DESC);
                 PRAGMA user_version = 2;
                 COMMIT;",
            )
            .map_err(|error| format!("升级文献库批注与笔记结构失败：{error}"))?;
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

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<LibraryNote> {
    Ok(LibraryNote {
        id: row.get(0)?,
        item_id: row.get(1)?,
        item_title: row.get(2)?,
        title: row.get(3)?,
        content: row.get(4)?,
        created_at: i64_to_u64(row.get(5)?),
        updated_at: i64_to_u64(row.get(6)?),
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
        created_at: i64_to_u64(row.get(6)?),
        updated_at: i64_to_u64(row.get(7)?),
    })
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
    use crate::library::types::{
        LibraryAnnotationColor, LibraryAnnotationCreate, LibraryAnnotationKind,
        LibraryAnnotationRect, LibraryAnnotationUpdate, LibraryItemUpdate, LibraryListRequest,
        LibraryNoteCreate, LibraryNoteUpdate, LibraryReadingStateUpdate, LibraryView,
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
                item_id: item.id.clone(),
                title: "Reading note".to_string(),
                content: "Initial content".to_string(),
            })
            .unwrap();
        assert_eq!(repository.list_notes(Some(&item.id)).unwrap().len(), 1);
        assert_eq!(
            repository.list_notes(None).unwrap()[0].item_title,
            item.title
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

        assert!(repository.delete_annotation(&annotation.id).unwrap());
        assert!(repository.delete_note(&note.id).unwrap());
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
                item_id: item.id.clone(),
                title: "Cascade note".to_string(),
                content: String::new(),
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
        assert_eq!(version, 2);
        let tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name IN ('library_annotations', 'library_notes')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 2);

        let _ = fs::remove_dir_all(directory);
    }
}
