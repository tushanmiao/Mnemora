//! 自定义背景图片的导入与管理。
//!
//! 为什么要「复制进数据目录」而不是记住原路径：
//!   1. 原文件被移动或删除后背景不会失效；
//!   2. 数据目录迁移时背景跟着走（`storage` 模块按目录整体搬迁）；
//!   3. `assetProtocol.scope` 只需放行一个固定目录，不必开放整个磁盘。
//!
//! 不信任扩展名：只按文件头魔数判定格式。扩展名可以随手改，魔数不会。

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::State;

use crate::state::AppState;

/// 数据目录下存放背景图的子目录。与 `tauri.conf.json` 的 assetProtocol scope 对应。
const BACKGROUNDS_DIR: &str = "backgrounds";

/// 单张背景图的字节上限。
///
/// 4K JPEG 通常在 8 MB 上下，给到 16 MB 留足余量；再大基本是未压缩的原始导出，
/// 当壁纸没有收益，只会拖慢每次启动的解码。
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// 允许的图片格式。value 是落盘时用的扩展名。
const ALLOWED_FORMATS: &[(&str, ImageKind)] = &[
    ("png", ImageKind::Png),
    ("jpg", ImageKind::Jpeg),
    ("jpeg", ImageKind::Jpeg),
    ("webp", ImageKind::Webp),
    ("avif", ImageKind::Avif),
    ("gif", ImageKind::Gif),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageKind {
    Png,
    Jpeg,
    Webp,
    Avif,
    Gif,
}

impl ImageKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Gif => "gif",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundImage {
    /// 文件名（含扩展名）。前端据此拼 `asset://` URL。
    pub name: String,
    /// 绝对路径。前端用 `convertFileSrc` 转成 asset URL。
    pub path: String,
    pub byte_size: u64,
}

/// 按文件头魔数判定图片格式；认不出就返回 None。
fn sniff_image_kind(bytes: &[u8]) -> Option<ImageKind> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(ImageKind::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(ImageKind::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageKind::Gif);
    }
    // RIFF 容器：前 4 字节 RIFF、第 8..12 字节 WEBP
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(ImageKind::Webp);
    }
    // ISO-BMFF：ftyp box 的 brand 决定是不是 AVIF
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"avif" || brand == b"avis" {
            return Some(ImageKind::Avif);
        }
    }
    None
}

fn backgrounds_dir(state: &AppState) -> Result<PathBuf, String> {
    let root = state.storage.current_data_dir().join(BACKGROUNDS_DIR);
    fs::create_dir_all(&root).map_err(|error| format!("创建背景图目录失败：{error}"))?;
    Ok(root)
}

/// 把用户选中的图片复制进数据目录，返回可用于 asset URL 的绝对路径。
#[tauri::command]
pub async fn import_background_image(
    state: State<'_, AppState>,
    source_path: String,
) -> Result<BackgroundImage, String> {
    let root = backgrounds_dir(&state)?;
    tauri::async_runtime::spawn_blocking(move || import_into(&root, Path::new(&source_path)))
        .await
        .map_err(|error| format!("背景图导入任务失败：{error}"))?
}

fn import_into(root: &Path, source: &Path) -> Result<BackgroundImage, String> {
    let metadata = fs::metadata(source).map_err(|error| format!("读取图片失败：{error}"))?;
    if !metadata.is_file() {
        return Err("所选路径不是文件。".to_string());
    }
    if metadata.len() == 0 {
        return Err("所选图片是空文件。".to_string());
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "背景图不能超过 {} MB。",
            MAX_IMAGE_BYTES / 1024 / 1024
        ));
    }

    // 扩展名先做一次快筛，真正的判定靠魔数——扩展名可以随手改。
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_FORMATS.iter().any(|(name, _)| *name == extension) {
        return Err("只支持 PNG、JPEG、WebP、AVIF 或 GIF 图片。".to_string());
    }

    let bytes = fs::read(source).map_err(|error| format!("读取图片失败：{error}"))?;
    let kind = sniff_image_kind(&bytes)
        .ok_or_else(|| "文件内容不是有效的图片（扩展名与实际格式不符）。".to_string())?;

    // 文件名用内容哈希：同一张图重复导入不会堆副本，也不会有路径注入。
    let digest = Sha256::digest(&bytes);
    let name = format!("{:x}.{}", digest, kind.extension());
    let target = root.join(&name);
    if !target.exists() {
        fs::write(&target, &bytes).map_err(|error| format!("保存背景图失败：{error}"))?;
    }

    Ok(BackgroundImage {
        name,
        path: target.to_string_lossy().into_owned(),
        byte_size: metadata.len(),
    })
}

/// 列出已导入的背景图，最近导入的排在前面。
#[tauri::command]
pub async fn list_background_images(
    state: State<'_, AppState>,
) -> Result<Vec<BackgroundImage>, String> {
    let root = backgrounds_dir(&state)?;
    tauri::async_runtime::spawn_blocking(move || list_in(&root))
        .await
        .map_err(|error| format!("背景图列举任务失败：{error}"))?
}

fn list_in(root: &Path) -> Result<Vec<BackgroundImage>, String> {
    // 先带上修改时间做排序键，排完再丢掉——BackgroundImage 本身不需要暴露它。
    let mut dated: Vec<(Option<std::time::SystemTime>, BackgroundImage)> = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        // 目录还没建起来等于没有图片，不该报错。
        Err(_) => return Ok(Vec::new()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        if !ALLOWED_FORMATS.iter().any(|(name, _)| *name == extension) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(value) if value.is_file() => value,
            _ => continue,
        };
        dated.push((
            metadata.modified().ok(),
            BackgroundImage {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: path.to_string_lossy().into_owned(),
                byte_size: metadata.len(),
            },
        ));
    }
    dated.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(dated.into_iter().map(|(_, image)| image).collect())
}

/// 删除一张已导入的背景图。
///
/// 只接受文件名，不接受路径：调用方传 `../../` 也只会得到「不存在」。
#[tauri::command]
pub async fn remove_background_image(
    state: State<'_, AppState>,
    name: String,
) -> Result<bool, String> {
    let root = backgrounds_dir(&state)?;
    tauri::async_runtime::spawn_blocking(move || remove_in(&root, &name))
        .await
        .map_err(|error| format!("背景图删除任务失败：{error}"))?
}

fn remove_in(root: &Path, name: &str) -> Result<bool, String> {
    // 名字里出现分隔符或 `..` 一律拒绝，别让它逃出背景目录。
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || Path::new(name).components().count() != 1
    {
        return Err("背景图名称无效。".to_string());
    }
    let target = root.join(name);
    if !target.is_file() {
        return Ok(false);
    }
    fs::remove_file(&target).map_err(|error| format!("删除背景图失败：{error}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mnemora-bg-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("创建测试目录");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn png_bytes() -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(b"payload");
        bytes
    }

    #[test]
    fn sniffs_every_supported_format() {
        assert_eq!(sniff_image_kind(&png_bytes()), Some(ImageKind::Png));
        assert_eq!(
            sniff_image_kind(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(ImageKind::Jpeg)
        );
        assert_eq!(sniff_image_kind(b"GIF89a...."), Some(ImageKind::Gif));

        let mut webp = b"RIFF".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBP");
        assert_eq!(sniff_image_kind(&webp), Some(ImageKind::Webp));

        let mut avif = vec![0, 0, 0, 0];
        avif.extend_from_slice(b"ftyp");
        avif.extend_from_slice(b"avif");
        assert_eq!(sniff_image_kind(&avif), Some(ImageKind::Avif));

        assert_eq!(sniff_image_kind(b"not an image"), None);
    }

    /// 扩展名对但内容不是图片，必须拒绝——否则 `asset://` 会加载一个畸形文件。
    #[test]
    fn rejects_content_that_is_not_an_image() {
        let dir = TestDirectory::new("mismatch");
        let source = dir.0.join("fake.png");
        fs::write(&source, b"<html>not a png</html>").expect("写入伪造文件");
        let root = dir.0.join("backgrounds");
        fs::create_dir_all(&root).expect("创建目标目录");

        let error = import_into(&root, &source).expect_err("必须拒绝");
        assert!(error.contains("不是有效的图片"), "{error}");
    }

    #[test]
    fn rejects_unsupported_extensions_and_empty_files() {
        let dir = TestDirectory::new("bounds");
        let root = dir.0.join("backgrounds");
        fs::create_dir_all(&root).expect("创建目标目录");

        let bmp = dir.0.join("wall.bmp");
        fs::write(&bmp, png_bytes()).expect("写入 bmp");
        let error = import_into(&root, &bmp).expect_err("扩展名不支持");
        assert!(error.contains("只支持"), "{error}");

        let empty = dir.0.join("empty.png");
        fs::write(&empty, b"").expect("写入空文件");
        let error = import_into(&root, &empty).expect_err("空文件");
        assert!(error.contains("空文件"), "{error}");
    }

    /// 同一张图导入两次应落到同一个文件名，不堆副本。
    #[test]
    fn imports_are_content_addressed() {
        let dir = TestDirectory::new("dedupe");
        let root = dir.0.join("backgrounds");
        fs::create_dir_all(&root).expect("创建目标目录");
        let first = dir.0.join("a.png");
        let second = dir.0.join("b.png");
        fs::write(&first, png_bytes()).expect("写入 a");
        fs::write(&second, png_bytes()).expect("写入 b");

        let one = import_into(&root, &first).expect("导入 a");
        let two = import_into(&root, &second).expect("导入 b");
        assert_eq!(one.name, two.name);
        assert_eq!(list_in(&root).expect("列举").len(), 1);
    }

    #[test]
    fn listing_skips_non_image_files_and_missing_directory() {
        let dir = TestDirectory::new("list");
        let root = dir.0.join("backgrounds");
        assert!(list_in(&root).expect("目录不存在时应返回空").is_empty());

        fs::create_dir_all(&root).expect("创建目标目录");
        fs::write(root.join("wall.png"), png_bytes()).expect("写入图片");
        fs::write(root.join("notes.txt"), b"hello").expect("写入干扰文件");
        let listed = list_in(&root).expect("列举");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "wall.png");
    }

    /// 删除只接受单段文件名；带路径分隔或 `..` 的一律拒绝。
    #[test]
    fn removal_refuses_to_escape_the_directory() {
        let dir = TestDirectory::new("escape");
        let root = dir.0.join("backgrounds");
        fs::create_dir_all(&root).expect("创建目标目录");
        let outside = dir.0.join("secret.png");
        fs::write(&outside, png_bytes()).expect("写入外部文件");

        for name in ["../secret.png", "..\\secret.png", "sub/wall.png", ""] {
            assert!(remove_in(&root, name).is_err(), "应拒绝 {name}");
        }
        assert!(outside.is_file(), "外部文件必须还在");

        fs::write(root.join("wall.png"), png_bytes()).expect("写入图片");
        assert!(remove_in(&root, "wall.png").expect("删除"));
        assert!(!remove_in(&root, "wall.png").expect("再删一次"));
    }
}
