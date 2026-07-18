//! 非敏感模型配置仓库。
//!
//! - `load`：读取 `model-settings.json`，反序列化后执行统一校验和迁移入口。
//! - `save`：序列化版本化设置，通过临时文件和备份替换降低写入中断风险。
//! - API Key 不属于此仓库，由同级 `secrets` 模块交给系统凭据存储。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::types::ModelSettings;

const SETTINGS_FILE_NAME: &str = "model-settings.json";

#[derive(Clone)]
pub struct ModelSettingsRepository {
    path: PathBuf,
}

impl ModelSettingsRepository {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            path: config_dir.join(SETTINGS_FILE_NAME),
        }
    }

    pub fn load(&self) -> Result<ModelSettings, String> {
        if !self.path.exists() {
            return Ok(ModelSettings::default());
        }

        let raw = fs::read_to_string(&self.path)
            .map_err(|error| format!("Failed to read model settings: {error}"))?;
        let settings: ModelSettings = serde_json::from_str(&raw)
            .map_err(|error| format!("Failed to parse model settings: {error}"))?;
        settings.normalize_and_validate()
    }

    pub fn save(&self, settings: &ModelSettings) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "Model settings path has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create model settings directory: {error}"))?;

        let json = serde_json::to_vec_pretty(settings)
            .map_err(|error| format!("Failed to serialize model settings: {error}"))?;
        let temporary_path = temporary_path(&self.path);
        let backup_path = self.path.with_extension("json.bak");

        let mut temporary_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| format!("Failed to create temporary settings file: {error}"))?;
        temporary_file
            .write_all(&json)
            .and_then(|_| temporary_file.sync_all())
            .map_err(|error| format!("Failed to write model settings: {error}"))?;
        drop(temporary_file);

        replace_file(&temporary_path, &self.path, &backup_path)
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.path
    }
}

fn temporary_path(settings_path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    settings_path.with_extension(format!("json.tmp-{}-{nonce}", std::process::id()))
}

fn replace_file(temporary: &Path, destination: &Path, backup: &Path) -> Result<(), String> {
    if !destination.exists() {
        return fs::rename(temporary, destination)
            .map_err(|error| format!("Failed to install model settings file: {error}"));
    }

    if backup.exists() {
        fs::remove_file(backup)
            .map_err(|error| format!("Failed to remove stale settings backup: {error}"))?;
    }
    fs::rename(destination, backup)
        .map_err(|error| format!("Failed to back up model settings: {error}"))?;

    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::rename(backup, destination);
        let _ = fs::remove_file(temporary);
        return Err(format!("Failed to replace model settings: {error}"));
    }

    let _ = fs::remove_file(backup);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::ModelSettingsRepository;
    use crate::settings::types::ModelSettings;

    fn test_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mnemora-settings-test-{}-{}-{name}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_file_loads_defaults() {
        let directory = test_directory("missing");
        let repository = ModelSettingsRepository::new(directory.clone());
        let settings = repository.load().unwrap();
        assert_eq!(settings.providers.len(), 3);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn saves_and_loads_versioned_settings_without_api_key_value() {
        let directory = test_directory("roundtrip");
        let repository = ModelSettingsRepository::new(directory.clone());
        let mut settings = ModelSettings::default();
        settings.providers[0].has_api_key = true;

        repository.save(&settings).unwrap();
        let raw = fs::read_to_string(repository.path()).unwrap();
        assert!(!raw.contains("secret-key"));
        assert!(!raw.contains("\"apiKey\""));
        assert!(raw.contains("\"version\": 1"));
        assert!(raw.contains("openAiResponses"));
        let loaded = repository.load().unwrap();
        assert!(!loaded.providers[0].has_api_key);
        settings.providers[0].has_api_key = false;
        assert_eq!(loaded, settings);

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_corrupt_settings_file() {
        let directory = test_directory("corrupt");
        let repository = ModelSettingsRepository::new(directory.clone());
        fs::create_dir_all(&directory).unwrap();
        fs::write(repository.path(), b"not-json").unwrap();

        assert!(repository.load().unwrap_err().contains("Failed to parse"));
        let _ = fs::remove_dir_all(directory);
    }
}
