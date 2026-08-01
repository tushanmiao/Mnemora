use serde::{Deserialize, Serialize};

pub const CURRENT_SYNC_SETTINGS_VERSION: u32 = 1;
const MAX_VAULT_PATH_BYTES: usize = 32_768;
const MAX_RELATIVE_DIRECTORY_CHARS: usize = 240;
const MAX_NOTION_PAGE_ID_CHARS: usize = 100;
const MAX_FEISHU_APP_ID_CHARS: usize = 128;
const MAX_FEISHU_FOLDER_TOKEN_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncTarget {
    #[default]
    Feishu,
    Obsidian,
    Notion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObsidianSettings {
    #[serde(default)]
    pub vault_path: String,
    #[serde(default = "default_obsidian_directory")]
    pub directory: String,
}

impl Default for ObsidianSettings {
    fn default() -> Self {
        Self {
            vault_path: String::new(),
            directory: default_obsidian_directory(),
        }
    }
}

fn default_obsidian_directory() -> String {
    "Mnemora".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionSettings {
    #[serde(default)]
    pub parent_page_id: String,
    #[serde(default)]
    pub has_token: bool,
}

/// 飞书自建应用配置。App Secret 只保存在系统凭据库，不进入此结构。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuSettings {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub folder_token: String,
    #[serde(default)]
    pub has_app_secret: bool,
}

impl Default for FeishuSettings {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            folder_token: String::new(),
            has_app_secret: false,
        }
    }
}

impl Default for NotionSettings {
    fn default() -> Self {
        Self {
            parent_page_id: String::new(),
            has_token: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettings {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub target: SyncTarget,
    #[serde(default)]
    pub auto_sync: bool,
    #[serde(default = "default_true")]
    pub include_annotations: bool,
    #[serde(default = "default_true")]
    pub include_metadata: bool,
    #[serde(default)]
    pub obsidian: ObsidianSettings,
    #[serde(default)]
    pub notion: NotionSettings,
    #[serde(default)]
    pub feishu: FeishuSettings,
}

fn current_version() -> u32 {
    CURRENT_SYNC_SETTINGS_VERSION
}

fn default_true() -> bool {
    true
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            version: CURRENT_SYNC_SETTINGS_VERSION,
            enabled: false,
            target: SyncTarget::Feishu,
            auto_sync: false,
            include_annotations: true,
            include_metadata: true,
            obsidian: ObsidianSettings::default(),
            notion: NotionSettings::default(),
            feishu: FeishuSettings::default(),
        }
    }
}

impl SyncSettings {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        if self.version > CURRENT_SYNC_SETTINGS_VERSION {
            return Err("同步设置版本高于当前应用支持的版本。".to_string());
        }
        self.version = CURRENT_SYNC_SETTINGS_VERSION;
        self.obsidian.vault_path = self.obsidian.vault_path.trim().to_string();
        self.obsidian.directory = normalize_relative_directory(&self.obsidian.directory)?;
        self.notion.parent_page_id = self.notion.parent_page_id.trim().to_string();
        self.feishu.app_id = self.feishu.app_id.trim().to_string();
        self.feishu.folder_token = self.feishu.folder_token.trim().to_string();
        if self.obsidian.vault_path.len() > MAX_VAULT_PATH_BYTES {
            return Err("Obsidian Vault 路径过长。".to_string());
        }
        if self.notion.parent_page_id.chars().count() > MAX_NOTION_PAGE_ID_CHARS {
            return Err("Notion 父页面 ID 过长。".to_string());
        }
        if self.feishu.app_id.chars().count() > MAX_FEISHU_APP_ID_CHARS {
            return Err("飞书 App ID 过长。".to_string());
        }
        if self.feishu.folder_token.chars().count() > MAX_FEISHU_FOLDER_TOKEN_CHARS {
            return Err("飞书文件夹 Token 过长。".to_string());
        }
        if self
            .feishu
            .app_id
            .chars()
            .any(|character| character.is_control())
            || self
                .feishu
                .folder_token
                .chars()
                .any(|character| character.is_control())
        {
            return Err("飞书配置包含不允许的控制字符。".to_string());
        }
        if self
            .notion
            .parent_page_id
            .chars()
            .any(|character| !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_')))
        {
            return Err("Notion 父页面 ID 包含不允许的字符。".to_string());
        }
        Ok(self)
    }
}

pub fn normalize_relative_directory(value: &str) -> Result<String, String> {
    let value = value.trim().replace('\\', "/");
    if value.chars().count() > MAX_RELATIVE_DIRECTORY_CHARS {
        return Err("同步目录名称过长。".to_string());
    }
    if value.is_empty() {
        return Ok(String::new());
    }
    let path = std::path::Path::new(&value);
    if path.is_absolute()
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || value.contains(':')
    {
        return Err("同步目录必须是 Vault 内的相对路径。".to_string());
    }
    if value.chars().any(|character| character.is_control()) {
        return Err("同步目录包含不允许的控制字符。".to_string());
    }
    Ok(value)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncItemResult {
    pub note_id: String,
    pub title: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub target: SyncTarget,
    pub attempted: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub items: Vec<SyncItemResult>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRequest {
    #[serde(default)]
    pub note_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{normalize_relative_directory, FeishuSettings, SyncSettings};

    #[test]
    fn normalizes_safe_relative_directories() {
        assert_eq!(
            normalize_relative_directory(" Notes\\Papers ").unwrap(),
            "Notes/Papers"
        );
        assert!(normalize_relative_directory("../outside").is_err());
        assert!(normalize_relative_directory("C:/outside").is_err());
    }

    #[test]
    fn old_settings_keep_their_selected_target_and_receive_field_defaults() {
        let settings: SyncSettings = serde_json::from_value(serde_json::json!({
            "version": 1,
            "enabled": true,
            "target": "obsidian"
        }))
        .unwrap();
        assert_eq!(settings.obsidian.directory, "Mnemora");
        assert_eq!(settings.target, super::SyncTarget::Obsidian);
        assert!(settings.include_annotations);

        let notion_settings: SyncSettings = serde_json::from_value(serde_json::json!({
            "version": 1,
            "target": "notion"
        }))
        .unwrap();
        assert_eq!(notion_settings.target, super::SyncTarget::Notion);
        assert_eq!(notion_settings.feishu, FeishuSettings::default());
    }

    #[test]
    fn settings_without_a_target_use_feishu() {
        let settings: SyncSettings = serde_json::from_value(serde_json::json!({
            "version": 1,
            "enabled": false
        }))
        .unwrap();
        assert_eq!(settings.target, super::SyncTarget::Feishu);
    }

    #[test]
    fn new_settings_prefer_feishu_without_enabling_background_sync() {
        let settings = SyncSettings::default();
        assert_eq!(settings.target, super::SyncTarget::Feishu);
        assert!(!settings.enabled);
        assert!(!settings.auto_sync);
    }
}
