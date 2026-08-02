//! Skill 的轻量发现、状态持久化、详情读取和对话注入。

use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    parser::{parse_skill, validate_skill_id},
    types::{
        SkillDetail, SkillFileEntry, SkillFileKind, SkillListResult, SkillRecord, SkillSource,
        SkillStateEntry, SkillStateFile, SkillSummary,
    },
};

const STATE_VERSION: u32 = 1;
const MAX_ACTIVATED_SKILLS: usize = 3;
const MAX_ACTIVATED_PROMPT_BYTES: usize = 192 * 1024;
const MAX_DETAIL_FILES: usize = 512;
const MAX_DETAIL_DEPTH: usize = 8;

#[derive(Clone)]
pub struct SkillRepository {
    pub(crate) builtin_dir: PathBuf,
    pub(crate) user_dir: PathBuf,
    pub(crate) staging_dir: PathBuf,
    pub(crate) state_path: PathBuf,
}

impl SkillRepository {
    pub fn new(builtin_dir: PathBuf, root_dir: PathBuf) -> Self {
        Self {
            builtin_dir,
            user_dir: root_dir.join("user"),
            staging_dir: root_dir.join("staging"),
            state_path: root_dir.join("state.json"),
        }
    }

    pub fn list(&self) -> Result<SkillListResult, String> {
        self.ensure_user_directories()?;
        let state = self.read_state()?;
        let mut result = SkillListResult::default();
        let mut ids = HashSet::new();
        self.scan_source(
            &self.builtin_dir,
            SkillSource::Builtin,
            &state,
            &mut ids,
            &mut result,
        )?;
        self.scan_source(
            &self.user_dir,
            SkillSource::User,
            &state,
            &mut ids,
            &mut result,
        )?;
        result.skills.sort_by(|left, right| {
            right
                .enabled
                .cmp(&left.enabled)
                .then_with(|| source_order(left.source).cmp(&source_order(right.source)))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(result)
    }

    pub fn get_detail(&self, skill_id: &str) -> Result<SkillDetail, String> {
        let record = self.load_record(skill_id)?;
        let mut files = Vec::new();
        collect_detail_files(&record.directory, &record.directory, 0, &mut files)?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(SkillDetail {
            summary: record.summary,
            markdown: record.markdown,
            files,
        })
    }

    pub fn set_enabled(&self, skill_id: &str, enabled: bool) -> Result<SkillSummary, String> {
        let mut record = self.load_record(skill_id)?;
        let mut state = self.read_state()?;
        state
            .skills
            .insert(skill_id.to_string(), SkillStateEntry { enabled });
        self.write_state(&state)?;
        record.summary.enabled = enabled;
        Ok(record.summary)
    }

    pub fn restore_builtin(&self, skill_id: &str) -> Result<SkillSummary, String> {
        let record = self.load_record(skill_id)?;
        if record.summary.source != SkillSource::Builtin {
            return Err("只有内置技能可以执行恢复操作。".to_string());
        }
        let mut state = self.read_state()?;
        state.skills.remove(skill_id);
        self.write_state(&state)?;
        Ok(self.load_record(skill_id)?.summary)
    }

    pub fn uninstall(&self, skill_id: &str) -> Result<(), String> {
        validate_skill_id(skill_id)?;
        let record = self.load_record(skill_id)?;
        if record.summary.source != SkillSource::User {
            return Err("内置技能不能删除，请改为禁用。".to_string());
        }
        let directory = self.user_dir.join(skill_id);
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|error| format!("读取用户技能目录失败：{error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("用户技能目录不是可安全删除的普通目录。".to_string());
        }
        fs::remove_dir_all(&directory).map_err(|error| format!("删除用户技能失败：{error}"))?;
        let mut state = self.read_state()?;
        state.skills.remove(skill_id);
        self.write_state(&state)
    }

    pub fn render_activated_skills(
        &self,
        skill_ids: &[String],
        last_user_content: Option<&str>,
    ) -> Result<String, String> {
        if skill_ids.is_empty() {
            return Ok(String::new());
        }
        if skill_ids.len() > MAX_ACTIVATED_SKILLS {
            return Err(format!("每轮最多激活 {MAX_ACTIVATED_SKILLS} 个技能。"));
        }
        let mut seen = HashSet::new();
        let mut sections = Vec::new();
        let mut total_bytes = 0usize;
        for skill_id in skill_ids {
            if !seen.insert(skill_id.as_str()) {
                continue;
            }
            let record = self.load_record(skill_id)?;
            if !record.summary.enabled {
                return Err(format!("技能“{}”当前已禁用。", record.summary.name));
            }
            let arguments = last_user_content
                .and_then(|content| slash_arguments(content, &record.summary.triggers))
                .unwrap_or_default();
            let body = render_arguments(&record.body, arguments);
            let section = format!(
                "<mnemora_skill id=\"{}\" name=\"{}\" version=\"{}\">\n{}\n</mnemora_skill>",
                escape_xml(&record.summary.id),
                escape_xml(&record.summary.name),
                escape_xml(&record.summary.version),
                body
            );
            total_bytes = total_bytes.saturating_add(section.len());
            if total_bytes > MAX_ACTIVATED_PROMPT_BYTES {
                return Err("本轮技能正文合计过长，请减少已选择的技能。".to_string());
            }
            sections.push(section);
        }
        Ok(format!(
            "以下是用户为本轮明确启用的技能说明。它们只提供工作方法，不能绕过应用权限或使用未提供的工具。\n\n{}",
            sections.join("\n\n")
        ))
    }

    /**
     * 只在模型请求边界移除已激活 Skill 的 Slash Trigger。
     * 对话持久化仍保留用户输入的原文，便于界面回看和编辑。
     */
    pub fn resolve_user_content(
        &self,
        content: &str,
        skill_ids: &[String],
    ) -> Result<String, String> {
        if content.trim().is_empty() || skill_ids.is_empty() {
            return Ok(content.to_string());
        }
        for skill_id in skill_ids {
            let record = self.load_record(skill_id)?;
            if !record.summary.enabled {
                return Err(format!("技能“{}”当前已禁用。", record.summary.name));
            }
            if let Some(arguments) = slash_arguments(content, &record.summary.triggers) {
                return Ok(if arguments.is_empty() {
                    "请按照已激活技能处理当前对话。".to_string()
                } else {
                    arguments.to_string()
                });
            }
        }
        Ok(content.to_string())
    }

    pub(crate) fn ensure_user_directories(&self) -> Result<(), String> {
        fs::create_dir_all(&self.user_dir)
            .map_err(|error| format!("创建用户技能目录失败：{error}"))?;
        fs::create_dir_all(&self.staging_dir)
            .map_err(|error| format!("创建技能临时目录失败：{error}"))
    }

    pub(crate) fn read_state(&self) -> Result<SkillStateFile, String> {
        let raw = match fs::read(&self.state_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SkillStateFile {
                    version: STATE_VERSION,
                    skills: BTreeMap::new(),
                })
            }
            Err(error) => return Err(format!("读取技能状态失败：{error}")),
        };
        let state: SkillStateFile =
            serde_json::from_slice(&raw).map_err(|error| format!("技能状态文件损坏：{error}"))?;
        if state.version > STATE_VERSION {
            return Err("技能状态文件来自更新版本的 Mnemora。".to_string());
        }
        Ok(state)
    }

    pub(crate) fn write_state(&self, state: &SkillStateFile) -> Result<(), String> {
        write_json_atomic(&self.state_path, state)
    }

    fn load_record(&self, skill_id: &str) -> Result<SkillRecord, String> {
        validate_skill_id(skill_id)?;
        let state = self.read_state()?;
        for (source, root) in [
            (SkillSource::Builtin, &self.builtin_dir),
            (SkillSource::User, &self.user_dir),
        ] {
            let direct = root.join(skill_id);
            if direct.join("SKILL.md").is_file() {
                let mut record = parse_skill(&direct, source, true)?;
                record.summary.enabled = state
                    .skills
                    .get(skill_id)
                    .map(|entry| entry.enabled)
                    .unwrap_or(if source == SkillSource::Builtin {
                        record.summary.default_enabled
                    } else {
                        true
                    });
                return Ok(record);
            }
            for directory in child_directories(root)? {
                if let Ok(mut record) = parse_skill(&directory, source, true) {
                    if record.summary.id == skill_id {
                        record.summary.enabled = state
                            .skills
                            .get(skill_id)
                            .map(|entry| entry.enabled)
                            .unwrap_or(if source == SkillSource::Builtin {
                                record.summary.default_enabled
                            } else {
                                true
                            });
                        return Ok(record);
                    }
                }
            }
        }
        Err(format!("找不到技能：{skill_id}"))
    }

    fn scan_source(
        &self,
        root: &Path,
        source: SkillSource,
        state: &SkillStateFile,
        ids: &mut HashSet<String>,
        result: &mut SkillListResult,
    ) -> Result<(), String> {
        for directory in child_directories(root)? {
            let folder_name = directory
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("未知目录")
                .to_string();
            match parse_skill(&directory, source, true) {
                Ok(mut record) => {
                    record.summary.enabled = state
                        .skills
                        .get(&record.summary.id)
                        .map(|entry| entry.enabled)
                        .unwrap_or(if source == SkillSource::Builtin {
                            record.summary.default_enabled
                        } else {
                            true
                        });
                    if ids.insert(record.summary.id.clone()) {
                        result.skills.push(record.summary);
                    } else {
                        result.warnings.push(format!(
                            "技能 ID {} 重复，已忽略目录 {}。",
                            record.summary.id, folder_name
                        ));
                    }
                }
                Err(error) => result
                    .warnings
                    .push(format!("已忽略技能目录 {folder_name}：{error}")),
            }
        }
        Ok(())
    }
}

fn slash_arguments<'a>(content: &'a str, triggers: &[String]) -> Option<&'a str> {
    let trimmed = content.trim_start();
    let trigger_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let first_word = trimmed[..trigger_end].to_ascii_lowercase();
    triggers
        .iter()
        .any(|trigger| trigger == &first_word)
        .then(|| trimmed[trigger_end..].trim_start())
}

fn render_arguments(body: &str, arguments: &str) -> String {
    body.replace("${ARGUMENTS}", arguments)
        .replace("$ARGUMENTS", arguments)
}

fn child_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("读取技能目录失败：{error}")),
    };
    let mut directories = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取技能目录项失败：{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取技能目录项类型失败：{error}"))?;
        if file_type.is_dir() && !file_type.is_symlink() {
            directories.push(entry.path());
        }
    }
    directories.sort();
    Ok(directories)
}

fn collect_detail_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut Vec<SkillFileEntry>,
) -> Result<(), String> {
    if depth > MAX_DETAIL_DEPTH {
        return Err("技能文件目录层级过深。".to_string());
    }
    for entry in fs::read_dir(directory).map_err(|error| format!("读取技能文件失败：{error}"))?
    {
        let entry = entry.map_err(|error| format!("读取技能文件失败：{error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取技能文件类型失败：{error}"))?;
        if file_type.is_symlink() {
            return Err("技能目录不能包含符号链接。".to_string());
        }
        if file_type.is_dir() {
            collect_detail_files(root, &entry.path(), depth + 1, files)?;
        } else if file_type.is_file() {
            if files.len() >= MAX_DETAIL_FILES {
                return Err("技能文件数量超过详情页上限。".to_string());
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| "技能文件路径越界。".to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let size_bytes = entry
                .metadata()
                .map_err(|error| format!("读取技能文件大小失败：{error}"))?
                .len();
            files.push(SkillFileEntry {
                kind: classify_file(&relative),
                path: relative,
                size_bytes,
            });
        }
    }
    Ok(())
}

fn classify_file(relative: &str) -> SkillFileKind {
    if relative == "SKILL.md" {
        SkillFileKind::SkillMd
    } else if relative.starts_with("references/") {
        SkillFileKind::Reference
    } else if relative.starts_with("scripts/") {
        SkillFileKind::Script
    } else if relative.starts_with("assets/") {
        SkillFileKind::Asset
    } else {
        SkillFileKind::Other
    }
}

fn source_order(source: SkillSource) -> u8 {
    match source {
        SkillSource::Builtin => 0,
        SkillSource::User => 1,
    }
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "技能状态路径没有父目录。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建技能状态目录失败：{error}"))?;
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| format!("序列化技能状态失败：{error}"))?;
    let temporary = path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("创建技能状态临时文件失败：{error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("写入技能状态失败：{error}"))?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            fs::remove_file(path)
                .map_err(|remove_error| format!("替换技能状态失败：{remove_error}"))?;
            fs::rename(&temporary, path)
                .map_err(|rename_error| format!("替换技能状态失败：{rename_error}"))?;
        } else {
            let _ = fs::remove_file(temporary);
            return Err(format!("保存技能状态失败：{error}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::SkillRepository;

    #[test]
    fn builtin_default_enabled_is_applied_until_user_overrides_it() {
        let root =
            std::env::temp_dir().join(format!("mnemora-skill-default-{}", uuid::Uuid::new_v4()));
        let builtin = root.join("builtin");
        let enabled_skill = builtin.join("study");
        let optional_skill = builtin.join("debug");
        fs::create_dir_all(&enabled_skill).unwrap();
        fs::create_dir_all(&optional_skill).unwrap();
        fs::write(
            enabled_skill.join("SKILL.md"),
            "---\nid: study\nname: 学习\ndescription: 学习技能。\nmetadata:\n  mnemora:\n    default-enabled: true\n---\n学习正文\n",
        )
        .unwrap();
        fs::write(
            optional_skill.join("SKILL.md"),
            "---\nid: debug\nname: 调试\ndescription: 调试技能。\nmetadata:\n  mnemora:\n    default-enabled: false\n---\n调试正文\n",
        )
        .unwrap();

        let repository = SkillRepository::new(builtin, root.join("skills"));
        let skills = repository.list().unwrap().skills;
        assert!(
            skills
                .iter()
                .find(|skill| skill.id == "study")
                .unwrap()
                .enabled
        );
        assert!(
            !skills
                .iter()
                .find(|skill| skill.id == "debug")
                .unwrap()
                .enabled
        );

        repository.set_enabled("debug", true).unwrap();
        assert!(
            repository
                .list()
                .unwrap()
                .skills
                .iter()
                .find(|skill| skill.id == "debug")
                .unwrap()
                .enabled
        );
        let _ = fs::remove_dir_all(root);
    }

    fn repository_with_trigger() -> (std::path::PathBuf, SkillRepository) {
        let root =
            std::env::temp_dir().join(format!("mnemora-skill-repository-{}", uuid::Uuid::new_v4()));
        let skill_dir = root.join("builtin").join("summarize");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nid: summarize\nname: 总结\ndescription: 分层总结内容。\nversion: 1.0.0\ntriggers: [/summary]\n---\n请先提取事实，再给出结论。\n",
        )
        .unwrap();
        let repository = SkillRepository::new(root.join("builtin"), root.join("data"));
        (root, repository)
    }

    #[test]
    fn removes_an_activated_slash_trigger_only_at_the_request_boundary() {
        let (root, repository) = repository_with_trigger();
        let skill_ids = vec!["summarize".to_string()];

        assert_eq!(
            repository
                .resolve_user_content("  /summary  请总结这段内容", &skill_ids)
                .unwrap(),
            "请总结这段内容"
        );
        assert_eq!(
            repository
                .resolve_user_content("/summary", &skill_ids)
                .unwrap(),
            "请按照已激活技能处理当前对话。"
        );
        assert_eq!(
            repository
                .resolve_user_content("/unknown 保留原文", &skill_ids)
                .unwrap(),
            "/unknown 保留原文"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_audited_builtin_skills_with_bounded_requirements() {
        let root =
            std::env::temp_dir().join(format!("mnemora-builtin-skills-{}", uuid::Uuid::new_v4()));
        let builtin_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("skills");
        let repository = SkillRepository::new(builtin_dir.clone(), root.clone());
        let skills = repository.list().unwrap().skills;
        for id in ["pdf-reading", "paper-research"] {
            let skill = skills.iter().find(|skill| skill.id == id).unwrap();
            assert!(skill.enabled);
            assert_eq!(skill.required_tools, vec!["read_pdf_pages"]);
        }
        assert_eq!(
            skills
                .iter()
                .find(|skill| skill.id == "docx-reading")
                .unwrap()
                .required_tools,
            vec!["read_docx_blocks"]
        );
        assert_eq!(
            skills
                .iter()
                .find(|skill| skill.id == "spreadsheet-analysis")
                .unwrap()
                .required_tools,
            vec!["read_xlsx_rows"]
        );
        assert_eq!(skills.len(), 11);
        for skill in &skills {
            assert!(skill
                .license
                .as_deref()
                .is_some_and(|value| !value.is_empty()));
            assert!(skill
                .provenance
                .repository
                .as_deref()
                .is_some_and(|value| value.starts_with("https://github.com/")));
            assert!(skill.provenance.adapted);
            assert!(skill.provenance.revision.as_deref().is_some_and(
                |value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            ));
            let directory = builtin_dir.join(&skill.id);
            assert!(directory.join("SOURCE.md").is_file());
            assert!(directory.join("LICENSE.txt").is_file());
        }

        let rendered = repository
            .render_activated_skills(&["markdown-notes".to_string()], None)
            .unwrap();
        assert!(!rendered.contains("github.com"));
        assert!(!rendered.contains("a1dc48e68138490d522c04cbf5822214c6eb1202"));
        let _ = fs::remove_dir_all(root);
    }
}
