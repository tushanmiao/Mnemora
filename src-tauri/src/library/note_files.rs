use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::types::LibraryNoteAttachment;

pub const NOTE_DIRECTORY_NAME: &str = "notes";
const NOTE_MARKDOWN_FILE: &str = "note.md";
const NOTE_META_FILE: &str = "meta.json";
const NOTE_SIDECAR_FILE: &str = "sidecar.json";

#[derive(Debug, Clone)]
pub struct NoteAttachmentSource {
    pub source_path: PathBuf,
    pub original_name: String,
    pub mime_type: Option<String>,
}

#[derive(Debug)]
pub struct PreparedNoteDirectory {
    pub relative_directory: String,
    #[cfg_attr(not(test), allow(dead_code))]
    pub absolute_directory: PathBuf,
    pub content: String,
    pub content_hash: String,
    pub attachments: Vec<LibraryNoteAttachment>,
}

pub fn content_hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

pub fn prepare_note_directory(
    library_root: &Path,
    note_id: &str,
    title: &str,
    content: &str,
    sidecar_json: Option<&str>,
    attachment_sources: &[NoteAttachmentSource],
    created_at: u64,
) -> Result<PreparedNoteDirectory, String> {
    let notes_root = library_root.join(NOTE_DIRECTORY_NAME);
    fs::create_dir_all(&notes_root).map_err(|error| format!("创建笔记目录失败：{error}"))?;
    let final_directory = notes_root.join(note_id);
    if final_directory.exists() {
        return Err(format!("笔记目录已存在，拒绝覆盖：{note_id}"));
    }
    let staging_directory = notes_root.join(format!(".mnemora-note-{}", Uuid::new_v4()));
    fs::create_dir(&staging_directory).map_err(|error| format!("创建笔记暂存目录失败：{error}"))?;

    let result = (|| {
        fs::create_dir(staging_directory.join("versions"))
            .map_err(|error| format!("创建笔记版本目录失败：{error}"))?;
        let mut attachments = Vec::new();
        if !attachment_sources.is_empty() {
            fs::create_dir(staging_directory.join("attachments"))
                .map_err(|error| format!("创建笔记附件目录失败：{error}"))?;
        }
        let mut used_names = HashSet::new();
        for source in attachment_sources {
            let (hash, byte_size) = hash_file(&source.source_path)?;
            let safe_name = safe_attachment_name(&source.original_name);
            let mut file_name = format!("{}-{safe_name}", &hash[..12]);
            let mut generation = 1u32;
            while !used_names.insert(file_name.to_ascii_lowercase()) {
                file_name = format!("{}-{generation}-{safe_name}", &hash[..12]);
                generation = generation.saturating_add(1);
            }
            let relative_path = format!("attachments/{file_name}");
            copy_and_sync(&source.source_path, &staging_directory.join(&relative_path))?;
            attachments.push(LibraryNoteAttachment {
                id: Uuid::new_v4().to_string(),
                note_id: note_id.to_string(),
                relative_path,
                original_name: source.original_name.clone(),
                content_hash: hash,
                byte_size,
                mime_type: source.mime_type.clone(),
                created_at,
            });
        }

        let content = append_attachment_links(content, &attachments);
        let content_hash = content_hash(&content);
        write_and_sync(
            &staging_directory.join(NOTE_MARKDOWN_FILE),
            content.as_bytes(),
        )?;
        let pipeline_metadata = sidecar_json
            .filter(|value| !value.trim().is_empty())
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
        let meta = serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "noteId": note_id,
            "title": title,
            "contentHash": content_hash,
            "attachments": attachments,
            "pipeline": pipeline_metadata,
            "createdAt": created_at,
        }))
        .map_err(|error| format!("序列化笔记元数据失败：{error}"))?;
        write_and_sync(&staging_directory.join(NOTE_META_FILE), &meta)?;
        if let Some(sidecar) = sidecar_json.filter(|value| !value.trim().is_empty()) {
            write_and_sync(
                &staging_directory.join(NOTE_SIDECAR_FILE),
                sidecar.as_bytes(),
            )?;
        }
        sync_directory(&staging_directory)?;
        fs::rename(&staging_directory, &final_directory)
            .map_err(|error| format!("原子落地笔记目录失败：{error}"))?;
        sync_directory(&notes_root)?;
        Ok(PreparedNoteDirectory {
            relative_directory: format!("{NOTE_DIRECTORY_NAME}/{note_id}"),
            absolute_directory: final_directory.clone(),
            content,
            content_hash,
            attachments,
        })
    })();

    if result.is_err() && staging_directory.exists() {
        let _ = fs::remove_dir_all(&staging_directory);
    }
    result
}

pub fn refresh_note_directory(
    library_root: &Path,
    stored_directory: Option<&str>,
    note_id: &str,
    title: &str,
    content: &str,
    updated_at: u64,
) -> Result<PreparedNoteDirectory, String> {
    let Some(stored_directory) = stored_directory else {
        return prepare_note_directory(
            library_root,
            note_id,
            title,
            content,
            None,
            &[],
            updated_at,
        );
    };
    let directory = resolve_note_directory(library_root, stored_directory)?;
    if !directory.is_dir() {
        return prepare_note_directory(
            library_root,
            note_id,
            title,
            content,
            None,
            &[],
            updated_at,
        );
    }
    let markdown_path = directory.join(NOTE_MARKDOWN_FILE);
    if let Ok(previous) = fs::read(&markdown_path) {
        if previous != content.as_bytes() {
            let versions = directory.join("versions");
            fs::create_dir_all(&versions)
                .map_err(|error| format!("创建笔记版本目录失败：{error}"))?;
            let mut version_path = versions.join(format!("{updated_at}-note.md"));
            if version_path.exists() {
                version_path = versions.join(format!("{updated_at}-{}-note.md", Uuid::new_v4()));
            }
            write_and_sync(&version_path, &previous)?;
            sync_directory(&versions)?;
        }
    }
    replace_file(&markdown_path, content.as_bytes())?;
    let content_hash = content_hash(content);
    let attachments = Vec::new();
    let mut meta = fs::read(directory.join(NOTE_META_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    meta.insert("schemaVersion".to_string(), serde_json::json!(1));
    meta.insert("noteId".to_string(), serde_json::json!(note_id));
    meta.insert("title".to_string(), serde_json::json!(title));
    meta.insert("contentHash".to_string(), serde_json::json!(content_hash));
    meta.entry("attachments".to_string())
        .or_insert_with(|| serde_json::json!([]));
    meta.insert("updatedAt".to_string(), serde_json::json!(updated_at));
    let meta = serde_json::to_vec_pretty(&serde_json::Value::Object(meta))
        .map_err(|error| format!("序列化笔记元数据失败：{error}"))?;
    replace_file(&directory.join(NOTE_META_FILE), &meta)?;
    sync_directory(&directory)?;
    Ok(PreparedNoteDirectory {
        relative_directory: stored_directory.replace('\\', "/"),
        absolute_directory: directory,
        content: content.to_string(),
        content_hash,
        attachments,
    })
}

pub fn resolve_note_directory(
    library_root: &Path,
    stored_directory: &str,
) -> Result<PathBuf, String> {
    let path = Path::new(stored_directory);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("笔记目录路径必须是 library 内的安全相对路径。".to_string());
    }
    let resolved = library_root.join(path);
    let notes_root = library_root.join(NOTE_DIRECTORY_NAME);
    if !resolved.starts_with(&notes_root) {
        return Err("笔记目录超出 notes 范围。".to_string());
    }
    Ok(resolved)
}

pub fn export_note_bundle(
    library_root: &Path,
    stored_directory: Option<&str>,
    title: &str,
    content: &str,
    destination_parent: &Path,
) -> Result<PathBuf, String> {
    fs::create_dir_all(destination_parent)
        .map_err(|error| format!("创建笔记导出位置失败：{error}"))?;
    if !destination_parent.is_dir() {
        return Err("笔记必须导出到目录。".to_string());
    }
    let base_name = safe_attachment_name(title);
    let final_directory = unique_directory(destination_parent, &base_name);
    let staging = destination_parent.join(format!(".mnemora-note-export-{}", Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|error| format!("创建笔记导出暂存目录失败：{error}"))?;
    let result = (|| {
        if let Some(stored_directory) = stored_directory {
            let source = resolve_note_directory(library_root, stored_directory)?;
            copy_directory_contents(&source, &staging)?;
        } else {
            let file_name = format!("{}.md", safe_attachment_name(title));
            write_and_sync(&staging.join(file_name), content.as_bytes())?;
        }
        fs::rename(&staging, &final_directory)
            .map_err(|error| format!("完成笔记导出失败：{error}"))?;
        Ok(final_directory.clone())
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

pub fn collect_orphan_note_directories(
    library_root: &Path,
    live_note_ids: &HashSet<String>,
    grace_period: Duration,
) -> Result<usize, String> {
    let notes_root = library_root.join(NOTE_DIRECTORY_NAME);
    if !notes_root.is_dir() {
        return Ok(0);
    }
    let trash = notes_root.join(".trash");
    fs::create_dir_all(&trash).map_err(|error| format!("创建笔记回收站失败：{error}"))?;
    let now = SystemTime::now();
    let timestamp = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut moved = 0usize;
    for entry in fs::read_dir(&notes_root).map_err(|error| format!("扫描笔记目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取笔记目录项失败：{error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".trash" || !entry.path().is_dir() {
            continue;
        }
        let is_staging = name.starts_with(".mnemora-note-");
        let is_orphan_note = Uuid::parse_str(&name).is_ok() && !live_note_ids.contains(&name);
        if !is_staging && !is_orphan_note {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|error| format!("读取孤儿笔记目录时间失败：{error}"))?;
        if now.duration_since(modified).unwrap_or_default() < grace_period {
            continue;
        }
        let destination = trash.join(format!("{timestamp}-{name}-{}", Uuid::new_v4()));
        fs::rename(entry.path(), destination)
            .map_err(|error| format!("回收孤儿笔记目录失败：{error}"))?;
        moved = moved.saturating_add(1);
    }
    Ok(moved)
}

fn append_attachment_links(content: &str, attachments: &[LibraryNoteAttachment]) -> String {
    if attachments.is_empty() {
        return content.to_string();
    }
    let mut output = content.trim_end().to_string();
    output.push_str("\n\n## 附件\n\n");
    for attachment in attachments {
        let label = attachment
            .original_name
            .replace('[', "\\[")
            .replace(']', "\\]");
        let destination = attachment.relative_path.replace(' ', "%20");
        let is_image = attachment
            .mime_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"));
        if is_image {
            output.push_str(&format!("- ![{label}](<{destination}>)\n"));
        } else {
            output.push_str(&format!("- [{label}](<{destination}>)\n"));
        }
    }
    output
}

fn safe_attachment_name(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let bounded = cleaned
        .trim()
        .trim_matches(['.', ' '])
        .chars()
        .take(120)
        .collect::<String>();
    if bounded.is_empty() {
        "attachment".to_string()
    } else {
        bounded
    }
}

fn unique_directory(parent: &Path, base_name: &str) -> PathBuf {
    let initial = parent.join(base_name);
    if !initial.exists() {
        return initial;
    }
    for generation in 2..=10_000u32 {
        let candidate = parent.join(format!("{base_name} ({generation})"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{base_name}-{}", Uuid::new_v4()))
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err("笔记目录缺失，无法导出。".to_string());
    }
    for entry in fs::read_dir(source).map_err(|error| format!("读取笔记目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取笔记目录项失败：{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取笔记目录项类型失败：{error}"))?;
        if file_type.is_symlink() {
            return Err("笔记导出不跟随符号链接。".to_string());
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir(&target).map_err(|error| format!("创建导出子目录失败：{error}"))?;
            copy_directory_contents(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target).map_err(|error| format!("复制笔记文件失败：{error}"))?;
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|error| format!("打开笔记附件失败：{error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("读取笔记附件失败：{error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    Ok((format!("{:x}", digest.finalize()), size))
}

fn copy_and_sync(source: &Path, destination: &Path) -> Result<(), String> {
    let mut source = File::open(source).map_err(|error| format!("打开笔记附件失败：{error}"))?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("创建笔记附件失败：{error}"))?;
    std::io::copy(&mut source, &mut destination)
        .map_err(|error| format!("复制笔记附件失败：{error}"))?;
    destination
        .sync_all()
        .map_err(|error| format!("同步笔记附件失败：{error}"))
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("创建笔记文件失败：{error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("写入笔记文件失败：{error}"))?;
    file.sync_all()
        .map_err(|error| format!("同步笔记文件失败：{error}"))
}

pub(super) fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "笔记文件缺少父目录。".to_string())?;
    let temporary = parent.join(format!(".mnemora-write-{}", Uuid::new_v4()));
    write_and_sync(&temporary, bytes)?;
    let backup = parent.join(format!(".mnemora-old-{}", Uuid::new_v4()));
    let had_existing = path.exists();
    if had_existing {
        fs::rename(path, &backup).map_err(|error| format!("暂存旧笔记文件失败：{error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_existing {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("替换笔记文件失败：{error}"));
    }
    if had_existing {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), String> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    match OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .and_then(|directory| directory.sync_all())
    {
        Ok(()) => Ok(()),
        // Windows 的 FlushFileBuffers 对目录句柄经常返回 ACCESS_DENIED；文件本身已经
        // sync_all，目录发布仍由同卷 rename 保证原子可见。
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(error) => Err(format!("同步笔记目录失败：{error}")),
    }
}

#[cfg(not(windows))]
fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("同步笔记目录失败：{error}"))
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs, time::Duration};

    use super::{
        collect_orphan_note_directories, export_note_bundle, prepare_note_directory,
        NoteAttachmentSource,
    };
    use uuid::Uuid;

    #[test]
    fn stages_and_atomically_publishes_note_bundle() {
        let root =
            std::env::temp_dir().join(format!("mnemora-note-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("chart image.png");
        fs::write(&source, b"image-bytes").unwrap();
        let prepared = prepare_note_directory(
            &root,
            "note-1",
            "Title",
            "# Body",
            Some("{\"schemaVersion\":1}"),
            &[NoteAttachmentSource {
                source_path: source,
                original_name: "chart image.png".to_string(),
                mime_type: Some("image/png".to_string()),
            }],
            1,
        )
        .unwrap();
        assert!(prepared.absolute_directory.join("note.md").is_file());
        assert!(prepared.absolute_directory.join("meta.json").is_file());
        assert!(prepared.absolute_directory.join("sidecar.json").is_file());
        assert_eq!(prepared.attachments.len(), 1);
        assert!(prepared.content.contains("attachments/"));
        assert!(!root.join("notes").read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mnemora-note-")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exports_bundle_and_moves_orphans_to_trash() {
        let root = std::env::temp_dir().join(format!("mnemora-note-gc-{}", uuid::Uuid::new_v4()));
        let export_root = root.join("exports");
        fs::create_dir_all(&root).unwrap();
        let live_id = Uuid::new_v4().to_string();
        let orphan_id = Uuid::new_v4().to_string();
        let live = prepare_note_directory(&root, &live_id, "Live", "# live", None, &[], 1).unwrap();
        prepare_note_directory(&root, &orphan_id, "Orphan", "# orphan", None, &[], 1).unwrap();
        fs::create_dir(
            root.join("notes")
                .join(format!(".mnemora-note-{}", Uuid::new_v4())),
        )
        .unwrap();

        let exported = export_note_bundle(
            &root,
            Some(&live.relative_directory),
            "Live",
            &live.content,
            &export_root,
        )
        .unwrap();
        assert!(exported.join("note.md").is_file());

        let moved = collect_orphan_note_directories(
            &root,
            &HashSet::from([live_id.clone()]),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(moved, 2);
        assert!(root.join("notes").join(live_id).is_dir());
        assert!(!root.join("notes").join(orphan_id).exists());
        assert_eq!(root.join("notes/.trash").read_dir().unwrap().count(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failure_before_rename_leaves_no_visible_note_directory() {
        let root = std::env::temp_dir().join(format!("mnemora-note-fail-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let note_id = Uuid::new_v4().to_string();
        let result = prepare_note_directory(
            &root,
            &note_id,
            "Broken",
            "# body",
            None,
            &[NoteAttachmentSource {
                source_path: root.join("missing.png"),
                original_name: "missing.png".to_string(),
                mime_type: Some("image/png".to_string()),
            }],
            1,
        );
        assert!(result.is_err());
        assert!(!root.join("notes").join(note_id).exists());
        assert!(!root.join("notes").read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".mnemora-note-")));
        let _ = fs::remove_dir_all(root);
    }
}
