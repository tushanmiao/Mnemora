//! 按月 JSONL 用量存储和逐行读取。

use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use super::types::UsageRecord;

const LEGACY_CURRENT: &str = "records.jsonl";
const LEGACY_PREVIOUS: &str = "records.previous.jsonl";

pub fn append_record(dir: &Path, record: &UsageRecord) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| format!("创建用量目录失败：{error}"))?;
    migrate_legacy_files(dir)?;
    let path = dir.join(format!("usage-{}.jsonl", month_key(record.created_at_ms)));
    let mut line =
        serde_json::to_vec(record).map_err(|error| format!("序列化用量记录失败：{error}"))?;
    line.push(b'\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(&line))
        .map_err(|error| format!("写入用量日志失败：{error}"))
}

pub fn visit_records(
    dir: &Path,
    since_ms: Option<u64>,
    until_ms: Option<u64>,
    mut visitor: impl FnMut(UsageRecord),
) -> usize {
    let mut skipped = 0usize;
    for path in candidate_paths(dir, since_ms, until_ms) {
        let Ok(file) = File::open(path) else {
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else {
                skipped = skipped.saturating_add(1);
                continue;
            };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<UsageRecord>(&line) {
                Ok(record) => visitor(record),
                Err(_) => skipped = skipped.saturating_add(1),
            }
        }
    }
    skipped
}

pub fn clear_records(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|error| format!("读取用量目录失败：{error}"))? {
        let entry = entry.map_err(|error| format!("读取用量文件失败：{error}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_file()
            && ((name.starts_with("usage-") && name.ends_with(".jsonl"))
                || matches!(name.as_str(), LEGACY_CURRENT | LEGACY_PREVIOUS))
        {
            fs::remove_file(entry.path()).map_err(|error| format!("清空用量日志失败：{error}"))?;
        }
    }
    let legacy = dir.join("legacy");
    if legacy.exists() {
        for name in [LEGACY_CURRENT, LEGACY_PREVIOUS] {
            let path = legacy.join(name);
            if path.exists() {
                fs::remove_file(path).map_err(|error| format!("清空旧版用量日志失败：{error}"))?;
            }
        }
    }
    Ok(())
}

fn migrate_legacy_files(dir: &Path) -> Result<(), String> {
    let legacy_dir = dir.join("legacy");
    for name in [LEGACY_CURRENT, LEGACY_PREVIOUS] {
        let source = dir.join(name);
        if !source.exists() {
            continue;
        }
        fs::create_dir_all(&legacy_dir)
            .map_err(|error| format!("创建旧版用量目录失败：{error}"))?;
        let destination = legacy_dir.join(name);
        if destination.exists() {
            continue;
        }
        fs::rename(&source, destination)
            .map_err(|error| format!("迁移旧版用量日志失败：{error}"))?;
    }
    Ok(())
}

fn candidate_paths(dir: &Path, since_ms: Option<u64>, until_ms: Option<u64>) -> Vec<PathBuf> {
    let mut paths = if let Some(since) = since_ms {
        let until = until_ms.unwrap_or_else(crate::usage::recorder::now_ms);
        month_keys_between(since, until.saturating_sub(1))
            .into_iter()
            .map(|month| dir.join(format!("usage-{month}.jsonl")))
            .collect::<Vec<_>>()
    } else {
        fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                (entry.path().is_file() && name.starts_with("usage-") && name.ends_with(".jsonl"))
                    .then_some(entry.path())
            })
            .collect::<Vec<_>>()
    };
    paths.sort();
    for name in [LEGACY_PREVIOUS, LEGACY_CURRENT] {
        let migrated = dir.join("legacy").join(name);
        let root_file = dir.join(name);
        // 迁移中断后可能同时存在两个副本；优先读取 legacy，避免重复统计。
        if migrated.exists() {
            paths.push(migrated);
        } else if root_file.exists() {
            paths.push(root_file);
        }
    }
    paths
}

fn month_keys_between(since_ms: u64, until_ms: u64) -> Vec<String> {
    let (mut year, mut month) = year_month(since_ms);
    let (end_year, end_month) = year_month(until_ms.max(since_ms));
    let mut keys = Vec::new();
    while (year, month) <= (end_year, end_month) && keys.len() < 1_200 {
        keys.push(format!("{year:04}-{month:02}"));
        if month == 12 {
            year += 1;
            month = 1;
        } else {
            month += 1;
        }
    }
    keys
}

fn month_key(timestamp_ms: u64) -> String {
    let (year, month) = year_month(timestamp_ms);
    format!("{year:04}-{month:02}")
}

/** 将 Unix 天数转换为公历年月，避免为日志文件名引入常驻日期库。 */
fn year_month(timestamp_ms: u64) -> (i32, u32) {
    let days = (timestamp_ms / 86_400_000) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = (year_of_era + era * 400) as i32;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += (month <= 2) as i32;
    (year, month as u32)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::{candidate_paths, month_key, month_keys_between, LEGACY_CURRENT};

    #[test]
    fn maps_epoch_and_known_timestamp_to_month_files() {
        assert_eq!(month_key(0), "1970-01");
        assert_eq!(month_key(1_783_440_000_000), "2026-07");
        assert_eq!(
            month_keys_between(1_782_892_800_000, 1_785_571_200_000),
            vec!["2026-07", "2026-08"]
        );
    }

    #[test]
    fn prefers_migrated_legacy_file_when_both_copies_exist() {
        let root = std::env::temp_dir().join(format!("mnemora-usage-paths-{}", Uuid::new_v4()));
        let legacy = root.join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(root.join(LEGACY_CURRENT), "root").unwrap();
        fs::write(legacy.join(LEGACY_CURRENT), "migrated").unwrap();

        let paths = candidate_paths(&root, None, None);
        assert!(paths.contains(&legacy.join(LEGACY_CURRENT)));
        assert!(!paths.contains(&root.join(LEGACY_CURRENT)));
        fs::remove_dir_all(root).unwrap();
    }
}
