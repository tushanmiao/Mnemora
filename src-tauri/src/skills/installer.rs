//! 用户 Skill 的有界目录复制、ZIP 解压和原子替换。

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use uuid::Uuid;
use zip::ZipArchive;

use super::{
    parser::parse_skill,
    repository::SkillRepository,
    types::{
        SkillImportKind, SkillImportRequest, SkillImportResult, SkillImportStatus, SkillSource,
        SkillStateEntry,
    },
};

const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 50 * 1024 * 1024;
const MAX_SINGLE_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_FILES: usize = 512;
const MAX_DEPTH: usize = 8;
const IGNORED_DIRECTORIES: &[&str] = &[".git", "node_modules", "target"];

#[derive(Default)]
struct CopyBudget {
    files: usize,
    bytes: u64,
}

impl SkillRepository {
    pub fn import(&self, request: SkillImportRequest) -> Result<SkillImportResult, String> {
        self.ensure_user_directories()?;
        let source = PathBuf::from(request.path.trim());
        if !source.is_absolute() {
            return Err("技能导入路径必须是绝对路径。".to_string());
        }
        let staging = self.staging_dir.join(Uuid::new_v4().to_string());
        fs::create_dir(&staging).map_err(|error| format!("创建技能安装临时目录失败：{error}"))?;
        let result =
            self.import_into_staging(&source, request.kind, request.replace_existing, &staging);
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn import_into_staging(
        &self,
        source: &Path,
        kind: SkillImportKind,
        replace_existing: bool,
        staging: &Path,
    ) -> Result<SkillImportResult, String> {
        let extracted = staging.join("extracted");
        match kind {
            SkillImportKind::Directory => {
                let metadata = fs::symlink_metadata(source)
                    .map_err(|error| format!("读取技能目录失败：{error}"))?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err("选择的技能来源不是普通目录。".to_string());
                }
                fs::create_dir(&extracted)
                    .map_err(|error| format!("创建技能复制目录失败：{error}"))?;
                copy_directory(source, &extracted, 0, &mut CopyBudget::default())?;
            }
            SkillImportKind::Zip => extract_zip(source, &extracted)?,
        }

        let roots = find_skill_roots(&extracted, 0)?;
        if roots.len() != 1 {
            return Err(format!(
                "导入内容必须且只能包含一个 SKILL.md，当前找到 {} 个。",
                roots.len()
            ));
        }
        let skill_root = roots.into_iter().next().expect("checked one skill root");
        let record = parse_skill(&skill_root, SkillSource::User, true)?;
        if self
            .builtin_dir
            .join(&record.summary.id)
            .join("SKILL.md")
            .is_file()
        {
            return Err("用户技能不能覆盖同 ID 的内置技能。".to_string());
        }
        let destination = self.user_dir.join(&record.summary.id);
        if destination.exists() && !replace_existing {
            return Ok(SkillImportResult {
                status: SkillImportStatus::AlreadyExists,
                skill: record.summary,
            });
        }

        let ready = staging.join("ready");
        fs::rename(&skill_root, &ready)
            .map_err(|error| format!("准备技能安装目录失败：{error}"))?;
        let backup = staging.join("previous");
        let had_previous = destination.exists();
        if had_previous {
            let metadata = fs::symlink_metadata(&destination)
                .map_err(|error| format!("读取旧技能目录失败：{error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("旧技能目录不是普通目录，已拒绝覆盖。".to_string());
            }
            fs::rename(&destination, &backup)
                .map_err(|error| format!("备份旧技能失败：{error}"))?;
        }
        if let Err(error) = fs::rename(&ready, &destination) {
            if had_previous {
                let _ = fs::rename(&backup, &destination);
            }
            return Err(format!("安装技能失败：{error}"));
        }

        let mut state = self.read_state()?;
        let enabled = state
            .skills
            .get(&record.summary.id)
            .map(|entry| entry.enabled)
            .unwrap_or(true);
        state
            .skills
            .insert(record.summary.id.clone(), SkillStateEntry { enabled });
        if let Err(error) = self.write_state(&state) {
            let _ = fs::remove_dir_all(&destination);
            if had_previous {
                let _ = fs::rename(&backup, &destination);
            }
            return Err(error);
        }
        if had_previous {
            let _ = fs::remove_dir_all(backup);
        }
        let installed = parse_skill(&destination, SkillSource::User, enabled)?;
        Ok(SkillImportResult {
            status: SkillImportStatus::Installed,
            skill: installed.summary,
        })
    }
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    depth: usize,
    budget: &mut CopyBudget,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("技能目录层级超过安全上限。".to_string());
    }
    for entry in fs::read_dir(source).map_err(|error| format!("读取技能来源失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取技能来源失败：{error}"))?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取技能文件类型失败：{error}"))?;
        if file_type.is_symlink() {
            return Err(format!("技能不能包含符号链接：{name_text}"));
        }
        if file_type.is_dir() {
            if IGNORED_DIRECTORIES.contains(&name_text.as_ref()) {
                continue;
            }
            let target = destination.join(&name);
            fs::create_dir_all(&target).map_err(|error| format!("创建技能子目录失败：{error}"))?;
            copy_directory(&entry.path(), &target, depth + 1, budget)?;
        } else if file_type.is_file() {
            let size = entry
                .metadata()
                .map_err(|error| format!("读取技能文件大小失败：{error}"))?
                .len();
            consume_budget(budget, size)?;
            fs::copy(entry.path(), destination.join(name))
                .map_err(|error| format!("复制技能文件失败：{error}"))?;
        } else {
            return Err(format!("技能包含不支持的文件类型：{name_text}"));
        }
    }
    Ok(())
}

fn extract_zip(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::metadata(source).map_err(|error| format!("读取 ZIP 失败：{error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err("ZIP 必须是 1 字节到 20 MB 的普通文件。".to_string());
    }
    fs::create_dir(destination).map_err(|error| format!("创建 ZIP 解压目录失败：{error}"))?;
    let file = File::open(source).map_err(|error| format!("打开 ZIP 失败：{error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("ZIP 格式无效：{error}"))?;
    let mut budget = CopyBudget::default();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("读取 ZIP 条目失败：{error}"))?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("ZIP 不能包含符号链接。".to_string());
        }
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "ZIP 包含越界路径。".to_string())?
            .to_path_buf();
        if relative.components().count() > MAX_DEPTH + 1 {
            return Err("ZIP 目录层级超过安全上限。".to_string());
        }
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
            return Err("ZIP 解压路径越界。".to_string());
        }
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| format!("创建 ZIP 目录失败：{error}"))?;
            continue;
        }
        consume_budget(&mut budget, entry.size())?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建 ZIP 文件目录失败：{error}"))?;
        }
        let mut output =
            File::create(&target).map_err(|error| format!("创建解压文件失败：{error}"))?;
        let copied = io::copy(&mut entry.take(MAX_SINGLE_FILE_BYTES + 1), &mut output)
            .map_err(|error| format!("解压技能文件失败：{error}"))?;
        if copied > MAX_SINGLE_FILE_BYTES {
            return Err("ZIP 中的单个文件超过 10 MB。".to_string());
        }
        output
            .flush()
            .map_err(|error| format!("写入解压文件失败：{error}"))?;
    }
    Ok(())
}

pub(crate) fn stage_package_source(
    source: &Path,
    kind: SkillImportKind,
    destination: &Path,
) -> Result<(), String> {
    match kind {
        SkillImportKind::Directory => {
            let metadata = fs::symlink_metadata(source)
                .map_err(|error| format!("Failed to inspect package directory: {error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("Package source must be a regular directory".to_string());
            }
            fs::create_dir(destination)
                .map_err(|error| format!("Failed to create package staging directory: {error}"))?;
            copy_directory(source, destination, 0, &mut CopyBudget::default())
        }
        SkillImportKind::Zip => extract_zip(source, destination),
    }
}

fn consume_budget(budget: &mut CopyBudget, size: u64) -> Result<(), String> {
    if size > MAX_SINGLE_FILE_BYTES {
        return Err("技能中的单个文件不能超过 10 MB。".to_string());
    }
    budget.files = budget.files.saturating_add(1);
    budget.bytes = budget.bytes.saturating_add(size);
    if budget.files > MAX_FILES {
        return Err("技能文件数量不能超过 512。".to_string());
    }
    if budget.bytes > MAX_EXTRACTED_BYTES {
        return Err("技能文件总大小不能超过 50 MB。".to_string());
    }
    Ok(())
}

fn find_skill_roots(directory: &Path, depth: usize) -> Result<Vec<PathBuf>, String> {
    if depth > MAX_DEPTH {
        return Err("技能目录层级超过安全上限。".to_string());
    }
    let mut roots = Vec::new();
    if directory.join("SKILL.md").is_file() {
        roots.push(directory.to_path_buf());
        return Ok(roots);
    }
    for entry in fs::read_dir(directory).map_err(|error| format!("扫描 SKILL.md 失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("扫描 SKILL.md 失败：{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取技能目录类型失败：{error}"))?;
        if file_type.is_symlink() {
            return Err("技能目录不能包含符号链接。".to_string());
        }
        if file_type.is_dir() {
            roots.extend(find_skill_roots(&entry.path(), depth + 1)?);
        }
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::skills::{
        types::{SkillImportKind, SkillImportRequest, SkillImportStatus},
        SkillRepository,
    };

    #[test]
    fn installs_and_uninstalls_a_directory_skill() {
        let root =
            std::env::temp_dir().join(format!("mnemora-skill-install-{}", uuid::Uuid::new_v4()));
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nid: test-skill\nname: 测试技能\ndescription: 用于安装测试。\nversion: 1.0.0\n---\n正文\n",
        )
        .unwrap();
        let repository = SkillRepository::new(root.join("builtin"), root.join("data"));
        let result = repository
            .import(SkillImportRequest {
                path: source.to_string_lossy().into_owned(),
                kind: SkillImportKind::Directory,
                replace_existing: false,
            })
            .unwrap();
        assert_eq!(result.status, SkillImportStatus::Installed);
        assert_eq!(repository.list().unwrap().skills.len(), 1);
        repository.uninstall("test-skill").unwrap();
        assert!(repository.list().unwrap().skills.is_empty());
        let _ = fs::remove_dir_all(root);
    }
}
