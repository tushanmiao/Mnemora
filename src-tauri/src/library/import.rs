//! PDF 导入、签名验证、流式 SHA-256 和应用内文件快照。

use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{store::library_now_millis, types::LibraryItem, LibraryRepository};

const MAX_PDF_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const PDF_SIGNATURE_SCAN_BYTES: usize = 1024;
const COPY_BUFFER_BYTES: usize = 1024 * 1024;

pub(crate) enum ImportOutcome {
    Imported(LibraryItem),
    Duplicate(LibraryItem),
}

pub(crate) fn import_pdf(
    repository: &LibraryRepository,
    source_path: &str,
    collection_id: Option<&str>,
) -> Result<ImportOutcome, String> {
    let source = validate_source_path(source_path)?;
    let metadata = fs::metadata(&source).map_err(|error| format!("读取 PDF 信息失败：{error}"))?;
    if !metadata.is_file() {
        return Err("选择的路径不是普通文件。".to_string());
    }
    if metadata.len() == 0 {
        return Err("PDF 文件为空。".to_string());
    }
    if metadata.len() > MAX_PDF_BYTES {
        return Err("单个 PDF 不能超过 2 GB。".to_string());
    }
    validate_pdf_signature(&source)?;
    let file_hash = hash_file(&source)?;
    let mut connection = repository.open_connection()?;
    if let Some(item) = repository.find_by_hash_with_connection(&connection, &file_hash)? {
        repository.attach_collection_if_needed(&connection, &item.id, collection_id)?;
        let item = repository
            .find_by_hash_with_connection(&connection, &file_hash)?
            .ok_or_else(|| "重复文献记录读取失败。".to_string())?;
        return Ok(ImportOutcome::Duplicate(item));
    }

    let original_name = source
        .file_name()
        .ok_or_else(|| "PDF 文件名无效。".to_string())?
        .to_string_lossy()
        .into_owned();
    let title = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().replace(['_', '-'], " "))
        .unwrap_or_else(|| "未命名文献".to_string())
        .trim()
        .to_string();
    let item_id = Uuid::new_v4().to_string();
    let file_id = Uuid::new_v4().to_string();
    let stored_name = format!("{file_id}.pdf");
    let destination = repository.resolve_stored_file_name(&stored_name)?;
    copy_file_atomic(&source, &destination)?;
    let now = library_now_millis();
    let result = repository.insert_imported_item(
        &mut connection,
        &item_id,
        &file_id,
        if title.is_empty() {
            "未命名文献"
        } else {
            &title
        },
        &original_name,
        &stored_name,
        &source.to_string_lossy(),
        metadata.len(),
        &file_hash,
        collection_id,
        now,
    );
    match result {
        Ok(item) => Ok(ImportOutcome::Imported(item)),
        Err(error) => {
            let _ = fs::remove_file(destination);
            Err(error)
        }
    }
}

fn validate_source_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 32_768 {
        return Err("PDF 路径无效。".to_string());
    }
    let path = PathBuf::from(value);
    let extension_is_pdf = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if !extension_is_pdf {
        return Err("只能导入 PDF 文件。".to_string());
    }
    path.canonicalize()
        .map_err(|error| format!("无法访问 PDF 文件：{error}"))
}

fn validate_pdf_signature(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("打开 PDF 失败：{error}"))?;
    let mut reader = BufReader::new(file);
    let mut header = vec![0u8; PDF_SIGNATURE_SCAN_BYTES];
    let read = reader
        .read(&mut header)
        .map_err(|error| format!("读取 PDF 文件头失败：{error}"))?;
    let is_pdf = header[..read].windows(5).any(|window| window == b"%PDF-");
    if !is_pdf {
        return Err("文件扩展名为 PDF，但内容不是有效的 PDF。".to_string());
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("打开 PDF 计算哈希失败：{error}"))?;
    let mut reader = BufReader::with_capacity(COPY_BUFFER_BYTES, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("读取 PDF 计算哈希失败：{error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_file_atomic(source: &Path, destination: &Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "PDF 快照目录无效。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建 PDF 快照目录失败：{error}"))?;
    let temporary =
        destination.with_extension(format!("pdf.tmp-{}-{}", std::process::id(), Uuid::new_v4()));
    let mut input = File::open(source).map_err(|error| format!("打开源 PDF 失败：{error}"))?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("创建 PDF 临时快照失败：{error}"))?;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let copy_result = (|| -> Result<(), String> {
        loop {
            let read = input
                .read(&mut buffer)
                .map_err(|error| format!("读取源 PDF 失败：{error}"))?;
            if read == 0 {
                break;
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| format!("写入 PDF 快照失败：{error}"))?;
        }
        output
            .sync_all()
            .map_err(|error| format!("同步 PDF 快照失败：{error}"))?;
        Ok(())
    })();
    drop(output);
    if let Err(error) = copy_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if destination.exists() {
        let _ = fs::remove_file(&temporary);
        return Err("PDF 快照文件名发生冲突。".to_string());
    }
    fs::rename(&temporary, destination).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("保存 PDF 快照失败：{error}")
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::validate_pdf_signature;

    fn test_file(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mnemora-pdf-signature-{label}-{}-{}.pdf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn validates_pdf_signature_with_optional_leading_bytes() {
        let valid = test_file("valid");
        let invalid = test_file("invalid");
        fs::write(&valid, b"leading bytes\n%PDF-1.7\nbody").unwrap();
        fs::write(&invalid, b"not a pdf").unwrap();
        assert!(validate_pdf_signature(&valid).is_ok());
        assert!(validate_pdf_signature(&invalid).is_err());
        let _ = fs::remove_file(valid);
        let _ = fs::remove_file(invalid);
    }
}
