//! 有界、脱敏的前端启动错误日志。
//!
//! 日志只在前端显式报告错误时写入，不启动后台线程，也不缓存消息、记忆或附件正文。

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};

const MAX_RECORD_BYTES: usize = 16 * 1024;
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupDiagnosticPayload {
    pub stage: String,
    #[serde(default)]
    pub context: Option<String>,
    pub name: String,
    pub message: String,
    #[serde(default)]
    pub stack: Option<String>,
    #[serde(default)]
    pub component_stack: Option<String>,
    pub occurred_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupDiagnosticRecord {
    app_version: &'static str,
    window_label: String,
    stage: String,
    context: Option<String>,
    name: String,
    message: String,
    stack: Option<String>,
    component_stack: Option<String>,
    occurred_at: String,
}

#[derive(Clone)]
pub struct StartupErrorLog {
    path: PathBuf,
    operations: Arc<Mutex<()>>,
}

impl StartupErrorLog {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            path: log_dir.join("startup-errors.jsonl"),
            operations: Arc::new(Mutex::new(())),
        }
    }

    pub fn record(
        &self,
        window_label: &str,
        payload: StartupDiagnosticPayload,
    ) -> Result<(), String> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| "Startup error log lock is unavailable".to_string())?;
        let mut record = StartupDiagnosticRecord {
            app_version: env!("CARGO_PKG_VERSION"),
            window_label: sanitize_text(window_label, 80),
            stage: sanitize_text(&payload.stage, 80),
            context: sanitize_optional(payload.context, 200),
            name: sanitize_text(&payload.name, 80),
            message: sanitize_text(&payload.message, 2_000),
            stack: sanitize_optional(payload.stack, 8_000),
            component_stack: sanitize_optional(payload.component_stack, 4_000),
            occurred_at: sanitize_text(&payload.occurred_at, 80),
        };
        let mut line = serialize_record(&record)?;
        if line.len().saturating_add(1) > MAX_RECORD_BYTES {
            record.component_stack = None;
            record.stack = record
                .stack
                .as_deref()
                .map(|value| truncate_utf8_bytes(value, 4_000));
            line = serialize_record(&record)?;
        }
        if line.len().saturating_add(1) > MAX_RECORD_BYTES {
            record.stack = None;
            record.message = truncate_utf8_bytes(&record.message, 1_000);
            line = serialize_record(&record)?;
        }
        if line.len().saturating_add(1) > MAX_RECORD_BYTES {
            return Err("Startup error record exceeds its size limit".to_string());
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create startup log directory: {error}"))?;
        }
        let current_bytes = fs::metadata(&self.path)
            .map(|value| value.len())
            .unwrap_or(0);
        if current_bytes.saturating_add(line.len() as u64 + 1) > MAX_FILE_BYTES {
            self.rotate()?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| format!("Failed to open startup error log: {error}"))?;
        line.push(b'\n');
        file.write_all(&line)
            .and_then(|_| file.sync_data())
            .map_err(|error| format!("Failed to write startup error log: {error}"))
    }

    fn rotate(&self) -> Result<(), String> {
        let rotated = self.path.with_file_name("startup-errors.1.jsonl");
        if rotated.exists() {
            fs::remove_file(&rotated)
                .map_err(|error| format!("Failed to remove rotated startup log: {error}"))?;
        }
        if self.path.exists() {
            fs::rename(&self.path, rotated)
                .map_err(|error| format!("Failed to rotate startup error log: {error}"))?;
        }
        Ok(())
    }
}

fn serialize_record(record: &StartupDiagnosticRecord) -> Result<Vec<u8>, String> {
    serde_json::to_vec(record)
        .map_err(|error| format!("Failed to serialize startup error record: {error}"))
}

fn sanitize_optional(value: Option<String>, max_bytes: usize) -> Option<String> {
    value
        .map(|value| sanitize_text(&value, max_bytes))
        .filter(|value| !value.is_empty())
}

fn sanitize_text(value: &str, max_bytes: usize) -> String {
    let normalized = value.replace('\0', "");
    let lower = normalized.to_ascii_lowercase();
    let sensitive_markers = [
        "api_key",
        "api key",
        "authorization:",
        "bearer ",
        "password",
        "password:",
        "data:image",
        "base64,",
        "<mnemora_memory_l1>",
        "\"messages\":",
        "\"attachments\":",
        "\"providerapikeys\":",
        "-----begin private key-----",
    ];
    if sensitive_markers
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return "[已隐藏敏感诊断内容]".to_string();
    }
    truncate_utf8_bytes(normalized.trim(), max_bytes)
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

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::{StartupDiagnosticPayload, StartupErrorLog, MAX_FILE_BYTES};

    fn payload(message: String) -> StartupDiagnosticPayload {
        StartupDiagnosticPayload {
            stage: "render-window".to_string(),
            context: Some("memory-settings".to_string()),
            name: "Error".to_string(),
            message,
            stack: Some("at App (src/App.tsx:1:1)".to_string()),
            component_stack: Some("at App".to_string()),
            occurred_at: "2026-07-22T00:00:00.000Z".to_string(),
        }
    }

    #[test]
    fn redacts_sensitive_values_and_bounds_each_record() {
        let root = std::env::temp_dir().join(format!("mnemora-startup-log-{}", Uuid::new_v4()));
        let log = StartupErrorLog::new(root.clone());
        log.record(
            "main",
            payload(format!(
                "authorization: Bearer secret {}",
                "x".repeat(40_000)
            )),
        )
        .unwrap();
        let content = fs::read_to_string(root.join("startup-errors.jsonl")).unwrap();
        assert!(content.contains("已隐藏敏感诊断内容"));
        assert!(!content.contains("secret"));
        assert!(content.len() <= 16 * 1024);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rotates_to_one_previous_file() {
        let root = std::env::temp_dir().join(format!("mnemora-startup-log-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("startup-errors.jsonl"),
            vec![b'x'; MAX_FILE_BYTES as usize],
        )
        .unwrap();
        let log = StartupErrorLog::new(root.clone());
        log.record("main", payload("render failed".to_string()))
            .unwrap();
        assert!(root.join("startup-errors.1.jsonl").exists());
        assert!(root.join("startup-errors.jsonl").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
