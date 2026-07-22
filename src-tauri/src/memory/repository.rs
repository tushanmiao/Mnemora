use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{MemoryLayer, MemoryModification, MemoryOperation};

#[derive(Clone)]
pub struct MemoryRepository {
    root: PathBuf,
    operations: Arc<Mutex<()>>,
}

impl MemoryRepository {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            root: app_data_dir.join("memory").join("global"),
            operations: Arc::new(Mutex::new(())),
        }
    }

    pub fn read(&self, layer: MemoryLayer) -> Result<String, String> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| "Memory operation lock is unavailable".to_string())?;
        self.read_unlocked(layer)
    }

    pub fn directory(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Failed to create memory directory: {error}"))?;
        Ok(self.root.clone())
    }

    pub fn save(&self, layer: MemoryLayer, content: &str) -> Result<(), String> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| "Memory operation lock is unavailable".to_string())?;
        self.write_unlocked(layer, content)
    }

    pub fn clear(&self, layer: MemoryLayer) -> Result<(), String> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| "Memory operation lock is unavailable".to_string())?;
        self.write_unlocked(layer, "")
    }

    pub fn read_lines_with_limit(
        &self,
        layer: MemoryLayer,
        start_line: usize,
        end_line: usize,
        max_bytes: usize,
    ) -> Result<String, String> {
        let content = self.read(layer)?;
        let start = start_line.max(1);
        let end = end_line.max(start).min(start.saturating_add(1_999));
        let limit = max_bytes.clamp(1, 32_000);
        let mut output = String::new();
        for (index, line) in content.lines().enumerate() {
            if (index + 1) < start || (index + 1) > end {
                continue;
            }
            let formatted = format!("{:>6}: {line}\n", index + 1);
            if output.len() + formatted.len() <= limit {
                output.push_str(&formatted);
                continue;
            }
            let remaining = limit.saturating_sub(output.len());
            output.push_str(&truncate_utf8_bytes(&formatted, remaining));
            break;
        }
        Ok(output.trim_end_matches('\n').to_string())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<String, String> {
        let query = query.trim().to_lowercase();
        if query.is_empty() || query.chars().count() > 200 {
            return Err("Memory search query must be between 1 and 200 characters".to_string());
        }
        let terms = split_search_terms(&query);
        let content = self.read(MemoryLayer::L2)?;
        let sections = split_sections(&content);
        let mut scored = sections
            .into_iter()
            .enumerate()
            .filter_map(|(order, section)| {
                let title = section.title.to_lowercase();
                let body = section.body.to_lowercase();
                let mut score = if title.contains(&query) { 100 } else { 0 };
                for term in &terms {
                    if title.contains(term) {
                        score += 20;
                    }
                    if body.contains(term) {
                        score += 2;
                    }
                }
                (score > 0).then_some((score, order, section))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        let matches = scored
            .into_iter()
            .take(limit.clamp(1, 20))
            .map(|(_, _, section)| {
                let body = truncate_utf8_bytes(&section.body, 1_200);
                format!(
                    "[L2 lines {}-{}]\n{}\n{}",
                    section.start_line, section.end_line, section.title, body
                )
            })
            .collect::<Vec<_>>();
        Ok(if matches.is_empty() {
            "没有找到匹配的长期记忆。".to_string()
        } else {
            matches.join("\n\n")
        })
    }

    pub fn modify_for_model(&self, change: &MemoryModification) -> Result<String, String> {
        validate_model_memory_text(&change.target)?;
        validate_model_memory_text(&change.content)?;
        let _guard = self
            .operations
            .lock()
            .map_err(|_| "Memory operation lock is unavailable".to_string())?;
        let current = self.read_unlocked(change.layer)?;
        let next = match change.operation {
            MemoryOperation::Append => {
                let addition = change.content.trim();
                if addition.is_empty() {
                    return Err("Append content cannot be empty".to_string());
                }
                if current.trim().is_empty() {
                    format!("{addition}\n")
                } else {
                    format!("{}\n{}\n", current.trim_end(), addition)
                }
            }
            MemoryOperation::Replace | MemoryOperation::Remove => {
                if change.target.is_empty() {
                    return Err("Memory target cannot be empty".to_string());
                }
                if current.matches(&change.target).count() != 1 {
                    return Err("Memory target must match exactly once".to_string());
                }
                let replacement = if change.operation == MemoryOperation::Replace {
                    change.content.as_str()
                } else {
                    ""
                };
                current.replacen(&change.target, replacement, 1)
            }
        };
        self.write_unlocked(change.layer, &next)?;
        Ok(format!(
            "已更新 {}，当前占用 {} / {} bytes。",
            change.layer.file_name(),
            next.len(),
            change.layer.max_bytes()
        ))
    }

    fn read_unlocked(&self, layer: MemoryLayer) -> Result<String, String> {
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Failed to create memory directory: {error}"))?;
        let path = self.root.join(layer.file_name());
        if !path.exists() {
            fs::write(&path, [])
                .map_err(|error| format!("Failed to create memory file: {error}"))?;
            return Ok(String::new());
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Failed to inspect memory file: {error}"))?;
        if metadata.len() > layer.max_bytes() as u64 {
            return Err(format!("{} exceeds its size limit", layer.file_name()));
        }
        fs::read_to_string(path).map_err(|error| format!("Failed to read memory: {error}"))
    }

    fn write_unlocked(&self, layer: MemoryLayer, content: &str) -> Result<(), String> {
        if content.len() > layer.max_bytes() {
            return Err(format!(
                "{} cannot exceed {} bytes",
                layer.file_name(),
                layer.max_bytes()
            ));
        }
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Failed to create memory directory: {error}"))?;
        let destination = self.root.join(layer.file_name());
        let backup = self.root.join(format!(".{}.bak", layer.file_name()));
        let temporary = self.root.join(format!(
            ".{}.tmp-{}-{}",
            layer.file_name(),
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
            .map_err(|error| format!("Failed to create temporary memory file: {error}"))?;
        if let Err(error) = file
            .write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
        {
            let _ = fs::remove_file(&temporary);
            return Err(format!("Failed to write memory: {error}"));
        }
        drop(file);
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("Failed to remove stale memory backup: {error}"))?;
        }
        if destination.exists() {
            fs::rename(&destination, &backup)
                .map_err(|error| format!("Failed to back up memory file: {error}"))?;
        }
        if let Err(error) = fs::rename(&temporary, &destination) {
            let _ = fs::rename(&backup, &destination);
            let _ = fs::remove_file(&temporary);
            return Err(format!("Failed to replace memory file: {error}"));
        }
        let _ = fs::remove_file(backup);
        Ok(())
    }
}

fn split_search_terms(query: &str) -> Vec<String> {
    let base_terms = query
        .split(|character: char| {
            character.is_whitespace() || "，。！？；、,.;!?".contains(character)
        })
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut terms = if base_terms.is_empty() {
        vec![query.to_string()]
    } else {
        base_terms.clone()
    };
    for term in base_terms {
        let chars = term.chars().collect::<Vec<_>>();
        if chars.len() > 2 && chars.iter().all(|character| !character.is_ascii()) {
            for pair in chars.windows(2).take(12) {
                terms.push(pair.iter().collect());
            }
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

#[derive(Debug)]
struct MemorySection {
    title: String,
    body: String,
    start_line: usize,
    end_line: usize,
}

fn split_sections(content: &str) -> Vec<MemorySection> {
    let mut sections = Vec::new();
    let mut title = "L2 记忆".to_string();
    let mut body = String::new();
    let mut start_line = 1usize;
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if line.trim_start().starts_with('#') {
            if !body.trim().is_empty() {
                sections.push(MemorySection {
                    title,
                    body: body.trim().to_string(),
                    start_line,
                    end_line: line_number.saturating_sub(1).max(start_line),
                });
            }
            title = line.trim().trim_start_matches('#').trim().to_string();
            body.clear();
            start_line = line_number;
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !body.trim().is_empty() || sections.is_empty() {
        sections.push(MemorySection {
            title,
            body: body.trim().to_string(),
            start_line,
            end_line: content.lines().count().max(start_line),
        });
    }
    sections
}

fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    value
        .char_indices()
        .take_while(|(index, character)| index + character.len_utf8() <= max_bytes)
        .map(|(_, character)| character)
        .collect()
}

fn validate_model_memory_text(value: &str) -> Result<(), String> {
    let normalized = value.to_lowercase();
    let forbidden = [
        "api key",
        "api_key",
        "authorization:",
        "bearer ",
        "password",
        "密码",
        "私钥",
        "-----begin",
        "ignore previous",
        "ignore all previous",
        "忽略之前",
        "忽略以上",
        "system prompt",
        "系统提示词",
    ];
    if forbidden.iter().any(|needle| normalized.contains(needle)) {
        return Err("拒绝把凭据或疑似提示注入内容写入长期记忆。".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::MemoryRepository;
    use crate::memory::{MemoryLayer, MemoryModification, MemoryOperation};

    #[test]
    fn enforces_limits_and_supports_bounded_search() {
        let root = std::env::temp_dir().join(format!("mnemora-memory-{}", Uuid::new_v4()));
        let repository = MemoryRepository::new(root.clone());
        repository.save(MemoryLayer::L1, "偏好：简洁回答").unwrap();
        assert_eq!(repository.read(MemoryLayer::L1).unwrap(), "偏好：简洁回答");
        assert!(repository
            .save(MemoryLayer::L1, &"x".repeat(5_001))
            .is_err());
        repository
            .save(MemoryLayer::L2, "项目 Mnemora 使用 Rust。\n另一个事实。")
            .unwrap();
        assert!(repository
            .search("Mnemora Rust", 5)
            .unwrap()
            .contains("Mnemora"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_modification_requires_a_unique_target_and_rejects_secrets() {
        let root = std::env::temp_dir().join(format!("mnemora-memory-{}", Uuid::new_v4()));
        let repository = MemoryRepository::new(root.clone());
        repository.save(MemoryLayer::L1, "偏好：中文").unwrap();
        repository
            .modify_for_model(&MemoryModification {
                layer: MemoryLayer::L1,
                operation: MemoryOperation::Replace,
                target: "中文".to_string(),
                content: "简体中文".to_string(),
            })
            .unwrap();
        assert!(repository
            .read(MemoryLayer::L1)
            .unwrap()
            .contains("简体中文"));
        assert!(repository
            .modify_for_model(&MemoryModification {
                layer: MemoryLayer::L2,
                operation: MemoryOperation::Append,
                target: String::new(),
                content: "API Key: secret".to_string(),
            })
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clear_releases_the_operation_lock_and_removes_content() {
        let root = std::env::temp_dir().join(format!("mnemora-memory-{}", Uuid::new_v4()));
        let repository = MemoryRepository::new(root.clone());
        repository.save(MemoryLayer::L1, "temporary").unwrap();
        repository.clear(MemoryLayer::L1).unwrap();
        assert!(repository.read(MemoryLayer::L1).unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounds_both_layers_and_never_splits_utf8_when_reading_lines() {
        let root = std::env::temp_dir().join(format!("mnemora-memory-{}", Uuid::new_v4()));
        let repository = MemoryRepository::new(root.clone());
        assert!(repository.save(MemoryLayer::L1, &"x".repeat(5_000)).is_ok());
        assert!(repository
            .save(MemoryLayer::L1, &"x".repeat(5_001))
            .is_err());
        assert!(repository
            .save(MemoryLayer::L2, &"x".repeat(1024 * 1024))
            .is_ok());
        assert!(repository
            .save(MemoryLayer::L2, &"x".repeat(1024 * 1024 + 1))
            .is_err());

        repository
            .save(MemoryLayer::L2, "第一行\n第二行\n第三行")
            .unwrap();
        let result = repository
            .read_lines_with_limit(MemoryLayer::L2, 1, 3, 13)
            .unwrap();
        assert!(result.len() <= 13);
        assert!(result.is_char_boundary(result.len()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn serializes_concurrent_model_writes_and_cleans_temporary_files() {
        use std::thread;

        let root = std::env::temp_dir().join(format!("mnemora-memory-{}", Uuid::new_v4()));
        let repository = MemoryRepository::new(root.clone());
        let handles = (0..8)
            .map(|index| {
                let repository = repository.clone();
                thread::spawn(move || {
                    repository
                        .modify_for_model(&MemoryModification {
                            layer: MemoryLayer::L2,
                            operation: MemoryOperation::Append,
                            target: String::new(),
                            content: format!("entry-{index}"),
                        })
                        .unwrap();
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }
        let content = repository.read(MemoryLayer::L2).unwrap();
        for index in 0..8 {
            assert!(content.contains(&format!("entry-{index}")));
        }
        let files = fs::read_dir(root.join("memory").join("global")).unwrap();
        assert!(files.flatten().all(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            !name.contains(".tmp-") && !name.ends_with(".bak")
        }));
        let _ = fs::remove_dir_all(root);
    }
}
