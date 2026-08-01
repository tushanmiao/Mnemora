//! 同步目标映射与内容哈希。映射文件只保存远端 ID/相对路径，不保存凭据或正文。

use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::types::SyncTarget;

const MAPPING_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncMapping {
    pub target: SyncTarget,
    pub note_id: String,
    pub remote_id: String,
    pub content_hash: String,
    pub synced_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncMappingStore {
    #[serde(default = "mapping_version")]
    version: u32,
    #[serde(default)]
    records: BTreeMap<String, SyncMapping>,
}

fn mapping_version() -> u32 {
    MAPPING_VERSION
}

#[derive(Clone)]
pub struct SyncMappingRepository {
    path: PathBuf,
}

impl SyncMappingRepository {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            path: app_data_dir.join("sync").join("mappings.json"),
        }
    }

    pub fn load_store(&self) -> Result<SyncMappingStore, String> {
        self.load()
    }

    pub fn save_store(&self, store: &SyncMappingStore) -> Result<(), String> {
        self.write(store)
    }

    fn load(&self) -> Result<SyncMappingStore, String> {
        if !self.path.exists() {
            return Ok(SyncMappingStore {
                version: MAPPING_VERSION,
                records: BTreeMap::new(),
            });
        }
        let raw =
            fs::read_to_string(&self.path).map_err(|error| format!("读取同步映射失败：{error}"))?;
        let file: SyncMappingStore =
            serde_json::from_str(&raw).map_err(|error| format!("解析同步映射失败：{error}"))?;
        if file.version > MAPPING_VERSION {
            return Err("同步映射版本高于当前应用支持的版本。".to_string());
        }
        Ok(file)
    }

    fn write(&self, file: &SyncMappingStore) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "同步映射路径无效。".to_string())?;
        fs::create_dir_all(parent).map_err(|error| format!("创建同步映射目录失败：{error}"))?;
        let temporary = self
            .path
            .with_extension(format!("json.tmp-{}", std::process::id()));
        let json = serde_json::to_vec_pretty(file)
            .map_err(|error| format!("序列化同步映射失败：{error}"))?;
        fs::write(&temporary, json).map_err(|error| format!("写入同步映射失败：{error}"))?;
        replace_file(&temporary, &self.path, "同步映射")
    }
}

impl SyncMappingStore {
    pub fn get(&self, target: SyncTarget, note_id: &str) -> Option<&SyncMapping> {
        self.records.get(&mapping_key(target, note_id))
    }

    pub fn insert(&mut self, mapping: SyncMapping) {
        self.records
            .insert(mapping_key(mapping.target, &mapping.note_id), mapping);
    }
}

pub fn content_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn replace_file(
    temporary: &std::path::Path,
    target: &std::path::Path,
    label: &str,
) -> Result<(), String> {
    let file_name = target
        .file_name()
        .ok_or_else(|| format!("{label}路径无效。"))?
        .to_string_lossy();
    let backup = target.with_file_name(format!(
        ".{file_name}.mnemora-backup-{}",
        std::process::id()
    ));
    if target.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(target, &backup).map_err(|error| format!("备份{label}失败：{error}"))?;
    }
    if let Err(error) = fs::rename(temporary, target) {
        let _ = fs::rename(&backup, target);
        let _ = fs::remove_file(temporary);
        return Err(format!("替换{label}失败：{error}"));
    }
    let _ = fs::remove_file(backup);
    Ok(())
}

fn mapping_key(target: SyncTarget, note_id: &str) -> String {
    format!(
        "{}:{note_id}",
        match target {
            SyncTarget::Feishu => "feishu",
            SyncTarget::Obsidian => "obsidian",
            SyncTarget::Notion => "notion",
        }
    )
}

#[cfg(test)]
mod tests {
    use super::content_hash;

    #[test]
    fn content_hash_is_stable() {
        assert_eq!(content_hash("note"), content_hash("note"));
        assert_ne!(content_hash("note"), content_hash("note 2"));
    }
}
