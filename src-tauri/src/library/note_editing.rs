//! Versioned editor saves, durable drafts, and note-owned image assets.
//! The operation row survives file publication so startup can complete a lost
//! SQLite commit without guessing which document bytes were intended.

use std::{fs, io::Write};

use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    note_files::{content_hash, refresh_note_directory, resolve_note_directory},
    types::{normalize_identifier, validate_note_content, LibraryNote, LibraryNoteUpdate},
    LibraryRepository,
};

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 25_000_000;
const DRAFT_IMAGE_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const FILE_BACKUP_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

pub(crate) fn migrate(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
         ALTER TABLE library_notes ADD COLUMN edit_version INTEGER NOT NULL DEFAULT 1;
         CREATE TABLE IF NOT EXISTS library_note_versions (
           id TEXT PRIMARY KEY,
           note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
           title TEXT NOT NULL,
           content TEXT NOT NULL,
           reason TEXT NOT NULL,
           created_at INTEGER NOT NULL
         );
         ALTER TABLE library_note_versions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
         CREATE TRIGGER note_edit_version_changed
         AFTER UPDATE OF content, title ON library_notes
         WHEN NEW.content <> OLD.content OR NEW.title <> OLD.title
         BEGIN
           UPDATE library_notes
           SET edit_version = OLD.edit_version + 1,
               updated_at = MAX(NEW.updated_at, OLD.updated_at + 1)
           WHERE id = NEW.id;
         END;
         CREATE TABLE note_save_operations (
           operation_id TEXT PRIMARY KEY,
           note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
           fingerprint TEXT NOT NULL,
           base_version INTEGER NOT NULL,
           base_hash TEXT NOT NULL,
           result_hash TEXT NOT NULL,
           title TEXT NOT NULL,
           content TEXT NOT NULL,
           state TEXT NOT NULL CHECK(state IN ('prepared','committed','conflict')),
           result_version INTEGER,
           request_json TEXT NOT NULL,
           created_at INTEGER NOT NULL
         );
         CREATE INDEX note_saves_pending ON note_save_operations(state, note_id);
         CREATE TABLE note_editor_drafts (
           note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
           session_id TEXT NOT NULL,
           generation INTEGER NOT NULL,
           base_version TEXT NOT NULL,
           title TEXT NOT NULL,
           content TEXT NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY(note_id, session_id)
         );
         CREATE TABLE note_staged_images (
           token TEXT PRIMARY KEY,
           note_id TEXT NOT NULL REFERENCES library_notes(id) ON DELETE CASCADE,
           session_id TEXT NOT NULL,
           content_hash TEXT NOT NULL,
           relative_path TEXT NOT NULL,
           mime_type TEXT NOT NULL,
           byte_size INTEGER NOT NULL,
           original_name TEXT NOT NULL,
           committed INTEGER NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL
         );
         PRAGMA user_version = 21;
         COMMIT;",
        )
        .map_err(|error| {
            let _ = connection.execute_batch("ROLLBACK;");
            format!("Note editing migration failed: {error}")
        })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEditingSnapshot {
    pub note: LibraryNote,
    pub note_version: String,
    pub content_hash: String,
    pub disk_hash: Option<String>,
    pub external_content: Option<String>,
    pub source_missing: bool,
    pub drafts: Vec<NoteDraft>,
    pub staged_images: Vec<NoteImageAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDraft {
    pub note_id: String,
    pub session_id: String,
    pub generation: u32,
    pub base_version: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveNoteRequest {
    pub note_id: String,
    pub session_id: String,
    pub operation_id: String,
    pub draft_generation: u32,
    pub expected_note_version: String,
    pub expected_content_hash: String,
    pub expected_disk_hash: Option<String>,
    pub title: String,
    pub markdown: String,
    #[serde(default)]
    pub accept_external_change: bool,
    #[serde(default = "save_reason")]
    pub reason: String,
}

fn save_reason() -> String {
    "typing".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteImageAsset {
    pub token: String,
    pub relative_path: String,
    pub content_hash: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteVersionEntry {
    pub id: String,
    pub title: String,
    pub content: String,
    pub content_hash: String,
    pub reason: String,
    pub created_at: u64,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveNoteReceipt {
    pub operation_id: String,
    pub draft_generation: u32,
    pub note_id: String,
    pub note_version: String,
    pub content_hash: String,
    pub title: String,
    pub committed_markdown: String,
    pub updated_at: u64,
}

fn now() -> i64 {
    i64::try_from(crate::usage::now_ms()).unwrap_or(i64::MAX)
}
fn storage(error: impl std::fmt::Display) -> String {
    format!("NOTE_STORAGE_UNAVAILABLE: {error}")
}
fn conflict() -> String {
    "NOTE_VERSION_CONFLICT: 笔记已变化，请比较当前版本后再保存。".to_string()
}

fn version(connection: &Connection, note_id: &str) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT edit_version FROM library_notes WHERE id = ?",
            [note_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| "NOTE_DELETED: 笔记不存在。".to_string())
}

impl LibraryRepository {
    /// Remove only artifacts whose durable references have expired. Committed
    /// attachments and database history remain available for citation/recovery.
    pub fn prune_note_editing_artifacts(&self, note_id: &str) -> Result<(), String> {
        let note = self.get_note(note_id)?;
        let connection = self.open_connection()?;
        let cutoff = now().saturating_sub(DRAFT_IMAGE_RETENTION_MS);
        let staged: Vec<(String, String)> = {
            let mut statement = connection.prepare(
                "SELECT token,relative_path FROM note_staged_images WHERE note_id=? AND committed=0 AND created_at<?",
            ).map_err(storage)?;
            let rows = statement
                .query_map(params![note_id, cutoff], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .map_err(storage)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(storage)?
        };
        for (token, relative_path) in staged {
            let referenced: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM library_notes WHERE id=? AND content LIKE '%' || ? || '%')
                 OR EXISTS(SELECT 1 FROM note_editor_drafts WHERE note_id=? AND content LIKE '%' || ? || '%')",
                params![note_id, relative_path, note_id, relative_path], |row| row.get(0),
            ).map_err(storage)?;
            if referenced {
                continue;
            }
            if let Some(directory) = note.directory_path.as_deref() {
                let path = std::path::Path::new(directory).join(&relative_path);
                if path.starts_with(directory) {
                    let _ = fs::remove_file(path);
                }
            }
            connection
                .execute(
                    "DELETE FROM note_staged_images WHERE token=? AND committed=0",
                    [&token],
                )
                .map_err(storage)?;
        }

        let backup_cutoff = now().saturating_sub(FILE_BACKUP_RETENTION_MS);
        let operations: Vec<String> = {
            let mut statement = connection.prepare(
                "SELECT operation_id FROM note_save_operations WHERE note_id=? AND state='committed' AND created_at<?",
            ).map_err(storage)?;
            let rows = statement
                .query_map(params![note_id, backup_cutoff], |row| row.get(0))
                .map_err(storage)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(storage)?
        };
        if let Some(directory) = note.directory_path.as_deref() {
            for operation_id in operations {
                let backup = std::path::Path::new(directory)
                    .join("versions")
                    .join(format!("{}-note.md", content_hash(&operation_id)));
                let _ = fs::remove_file(backup);
            }
        }
        Ok(())
    }

    pub fn validate_note_selection(
        &self,
        note_id: &str,
        note_version: &str,
        hash: &str,
        start: u32,
        end: u32,
        text: &str,
    ) -> Result<(), String> {
        let snapshot = self.note_editing_snapshot(note_id)?;
        let canonical = snapshot
            .note
            .content
            .trim_start_matches('\u{feff}')
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        if snapshot.source_missing
            || snapshot.external_content.is_some()
            || snapshot.note_version != note_version
            || snapshot.content_hash != hash
            || text.is_empty()
            || text.len() > 16 * 1024
            || start >= end
            || canonical.get(start as usize..end as usize) != Some(text)
        {
            return Err("NOTE_RANGE_STALE: 选区与已保存版本不匹配。".into());
        }
        Ok(())
    }
    pub fn note_editing_snapshot(&self, note_id: &str) -> Result<NoteEditingSnapshot, String> {
        let mut note = self.get_note(note_id)?;
        let connection = self.open_connection()?;
        if note.directory_path.is_none() {
            let prepared = refresh_note_directory(
                &self.root_directory,
                None,
                note_id,
                &note.title,
                &note.content,
                note.updated_at,
            )?;
            connection.execute("UPDATE library_notes SET directory_path=?,content_hash=? WHERE id=? AND directory_path IS NULL",
                params![prepared.relative_directory,prepared.content_hash,note_id]).map_err(storage)?;
            note = self.get_note(note_id)?;
        }
        let note_version = version(&connection, note_id)?.to_string();
        let expected = content_hash(&note.content);
        let disk = note
            .directory_path
            .as_ref()
            .map(|directory| fs::read_to_string(std::path::Path::new(directory).join("note.md")));
        let source_missing = disk.as_ref().is_some_and(|result| result.is_err());
        let disk_content = disk.and_then(Result::ok);
        let disk_hash = disk_content.as_deref().map(content_hash);
        let mut external_content = disk_content.filter(|text| content_hash(text) != expected);
        let interrupted:Option<(String,String,String)>=connection.query_row(
            "SELECT operation_id,base_hash,result_hash FROM note_save_operations WHERE note_id=? AND base_version=? AND state IN ('prepared','conflict') ORDER BY created_at DESC LIMIT 1",
            params![note_id,note_version],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional().map_err(storage)?;
        if let (Some((operation, base_hash, result_hash)), Some(directory)) =
            (interrupted, note.directory_path.as_ref())
        {
            if disk_hash.as_deref() == Some(&result_hash) {
                let backup = std::path::Path::new(directory)
                    .join("versions")
                    .join(format!("{}-note.md", content_hash(&operation)));
                if let Ok(actual) = fs::read_to_string(backup) {
                    if content_hash(&actual) != base_hash {
                        external_content = Some(actual);
                    }
                }
            }
        }
        let mut statement = connection
            .prepare(
                "SELECT session_id, generation, base_version, title, content, updated_at
             FROM note_editor_drafts WHERE note_id = ? ORDER BY updated_at DESC LIMIT 20",
            )
            .map_err(storage)?;
        let drafts = statement
            .query_map([note_id], |row| {
                Ok(NoteDraft {
                    note_id: note_id.to_string(),
                    session_id: row.get(0)?,
                    generation: row.get(1)?,
                    base_version: row.get(2)?,
                    title: row.get(3)?,
                    content: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        let staged_images = {
            let mut statement = connection.prepare(
                "SELECT token,relative_path,content_hash,mime_type FROM note_staged_images WHERE note_id=? AND committed=0 ORDER BY created_at DESC LIMIT 100",
            ).map_err(storage)?;
            let rows = statement
                .query_map([note_id], |row| {
                    Ok(NoteImageAsset {
                        token: row.get(0)?,
                        relative_path: row.get(1)?,
                        content_hash: row.get(2)?,
                        mime_type: row.get(3)?,
                    })
                })
                .map_err(storage)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(storage)?
        };
        Ok(NoteEditingSnapshot {
            note,
            note_version,
            content_hash: expected,
            disk_hash,
            external_content,
            source_missing,
            drafts,
            staged_images,
        })
    }

    pub fn checkpoint_note_draft(&self, mut draft: NoteDraft) -> Result<(), String> {
        draft.note_id = normalize_identifier("note ID", &draft.note_id)?;
        draft.session_id = normalize_identifier("session ID", &draft.session_id)?;
        validate_note_content(&draft.content)?;
        if draft.title.chars().count() > 500 {
            return Err("NOTE_CONTENT_LIMIT: 笔记标题过长。".to_string());
        }
        self.get_note(&draft.note_id)?;
        let connection = self.open_connection()?;
        connection
            .execute(
                "INSERT INTO note_editor_drafts
             (note_id,session_id,generation,base_version,title,content,updated_at)
             VALUES (?,?,?,?,?,?,?)
             ON CONFLICT(note_id,session_id) DO UPDATE SET
               generation=excluded.generation,base_version=excluded.base_version,
               title=excluded.title,content=excluded.content,updated_at=excluded.updated_at
             WHERE excluded.generation >= note_editor_drafts.generation",
                params![
                    draft.note_id,
                    draft.session_id,
                    draft.generation,
                    draft.base_version,
                    draft.title,
                    draft.content,
                    now()
                ],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn discard_note_draft(
        &self,
        note_id: &str,
        session_id: &str,
        through_generation: u32,
    ) -> Result<(), String> {
        self.open_connection()?
            .execute(
                "DELETE FROM note_editor_drafts WHERE note_id=? AND session_id=? AND generation<=?",
                params![note_id, session_id, through_generation],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn save_note_checked(&self, request: SaveNoteRequest) -> Result<SaveNoteReceipt, String> {
        let note_id = normalize_identifier("note ID", &request.note_id)?;
        let operation_id = normalize_identifier("operation ID", &request.operation_id)?;
        normalize_identifier("session ID", &request.session_id)?;
        let normalized = LibraryNoteUpdate {
            note_id: note_id.clone(),
            title: request.title.clone(),
            content: request.markdown.clone(),
        }
        .normalize_and_validate()?;
        if !["typing", "explicitSave", "aiApply", "restore", "normalize"]
            .contains(&request.reason.as_str())
        {
            return Err("NOTE_CONTENT_INVALID: 保存原因无效。".to_string());
        }
        let fingerprint = content_hash(&serde_json::to_string(&request).map_err(storage)?);
        let connection = self.open_connection()?;
        let previous: Option<(String, String)> = connection
            .query_row(
                "SELECT fingerprint,state FROM note_save_operations WHERE operation_id=?",
                [&operation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((existing, state)) = previous {
            if existing != fingerprint {
                return Err("NOTE_OPERATION_MISMATCH: 保存操作内容不一致。".to_string());
            }
            if state == "committed" {
                return self.note_save_receipt(&operation_id);
            }
            self.recover_note_saves()?;
            let state: String = connection
                .query_row(
                    "SELECT state FROM note_save_operations WHERE operation_id=?",
                    [&operation_id],
                    |row| row.get(0),
                )
                .map_err(storage)?;
            if state == "committed" {
                return self.note_save_receipt(&operation_id);
            }
            return Err("NOTE_RECOVERY_REQUIRED: 上次保存需要恢复，请重新打开笔记。".to_string());
        }
        let base = self.note_editing_snapshot(&note_id)?;
        if base.source_missing {
            return Err("NOTE_SOURCE_MISSING: 笔记文件不可用，草稿仍被保留。".to_string());
        }
        if base.note_version != request.expected_note_version
            || base.content_hash != request.expected_content_hash
        {
            return Err(conflict());
        }
        if base.disk_hash != request.expected_disk_hash
            || (base.external_content.is_some() && !request.accept_external_change)
        {
            return Err(conflict());
        }
        let pending: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM note_save_operations WHERE note_id=? AND state='prepared')",
            [&note_id], |r| r.get(0),
        ).map_err(storage)?;
        if pending {
            return Err("NOTE_RECOVERY_REQUIRED: 上次保存尚未恢复。".into());
        }
        let base_version = base.note_version.parse::<i64>().map_err(storage)?;
        if base_version == i64::MAX {
            return Err("NOTE_CONTENT_LIMIT: 笔记版本已达到上限。".into());
        }
        let text = if normalized.content == base.note.content && request.reason != "normalize" {
            normalized.content.clone()
        } else {
            normalized
                .content
                .strip_prefix('\u{feff}')
                .unwrap_or(&normalized.content)
                .replace("\r\n", "\n")
                .replace('\r', "\n")
        };
        let noop = normalized.title == base.note.title
            && text == base.note.content
            && base.external_content.is_none();
        let created = if noop {
            base.note.updated_at as i64
        } else {
            now().max(
                i64::try_from(base.note.updated_at)
                    .unwrap_or_default()
                    .saturating_add(1),
            )
        };
        connection.execute(
            "INSERT INTO note_save_operations(operation_id,note_id,fingerprint,base_version,base_hash,result_hash,title,content,state,created_at,request_json,result_version)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            params![operation_id,note_id,fingerprint,base_version,base.disk_hash.clone().unwrap_or_else(||base.content_hash.clone()),content_hash(&text),normalized.title,text,
                if noop {"committed"} else {"prepared"},created,serde_json::to_string(&request).map_err(storage)?,if noop {Some(base_version)} else {None}],
        ).map_err(storage)?;
        drop(connection);
        if !noop {
            if let Err(error) = self.publish_note_save(&operation_id) {
                if error.starts_with("NOTE_VERSION_CONFLICT") {
                    self.open_connection()?
                        .execute(
                            "UPDATE note_save_operations SET state='conflict' WHERE operation_id=?",
                            [&operation_id],
                        )
                        .map_err(storage)?;
                }
                return Err(error);
            }
        }
        self.discard_note_draft(&note_id, &request.session_id, request.draft_generation)?;
        self.note_save_receipt(&operation_id)
    }

    fn note_save_receipt(&self, operation_id: &str) -> Result<SaveNoteReceipt, String> {
        let connection = self.open_connection()?;
        let (note_id,version,hash,title,content,created,json): (String,i64,String,String,String,u64,String) = connection.query_row(
            "SELECT note_id,result_version,result_hash,title,content,created_at,request_json FROM note_save_operations WHERE operation_id=? AND state='committed'",
            [operation_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)),
        ).map_err(storage)?;
        let request: SaveNoteRequest = serde_json::from_str(&json).map_err(storage)?;
        Ok(SaveNoteReceipt {
            operation_id: operation_id.into(),
            draft_generation: request.draft_generation,
            note_id,
            note_version: version.to_string(),
            content_hash: hash,
            title,
            committed_markdown: content,
            updated_at: created,
        })
    }

    pub(crate) fn note_save_request(
        &self,
        operation_id: &str,
    ) -> Result<Option<SaveNoteRequest>, String> {
        let json: Option<String> = self
            .open_connection()?
            .query_row(
                "SELECT request_json FROM note_save_operations WHERE operation_id=?",
                [operation_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        json.map(|json| serde_json::from_str(&json).map_err(storage))
            .transpose()
    }

    fn publish_note_save(&self, operation_id: &str) -> Result<(), String> {
        let mut connection = self.open_connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let (note_id,base_version,base_hash,result_hash,title,text,created,json): (String,i64,String,String,String,String,i64,String) = tx.query_row(
            "SELECT note_id,base_version,base_hash,result_hash,title,content,created_at,request_json
             FROM note_save_operations WHERE operation_id=? AND state='prepared'",
            [operation_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)),
        ).map_err(storage)?;
        let request: SaveNoteRequest = serde_json::from_str(&json).map_err(storage)?;
        if version(&tx, &note_id)? != base_version {
            return Err(conflict());
        }
        let (directory,old_title,old_content): (Option<String>,String,String) = tx.query_row(
            "SELECT directory_path,title,content FROM library_notes WHERE id=? AND
             (item_id IS NULL OR EXISTS(SELECT 1 FROM library_items i WHERE i.id=library_notes.item_id AND i.deleted_at IS NULL))",
            [&note_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).map_err(storage)?;
        if let Some(directory) = &directory {
            let absolute = resolve_note_directory(&self.root_directory, directory)?;
            let current = fs::read_to_string(absolute.join("note.md"))
                .map_err(|_| "NOTE_SOURCE_MISSING: 笔记文件不可用。".to_string())?;
            let hash = content_hash(&current);
            if hash != base_hash && hash != result_hash {
                return Err(conflict());
            }
            publish_markdown(&absolute, operation_id, &base_hash, &result_hash, &text)?;
        }
        let prepared = refresh_note_directory(
            &self.root_directory,
            directory.as_deref(),
            &note_id,
            &title,
            &text,
            created as u64,
        )?;
        let recent_typing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_note_versions WHERE note_id=? AND reason='typing' AND created_at>?)",
            params![note_id,created.saturating_sub(30_000)], |r| r.get(0),
        ).map_err(storage)?;
        if request.reason != "typing" || !recent_typing {
            tx.execute(
            "INSERT INTO library_note_versions(id,note_id,title,content,reason,created_at) VALUES (?,?,?,?,?,?)",
            params![operation_id,note_id,old_title,old_content,if request.reason=="aiApply" {"noteEdit"} else {request.reason.as_str()},created],
        ).map_err(storage)?;
        }
        tx.execute(
            "UPDATE library_notes SET title=?,content=?,content_hash=?,directory_path=?,updated_at=? WHERE id=? AND edit_version=?",
            params![title,text,result_hash,prepared.relative_directory,created,note_id,base_version],
        ).map_err(storage)?;
        // Assets are note-owned even before publication. Retain unreferenced
        // files for drafts/history; reference removal never deletes an image.
        let images = {
            let mut statement = tx.prepare("SELECT token,relative_path,content_hash,mime_type,byte_size,original_name FROM note_staged_images WHERE note_id=? AND committed=0").map_err(storage)?;
            let rows = statement
                .query_map([&note_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                })
                .map_err(storage)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(storage)?
        };
        let references = image_references(&text);
        let mut attachments_changed = false;
        for (token, path, hash, mime, size, name) in images {
            if !references.contains(&path) {
                continue;
            }
            let added = tx.execute(
                "INSERT OR IGNORE INTO note_attachments(id,note_id,relative_path,original_name,content_hash,byte_size,mime_type,created_at) VALUES(?,?,?,?,?,?,?,?)",
                params![token,note_id,path,name,hash,size,mime,created],
            ).map_err(storage)?;
            attachments_changed |= added > 0;
            tx.execute(
                "UPDATE note_staged_images SET committed=1 WHERE token=?",
                [&token],
            )
            .map_err(storage)?;
        }
        if attachments_changed {
            let attachments = {
                let mut statement=tx.prepare("SELECT id,relative_path,original_name,content_hash,byte_size,mime_type,created_at FROM note_attachments WHERE note_id=? ORDER BY relative_path").map_err(storage)?;
                let rows = statement
                    .query_map([&note_id], |row| {
                        Ok(super::types::LibraryNoteAttachment {
                            id: row.get(0)?,
                            note_id: note_id.clone(),
                            relative_path: row.get(1)?,
                            original_name: row.get(2)?,
                            content_hash: row.get(3)?,
                            byte_size: row.get(4)?,
                            mime_type: row.get(5)?,
                            created_at: row.get(6)?,
                        })
                    })
                    .map_err(storage)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(storage)?
            };
            let meta_path = prepared.absolute_directory.join("meta.json");
            let mut meta: serde_json::Value =
                serde_json::from_slice(&fs::read(&meta_path).map_err(storage)?).map_err(storage)?;
            meta["attachments"] = serde_json::to_value(attachments).map_err(storage)?;
            super::note_files::replace_file(
                &meta_path,
                &serde_json::to_vec_pretty(&meta).map_err(storage)?,
            )?;
        }
        let result_version = version(&tx, &note_id)?;
        tx.execute("UPDATE note_save_operations SET state='committed',result_version=? WHERE operation_id=?", params![result_version,operation_id]).map_err(storage)?;
        tx.execute(
            "DELETE FROM note_editor_drafts WHERE note_id=? AND session_id=? AND generation<=?",
            params![note_id, request.session_id, request.draft_generation],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)
    }

    pub fn recover_note_saves(&self) -> Result<(), String> {
        let connection = self.open_connection()?;
        let ops = {
            let mut statement = connection.prepare("SELECT operation_id FROM note_save_operations WHERE state='prepared' ORDER BY created_at LIMIT 100").map_err(storage)?;
            let rows = statement
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(storage)?
        };
        for operation_id in ops {
            if let Err(error) = self.publish_note_save(&operation_id) {
                if error.starts_with("NOTE_VERSION_CONFLICT") {
                    connection
                        .execute(
                            "UPDATE note_save_operations SET state='conflict' WHERE operation_id=?",
                            [&operation_id],
                        )
                        .map_err(storage)?;
                }
            }
        }
        Ok(())
    }

    pub fn stage_note_image(
        &self,
        note_id: &str,
        session_id: &str,
        name: &str,
        data_base64: &str,
    ) -> Result<NoteImageAsset, String> {
        normalize_identifier("session ID", session_id)?;
        if data_base64.len() > MAX_IMAGE_BYTES * 4 / 3 + 8 {
            return Err("NOTE_ASSET_INVALID: 图片不能超过 8 MiB。".to_string());
        }
        let bytes = STANDARD
            .decode(data_base64)
            .map_err(|_| "NOTE_ASSET_INVALID: 图片编码无效。".to_string())?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("NOTE_ASSET_INVALID: 图片过大。".to_string());
        }
        let format = image::guess_format(&bytes)
            .map_err(|_| "NOTE_ASSET_INVALID: 无法识别图片格式。".to_string())?;
        let (extension, mime) = match format {
            image::ImageFormat::Png => ("png", "image/png"),
            image::ImageFormat::Jpeg => ("jpg", "image/jpeg"),
            image::ImageFormat::WebP => ("webp", "image/webp"),
            image::ImageFormat::Gif => ("gif", "image/gif"),
            _ => return Err("NOTE_ASSET_INVALID: 图片格式不支持。".to_string()),
        };
        let dimensions = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
            .into_dimensions()
            .map_err(|_| "NOTE_ASSET_INVALID: 图片损坏。".to_string())?;
        if u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_IMAGE_PIXELS {
            return Err("NOTE_ASSET_INVALID: 图片像素超限。".to_string());
        }
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        reader
            .decode()
            .map_err(|_| "NOTE_ASSET_INVALID: 图片内容损坏或解码超限。".to_string())?;
        self.get_note(note_id)?;
        let connection = self.open_connection()?;
        let directory: String = connection
            .query_row(
                "SELECT directory_path FROM library_notes WHERE id=?",
                [note_id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let directory = resolve_note_directory(&self.root_directory, &directory)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let relative_path = format!("attachments/{hash}.{extension}");
        let assets = directory.join("attachments");
        fs::create_dir_all(&assets).map_err(storage)?;
        if !assets
            .canonicalize()
            .map_err(storage)?
            .starts_with(directory.canonicalize().map_err(storage)?)
        {
            return Err("NOTE_ASSET_INVALID: 图片目录不可用。".into());
        }
        let target = directory.join(&relative_path);
        if target.exists() {
            if fs::read(&target).map_err(storage)? != bytes {
                return Err("NOTE_ASSET_INVALID: 图片路径发生冲突。".to_string());
            }
        } else {
            let temporary = assets.join(format!(".stage-{}", Uuid::new_v4()));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(storage)?;
            file.write_all(&bytes).map_err(storage)?;
            file.sync_all().map_err(storage)?;
            drop(file);
            fs::rename(&temporary, &target).map_err(storage)?;
        }
        let token = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO note_staged_images(token,note_id,session_id,content_hash,relative_path,mime_type,byte_size,original_name,created_at) VALUES(?,?,?,?,?,?,?,?,?)",
            params![token,note_id,session_id,hash,relative_path,mime,bytes.len() as i64,name.chars().take(200).collect::<String>(),now()],
        ).map_err(storage)?;
        Ok(NoteImageAsset {
            token,
            relative_path,
            content_hash: hash,
            mime_type: mime.to_string(),
        })
    }

    pub fn note_versions(&self, note_id: &str) -> Result<Vec<NoteVersionEntry>, String> {
        self.get_note(note_id)?;
        let connection = self.open_connection()?;
        let mut statement=connection.prepare(
            "SELECT id,title,content,reason,created_at,pinned FROM library_note_versions WHERE note_id=? ORDER BY pinned DESC,created_at DESC,id DESC LIMIT 100",
        ).map_err(storage)?;
        let rows = statement
            .query_map([note_id], |r| {
                let content: String = r.get(2)?;
                Ok(NoteVersionEntry {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    content_hash: content_hash(&content),
                    content,
                    reason: r.get(3)?,
                    created_at: r.get(4)?,
                    pinned: r.get(5)?,
                })
            })
            .map_err(storage)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(storage)
    }

    pub fn pin_note_version(
        &self,
        note_id: &str,
        version_id: &str,
        pinned: bool,
    ) -> Result<(), String> {
        self.get_note(note_id)?;
        self.open_connection()?
            .execute(
                "UPDATE library_note_versions SET pinned=? WHERE note_id=? AND id=?",
                params![pinned, note_id, version_id],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Create the new business row only after every referenced asset is
    /// verified. Failed publication leaves the original version untouched.
    pub fn copy_note_version(
        &self,
        note_id: &str,
        version_id: &str,
    ) -> Result<LibraryNote, String> {
        self.get_note(note_id)?;
        let mut connection = self.open_connection()?;
        let (title, content): (String, String) = connection
            .query_row(
                "SELECT title,content FROM library_note_versions WHERE note_id=? AND id=?",
                params![note_id, version_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(storage)?;
        let assets = attachment_references(&content)
            .into_iter()
            .map(|path| {
                let (bytes, mime) = self.note_asset_bytes(note_id, &path)?;
                let decoded = percent_encoding::percent_decode_str(&path)
                    .decode_utf8()
                    .map_err(storage)?
                    .into_owned();
                Ok((decoded, bytes, mime))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let id = Uuid::new_v4().to_string();
        let created = now();
        let prepared = super::note_files::prepare_note_directory(
            &self.root_directory,
            &id,
            &title,
            &content,
            None,
            &[],
            created as u64,
        )?;
        let result = (|| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(storage)?;
            tx.execute("INSERT INTO library_notes(id,item_id,title,content,group_name,created_at,updated_at,directory_path,content_hash) VALUES(?,NULL,?,?,NULL,?,?,?,?)",
                params![id,title,content,created,created,prepared.relative_directory,prepared.content_hash]).map_err(storage)?;
            let mut attachments = Vec::new();
            let mut copied = std::collections::HashSet::new();
            for (path, bytes, mime) in assets {
                if !copied.insert(path.clone()) {
                    continue;
                }
                let target = prepared.absolute_directory.join(&path);
                fs::create_dir_all(target.parent().unwrap()).map_err(storage)?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .map_err(storage)?;
                file.write_all(&bytes).map_err(storage)?;
                file.sync_all().map_err(storage)?;
                let attachment = super::types::LibraryNoteAttachment {
                    id: Uuid::new_v4().to_string(),
                    note_id: id.clone(),
                    relative_path: path.clone(),
                    original_name: std::path::Path::new(&path)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    content_hash: format!("{:x}", Sha256::digest(&bytes)),
                    byte_size: bytes.len() as u64,
                    mime_type: Some(mime),
                    created_at: created as u64,
                };
                tx.execute("INSERT INTO note_attachments(id,note_id,relative_path,original_name,content_hash,byte_size,mime_type,created_at) VALUES(?,?,?,?,?,?,?,?)",
                    params![attachment.id,id,attachment.relative_path,attachment.original_name,attachment.content_hash,attachment.byte_size,attachment.mime_type,created]).map_err(storage)?;
                attachments.push(attachment);
            }
            let meta = serde_json::json!({"schemaVersion":1,"noteId":id,"title":title,"contentHash":prepared.content_hash,"attachments":attachments,"createdAt":created});
            super::note_files::replace_file(
                &prepared.absolute_directory.join("meta.json"),
                &serde_json::to_vec_pretty(&meta).map_err(storage)?,
            )?;
            tx.commit().map_err(storage)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&prepared.absolute_directory);
        }
        result?;
        self.get_note(&id)
    }

    pub fn note_asset_bytes(
        &self,
        note_id: &str,
        relative_path: &str,
    ) -> Result<(Vec<u8>, String), String> {
        let note = self.get_note(note_id)?;
        let relative = percent_encoding::percent_decode_str(relative_path)
            .decode_utf8()
            .map_err(storage)?;
        let path = std::path::Path::new(relative.as_ref());
        if !relative.starts_with("attachments/")
            || path
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("NOTE_ASSET_INVALID: 附件路径不可用。".into());
        }
        let directory = note
            .directory_path
            .ok_or_else(|| "NOTE_SOURCE_MISSING".to_string())?;
        let directory = std::path::Path::new(&directory)
            .canonicalize()
            .map_err(storage)?;
        let resolved = directory.join(path).canonicalize().map_err(storage)?;
        if !resolved.starts_with(&directory) {
            return Err("NOTE_ASSET_INVALID: 附件不属于当前笔记。".into());
        }
        let connection = self.open_connection()?;
        let mime:Option<String>=connection.query_row(
            "SELECT COALESCE(mime_type,'application/octet-stream') FROM note_attachments WHERE note_id=?1 AND relative_path=?2
             UNION ALL SELECT COALESCE(mime_type,'application/octet-stream') FROM note_staged_images WHERE note_id=?1 AND relative_path=?2 LIMIT 1",
            params![note_id,relative],|r|r.get(0),
        ).optional().map_err(storage)?;
        let mime = mime.ok_or_else(|| "NOTE_ASSET_INVALID: 未登记的附件。".to_string())?;
        let file = fs::File::open(&resolved).map_err(storage)?;
        if file.metadata().map_err(storage)?.len() > 32 * 1024 * 1024 {
            return Err("NOTE_ASSET_INVALID: 附件超过导出预算。".into());
        }
        let mut bytes = Vec::new();
        use std::io::Read;
        file.take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(storage)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("NOTE_ASSET_INVALID: 附件超过导出预算。".into());
        }
        Ok((bytes, mime))
    }

    pub fn export_note_snapshot(
        &self,
        note_id: &str,
        title: &str,
        markdown: &str,
        destination: &str,
    ) -> Result<String, String> {
        self.get_note(note_id)?;
        validate_note_content(markdown)?;
        let destination = std::path::Path::new(destination);
        if !destination.is_dir() {
            return Err("NOTE_STORAGE_UNAVAILABLE: 请选择导出目录。".into());
        }
        let title = title
            .chars()
            .take(100)
            .map(|c| {
                if c.is_control() || "<>:\"/\\|?*".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect::<String>();
        let name = title.trim().trim_matches(['.', ' ']);
        let output = destination.join(format!(
            "{}-{}",
            if name.is_empty() { "note" } else { name },
            Uuid::new_v4()
        ));
        let staging = destination.join(format!(".mnemora-export-{}", Uuid::new_v4()));
        fs::create_dir(&staging).map_err(storage)?;
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(staging.join("note.md"))
                .map_err(storage)?;
            file.write_all(markdown.as_bytes()).map_err(storage)?;
            file.sync_all().map_err(storage)?;
            drop(file);
            let references = attachment_references(markdown);
            let mut copied = std::collections::HashSet::new();
            for reference in references {
                let (bytes, _) = self.note_asset_bytes(note_id, &reference)?;
                let relative = percent_encoding::percent_decode_str(&reference)
                    .decode_utf8()
                    .map_err(storage)?;
                if !copied.insert(relative.to_string()) {
                    continue;
                }
                let target = staging.join(relative.as_ref());
                fs::create_dir_all(target.parent().unwrap()).map_err(storage)?;
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(target)
                    .map_err(storage)?;
                file.write_all(&bytes).map_err(storage)?;
                file.sync_all().map_err(storage)?;
            }
            let sources = self.list_note_sources(note_id)?;
            let summary = serde_json::json!({"title":title,"contentHash":content_hash(markdown),"sources":sources});
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staging.join("sources.json"))
                .map_err(storage)?;
            file.write_all(&serde_json::to_vec_pretty(&summary).map_err(storage)?)
                .map_err(storage)?;
            file.sync_all().map_err(storage)?;
            drop(file);
            fs::rename(&staging, &output).map_err(storage)?;
            Ok(output.to_string_lossy().into_owned())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }
}

fn image_references(markdown: &str) -> std::collections::HashSet<String> {
    attachment_references(markdown)
        .into_iter()
        .filter_map(|path| {
            percent_encoding::percent_decode_str(&path)
                .decode_utf8()
                .ok()
                .map(|path| path.into_owned())
        })
        .collect()
}

fn attachment_references(markdown: &str) -> std::collections::HashSet<String> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let mut references = std::collections::HashSet::new();
    for event in Parser::new_ext(markdown, Options::all()) {
        match event {
            Event::Start(Tag::Image { dest_url, .. })
            | Event::Start(Tag::Link { dest_url, .. })
                if dest_url.starts_with("attachments/") =>
            {
                references.insert(dest_url.to_string());
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                // Parse attributes structurally, including images preserved in
                // merged-cell HTML tables. The asset reader rechecks ownership.
                let mut reader = quick_xml::Reader::from_str(&html);
                reader.config_mut().check_end_names = false;
                loop {
                    match reader.read_event() {
                        Ok(
                            quick_xml::events::Event::Start(element)
                            | quick_xml::events::Event::Empty(element),
                        ) => {
                            let tag = element.name().as_ref().to_ascii_lowercase();
                            if tag != b"img" && tag != b"a" {
                                continue;
                            }
                            for attribute in element.attributes().flatten() {
                                let name = attribute.key.as_ref().to_ascii_lowercase();
                                if name != b"src" && name != b"href" {
                                    continue;
                                }
                                if let Ok(decoded) =
                                    reader.decoder().decode(attribute.value.as_ref())
                                {
                                    if let Ok(value) = quick_xml::escape::unescape(&decoded) {
                                        if value.starts_with("attachments/") {
                                            references.insert(value.into_owned());
                                        }
                                    }
                                }
                            }
                        }
                        Ok(quick_xml::events::Event::Eof) | Err(_) => break,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    references
}

fn publish_markdown(
    directory: &std::path::Path,
    operation_id: &str,
    base_hash: &str,
    result_hash: &str,
    text: &str,
) -> Result<(), String> {
    let versions = directory.join("versions");
    fs::create_dir_all(&versions).map_err(storage)?;
    let key = content_hash(operation_id);
    let backup = versions.join(format!("{key}-note.md"));
    let target = directory.join("note.md");
    if backup.exists() {
        if content_hash(&fs::read_to_string(&backup).map_err(storage)?) != base_hash {
            return Err(conflict());
        }
        if content_hash(&fs::read_to_string(&target).map_err(storage)?) == result_hash {
            return Ok(());
        }
        return Err(conflict());
    }
    if content_hash(&fs::read_to_string(&target).map_err(storage)?) == result_hash {
        return Ok(());
    }
    let temporary = directory.join(format!(".save-{key}-{}", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(storage)?;
    file.write_all(text.as_bytes()).map_err(storage)?;
    file.sync_all().map_err(storage)?;
    drop(file);
    replace_with_backup(&target, &temporary, &backup)?;
    if content_hash(&fs::read_to_string(&backup).map_err(storage)?) != base_hash {
        return Err(conflict());
    }
    Ok(())
}

#[cfg(windows)]
fn replace_with_backup(
    target: &std::path::Path,
    temporary: &std::path::Path,
    backup: &std::path::Path,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let wide = |path: &std::path::Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let (target, temporary, backup) = (wide(target), wide(temporary), wide(backup));
    // The backup contains the bytes actually replaced, even if an external
    // editor writes between our preflight check and this atomic replacement.
    let result = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            temporary.as_ptr(),
            backup.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(storage(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_with_backup(
    target: &std::path::Path,
    temporary: &std::path::Path,
    backup: &std::path::Path,
) -> Result<(), String> {
    fs::hard_link(target, backup).map_err(storage)?;
    fs::rename(temporary, target).map_err(storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::types::LibraryNoteCreate;

    fn setup() -> (std::path::PathBuf, LibraryRepository, NoteEditingSnapshot) {
        let root = std::env::temp_dir().join(format!("mnemora-note-edit-{}", Uuid::new_v4()));
        let repo = LibraryRepository::new(root.clone());
        repo.initialize().unwrap();
        let note = repo
            .create_note(LibraryNoteCreate {
                item_id: None,
                title: "Test".into(),
                content: "  original  \n\n".into(),
                group_name: None,
            })
            .unwrap();
        let snapshot = repo.note_editing_snapshot(&note.id).unwrap();
        (root, repo, snapshot)
    }

    fn request(base: &NoteEditingSnapshot, text: &str) -> SaveNoteRequest {
        SaveNoteRequest {
            note_id: base.note.id.clone(),
            session_id: "session-1".into(),
            operation_id: Uuid::new_v4().to_string(),
            draft_generation: 1,
            expected_note_version: base.note_version.clone(),
            expected_content_hash: base.content_hash.clone(),
            expected_disk_hash: base.disk_hash.clone(),
            title: base.note.title.clone(),
            markdown: text.into(),
            accept_external_change: false,
            reason: "typing".into(),
        }
    }

    #[test]
    fn versioned_selection_validation_rejects_stale_hash_and_utf8_boundaries() {
        let (root, repo, base) = setup();
        let saved = repo
            .save_note_checked(request(&base, "😀 原文\n"))
            .unwrap();
        let bytes = "😀 原文\n".as_bytes().len() as u32;
        repo.validate_note_selection(
            &base.note.id,
            &saved.note_version,
            &saved.content_hash,
            0,
            bytes,
            "😀 原文\n",
        )
        .unwrap();
        assert!(repo
            .validate_note_selection(
                &base.note.id,
                &saved.note_version,
                &saved.content_hash,
                1,
                bytes,
                "😀 原文\n",
            )
            .is_err());
        assert!(repo
            .validate_note_selection(
                &base.note.id,
                &saved.note_version,
                "stale",
                0,
                bytes,
                "😀 原文\n",
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn versioned_saves_keep_whitespace_reject_stale_edits_and_retry_idempotently() {
        let (root, repo, base) = setup();
        assert_eq!(base.note.content, "  original  \n\n");
        let write = request(&base, "  replaced  \n\n");
        let saved = repo.save_note_checked(write.clone()).unwrap();
        assert_eq!(saved.committed_markdown, "  replaced  \n\n");
        assert_ne!(saved.note_version, base.note_version);
        assert!(repo
            .save_note_checked(request(&base, "stale"))
            .unwrap_err()
            .starts_with("NOTE_VERSION_CONFLICT"));
        assert_eq!(
            repo.save_note_checked(write.clone()).unwrap().note_version,
            saved.note_version
        );
        let mut changed = write;
        changed.markdown = "different".into();
        assert!(repo
            .save_note_checked(changed)
            .unwrap_err()
            .starts_with("NOTE_OPERATION_MISMATCH"));
        assert_eq!(repo.note_versions(&base.note.id).unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn draft_generations_and_external_file_changes_are_not_overwritten() {
        let (root, repo, base) = setup();
        let draft = NoteDraft {
            note_id: base.note.id.clone(),
            session_id: "session-1".into(),
            generation: 2,
            base_version: base.note_version.clone(),
            title: "Test".into(),
            content: "new draft".into(),
            updated_at: 0,
        };
        repo.checkpoint_note_draft(draft.clone()).unwrap();
        let mut stale = draft;
        stale.generation = 1;
        stale.content = "stale".into();
        repo.checkpoint_note_draft(stale).unwrap();
        repo.discard_note_draft(&base.note.id, "session-1", 1)
            .unwrap();
        assert_eq!(
            repo.note_editing_snapshot(&base.note.id).unwrap().drafts[0].content,
            "new draft"
        );
        fs::write(
            std::path::Path::new(base.note.directory_path.as_ref().unwrap()).join("note.md"),
            "external edit",
        )
        .unwrap();
        assert!(repo.save_note_checked(request(&base, "overwrite")).is_err());
        assert_eq!(
            repo.note_editing_snapshot(&base.note.id)
                .unwrap()
                .external_content
                .as_deref(),
            Some("external edit")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn original_receipt_survives_later_saves_and_noop_retries() {
        let (root, repo, base) = setup();
        let first = request(&base, "first");
        let receipt = repo.save_note_checked(first.clone()).unwrap();
        let current = repo.note_editing_snapshot(&base.note.id).unwrap();
        repo.save_note_checked(request(&current, "second")).unwrap();
        let retry = repo.save_note_checked(first).unwrap();
        assert_eq!(retry.note_version, receipt.note_version);
        assert_eq!(retry.committed_markdown, "first");
        let current = repo.note_editing_snapshot(&base.note.id).unwrap();
        let noop = request(&current, "second");
        let result = repo.save_note_checked(noop.clone()).unwrap();
        assert_eq!(result.note_version, current.note_version);
        assert_eq!(
            repo.save_note_checked(noop).unwrap().operation_id,
            result.operation_id
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn published_file_is_recovered_after_database_failure() {
        let (root, repo, base) = setup();
        let connection = repo.open_connection().unwrap();
        connection.execute_batch("CREATE TRIGGER fail_note_update BEFORE UPDATE OF content ON library_notes BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        let write = request(&base, "recovered content");
        assert!(repo.save_note_checked(write.clone()).is_err());
        assert_eq!(
            fs::read_to_string(
                std::path::Path::new(base.note.directory_path.as_ref().unwrap()).join("note.md")
            )
            .unwrap(),
            "recovered content"
        );
        assert_eq!(
            repo.get_note(&base.note.id).unwrap().content,
            base.note.content
        );
        repo.recover_note_saves().unwrap();
        let state: String = connection
            .query_row(
                "SELECT state FROM note_save_operations WHERE operation_id=?",
                [&write.operation_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "prepared");
        connection
            .execute_batch("DROP TRIGGER fail_note_update")
            .unwrap();
        repo.initialize().unwrap();
        assert_eq!(
            repo.get_note(&base.note.id).unwrap().content,
            "recovered content"
        );
        assert_eq!(
            repo.save_note_checked(write).unwrap().committed_markdown,
            "recovered content"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn image_hash_uses_bytes_and_code_examples_are_not_asset_references() {
        let (root, repo, base) = setup();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let bytes = bytes.into_inner();
        let asset = repo
            .stage_note_image(
                &base.note.id,
                "session-1",
                "figure.png",
                &STANDARD.encode(&bytes),
            )
            .unwrap();
        assert_eq!(asset.content_hash, format!("{:x}", Sha256::digest(&bytes)));
        let code = format!("```md\n![]({})\n```", asset.relative_path);
        repo.save_note_checked(request(&base, &code)).unwrap();
        assert!(repo.get_note(&base.note.id).unwrap().attachments.is_empty());
        let current = repo.note_editing_snapshot(&base.note.id).unwrap();
        repo.save_note_checked(request(
            &current,
            &format!("![figure]({})", asset.relative_path),
        ))
        .unwrap();
        assert_eq!(repo.get_note(&base.note.id).unwrap().attachments.len(), 1);
        assert!(repo
            .stage_note_image(
                &base.note.id,
                "session-1",
                "bad.svg",
                &STANDARD.encode(b"<svg></svg>")
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn checked_replace_retains_actual_external_bytes() {
        let (root, _repo, base) = setup();
        let directory = std::path::Path::new(base.note.directory_path.as_ref().unwrap());
        fs::write(directory.join("note.md"), "raced external content").unwrap();
        let result = publish_markdown(
            directory,
            "racing-op",
            &base.content_hash,
            &content_hash("mine"),
            "mine",
        );
        assert!(result.unwrap_err().starts_with("NOTE_VERSION_CONFLICT"));
        assert_eq!(
            fs::read_to_string(
                directory
                    .join("versions")
                    .join(format!("{}-note.md", content_hash("racing-op")))
            )
            .unwrap(),
            "raced external content"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ai_confirmation_resumes_after_metadata_failure_without_reapplying_text() {
        use crate::library::types::NoteEditProposalCreate;
        let (root, repo, base) = setup();
        repo.create_note_edit_proposal(NoteEditProposalCreate {
            id: "ai-recovery-test".into(),
            note_id: base.note.id.clone(),
            conversation_id: "conversation-1".into(),
            source_message_id: None,
            expected_note_updated_at: base.note.updated_at,
            old_title: base.note.title.clone(),
            new_title: base.note.title.clone(),
            old_content: base.note.content.clone(),
            new_content: "AI edited content".into(),
            diff: "changed".into(),
            sources: vec![],
            coverage_snapshot_json: String::new(),
            source_units: vec![],
        })
        .unwrap();
        let connection = repo.open_connection().unwrap();
        connection.execute_batch("CREATE TRIGGER fail_proposal_metadata BEFORE UPDATE ON note_edit_proposals BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        assert!(repo
            .resolve_note_edit_proposal("ai-recovery-test", true)
            .is_err());
        let first = repo.note_editing_snapshot(&base.note.id).unwrap();
        assert_eq!(first.note.content, "AI edited content");
        connection
            .execute_batch("DROP TRIGGER fail_proposal_metadata")
            .unwrap();
        repo.resolve_note_edit_proposal("ai-recovery-test", true)
            .unwrap();
        assert_eq!(
            repo.note_editing_snapshot(&base.note.id)
                .unwrap()
                .note_version,
            first.note_version
        );
        assert_eq!(repo.note_versions(&base.note.id).unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bundle_exports_the_selected_draft_and_only_referenced_assets() {
        let (root, repo, base) = setup();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let bytes = bytes.into_inner();
        let image = repo
            .stage_note_image(
                &base.note.id,
                "session-1",
                "figure.png",
                &STANDARD.encode(bytes),
            )
            .unwrap();
        let draft = format!("# Draft\n\n![figure]({})\n", image.relative_path);
        let output = repo
            .export_note_snapshot(
                &base.note.id,
                "exported-draft",
                &draft,
                root.to_str().unwrap(),
            )
            .unwrap();
        let output = std::path::Path::new(&output);
        assert_eq!(fs::read_to_string(output.join("note.md")).unwrap(), draft);
        assert!(output.join(&image.relative_path).is_file());
        assert!(!output.join("versions").exists());
        assert!(!output.join("meta.json").exists());
        assert!(repo
            .note_asset_bytes(&base.note.id, "attachments/../../library.sqlite")
            .is_err());
        assert_eq!(
            repo.get_note(&base.note.id).unwrap().content,
            base.note.content
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn version_copy_preserves_images_after_the_current_note_removed_them() {
        let (root, repo, base) = setup();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let asset = repo
            .stage_note_image(
                &base.note.id,
                "session-1",
                "figure.png",
                &STANDARD.encode(bytes.into_inner()),
            )
            .unwrap();
        let with_image = format!(
            "<table><tr><td colspan=\"2\"><img src=\"{}\" /></td></tr></table>",
            asset.relative_path
        );
        let mut save = request(&base, &with_image);
        save.reason = "explicitSave".into();
        repo.save_note_checked(save).unwrap();
        assert_eq!(repo.get_note(&base.note.id).unwrap().attachments.len(), 1);
        let current = repo.note_editing_snapshot(&base.note.id).unwrap();
        let mut save = request(&current, "Image removed");
        save.reason = "explicitSave".into();
        repo.save_note_checked(save).unwrap();
        let historical = repo
            .note_versions(&base.note.id)
            .unwrap()
            .into_iter()
            .find(|version| version.content == with_image)
            .unwrap();
        let copy = repo
            .copy_note_version(&base.note.id, &historical.id)
            .unwrap();
        assert_ne!(copy.id, base.note.id);
        assert_eq!(copy.content, with_image);
        assert_eq!(copy.attachments.len(), 1);
        assert_eq!(
            repo.note_asset_bytes(&copy.id, &asset.relative_path)
                .unwrap()
                .0,
            repo.note_asset_bytes(&base.note.id, &asset.relative_path)
                .unwrap()
                .0
        );
        assert_eq!(
            repo.get_note(&base.note.id).unwrap().content,
            "Image removed"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn staged_images_survive_restart_without_committing_or_indexing_drafts() {
        let (root, repo, base) = setup();
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let data = bytes.into_inner();
        let asset = repo
            .stage_note_image(
                &base.note.id,
                "session-1",
                "figure.png",
                &STANDARD.encode(&data),
            )
            .unwrap();
        repo.initialize().unwrap();
        let snapshot = repo.note_editing_snapshot(&base.note.id).unwrap();
        assert_eq!(snapshot.staged_images[0].token, asset.token);
        assert!(snapshot.note.attachments.is_empty());
        assert_eq!(snapshot.note.content, base.note.content);
        assert!(repo
            .stage_note_image(
                &base.note.id,
                "session-1",
                "broken.png",
                &STANDARD.encode(&data[..33])
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }
}
