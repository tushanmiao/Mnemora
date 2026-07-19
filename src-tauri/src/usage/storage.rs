//! 有界 JSONL 用量存储。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use super::types::UsageRecord;

const CURRENT_FILE_NAME: &str = "records.jsonl";
const PREVIOUS_FILE_NAME: &str = "records.previous.jsonl";
const MAX_CURRENT_FILE_BYTES: u64 = 8 * 1024 * 1024;

pub fn append_record(dir: &Path, record: &UsageRecord) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| format!("创建用量目录失败：{error}"))?;
    let current = dir.join(CURRENT_FILE_NAME);
    if current.metadata().map(|value| value.len()).unwrap_or(0) >= MAX_CURRENT_FILE_BYTES {
        let previous = dir.join(PREVIOUS_FILE_NAME);
        if previous.exists() {
            fs::remove_file(&previous).map_err(|error| format!("删除旧用量日志失败：{error}"))?;
        }
        fs::rename(&current, previous).map_err(|error| format!("轮转用量日志失败：{error}"))?;
    }

    let mut line =
        serde_json::to_vec(record).map_err(|error| format!("序列化用量记录失败：{error}"))?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(current)
        .map_err(|error| format!("打开用量日志失败：{error}"))?;
    file.write_all(&line)
        .map_err(|error| format!("写入用量日志失败：{error}"))
}

pub fn read_records(dir: &Path) -> (Vec<UsageRecord>, usize) {
    let mut records = Vec::new();
    let mut skipped = 0usize;
    for file_name in [PREVIOUS_FILE_NAME, CURRENT_FILE_NAME] {
        let path = dir.join(file_name);
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            match serde_json::from_str::<UsageRecord>(line) {
                Ok(record) => records.push(record),
                Err(_) => skipped = skipped.saturating_add(1),
            }
        }
    }
    (records, skipped)
}

pub fn clear_records(dir: &Path) -> Result<(), String> {
    for file_name in [CURRENT_FILE_NAME, PREVIOUS_FILE_NAME] {
        let path = dir.join(file_name);
        if path.exists() {
            fs::remove_file(path).map_err(|error| format!("清空用量日志失败：{error}"))?;
        }
    }
    Ok(())
}
