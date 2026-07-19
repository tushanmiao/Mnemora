//! 基础设置 JSON 仓库。
//!
//! 文件只包含非敏感应用设置，使用临时文件加备份替换；API Key 永远由 `secrets` 模块管理。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::app_types::AppSettings;

const SETTINGS_FILE_NAME: &str = "app-settings.json";

#[derive(Clone)]
pub struct AppSettingsRepository {
    path: PathBuf,
}

impl AppSettingsRepository {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            path: config_dir.join(SETTINGS_FILE_NAME),
        }
    }

    pub fn load(&self) -> Result<AppSettings, String> {
        if !self.path.exists() {
            return Ok(AppSettings::default());
        }
        let raw = fs::read_to_string(&self.path)
            .map_err(|error| format!("Failed to read app settings: {error}"))?;
        let settings: AppSettings = serde_json::from_str(&raw)
            .map_err(|error| format!("Failed to parse app settings: {error}"))?;
        settings.normalize_and_validate()
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "App settings path has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create app settings directory: {error}"))?;
        let json = serde_json::to_vec_pretty(settings)
            .map_err(|error| format!("Failed to serialize app settings: {error}"))?;
        let temporary = self.path.with_extension(format!(
            "json.tmp-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let backup = self.path.with_extension("json.bak");
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Failed to create temporary app settings file: {error}"))?;
        file.write_all(&json)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("Failed to write app settings: {error}"))?;
        drop(file);

        if self.path.exists() {
            if backup.exists() {
                fs::remove_file(&backup).map_err(|error| {
                    format!("Failed to remove stale app settings backup: {error}")
                })?;
            }
            fs::rename(&self.path, &backup)
                .map_err(|error| format!("Failed to back up app settings: {error}"))?;
        }
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::rename(&backup, &self.path);
            let _ = fs::remove_file(&temporary);
            return Err(format!("Failed to replace app settings: {error}"));
        }
        let _ = fs::remove_file(backup);
        Ok(())
    }
}
