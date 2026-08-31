//! 远端资源包的暂存与预览。
//!
//! 流程：zipball 字节 → 受限解压到暂存目录 → 剥掉 GitHub 的顶层包装目录
//! → 解析清单生成预览 → 用户确认后，把**暂存目录路径**交给既有安装器。
//!
//! 关键点：安装动作本身完全复用 skills / plugins / pet 现有安装器，
//! 因此哈希校验、路径穿越防护、容量上限、以及「插件不得贡献可执行 stdio MCP」
//! 这些既有边界一条都不会被绕开。本模块只负责把远端内容安全落到本地磁盘。

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use zip::ZipArchive;

use super::types::{RemotePackageKind, RemotePackagePreview};

/// 解压限额。与 skills 安装器同量级，略放宽以容纳仓库里的说明文件与图片。
const MAX_EXTRACTED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SINGLE_FILE_BYTES: u64 = 12 * 1024 * 1024;
const MAX_FILES: usize = 1024;
const MAX_DEPTH: usize = 10;
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
/// 暂存条目的存活时间；超时后由下一次调用顺带清理。
const STAGING_TTL_MS: u64 = 30 * 60 * 1000;

/// 仓库里常见但对安装无意义的目录，跳过以省下解压预算。
const IGNORED_DIRECTORIES: &[&str] = &[
    ".git",
    ".github",
    "node_modules",
    "__pycache__",
    ".venv",
    "target",
    "dist",
];

pub struct StagingEntry {
    pub kind: RemotePackageKind,
    pub path: PathBuf,
    pub staging_root: PathBuf,
    pub full_name: String,
    pub commit_sha: String,
    pub replaces_existing: bool,
    created_at_ms: u64,
}

/// 暂存区：token → 条目。前端只持有 token，拿不到也传不了真实路径。
#[derive(Default)]
pub struct StagingArea {
    entries: Mutex<HashMap<String, StagingEntry>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

impl StagingArea {
    pub fn insert(&self, token: String, entry: StagingEntry) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(token, entry);
        }
    }

    /// 取出并移除。安装是一次性动作，token 不可重放。
    pub fn take(&self, token: &str) -> Option<StagingEntry> {
        self.entries.lock().ok()?.remove(token)
    }

    /// 清理过期暂存目录，避免下载物长期堆在数据目录里。
    pub fn sweep(&self) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let now = now_ms();
        let expired = entries
            .iter()
            .filter(|(_, entry)| now.saturating_sub(entry.created_at_ms) > STAGING_TTL_MS)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        for token in expired {
            if let Some(entry) = entries.remove(&token) {
                let _ = fs::remove_dir_all(&entry.staging_root);
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifestPreview {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    publisher: String,
    #[serde(default)]
    capabilities: PluginCapabilitiesPreview,
    #[serde(default)]
    permissions: PluginPermissionsPreview,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginCapabilitiesPreview {
    #[serde(default)]
    skills: Vec<serde_json::Value>,
    #[serde(default)]
    mcp_servers: Vec<McpServerPreview>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerPreview {
    #[serde(default)]
    id: String,
    #[serde(default)]
    transport: serde_json::Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginPermissionsPreview {
    #[serde(default)]
    network_domains: Vec<String>,
    #[serde(default)]
    secrets: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PetManifestPreview {
    #[serde(default)]
    id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    kind: String,
}

#[derive(Debug)]
struct ExtractStats {
    file_count: usize,
    total_bytes: u64,
}

/// 受限解压 zipball。防护与 skills 安装器保持一致：
/// 拒绝符号链接、用 enclosed_name 挡 zip-slip、限制层级与总量。
fn extract_zipball(bytes: &[u8], destination: &Path) -> Result<ExtractStats, String> {
    fs::create_dir_all(destination).map_err(|error| format!("创建暂存目录失败：{error}"))?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("仓库快照不是有效 ZIP：{error}"))?;

    let mut stats = ExtractStats {
        file_count: 0,
        total_bytes: 0,
    };

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("读取快照条目失败：{error}"))?;

        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("仓库快照包含符号链接，已拒绝安装。".to_string());
        }

        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "仓库快照包含越界路径。".to_string())?
            .to_path_buf();

        if relative.components().count() > MAX_DEPTH + 1 {
            return Err("仓库快照目录层级超过安全上限。".to_string());
        }

        // 跳过与安装无关的目录，省下预算给真正的包内容。
        if relative.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|value| IGNORED_DIRECTORIES.contains(&value))
        }) {
            continue;
        }

        let target = destination.join(&relative);
        if !target.starts_with(destination) {
            return Err("仓库快照解压路径越界。".to_string());
        }

        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| format!("创建快照目录失败：{error}"))?;
            continue;
        }

        stats.file_count += 1;
        if stats.file_count > MAX_FILES {
            return Err(format!("仓库快照文件数超过 {MAX_FILES} 个上限。"));
        }
        stats.total_bytes = stats.total_bytes.saturating_add(entry.size());
        if stats.total_bytes > MAX_EXTRACTED_BYTES {
            return Err(format!(
                "仓库快照解压后超过 {} MB 上限。",
                MAX_EXTRACTED_BYTES / 1024 / 1024
            ));
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建快照文件目录失败：{error}"))?;
        }
        let mut output =
            File::create(&target).map_err(|error| format!("创建快照文件失败：{error}"))?;
        let copied = io::copy(&mut entry.take(MAX_SINGLE_FILE_BYTES + 1), &mut output)
            .map_err(|error| format!("解压快照文件失败：{error}"))?;
        if copied > MAX_SINGLE_FILE_BYTES {
            return Err(format!(
                "仓库快照中存在超过 {} MB 的单个文件。",
                MAX_SINGLE_FILE_BYTES / 1024 / 1024
            ));
        }
        output
            .flush()
            .map_err(|error| format!("写入快照文件失败：{error}"))?;
    }

    Ok(stats)
}

/// GitHub zipball 总是把内容包在 `owner-repo-<sha>/` 一层里。
/// 既有的插件与宠物安装器都从根目录直接读清单，所以这层必须剥掉。
fn unwrap_single_directory(root: &Path) -> Result<PathBuf, String> {
    let mut entries = fs::read_dir(root)
        .map_err(|error| format!("读取暂存目录失败：{error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("读取暂存目录失败：{error}"))?;
    entries.retain(|entry| entry.file_name() != ".DS_Store");
    if entries.len() != 1 {
        return Ok(root.to_path_buf());
    }
    let only = &entries[0];
    let metadata = only
        .metadata()
        .map_err(|error| format!("读取暂存条目失败：{error}"))?;
    if metadata.is_dir() {
        return Ok(only.path());
    }
    Ok(root.to_path_buf())
}

fn read_manifest_text(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("读取清单失败：{error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
        return Err("清单文件为空或过大。".to_string());
    }
    fs::read_to_string(path).map_err(|error| format!("读取清单失败：{error}"))
}

/// 从 SKILL.md 的 YAML frontmatter 里取有限几个字段。
///
/// 这里刻意只做「取值预览」，不做完整 YAML 解析——真正的校验由
/// skills 安装器负责，预览拿不到的字段留空即可，不能因为预览解析
/// 失败就阻断安装。
fn parse_skill_frontmatter(text: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return fields;
    };
    let Some(end) = rest.find("\n---") else {
        return fields;
    };
    for line in rest[..end].lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.starts_with('#') {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !value.is_empty() {
            fields.insert(key.to_ascii_lowercase(), value.to_string());
        }
    }
    fields
}

pub struct StagedPackage {
    pub entry: StagingEntry,
    pub preview: RemotePackagePreview,
}

fn validate_package_path(value: &str) -> Result<PathBuf, String> {
    let normalized = value.trim().trim_matches('/').replace('\\', "/");
    if normalized.is_empty() {
        return Ok(PathBuf::new());
    }
    let path = PathBuf::from(normalized);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("仓库内路径无效。".to_string());
    }
    Ok(path)
}

fn selected_path_root(repository_root: &Path, value: &str) -> Result<PathBuf, String> {
    let relative = validate_package_path(value)?;
    let selected = repository_root.join(&relative);
    if !selected.exists() || !selected.starts_with(repository_root) {
        return Err(format!("仓库中不存在路径：{}", relative.display()));
    }
    if selected.is_dir() {
        return Ok(selected);
    }
    let name = selected
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let parent = selected
        .parent()
        .ok_or_else(|| "仓库内文件没有有效父目录。".to_string())?;
    if name == "plugin.json"
        && parent.file_name().and_then(|value| value.to_str()) == Some(".codex-plugin")
    {
        return parent
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Codex 插件目录结构无效。".to_string());
    }
    Ok(parent.to_path_buf())
}

fn find_manifest_roots(
    directory: &Path,
    kind: RemotePackageKind,
    depth: usize,
) -> Result<Vec<PathBuf>, String> {
    if depth > MAX_DEPTH {
        return Err("仓库目录层级超过安全上限。".to_string());
    }
    let is_root = match kind {
        RemotePackageKind::Skill => directory.join("SKILL.md").is_file(),
        RemotePackageKind::Plugin => {
            directory.join("plugin.json").is_file()
                || directory
                    .join(".codex-plugin")
                    .join("plugin.json")
                    .is_file()
        }
        RemotePackageKind::Pet => directory.join("pet.json").is_file(),
    };
    if is_root {
        return Ok(vec![directory.to_path_buf()]);
    }
    let mut roots = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| format!("扫描仓库清单失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("扫描仓库清单失败：{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取仓库条目类型失败：{error}"))?;
        if file_type.is_symlink() {
            return Err("仓库内容包含符号链接，已拒绝安装。".to_string());
        }
        if file_type.is_dir() {
            roots.extend(find_manifest_roots(&entry.path(), kind, depth + 1)?);
        }
    }
    Ok(roots)
}

fn match_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn candidate_score(
    root: &Path,
    repository_root: &Path,
    kind: RemotePackageKind,
    selector: &str,
) -> u8 {
    let expected = match_key(selector);
    if expected.is_empty() {
        return 0;
    }
    let relative = root
        .strip_prefix(repository_root)
        .unwrap_or(root)
        .to_string_lossy();
    let path_key = match_key(&relative);
    let mut values = vec![path_key];
    if kind == RemotePackageKind::Skill {
        if let Ok(text) = read_manifest_text(&root.join("SKILL.md")) {
            let fields = parse_skill_frontmatter(&text);
            if let Some(name) = fields.get("name") {
                values.push(match_key(name));
            }
            if let Some(id) = fields.get("id") {
                values.push(match_key(id));
            }
        }
    }
    values
        .into_iter()
        .map(|value| {
            if value == expected {
                100
            } else if value.ends_with(&expected) {
                90
            } else if value.contains(&expected) {
                80
            } else if expected.contains(&value) && value.len() >= 4 {
                60
            } else {
                0
            }
        })
        .max()
        .unwrap_or(0)
}

fn choose_discovered_root(
    repository_root: &Path,
    kind: RemotePackageKind,
    selector: Option<&str>,
) -> Result<PathBuf, String> {
    let roots = find_manifest_roots(repository_root, kind, 0)?;
    if roots.is_empty() {
        let expected = match kind {
            RemotePackageKind::Skill => "SKILL.md",
            RemotePackageKind::Plugin => "plugin.json 或 .codex-plugin/plugin.json",
            RemotePackageKind::Pet => "pet.json",
        };
        return Err(format!("仓库中没有找到 {expected}。"));
    }
    if roots.len() == 1 {
        return Ok(roots[0].clone());
    }
    if let Some(selector) = selector.filter(|value| !value.trim().is_empty()) {
        let mut scored = roots
            .iter()
            .map(|root| (candidate_score(root, repository_root, kind, selector), root))
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.0.cmp(&left.0));
        if scored[0].0 > 0 && scored.get(1).is_none_or(|next| next.0 < scored[0].0) {
            return Ok(scored[0].1.clone());
        }
    }
    let choices = roots
        .iter()
        .take(8)
        .map(|root| {
            root.strip_prefix(repository_root)
                .unwrap_or(root)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>()
        .join("、");
    Err(format!(
        "仓库包含多个可安装条目（{choices}）。请粘贴目标目录的 GitHub tree URL。"
    ))
}

fn package_path_label(root: &Path, repository_root: &Path) -> String {
    root.strip_prefix(repository_root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

/// 把下载到的 zipball 落到暂存目录并生成待确认清单。
///
/// `installed_ids` 由调用方从各自的仓库读出，用于判定是否覆盖已有条目——
/// 覆盖必须让用户在确认框里看到，不能安装到一半才发现。
pub fn stage_download(
    downloads_dir: &Path,
    kind: RemotePackageKind,
    full_name: &str,
    commit_sha: &str,
    source_url: &str,
    bytes: &[u8],
    installed_ids: &[String],
    package_path: Option<&str>,
    selector: Option<&str>,
) -> Result<StagedPackage, String> {
    let token = format!("stg-{}-{}", now_ms(), uuid_like(bytes));
    let staging_root = downloads_dir.join(&token);
    if staging_root.exists() {
        return Err("暂存目录冲突，请重试。".to_string());
    }

    let stats = match extract_zipball(bytes, &staging_root) {
        Ok(stats) => stats,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
    };

    let repository_root = match unwrap_single_directory(&staging_root) {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
    };

    let selected_root = match package_path.filter(|value| !value.trim().is_empty()) {
        Some(value) => selected_path_root(&repository_root, value),
        None => choose_discovered_root(&repository_root, kind, selector),
    };
    let mut package_root = match selected_root {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
    };
    let codex_plugin = kind == RemotePackageKind::Plugin
        && package_root
            .join(".codex-plugin")
            .join("plugin.json")
            .is_file()
        && !package_root.join("plugin.json").is_file();
    if kind == RemotePackageKind::Plugin {
        package_root = match crate::plugins::prepare_plugin_package(&package_root) {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging_root);
                return Err(error);
            }
        };
    }

    let mut warnings = Vec::new();
    if codex_plugin {
        warnings.push(
            "检测到 Codex 插件清单；将兼容导入其中的 Agent Skills，连接器、Hooks 与专用 UI 不会被执行。"
                .to_string(),
        );
    }
    let preview_fields = match build_preview_fields(kind, &package_root, &mut warnings) {
        Ok(fields) => fields,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
    };

    let replaces_existing = !preview_fields.id.is_empty()
        && installed_ids
            .iter()
            .any(|value| value == &preview_fields.id);

    let preview = RemotePackagePreview {
        staging_token: token.clone(),
        kind,
        full_name: full_name.to_string(),
        commit_sha: commit_sha.to_string(),
        source_url: source_url.to_string(),
        package_path: package_path_label(&package_root, &repository_root),
        id: preview_fields.id.clone(),
        name: preview_fields.name,
        version: preview_fields.version,
        description: preview_fields.description,
        publisher: preview_fields.publisher,
        skill_count: preview_fields.skill_count,
        mcp_server_ids: preview_fields.mcp_server_ids,
        network_domains: preview_fields.network_domains,
        secrets: preview_fields.secrets,
        replaces_existing,
        warnings,
        total_bytes: stats.total_bytes,
        file_count: stats.file_count,
    };

    Ok(StagedPackage {
        entry: StagingEntry {
            kind,
            path: package_root,
            staging_root,
            full_name: full_name.to_string(),
            commit_sha: commit_sha.to_string(),
            replaces_existing,
            created_at_ms: now_ms(),
        },
        preview,
    })
}

/// token 里带一段内容摘要，避免同一毫秒内的两次下载撞名。
fn uuid_like(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Default)]
struct PreviewFields {
    id: String,
    name: String,
    version: String,
    description: String,
    publisher: String,
    skill_count: usize,
    mcp_server_ids: Vec<String>,
    network_domains: Vec<String>,
    secrets: Vec<String>,
}

fn build_preview_fields(
    kind: RemotePackageKind,
    root: &Path,
    warnings: &mut Vec<String>,
) -> Result<PreviewFields, String> {
    match kind {
        RemotePackageKind::Plugin => {
            let text = read_manifest_text(&root.join("plugin.json"))
                .map_err(|error| format!("仓库根目录缺少可用的 plugin.json：{error}"))?;
            let manifest: PluginManifestPreview = serde_json::from_str(&text)
                .map_err(|error| format!("plugin.json 解析失败：{error}"))?;

            // stdio MCP 会被安装器硬拒；提前在预览里说明，省得用户确认完才失败。
            for server in &manifest.capabilities.mcp_servers {
                let is_stdio = server
                    .transport
                    .get("type")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value == "stdio");
                if is_stdio {
                    warnings.push(format!(
                        "声明了本地可执行 stdio MCP «{}»，安装会被拒绝：声明式插件不允许贡献可执行服务器。",
                        server.id
                    ));
                }
            }
            if !manifest.permissions.secrets.is_empty() {
                warnings.push("该插件申请了凭据访问权限，启用前请确认用途。".to_string());
            }

            Ok(PreviewFields {
                id: manifest.id,
                name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                publisher: manifest.publisher,
                skill_count: manifest.capabilities.skills.len(),
                mcp_server_ids: manifest
                    .capabilities
                    .mcp_servers
                    .iter()
                    .map(|server| server.id.clone())
                    .collect(),
                network_domains: manifest.permissions.network_domains,
                secrets: manifest.permissions.secrets,
            })
        }
        RemotePackageKind::Pet => {
            let text = read_manifest_text(&root.join("pet.json"))
                .map_err(|error| format!("仓库根目录缺少可用的 pet.json：{error}"))?;
            let manifest: PetManifestPreview = serde_json::from_str(&text)
                .map_err(|error| format!("pet.json 解析失败：{error}"))?;
            Ok(PreviewFields {
                id: manifest.id.clone(),
                name: if manifest.display_name.is_empty() {
                    manifest.id
                } else {
                    manifest.display_name
                },
                description: manifest.description,
                publisher: manifest.kind,
                ..PreviewFields::default()
            })
        }
        RemotePackageKind::Skill => {
            let manifest_path = root.join("SKILL.md");
            if !manifest_path.is_file() {
                return Err("仓库根目录缺少 SKILL.md。".to_string());
            }
            let text = read_manifest_text(&manifest_path)?;
            let fields = parse_skill_frontmatter(&text);
            if fields.is_empty() {
                warnings.push(
                    "SKILL.md 没有可解析的 frontmatter，安装时由技能安装器再次校验。".to_string(),
                );
            }
            Ok(PreviewFields {
                id: fields.get("id").cloned().unwrap_or_default(),
                name: fields
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| "未命名技能".to_string()),
                version: fields.get("version").cloned().unwrap_or_default(),
                description: fields.get("description").cloned().unwrap_or_default(),
                ..PreviewFields::default()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_selected_frontmatter_fields() {
        let fields = parse_skill_frontmatter(
            "---\nid: demo\nname: \"Demo Skill\"\nversion: 1.0.0\n---\n正文",
        );
        assert_eq!(fields.get("id").unwrap(), "demo");
        assert_eq!(fields.get("name").unwrap(), "Demo Skill");
        assert_eq!(fields.get("version").unwrap(), "1.0.0");
    }

    #[test]
    fn returns_empty_fields_without_frontmatter() {
        assert!(parse_skill_frontmatter("# 只有正文").is_empty());
        assert!(parse_skill_frontmatter("---\n没有结束标记").is_empty());
    }

    /// 必须用 add_symlink 构造：`unix_permissions(0o120777)` 会被 zip crate
    /// 把文件类型位改写成普通文件（0o100777），造不出真的符号链接条目，
    /// 那样测出来的「通过」是假的。
    #[test]
    fn rejects_archive_with_symlink() {
        use zip::{write::SimpleFileOptions, ZipWriter};
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buffer);
            writer
                .add_symlink("link", "/etc/passwd", SimpleFileOptions::default())
                .unwrap();
            writer.finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("mnemora-symlink-{}", now_ms()));
        let result = extract_zipball(buffer.get_ref(), &dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_err(), "symlink entry must be rejected");
        assert!(result.unwrap_err().contains("符号链接"));
    }

    /// zip-slip：`../` 越界路径必须被 enclosed_name 挡住。
    #[test]
    fn rejects_archive_escaping_destination() {
        use zip::{write::SimpleFileOptions, ZipWriter};
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buffer);
            writer
                .start_file("../escaped.txt", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"nope").unwrap();
            writer.finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("mnemora-slip-{}", now_ms()));
        let result = extract_zipball(buffer.get_ref(), &dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_err(), "path traversal must be rejected");
    }

    /// 超出文件数上限要报错，而不是默默截断。
    #[test]
    fn rejects_archive_with_too_many_files() {
        use zip::{write::SimpleFileOptions, ZipWriter};
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buffer);
            for index in 0..(MAX_FILES + 1) {
                writer
                    .start_file(format!("f{index}.txt"), SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(b"x").unwrap();
            }
            writer.finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("mnemora-count-{}", now_ms()));
        let result = extract_zipball(buffer.get_ref(), &dir);
        let _ = fs::remove_dir_all(&dir);
        assert!(result.is_err(), "file count cap must be enforced");
    }

    /// .git 等目录被跳过：既省预算，也避免把仓库元数据拷进安装物。
    #[test]
    fn skips_ignored_directories() {
        use zip::{write::SimpleFileOptions, ZipWriter};
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buffer);
            for name in [
                "pkg/.git/config",
                "pkg/node_modules/a.js",
                "pkg/plugin.json",
            ] {
                writer
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(b"{}").unwrap();
            }
            writer.finish().unwrap();
        }
        let dir = std::env::temp_dir().join(format!("mnemora-skip-{}", now_ms()));
        let stats = extract_zipball(buffer.get_ref(), &dir).unwrap();
        assert_eq!(stats.file_count, 1, "only plugin.json should be extracted");
        assert!(dir.join("pkg/plugin.json").is_file());
        assert!(!dir.join("pkg/.git").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unwraps_github_wrapper_directory() {
        let root = std::env::temp_dir().join(format!("mnemora-unwrap-{}", now_ms()));
        let inner = root.join("owner-repo-abc1234");
        fs::create_dir_all(inner.join("nested")).unwrap();
        fs::write(inner.join("plugin.json"), "{}").unwrap();
        let resolved = unwrap_single_directory(&root).unwrap();
        assert_eq!(resolved, inner);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn keeps_root_when_multiple_entries_exist() {
        let root = std::env::temp_dir().join(format!("mnemora-multi-{}", now_ms()));
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("pet.json"), "{}").unwrap();
        let resolved = unwrap_single_directory(&root).unwrap();
        assert_eq!(resolved, root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn selects_a_named_skill_from_a_multi_skill_repository() {
        let root = std::env::temp_dir().join(format!("mnemora-multi-skill-{}", now_ms()));
        let framing = root
            .join("skills")
            .join("research")
            .join("research-question-framing");
        let writing = root.join("skills").join("writing");
        fs::create_dir_all(&framing).unwrap();
        fs::create_dir_all(&writing).unwrap();
        fs::write(
            framing.join("SKILL.md"),
            "---\nname: research-question-framing\ndescription: Frame research.\n---\nBody",
        )
        .unwrap();
        fs::write(
            writing.join("SKILL.md"),
            "---\nname: writing\ndescription: Write.\n---\nBody",
        )
        .unwrap();

        let selected =
            choose_discovered_root(&root, RemotePackageKind::Skill, Some("question-framing"))
                .unwrap();
        assert_eq!(selected, framing);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_github_blob_path_to_its_package_directory() {
        let root = std::env::temp_dir().join(format!("mnemora-explicit-skill-{}", now_ms()));
        let skill = root.join("skills").join("demo");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo.\n---\nBody",
        )
        .unwrap();
        assert_eq!(
            selected_path_root(&root, "skills/demo/SKILL.md").unwrap(),
            skill
        );
        assert!(selected_path_root(&root, "../outside").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
