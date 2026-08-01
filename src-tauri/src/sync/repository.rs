//! 同步设置的轻量 JSON 仓库，不把同步配置混入基础外观设置。

use std::{fs, path::PathBuf};

use super::{mapping::replace_file, types::SyncSettings};

const SETTINGS_FILE_NAME: &str = "sync-settings.json";

#[derive(Clone)]
pub struct SyncSettingsRepository {
    path: PathBuf,
}

impl SyncSettingsRepository {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            path: config_dir.join(SETTINGS_FILE_NAME),
        }
    }

    pub fn load(&self) -> Result<SyncSettings, String> {
        if !self.path.exists() {
            return Ok(SyncSettings::default());
        }
        let raw =
            fs::read_to_string(&self.path).map_err(|error| format!("读取同步设置失败：{error}"))?;
        serde_json::from_str::<SyncSettings>(&raw)
            .map_err(|error| format!("解析同步设置失败：{error}"))?
            .normalize_and_validate()
    }

    pub fn save(&self, settings: &SyncSettings) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "同步设置路径无效。".to_string())?;
        fs::create_dir_all(parent).map_err(|error| format!("创建同步设置目录失败：{error}"))?;
        let temporary = self
            .path
            .with_extension(format!("json.tmp-{}", std::process::id()));
        let mut persisted = settings.clone();
        // `has_token` 是运行时脱敏状态，凭据本体及其状态都不写入普通 JSON。
        persisted.notion.has_token = false;
        persisted.feishu.has_app_secret = false;
        let json = serde_json::to_vec_pretty(&persisted)
            .map_err(|error| format!("序列化同步设置失败：{error}"))?;
        fs::write(&temporary, json).map_err(|error| format!("写入同步设置失败：{error}"))?;
        replace_file(&temporary, &self.path, "同步设置")
    }
}
