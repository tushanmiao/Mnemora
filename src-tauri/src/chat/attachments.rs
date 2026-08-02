//! 聊天附件的本地生命周期。
//!
//! 文件选择和剪贴板粘贴只产生临时来源；真正发送时才复制到当前会话的独立目录。
//! 会话 JSON 只记录相对文件名和元数据，模型请求按需读取图片并临时转为 Base64。

use std::{
    collections::HashSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use base64::{engine::general_purpose, Engine as _};
use image::{codecs::jpeg::JpegEncoder, ColorType, ImageReader};
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ai::types::ModelImage;

use super::{
    conversation_types::{StoredChatAttachment, StoredConversation},
    storage::ConversationRepository,
};

pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 8;
pub const MAX_VISUAL_IMAGES_PER_MESSAGE: usize = 4;
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_TOTAL_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_CONVERSATION_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;
pub const STAGED_ATTACHMENT_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const THUMBNAIL_MAX_DIMENSION: u32 = 640;
const THUMBNAIL_MIN_DIMENSION: u32 = 256;
const MAX_THUMBNAIL_BYTES: usize = 512 * 1024;
const THUMBNAIL_JPEG_QUALITY: u8 = 78;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChatAttachment {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

struct InspectedAttachment {
    kind: &'static str,
    name: String,
    mime_type: String,
    size_bytes: u64,
    path: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
}

struct Thumbnail {
    bytes: Vec<u8>,
}

pub fn inspect_attachment_paths(paths: Vec<String>) -> Result<Vec<PendingChatAttachment>, String> {
    let inspected = inspect_sources(paths)?;
    Ok(inspected
        .into_iter()
        .map(|attachment| PendingChatAttachment {
            id: Uuid::new_v4().to_string(),
            kind: attachment.kind.to_string(),
            name: attachment.name,
            mime_type: attachment.mime_type,
            size_bytes: attachment.size_bytes,
            path: attachment.path.to_string_lossy().into_owned(),
            width: attachment.width,
            height: attachment.height,
        })
        .collect())
}

pub fn save_pasted_attachment(
    name: &str,
    mime_type: &str,
    data_base64: &str,
) -> Result<PendingChatAttachment, String> {
    let payload = data_base64.trim();
    if payload.is_empty() {
        return Err("剪贴板附件为空".to_string());
    }
    let bytes = general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| format!("解析剪贴板附件失败：{error}"))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "剪贴板附件不能超过 {} MB",
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }

    let safe_name = sanitize_file_name(name);
    let directory = staged_attachment_directory();
    fs::create_dir_all(&directory).map_err(|error| format!("创建剪贴板附件目录失败：{error}"))?;
    let path = directory.join(format!("{}_{}", Uuid::new_v4(), safe_name));
    fs::write(&path, bytes).map_err(|error| format!("保存剪贴板附件失败：{error}"))?;

    let inspected = inspect_source(&path).inspect_err(|_| {
        let _ = fs::remove_file(&path);
    })?;
    if mime_type.trim().starts_with("image/") && inspected.kind != "image" {
        let _ = fs::remove_file(&path);
        return Err("剪贴板图片格式无效，仅支持 PNG、JPEG、WebP 和 GIF".to_string());
    }

    Ok(PendingChatAttachment {
        id: Uuid::new_v4().to_string(),
        kind: inspected.kind.to_string(),
        name: safe_name,
        mime_type: inspected.mime_type,
        size_bytes: inspected.size_bytes,
        path: path.to_string_lossy().into_owned(),
        width: inspected.width,
        height: inspected.height,
    })
}

pub fn discard_staged_attachment(path: &str) -> Result<bool, String> {
    let candidate = PathBuf::from(path);
    if candidate.parent() != Some(staged_attachment_directory().as_path()) {
        return Ok(false);
    }
    if !candidate.exists() {
        return Ok(false);
    }
    fs::remove_file(&candidate).map_err(|error| format!("清理剪贴板临时附件失败：{error}"))?;
    Ok(true)
}

pub fn import_attachments(
    repository: &ConversationRepository,
    conversation_id: &str,
    paths: Vec<String>,
    cancellation: Option<&CancellationToken>,
) -> Result<Vec<StoredChatAttachment>, String> {
    ensure_not_cancelled(cancellation)?;
    let inspected = inspect_sources(paths)?;
    if inspected.is_empty() {
        return Ok(Vec::new());
    }
    let directory = repository.attachments_directory(conversation_id)?;
    fs::create_dir_all(&directory).map_err(|error| format!("创建会话附件目录失败：{error}"))?;

    let mut stored = Vec::with_capacity(inspected.len());
    let mut copied_paths = Vec::with_capacity(inspected.len() * 2);
    let mut staged_sources = Vec::new();
    let mut directory_bytes = attachment_directory_size(&directory)?;
    for attachment in inspected {
        if let Err(error) = ensure_not_cancelled(cancellation) {
            remove_files_best_effort(&copied_paths);
            return Err(error);
        }
        let id = Uuid::new_v4().to_string();
        let stored_name = format!("{}_{}", id, sanitize_file_name(&attachment.name));
        let thumbnail = if attachment.kind == "image" {
            match create_thumbnail(&attachment.path, cancellation) {
                Ok(thumbnail) => Some(thumbnail),
                Err(error) => {
                    remove_files_best_effort(&copied_paths);
                    return Err(error);
                }
            }
        } else {
            None
        };
        let added_bytes = attachment
            .size_bytes
            .saturating_add(thumbnail.as_ref().map_or(0, |item| item.bytes.len() as u64));
        if directory_bytes.saturating_add(added_bytes) > MAX_CONVERSATION_ATTACHMENT_BYTES {
            remove_files_best_effort(&copied_paths);
            return Err(format!(
                "单个会话的附件总大小不能超过 {} MB",
                MAX_CONVERSATION_ATTACHMENT_BYTES / 1024 / 1024
            ));
        }
        let destination = directory.join(&stored_name);
        if let Err(error) = fs::copy(&attachment.path, &destination) {
            remove_files_best_effort(&copied_paths);
            return Err(format!("保存会话附件失败：{error}"));
        }
        ensure_not_cancelled(cancellation).inspect_err(|_| {
            let _ = fs::remove_file(&destination);
            remove_files_best_effort(&copied_paths);
        })?;
        copied_paths.push(destination);
        let preview_path = if let Some(thumbnail) = thumbnail {
            let preview_name = format!("preview_{id}.jpg");
            let preview_destination = directory.join(&preview_name);
            if let Err(error) = fs::write(&preview_destination, thumbnail.bytes) {
                remove_files_best_effort(&copied_paths);
                return Err(format!("保存附件缩略图失败：{error}"));
            }
            copied_paths.push(preview_destination);
            Some(preview_name)
        } else {
            None
        };
        if let Err(error) = ensure_not_cancelled(cancellation) {
            remove_files_best_effort(&copied_paths);
            return Err(error);
        }
        stored.push(StoredChatAttachment {
            id,
            kind: attachment.kind.to_string(),
            name: attachment.name,
            mime_type: attachment.mime_type,
            size_bytes: attachment.size_bytes,
            path: stored_name,
            preview_path,
            width: attachment.width,
            height: attachment.height,
        });
        if attachment.path.parent() == Some(staged_attachment_directory().as_path()) {
            staged_sources.push(attachment.path);
        }
        directory_bytes = directory_bytes.saturating_add(added_bytes);
    }
    for staged_source in staged_sources {
        let _ = fs::remove_file(staged_source);
    }
    Ok(stored)
}

pub fn read_attachment_preview(
    repository: &ConversationRepository,
    conversation_id: Option<&str>,
    path: &str,
    preview_path: Option<&str>,
    cancellation: Option<&CancellationToken>,
) -> Result<String, String> {
    ensure_not_cancelled(cancellation)?;
    if let (Some(conversation_id), Some(preview_path)) = (conversation_id, preview_path) {
        let full_path = repository.resolve_attachment_path(conversation_id, preview_path)?;
        let metadata =
            fs::metadata(&full_path).map_err(|error| format!("读取附件缩略图失败：{error}"))?;
        if !metadata.is_file() || metadata.len() > MAX_THUMBNAIL_BYTES as u64 {
            return Err("附件缩略图无效。".to_string());
        }
        let bytes = fs::read(&full_path).map_err(|error| format!("读取附件缩略图失败：{error}"))?;
        ensure_not_cancelled(cancellation)?;
        return Ok(format!(
            "data:image/jpeg;base64,{}",
            general_purpose::STANDARD.encode(bytes)
        ));
    }

    let full_path = match conversation_id {
        Some(conversation_id) => repository.resolve_attachment_path(conversation_id, path)?,
        None => PathBuf::from(path),
    };
    let inspected = inspect_source(&full_path)?;
    if inspected.kind != "image" {
        return Err("只有图片附件可以预览".to_string());
    }
    let thumbnail = create_thumbnail(&full_path, cancellation)?;
    ensure_not_cancelled(cancellation)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        general_purpose::STANDARD.encode(thumbnail.bytes)
    ))
}

pub fn read_attachment_image(
    repository: &ConversationRepository,
    conversation_id: &str,
    path: &str,
    cancellation: Option<&CancellationToken>,
) -> Result<String, String> {
    ensure_not_cancelled(cancellation)?;
    let full_path = repository.resolve_attachment_path(conversation_id, path)?;
    let inspected = inspect_source(&full_path)?;
    if inspected.kind != "image" {
        return Err("只有图片附件可以在应用内查看。".to_string());
    }
    let metadata =
        fs::metadata(&full_path).map_err(|error| format!("读取图片附件信息失败：{error}"))?;
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "图片附件超过 {} MB，无法在应用内查看。",
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }
    let bytes = fs::read(&full_path).map_err(|error| format!("读取图片附件失败：{error}"))?;
    ensure_not_cancelled(cancellation)?;
    Ok(format!(
        "data:{};base64,{}",
        inspected.mime_type,
        general_purpose::STANDARD.encode(bytes)
    ))
}

pub fn load_model_image(
    repository: &ConversationRepository,
    conversation_id: &str,
    attachment: &StoredChatAttachment,
) -> Result<ModelImage, String> {
    if attachment.kind != "image" {
        return Err("Only image attachments can be added to a model request".to_string());
    }
    let path = repository.resolve_attachment_path(conversation_id, &attachment.path)?;
    let inspected = inspect_source(&path)?;
    if inspected.kind != "image" || inspected.mime_type != attachment.mime_type {
        return Err(format!("图片附件格式与记录不一致：{}", attachment.name));
    }
    let bytes = fs::read(&path).map_err(|error| format!("读取图片附件失败：{error}"))?;
    Ok(ModelImage {
        name: attachment.name.clone(),
        media_type: attachment.mime_type.clone(),
        data_base64: general_purpose::STANDARD.encode(bytes),
    })
}

fn inspect_sources(paths: Vec<String>) -> Result<Vec<InspectedAttachment>, String> {
    if paths.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(format!(
            "每条消息最多添加 {MAX_ATTACHMENTS_PER_MESSAGE} 个附件"
        ));
    }
    let mut total = 0u64;
    let mut image_total = 0u64;
    let mut image_count = 0usize;
    let mut inspected = Vec::with_capacity(paths.len());
    for raw_path in paths {
        let attachment = inspect_source(Path::new(&raw_path))?;
        total = total.saturating_add(attachment.size_bytes);
        if total > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(format!(
                "附件总大小不能超过 {} MB",
                MAX_TOTAL_ATTACHMENT_BYTES / 1024 / 1024
            ));
        }
        if attachment.kind == "image" {
            image_count += 1;
            image_total = image_total.saturating_add(attachment.size_bytes);
            if image_count > MAX_VISUAL_IMAGES_PER_MESSAGE {
                return Err(format!(
                    "每条消息最多添加 {MAX_VISUAL_IMAGES_PER_MESSAGE} 张图片"
                ));
            }
            if image_total > MAX_TOTAL_IMAGE_BYTES {
                return Err(format!(
                    "图片总大小不能超过 {} MB",
                    MAX_TOTAL_IMAGE_BYTES / 1024 / 1024
                ));
            }
        }
        inspected.push(attachment);
    }
    Ok(inspected)
}

fn inspect_source(path: &Path) -> Result<InspectedAttachment, String> {
    let metadata = fs::metadata(path).map_err(|_| format!("附件不存在：{}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!("附件不是有效文件：{}", path.display()));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "单个附件不能超过 {} MB",
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| "附件文件名无效".to_string())?;
    let image_mime = detect_image_mime(path)?;
    if image_mime.is_some() && metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "单张图片不能超过 {} MB",
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }
    if image_extension(&name).is_some() && image_mime.is_none() {
        return Err(format!("图片格式无效或文件已损坏：{name}"));
    }
    let dimensions = if image_mime.is_some() {
        let (width, height) =
            image::image_dimensions(path).map_err(|error| format!("读取图片尺寸失败：{error}"))?;
        let pixels = u64::from(width).saturating_mul(u64::from(height));
        if pixels == 0 || pixels > MAX_IMAGE_PIXELS {
            return Err(format!(
                "图片像素不能超过 {} MP",
                MAX_IMAGE_PIXELS / 1_000_000
            ));
        }
        Some((width, height))
    } else {
        None
    };
    Ok(InspectedAttachment {
        kind: if image_mime.is_some() {
            "image"
        } else {
            "file"
        },
        mime_type: image_mime
            .map(str::to_string)
            .unwrap_or_else(|| mime_type_for_name(&name).to_string()),
        name,
        size_bytes: metadata.len(),
        path: path.to_path_buf(),
        width: dimensions.map(|(width, _)| width),
        height: dimensions.map(|(_, height)| height),
    })
}

fn detect_image_mime(path: &Path) -> Result<Option<&'static str>, String> {
    let mut file = File::open(path).map_err(|error| format!("读取附件失败：{error}"))?;
    let mut signature = [0u8; 16];
    let read = file
        .read(&mut signature)
        .map_err(|error| format!("读取附件失败：{error}"))?;
    let bytes = &signature[..read];
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(Some("image/png"));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok(Some("image/jpeg"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok(Some("image/gif"));
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(Some("image/webp"));
    }
    Ok(None)
}

fn image_extension(name: &str) -> Option<&'static str> {
    match extension(name).as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn mime_type_for_name(name: &str) -> &'static str {
    match extension(name).as_str() {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

fn sanitize_file_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches(['.', ' ', '_']).trim();
    if trimmed.is_empty() {
        "attachment".to_string()
    } else {
        trimmed.chars().take(180).collect()
    }
}

fn staged_attachment_directory() -> PathBuf {
    std::env::temp_dir().join("mnemora-chat-paste")
}

fn ensure_not_cancelled(cancellation: Option<&CancellationToken>) -> Result<(), String> {
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Err("附件任务已取消。".to_string());
    }
    Ok(())
}

fn create_thumbnail(
    path: &Path,
    cancellation: Option<&CancellationToken>,
) -> Result<Thumbnail, String> {
    ensure_not_cancelled(cancellation)?;
    let reader = ImageReader::open(path)
        .map_err(|error| format!("打开图片失败：{error}"))?
        .with_guessed_format()
        .map_err(|error| format!("识别图片格式失败：{error}"))?;
    let image = reader
        .decode()
        .map_err(|error| format!("解码图片失败：{error}"))?;
    ensure_not_cancelled(cancellation)?;

    let mut max_dimension = THUMBNAIL_MAX_DIMENSION;
    loop {
        let thumbnail = image.thumbnail(max_dimension, max_dimension);
        let rgb = thumbnail.to_rgb8();
        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(&mut bytes, THUMBNAIL_JPEG_QUALITY)
            .encode(&rgb, rgb.width(), rgb.height(), ColorType::Rgb8.into())
            .map_err(|error| format!("编码附件缩略图失败：{error}"))?;
        ensure_not_cancelled(cancellation)?;
        if bytes.len() <= MAX_THUMBNAIL_BYTES || max_dimension <= THUMBNAIL_MIN_DIMENSION {
            return Ok(Thumbnail { bytes });
        }
        max_dimension = (max_dimension * 3 / 4).max(THUMBNAIL_MIN_DIMENSION);
    }
}

fn attachment_directory_size(directory: &Path) -> Result<u64, String> {
    if !directory.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in
        fs::read_dir(directory).map_err(|error| format!("读取会话附件目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("检查会话附件失败：{error}"))?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取会话附件信息失败：{error}"))?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

fn remove_files_best_effort(paths: &[PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

pub fn cleanup_staged_attachments_older_than(max_age: Duration) -> Result<usize, String> {
    let directory = staged_attachment_directory();
    if !directory.exists() {
        return Ok(0);
    }
    let now = SystemTime::now();
    let mut removed = 0usize;
    for entry in
        fs::read_dir(&directory).map_err(|error| format!("读取剪贴板临时目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("检查剪贴板临时附件失败：{error}"))?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取剪贴板临时附件信息失败：{error}"))?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let age = now.duration_since(modified).unwrap_or_default();
        if age < max_age {
            continue;
        }
        fs::remove_file(entry.path())
            .map_err(|error| format!("清理过期剪贴板附件失败：{error}"))?;
        removed += 1;
    }
    Ok(removed)
}

pub fn discard_stored_attachments(
    repository: &ConversationRepository,
    conversation_id: &str,
    attachments: &[StoredChatAttachment],
) -> Result<usize, String> {
    let mut removed = 0usize;
    for attachment in attachments {
        let mut paths = vec![attachment.path.as_str()];
        if let Some(preview_path) = attachment.preview_path.as_deref() {
            paths.push(preview_path);
        }
        for stored_name in paths {
            let path = repository.resolve_attachment_path(conversation_id, stored_name)?;
            if path.is_file() {
                fs::remove_file(&path).map_err(|error| format!("清理未提交附件失败：{error}"))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

pub fn sweep_unreferenced_attachments(
    repository: &ConversationRepository,
    conversation: &StoredConversation,
) -> Result<usize, String> {
    let directory = repository.attachments_directory(&conversation.id)?;
    if !directory.exists() {
        return Ok(0);
    }
    let referenced = conversation
        .messages
        .iter()
        .flat_map(|message| message.attachments.iter())
        .flat_map(|attachment| {
            std::iter::once(attachment.path.as_str()).chain(attachment.preview_path.as_deref())
        })
        .collect::<HashSet<_>>();
    let mut removed = 0usize;
    for entry in
        fs::read_dir(&directory).map_err(|error| format!("读取会话附件目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("检查会话附件失败：{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取会话附件类型失败：{error}"))?;
        if !file_type.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if referenced.contains(file_name.as_ref()) || !is_managed_attachment_name(&file_name) {
            continue;
        }
        if let Err(error) = fs::remove_file(entry.path()) {
            eprintln!("Failed to remove orphan attachment {file_name}: {error}");
            continue;
        }
        removed += 1;
    }
    Ok(removed)
}

fn is_managed_attachment_name(file_name: &str) -> bool {
    if let Some(id) = file_name
        .strip_prefix("preview_")
        .and_then(|value| value.strip_suffix(".jpg"))
    {
        return Uuid::parse_str(id).is_ok();
    }
    file_name
        .split_once('_')
        .is_some_and(|(id, _)| Uuid::parse_str(id).is_ok())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use base64::{engine::general_purpose, Engine as _};
    use uuid::Uuid;

    use super::{
        import_attachments, inspect_attachment_paths, load_model_image, read_attachment_image,
        save_pasted_attachment,
    };
    use crate::chat::storage::ConversationRepository;

    fn write_test_png(path: &Path) {
        let image = image::RgbImage::from_pixel(8, 6, image::Rgb([32, 96, 160]));
        image.save(path).unwrap();
    }

    #[test]
    fn detects_png_from_file_signature() {
        let directory =
            std::env::temp_dir().join(format!("mnemora-attachment-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("capture.png");
        write_test_png(&path);

        let attachments =
            inspect_attachment_paths(vec![path.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(attachments[0].kind, "image");
        assert_eq!(attachments[0].mime_type, "image/png");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_invalid_image_extension() {
        let directory =
            std::env::temp_dir().join(format!("mnemora-attachment-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("fake.png");
        fs::write(&path, b"not an image").unwrap();

        assert!(inspect_attachment_paths(vec![path.to_string_lossy().into_owned()]).is_err());
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_invalid_pasted_image_payload() {
        let encoded = general_purpose::STANDARD.encode(b"not an image");
        assert!(save_pasted_attachment("capture.png", "image/png", &encoded).is_err());
    }

    #[test]
    fn imports_image_as_relative_safe_copy_for_model_requests() {
        let root = std::env::temp_dir().join(format!("mnemora-attachment-test-{}", Uuid::new_v4()));
        let source_directory = root.join("source");
        fs::create_dir_all(&source_directory).unwrap();
        let source = source_directory.join("capture.png");
        write_test_png(&source);
        let repository = ConversationRepository::new(root.clone());

        let stored = import_attachments(
            &repository,
            "conversation-1",
            vec![source.to_string_lossy().into_owned()],
            None,
        )
        .unwrap();
        assert_eq!(stored.len(), 1);
        assert!(!Path::new(&stored[0].path).is_absolute());
        assert_eq!(stored[0].width, Some(8));
        assert_eq!(stored[0].height, Some(6));
        assert!(repository
            .resolve_attachment_path("conversation-1", &stored[0].path)
            .unwrap()
            .is_file());
        assert!(repository
            .resolve_attachment_path("conversation-1", stored[0].preview_path.as_deref().unwrap(),)
            .unwrap()
            .is_file());

        let image = load_model_image(&repository, "conversation-1", &stored[0]).unwrap();
        assert_eq!(image.media_type, "image/png");
        assert!(!image.data_base64.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reads_original_image_only_from_the_conversation_attachment_directory() {
        let root = std::env::temp_dir().join(format!("mnemora-attachment-test-{}", Uuid::new_v4()));
        let source_directory = root.join("source");
        fs::create_dir_all(&source_directory).unwrap();
        let source = source_directory.join("capture.png");
        write_test_png(&source);
        let repository = ConversationRepository::new(root.clone());
        let stored = import_attachments(
            &repository,
            "conversation-1",
            vec![source.to_string_lossy().into_owned()],
            None,
        )
        .unwrap();

        let data_url =
            read_attachment_image(&repository, "conversation-1", &stored[0].path, None).unwrap();
        assert!(data_url.starts_with("data:image/png;base64,"));
        assert!(read_attachment_image(
            &repository,
            "conversation-1",
            "../source/capture.png",
            None
        )
        .is_err());
        let _ = fs::remove_dir_all(root);
    }
}
