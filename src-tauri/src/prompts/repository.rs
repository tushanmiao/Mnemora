use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{PromptTemplate, PromptTemplateInput};

const STORE_VERSION: u32 = 1;
const FILE_NAME: &str = "prompt_templates.json";
const MAX_TEMPLATES: usize = 100;
const MAX_TITLE_CHARS: usize = 80;
const MAX_CONTENT_CHARS: usize = 16_000;
const MAX_TOTAL_CHARS: usize = 256_000;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptTemplateStore {
    #[serde(default = "store_version")]
    version: u32,
    #[serde(default)]
    templates: Vec<PromptTemplate>,
}

fn store_version() -> u32 {
    STORE_VERSION
}

#[derive(Clone)]
pub struct PromptTemplateRepository {
    path: PathBuf,
}

impl PromptTemplateRepository {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            path: app_data_dir.join("prompts").join(FILE_NAME),
        }
    }

    pub fn list(&self) -> Result<Vec<PromptTemplate>, String> {
        Ok(self.load()?.templates)
    }

    pub fn upsert(&self, input: PromptTemplateInput) -> Result<PromptTemplate, String> {
        let mut store = self.load()?;
        let title = input.title.trim().to_string();
        let content = input.content.trim().to_string();
        validate_text(&title, &content)?;

        let now = now_ms();
        let id = input
            .id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        validate_id(&id)?;

        let template = if let Some(index) = store.templates.iter().position(|item| item.id == id) {
            let created_at = store.templates[index].created_at;
            let updated = PromptTemplate {
                id,
                title,
                content,
                created_at,
                updated_at: now,
            };
            store.templates.remove(index);
            store.templates.insert(0, updated.clone());
            updated
        } else {
            if input.id.is_some() {
                return Err("要编辑的提示词不存在，请刷新后重试。".to_string());
            }
            if store.templates.len() >= MAX_TEMPLATES {
                return Err(format!("提示词数量不能超过 {MAX_TEMPLATES} 条。"));
            }
            let created = PromptTemplate {
                id,
                title,
                content,
                created_at: now,
                updated_at: now,
            };
            store.templates.insert(0, created.clone());
            created
        };

        validate_templates(&store.templates)?;
        self.write(&store)?;
        Ok(template)
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let id = id.trim();
        validate_id(id)?;
        let mut store = self.load()?;
        let before = store.templates.len();
        store.templates.retain(|template| template.id != id);
        if store.templates.len() == before {
            return Ok(false);
        }
        self.write(&store)?;
        Ok(true)
    }

    fn load(&self) -> Result<PromptTemplateStore, String> {
        if !self.path.exists() {
            return Ok(PromptTemplateStore {
                version: STORE_VERSION,
                templates: Vec::new(),
            });
        }
        let raw =
            fs::read_to_string(&self.path).map_err(|error| format!("读取提示词库失败：{error}"))?;
        let store = serde_json::from_str::<PromptTemplateStore>(&raw)
            .map_err(|error| format!("解析提示词库失败：{error}"))?;
        if store.version > STORE_VERSION {
            return Err("提示词库版本高于当前应用支持的版本。".to_string());
        }
        validate_templates(&store.templates)?;
        Ok(store)
    }

    fn write(&self, store: &PromptTemplateStore) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "提示词库路径无效。".to_string())?;
        fs::create_dir_all(parent).map_err(|error| format!("创建提示词库目录失败：{error}"))?;
        let json = serde_json::to_vec_pretty(store)
            .map_err(|error| format!("序列化提示词库失败：{error}"))?;
        let temporary = self.path.with_extension(format!(
            "json.tmp-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let backup = self.path.with_extension("json.bak");
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("创建提示词库临时文件失败：{error}"))?;
        file.write_all(&json)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("写入提示词库失败：{error}"))?;
        drop(file);

        if self.path.exists() {
            if backup.exists() {
                fs::remove_file(&backup)
                    .map_err(|error| format!("清理提示词库备份失败：{error}"))?;
            }
            fs::rename(&self.path, &backup)
                .map_err(|error| format!("备份提示词库失败：{error}"))?;
        }
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::rename(&backup, &self.path);
            let _ = fs::remove_file(&temporary);
            return Err(format!("替换提示词库失败：{error}"));
        }
        let _ = fs::remove_file(backup);
        Ok(())
    }
}

fn validate_templates(templates: &[PromptTemplate]) -> Result<(), String> {
    if templates.len() > MAX_TEMPLATES {
        return Err(format!("提示词数量不能超过 {MAX_TEMPLATES} 条。"));
    }
    let mut ids = HashSet::new();
    let mut total_chars = 0usize;
    for template in templates {
        validate_id(&template.id)?;
        if !ids.insert(template.id.as_str()) {
            return Err("提示词 ID 不能重复。".to_string());
        }
        validate_text(&template.title, &template.content)?;
        total_chars = total_chars
            .saturating_add(template.title.chars().count())
            .saturating_add(template.content.chars().count());
        if total_chars > MAX_TOTAL_CHARS {
            return Err(format!("提示词库总长度不能超过 {MAX_TOTAL_CHARS} 个字符。"));
        }
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("提示词 ID 格式无效。".to_string());
    }
    Ok(())
}

fn validate_text(title: &str, content: &str) -> Result<(), String> {
    let title_chars = title.chars().count();
    if title_chars == 0 || title_chars > MAX_TITLE_CHARS {
        return Err(format!("提示词标题必须为 1–{MAX_TITLE_CHARS} 个字符。"));
    }
    let content_chars = content.chars().count();
    if content_chars == 0 || content_chars > MAX_CONTENT_CHARS {
        return Err(format!("提示词内容必须为 1–{MAX_CONTENT_CHARS} 个字符。"));
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> (PathBuf, PromptTemplateRepository) {
        let root = std::env::temp_dir().join(format!("mnemora-prompts-{}", Uuid::new_v4()));
        (root.clone(), PromptTemplateRepository::new(root))
    }

    #[test]
    fn creates_updates_and_deletes_prompt_templates() {
        let (root, repository) = repository();
        let created = repository
            .upsert(PromptTemplateInput {
                id: None,
                title: "  文献  ".to_string(),
                content: "  翻译并解释文献  ".to_string(),
            })
            .unwrap();
        assert_eq!(created.title, "文献");
        assert_eq!(repository.list().unwrap(), vec![created.clone()]);

        let updated = repository
            .upsert(PromptTemplateInput {
                id: Some(created.id.clone()),
                title: "论文".to_string(),
                content: "总结核心结论".to_string(),
            })
            .unwrap();
        assert_eq!(updated.created_at, created.created_at);
        assert_eq!(updated.title, "论文");
        assert!(repository.delete(&created.id).unwrap());
        assert!(repository.list().unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_empty_and_unknown_updates() {
        let (root, repository) = repository();
        assert!(repository
            .upsert(PromptTemplateInput {
                id: None,
                title: "".to_string(),
                content: "content".to_string(),
            })
            .is_err());
        assert!(repository
            .upsert(PromptTemplateInput {
                id: Some("missing".to_string()),
                title: "title".to_string(),
                content: "content".to_string(),
            })
            .is_err());
        let _ = fs::remove_dir_all(root);
    }
}
