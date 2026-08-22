use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;
use zip::ZipArchive;

use crate::{settings::app_types::AppSettings, state::AppState, window_lifecycle};

const BUILTIN_PET_ID: &str = "mimo";
const PET_MANIFEST_FILE: &str = "pet.json";
const PET_SPRITESHEET_FILE: &str = "spritesheet.webp";
const MAX_PET_MANIFEST_BYTES: u64 = 16 * 1024;
const MAX_PET_SPRITESHEET_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PET_ARCHIVE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_PET_ARCHIVE_FILES: usize = 16;
const MAX_PET_ARCHIVE_DEPTH: usize = 3;
const CODEX_ATLAS_WIDTH: u32 = 1536;
const CODEX_ATLAS_HEIGHT: u32 = 1872;
const CODEX_LEGACY_ATLAS_HEIGHT: u32 = 2288;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub kind: String,
    pub source: String,
    pub selected: bool,
    pub spritesheet_url: Option<String>,
    pub atlas_width: Option<u32>,
    pub atlas_height: Option<u32>,
    pub columns: Option<u8>,
    pub rows: Option<u8>,
    pub compatible: bool,
    pub compatibility_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexPetImportResult {
    pub found: usize,
    pub imported: usize,
    pub selected_pet_id: Option<String>,
    pub failures: Vec<String>,
    pub pets: Vec<PetDescriptor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PetManifest {
    id: String,
    display_name: String,
    #[serde(default)]
    description: String,
    spritesheet_path: String,
    #[serde(default)]
    kind: String,
}

fn save_pet_settings(
    state: &State<'_, AppState>,
    update: impl FnOnce(&mut AppSettings),
) -> Result<AppSettings, String> {
    let mut settings = state
        .app_settings
        .read()
        .map_err(|_| "App settings lock is unavailable".to_string())?
        .clone();
    update(&mut settings);
    settings = settings.normalize_and_validate()?;
    state.app_settings_repository.save(&settings)?;
    *state
        .app_settings
        .write()
        .map_err(|_| "App settings lock is unavailable".to_string())? = settings.clone();
    Ok(settings)
}

fn pet_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("pets"))
        .map_err(|error| format!("无法确定桌面宠物目录：{error}"))
}

fn ensure_pet_root(app: &AppHandle) -> Result<PathBuf, String> {
    let root = pet_root(app)?;
    fs::create_dir_all(&root).map_err(|error| format!("创建桌面宠物目录失败：{error}"))?;
    Ok(root)
}

fn codex_pet_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(value) = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(value).join("pets"));
    }
    if let Some(value) = std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(value).join(".codex").join("pets"));
    }
    if let Some(value) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(value).join(".codex").join("pets"));
    }
    roots.sort();
    roots.dedup();
    roots
}

fn discover_codex_pet_directories() -> Result<Vec<PathBuf>, String> {
    let mut directories = Vec::new();
    let roots = codex_pet_roots();
    for root in roots.iter().filter(|root| root.is_dir()) {
        for entry in
            fs::read_dir(root).map_err(|error| format!("读取 Codex 宠物目录失败：{error}"))?
        {
            let entry = entry.map_err(|error| format!("读取 Codex 宠物目录失败：{error}"))?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("读取 Codex 宠物类型失败：{error}"))?;
            if file_type.is_dir() && !file_type.is_symlink() {
                directories.push(entry.path());
            }
        }
    }
    directories.sort();
    directories.dedup();
    if directories.is_empty() && !roots.iter().any(|root| root.is_dir()) {
        return Err(
            "未找到 Codex 宠物目录。请先在 Codex 中安装宠物，或设置 CODEX_HOME。".to_string(),
        );
    }
    Ok(directories)
}

fn regular_file_size(path: &Path, label: &str) -> Result<u64, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("读取{label}失败：{error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{label}必须是普通文件。"));
    }
    Ok(metadata.len())
}

fn valid_pet_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn read_manifest(directory: &Path) -> Result<PetManifest, String> {
    let manifest_path = directory.join(PET_MANIFEST_FILE);
    if regular_file_size(&manifest_path, "宠物清单")? > MAX_PET_MANIFEST_BYTES {
        return Err("宠物清单无效或过大。".to_string());
    }
    let value =
        fs::read_to_string(&manifest_path).map_err(|error| format!("读取宠物清单失败：{error}"))?;
    let manifest: PetManifest =
        serde_json::from_str(&value).map_err(|error| format!("解析宠物清单失败：{error}"))?;
    if !valid_pet_id(&manifest.id) || manifest.display_name.trim().is_empty() {
        return Err("宠物清单中的名称或 ID 无效。".to_string());
    }
    if manifest.spritesheet_path != PET_SPRITESHEET_FILE {
        return Err("宠物清单必须使用 spritesheet.webp。".to_string());
    }
    Ok(manifest)
}

fn inspect_webp_dimensions(path: &Path) -> Result<(u32, u32), String> {
    let file_size = regular_file_size(path, "宠物 Sprite")?;
    if file_size == 0 || file_size > MAX_PET_SPRITESHEET_BYTES {
        return Err("宠物 Sprite 无效或超过 20 MiB。".to_string());
    }
    let bytes = fs::read(path).map_err(|error| format!("读取宠物 Sprite 失败：{error}"))?;
    if bytes.len() < 30 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err("宠物 Sprite 不是有效 WebP。".to_string());
    }
    match &bytes[12..16] {
        b"VP8X" if bytes.len() >= 30 => {
            Ok((read_u24(&bytes[24..27]) + 1, read_u24(&bytes[27..30]) + 1))
        }
        b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2f => {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            Ok(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9d, 0x01, 0x2a] => {
            let width = u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff;
            let height = u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff;
            Ok((u32::from(width), u32::from(height)))
        }
        _ => Err("暂不支持该 WebP Sprite 编码。".to_string()),
    }
}

fn read_u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

fn pet_compatibility(width: u32, height: u32) -> (bool, Option<String>) {
    if width == CODEX_ATLAS_WIDTH && height == CODEX_ATLAS_HEIGHT {
        return (true, None);
    }
    if width == CODEX_ATLAS_WIDTH && height == CODEX_LEGACY_ATLAS_HEIGHT {
        return (
            true,
            Some("检测到旧版 Codex 8×11 Sprite；Mnemora 会按兼容布局播放前 9 个状态。".to_string()),
        );
    }
    (
        false,
        Some(format!(
            "Sprite 尺寸为 {width}×{height}；需要 Codex 1536×1872（8×9）或兼容的 1536×2288。"
        )),
    )
}

fn local_pet_descriptor(
    app: &AppHandle,
    directory: &Path,
    selected_pet_id: &str,
) -> Result<PetDescriptor, String> {
    let manifest = read_manifest(directory)?;
    let spritesheet = directory.join(PET_SPRITESHEET_FILE);
    let (width, height) = inspect_webp_dimensions(&spritesheet)?;
    let (compatible, compatibility_message) = pet_compatibility(width, height);
    app.asset_protocol_scope()
        .allow_file(&spritesheet)
        .map_err(|error| format!("授权宠物 Sprite 失败：{error}"))?;
    Ok(PetDescriptor {
        selected: selected_pet_id == manifest.id,
        id: manifest.id,
        display_name: manifest.display_name.trim().chars().take(100).collect(),
        description: manifest.description.trim().chars().take(500).collect(),
        kind: if manifest.kind.trim().is_empty() {
            "custom".to_string()
        } else {
            manifest.kind.trim().chars().take(40).collect()
        },
        source: "local".to_string(),
        spritesheet_url: Some(spritesheet.to_string_lossy().into_owned()),
        atlas_width: Some(width),
        atlas_height: Some(height),
        columns: Some(8),
        rows: Some(if height == CODEX_LEGACY_ATLAS_HEIGHT {
            11
        } else {
            9
        }),
        compatible,
        compatibility_message,
    })
}

fn list_local_pets(app: &AppHandle, selected_pet_id: &str) -> Result<Vec<PetDescriptor>, String> {
    let root = ensure_pet_root(app)?;
    let mut pets = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| format!("读取桌面宠物目录失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取桌面宠物目录失败：{error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("读取宠物类型失败：{error}"))?
            .is_dir()
        {
            continue;
        }
        match local_pet_descriptor(app, &entry.path(), selected_pet_id) {
            Ok(pet) => pets.push(pet),
            Err(error) => eprintln!("Skipping invalid pet {}: {error}", entry.path().display()),
        }
    }
    pets.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    Ok(pets)
}

fn builtin_pet(selected_pet_id: &str) -> PetDescriptor {
    PetDescriptor {
        id: BUILTIN_PET_ID.to_string(),
        display_name: "Mimo · Memory Seed".to_string(),
        description: "Mnemora 内置的低干扰记忆种子伙伴。".to_string(),
        kind: "builtin".to_string(),
        source: "builtin".to_string(),
        selected: selected_pet_id == BUILTIN_PET_ID,
        spritesheet_url: None,
        atlas_width: None,
        atlas_height: None,
        columns: None,
        rows: None,
        compatible: true,
        compatibility_message: None,
    }
}

fn resolve_import_directory(path: &Path) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("读取宠物包失败：{error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("不接受符号链接宠物包。".to_string());
    }
    if metadata.is_dir() {
        return Ok(path.to_path_buf());
    }
    if metadata.is_file()
        && path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(PET_MANIFEST_FILE))
    {
        return path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "宠物清单没有父目录。".to_string());
    }
    Err("请选择包含 pet.json 与 spritesheet.webp 的目录或 pet.json 文件。".to_string())
}

fn extract_pet_archive(source: &Path, destination_root: &Path) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(source).map_err(|error| format!("读取宠物 ZIP 失败：{error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_PET_ARCHIVE_BYTES
    {
        return Err("宠物 ZIP 必须是 1 字节到 25 MiB 的普通文件。".to_string());
    }
    let file = File::open(source).map_err(|error| format!("打开宠物 ZIP 失败：{error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("宠物 ZIP 格式无效：{error}"))?;
    if archive.len() > MAX_PET_ARCHIVE_FILES {
        return Err(format!(
            "宠物 ZIP 最多包含 {MAX_PET_ARCHIVE_FILES} 个条目。"
        ));
    }
    let extracted = destination_root.join(format!(".archive-{}", Uuid::new_v4()));
    fs::create_dir(&extracted).map_err(|error| format!("创建宠物解压目录失败：{error}"))?;
    let result = (|| {
        let mut extracted_files = 0usize;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|error| format!("读取宠物 ZIP 条目失败：{error}"))?;
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("宠物 ZIP 不能包含符号链接。".to_string());
            }
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| "宠物 ZIP 包含越界路径。".to_string())?
                .to_path_buf();
            if relative.components().count() > MAX_PET_ARCHIVE_DEPTH {
                return Err("宠物 ZIP 目录层级超过安全上限。".to_string());
            }
            let target = extracted.join(&relative);
            if !target.starts_with(&extracted) {
                return Err("宠物 ZIP 解压路径越界。".to_string());
            }
            if entry.is_dir() {
                fs::create_dir_all(&target)
                    .map_err(|error| format!("创建宠物 ZIP 目录失败：{error}"))?;
                continue;
            }
            let file_name = relative
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if !matches!(file_name, PET_MANIFEST_FILE | PET_SPRITESHEET_FILE) {
                return Err(format!("宠物 ZIP 包含不允许的文件：{file_name}"));
            }
            extracted_files = extracted_files.saturating_add(1);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("创建宠物 ZIP 子目录失败：{error}"))?;
            }
            let max = if file_name == PET_MANIFEST_FILE {
                MAX_PET_MANIFEST_BYTES
            } else {
                MAX_PET_SPRITESHEET_BYTES
            };
            if entry.size() == 0 || entry.size() > max {
                return Err(format!("宠物 ZIP 中的 {file_name} 无效或过大。"));
            }
            let mut output =
                File::create(&target).map_err(|error| format!("创建宠物 ZIP 文件失败：{error}"))?;
            let copied = io::copy(&mut entry.by_ref().take(max + 1), &mut output)
                .map_err(|error| format!("解压宠物 ZIP 失败：{error}"))?;
            if copied > max {
                return Err(format!("宠物 ZIP 中的 {file_name} 超过大小限制。"));
            }
            output
                .flush()
                .map_err(|error| format!("写入宠物 ZIP 文件失败：{error}"))?;
        }
        if extracted_files != 2 {
            return Err(
                "宠物 ZIP 必须且只能包含 pet.json 与 spritesheet.webp 两个文件。".to_string(),
            );
        }
        let mut roots = Vec::new();
        collect_pet_package_roots(&extracted, 0, &mut roots)?;
        if roots.len() != 1 {
            return Err(format!(
                "宠物 ZIP 必须且只能包含一个 pet.json，当前找到 {} 个。",
                roots.len()
            ));
        }
        let root = roots.remove(0);
        if !root.join(PET_SPRITESHEET_FILE).is_file() {
            return Err("宠物 ZIP 缺少 spritesheet.webp。".to_string());
        }
        Ok(root)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&extracted);
    }
    result
}

fn collect_pet_package_roots(
    directory: &Path,
    depth: usize,
    roots: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if depth > MAX_PET_ARCHIVE_DEPTH {
        return Err("宠物 ZIP 目录层级超过安全上限。".to_string());
    }
    if directory.join(PET_MANIFEST_FILE).is_file() {
        roots.push(directory.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(directory).map_err(|error| format!("扫描宠物 ZIP 失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("扫描宠物 ZIP 失败：{error}"))?;
        if entry
            .file_type()
            .map_err(|error| format!("读取宠物 ZIP 类型失败：{error}"))?
            .is_dir()
        {
            collect_pet_package_roots(&entry.path(), depth + 1, roots)?;
        }
    }
    Ok(())
}

fn copy_pet_package(source: &Path, destination_root: &Path) -> Result<PetManifest, String> {
    let manifest = read_manifest(source)?;
    let source_sprite = source.join(PET_SPRITESHEET_FILE);
    let (width, height) = inspect_webp_dimensions(&source_sprite)?;
    if !pet_compatibility(width, height).0 {
        return Err(format!("宠物 Sprite 尺寸不兼容：{width}×{height}。"));
    }
    let staging = destination_root.join(format!(".install-{}", Uuid::new_v4()));
    fs::create_dir_all(&staging).map_err(|error| format!("准备宠物导入目录失败：{error}"))?;
    let target = destination_root.join(&manifest.id);
    let backup = destination_root.join(format!(".backup-{}", Uuid::new_v4()));
    let result = (|| {
        fs::copy(
            source.join(PET_MANIFEST_FILE),
            staging.join(PET_MANIFEST_FILE),
        )
        .map_err(|error| format!("复制宠物清单失败：{error}"))?;
        fs::copy(&source_sprite, staging.join(PET_SPRITESHEET_FILE))
            .map_err(|error| format!("复制宠物 Sprite 失败：{error}"))?;
        if target.exists() {
            fs::rename(&target, &backup).map_err(|error| format!("备份旧宠物失败：{error}"))?;
        }
        match fs::rename(&staging, &target) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&backup);
                Ok(())
            }
            Err(error) => {
                if backup.exists() {
                    let _ = fs::rename(&backup, &target);
                }
                Err(format!("安装宠物失败：{error}"))
            }
        }
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(manifest)
}

#[tauri::command]
pub async fn pet_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<PetDescriptor>, String> {
    let selected = state
        .app_settings
        .read()
        .map_err(|_| "App settings lock is unavailable".to_string())?
        .pet
        .selected_pet_id
        .clone();
    let mut pets = vec![builtin_pet(&selected)];
    pets.extend(list_local_pets(&app, &selected)?);
    Ok(pets)
}

#[tauri::command]
pub async fn pet_open_directory(app: AppHandle) -> Result<(), String> {
    let root = ensure_pet_root(&app)?;
    app.opener()
        .open_path(root.to_string_lossy().into_owned(), None::<String>)
        .map_err(|error| format!("打开宠物目录失败：{error}"))
}

#[tauri::command]
pub async fn pet_import(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<PetDescriptor>, String> {
    let source = resolve_import_directory(Path::new(path.trim()))?;
    let root = ensure_pet_root(&app)?;
    let manifest = copy_pet_package(&source, &root)?;
    let settings = save_pet_settings(&state, |settings| {
        settings.pet.selected_pet_id = manifest.id.clone()
    })?;
    let _ = app.emit_to("main", "mnemora://app-settings-updated", &settings);
    if settings.pet.enabled {
        window_lifecycle::update_pet_window_runtime(&app, &settings.pet)?;
    }
    let mut pets = vec![builtin_pet(&settings.pet.selected_pet_id)];
    pets.extend(list_local_pets(&app, &settings.pet.selected_pet_id)?);
    Ok(pets)
}

#[tauri::command]
pub async fn pet_import_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<PetDescriptor>, String> {
    let root = ensure_pet_root(&app)?;
    let extracted_root = extract_pet_archive(Path::new(path.trim()), &root)?;
    let extracted_container = extracted_root
        .ancestors()
        .find(|value| {
            value.parent() == Some(root.as_path())
                && value
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".archive-"))
        })
        .map(Path::to_path_buf);
    let result = (|| {
        let manifest = copy_pet_package(&extracted_root, &root)?;
        let settings = save_pet_settings(&state, |settings| {
            settings.pet.selected_pet_id = manifest.id.clone()
        })?;
        let _ = app.emit_to("main", "mnemora://app-settings-updated", &settings);
        if settings.pet.enabled {
            window_lifecycle::update_pet_window_runtime(&app, &settings.pet)?;
        }
        let mut pets = vec![builtin_pet(&settings.pet.selected_pet_id)];
        pets.extend(list_local_pets(&app, &settings.pet.selected_pet_id)?);
        Ok(pets)
    })();
    if let Some(container) = extracted_container {
        let _ = fs::remove_dir_all(container);
    }
    result
}

#[tauri::command]
pub async fn pet_import_codex(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CodexPetImportResult, String> {
    let sources = discover_codex_pet_directories()?;
    let root = ensure_pet_root(&app)?;
    let mut imported_ids = Vec::new();
    let mut failures = Vec::new();

    for source in &sources {
        match copy_pet_package(source, &root) {
            Ok(manifest) => imported_ids.push(manifest.id),
            Err(error) => {
                let name = source
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("未知宠物");
                failures.push(format!("{name}：{error}"));
            }
        }
    }

    let selected_pet_id = imported_ids.first().cloned();
    let settings = if let Some(selected_pet_id) = selected_pet_id.as_ref() {
        let settings = save_pet_settings(&state, |settings| {
            settings.pet.selected_pet_id = selected_pet_id.clone()
        })?;
        let _ = app.emit_to("main", "mnemora://app-settings-updated", &settings);
        if settings.pet.enabled {
            window_lifecycle::update_pet_window_runtime(&app, &settings.pet)?;
        }
        settings
    } else {
        state
            .app_settings
            .read()
            .map_err(|_| "App settings lock is unavailable".to_string())?
            .clone()
    };

    let mut pets = vec![builtin_pet(&settings.pet.selected_pet_id)];
    pets.extend(list_local_pets(&app, &settings.pet.selected_pet_id)?);
    Ok(CodexPetImportResult {
        found: sources.len(),
        imported: imported_ids.len(),
        selected_pet_id,
        failures,
        pets,
    })
}

#[tauri::command]
pub async fn pet_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    pet_id: String,
) -> Result<Vec<PetDescriptor>, String> {
    let pet_id = pet_id.trim();
    if pet_id == BUILTIN_PET_ID || !valid_pet_id(pet_id) {
        return Err("不能删除内置宠物或无效宠物。".to_string());
    }
    let root = ensure_pet_root(&app)?;
    let target = root.join(pet_id);
    if target.parent() != Some(root.as_path()) {
        return Err("宠物删除路径无效。".to_string());
    }
    if target.exists() {
        if fs::symlink_metadata(&target)
            .map_err(|error| format!("读取待删除宠物失败：{error}"))?
            .file_type()
            .is_symlink()
        {
            return Err("不接受符号链接宠物目录。".to_string());
        }
        fs::remove_dir_all(&target).map_err(|error| format!("删除宠物失败：{error}"))?;
    }
    let settings = save_pet_settings(&state, |settings| {
        if settings.pet.selected_pet_id == pet_id {
            settings.pet.selected_pet_id = BUILTIN_PET_ID.to_string();
        }
    })?;
    let _ = app.emit_to("main", "mnemora://app-settings-updated", &settings);
    if settings.pet.enabled {
        window_lifecycle::update_pet_window_runtime(&app, &settings.pet)?;
    }
    let mut pets = vec![builtin_pet(&settings.pet.selected_pet_id)];
    pets.extend(list_local_pets(&app, &settings.pet.selected_pet_id)?);
    Ok(pets)
}

#[tauri::command]
pub async fn pet_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, String> {
    let settings = save_pet_settings(&state, |settings| settings.pet.enabled = enabled)?;
    let _ = app.emit_to("main", "mnemora://app-settings-updated", &settings);
    if enabled {
        if app
            .get_webview_window(window_lifecycle::PET_WINDOW_LABEL)
            .is_some()
        {
            window_lifecycle::update_pet_window_runtime(&app, &settings.pet)?;
        } else {
            window_lifecycle::sync_pet_window(&app, &settings.pet)?;
        }
    } else {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            let _ = window_lifecycle::destroy_pet_window(&app);
        });
    }
    Ok(settings)
}

#[tauri::command]
pub async fn pet_update_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    save_pet_settings(&state, |settings| {
        settings.pet.position_x = Some(x);
        settings.pet.position_y = Some(y);
    })?;
    Ok(())
}

#[tauri::command]
pub async fn pet_open_main(app: AppHandle) -> Result<(), String> {
    window_lifecycle::open_main_window(&app)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
    };

    use zip::{write::SimpleFileOptions, ZipWriter};

    use super::{
        codex_pet_roots, extract_pet_archive, inspect_webp_dimensions, pet_compatibility,
        valid_pet_id,
    };

    #[test]
    fn validates_safe_pet_ids() {
        assert!(valid_pet_id("trump-groove"));
        assert!(!valid_pet_id("../trump"));
        assert!(!valid_pet_id("Trump"));
    }

    #[test]
    fn accepts_current_and_legacy_codex_atlases() {
        assert!(pet_compatibility(1536, 1872).0);
        assert!(pet_compatibility(1536, 2288).0);
        assert!(!pet_compatibility(1024, 1024).0);
    }

    #[test]
    fn rejects_missing_webp() {
        assert!(inspect_webp_dimensions(Path::new("missing.webp")).is_err());
    }

    #[test]
    fn codex_pet_roots_always_point_at_a_pets_directory() {
        assert!(codex_pet_roots()
            .iter()
            .all(|path| path.ends_with(PathBuf::from("pets"))));
    }

    #[test]
    fn rejects_pet_archives_with_extra_files() {
        let root = std::env::temp_dir().join(format!("mnemora-pet-zip-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let archive_path = root.join("pet.zip");
        let file = fs::File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        archive.start_file("pet.json", options).unwrap();
        archive
            .write_all(
                br#"{"id":"test","displayName":"Test","spritesheetPath":"spritesheet.webp"}"#,
            )
            .unwrap();
        archive.start_file("install.ps1", options).unwrap();
        archive.write_all(b"Write-Host unsafe").unwrap();
        archive.finish().unwrap();

        assert!(extract_pet_archive(&archive_path, &root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
