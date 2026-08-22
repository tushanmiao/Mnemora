//! Bounded diagnostics for background-task panics and forced termination.
//!
//! Records contain task/run identifiers, the logical event tail supplied by the
//! caller, thread metadata, and a Rust backtrace. They deliberately exclude
//! prompts, attachment contents, API keys, and model responses.

use std::{
    backtrace::Backtrace,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use serde::Serialize;
use serde_json::Value;

const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

tokio::task_local! {
    static TASK_CONTEXT: TaskDiagnosticContext;
}

#[derive(Clone)]
pub struct TaskDiagnosticContext {
    pub task_kind: String,
    pub run_id: String,
    pub instance_id: String,
}

impl TaskDiagnosticContext {
    pub fn note_pipeline(
        task_kind: impl Into<String>,
        run_id: impl Into<String>,
        instance_id: impl Into<String>,
    ) -> Self {
        Self {
            task_kind: task_kind.into(),
            run_id: run_id.into(),
            instance_id: instance_id.into(),
        }
    }
}

pub fn current_task_instance_id() -> Option<String> {
    TASK_CONTEXT
        .try_with(|context| context.instance_id.clone())
        .ok()
}

pub async fn scope_task_diagnostics<F, T>(context: TaskDiagnosticContext, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TASK_CONTEXT.scope(context, future).await
}

#[derive(Clone)]
pub struct TaskDiagnosticLog {
    path: PathBuf,
    operations: Arc<Mutex<()>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskDiagnosticRecord {
    app_version: &'static str,
    occurred_at_ms: u64,
    outcome: String,
    task_kind: String,
    run_id: Option<String>,
    message: String,
    thread_name: Option<String>,
    thread_id: String,
    location: Option<String>,
    backtrace: String,
    metadata: Value,
}

impl TaskDiagnosticLog {
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            path: log_dir.join("task-diagnostics.jsonl"),
            operations: Arc::new(Mutex::new(())),
        }
    }

    pub fn install_panic_hook(&self) {
        if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        let logger = self.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|value| (*value).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "非字符串 panic payload".to_string());
            let location = info
                .location()
                .map(|value| format!("{}:{}:{}", value.file(), value.line(), value.column()));
            let context = TASK_CONTEXT.try_with(Clone::clone).ok();
            let _ = logger.record(
                "panic",
                context
                    .as_ref()
                    .map(|value| value.task_kind.as_str())
                    .unwrap_or("rust-thread"),
                context.as_ref().map(|value| value.run_id.as_str()),
                &message,
                location,
                serde_json::json!({
                    "panic": true,
                    "taskScoped": context.is_some(),
                    "instanceId": context.as_ref().map(|value| value.instance_id.clone()),
                }),
            );
            previous(info);
        }));
    }

    pub fn record_note_pipeline(
        &self,
        outcome: &str,
        task_kind: &str,
        run_id: &str,
        message: &str,
        metadata: Value,
    ) -> Result<String, String> {
        self.record(outcome, task_kind, Some(run_id), message, None, metadata)?;
        Ok(self.path.to_string_lossy().to_string())
    }

    pub fn path(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    fn record(
        &self,
        outcome: &str,
        task_kind: &str,
        run_id: Option<&str>,
        message: &str,
        location: Option<String>,
        metadata: Value,
    ) -> Result<(), String> {
        let _guard = self
            .operations
            .try_lock()
            .map_err(|_| "任务诊断日志正忙，未重复写入。".to_string())?;
        let thread = std::thread::current();
        let mut record = TaskDiagnosticRecord {
            app_version: env!("CARGO_PKG_VERSION"),
            occurred_at_ms: crate::usage::now_ms(),
            outcome: sanitize(outcome, 80),
            task_kind: sanitize(task_kind, 120),
            run_id: run_id.map(|value| sanitize(value, 160)),
            message: sanitize(message, 4_000),
            thread_name: thread.name().map(|value| sanitize(value, 160)),
            thread_id: format!("{:?}", thread.id()),
            location: location.map(|value| sanitize(&value, 500)),
            backtrace: truncate_utf8(&Backtrace::force_capture().to_string(), 24 * 1024),
            metadata: sanitize_metadata(metadata, 0),
        };
        let mut bytes =
            serde_json::to_vec(&record).map_err(|error| format!("序列化任务诊断失败：{error}"))?;
        if bytes.len() > MAX_RECORD_BYTES {
            record.backtrace = truncate_utf8(&record.backtrace, 8 * 1024);
            record.metadata = serde_json::json!({ "diagnosticTruncated": true });
            bytes = serde_json::to_vec(&record)
                .map_err(|error| format!("序列化截断任务诊断失败：{error}"))?;
        }
        if bytes.len() > MAX_RECORD_BYTES {
            return Err("任务诊断记录超过安全上限。".to_string());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建任务诊断目录失败：{error}"))?;
        }
        let current_size = fs::metadata(&self.path)
            .map(|value| value.len())
            .unwrap_or(0);
        if current_size.saturating_add(bytes.len() as u64 + 1) > MAX_FILE_BYTES {
            let rotated = self.path.with_file_name("task-diagnostics.1.jsonl");
            if rotated.exists() {
                fs::remove_file(&rotated)
                    .map_err(|error| format!("删除旧任务诊断失败：{error}"))?;
            }
            if self.path.exists() {
                fs::rename(&self.path, rotated)
                    .map_err(|error| format!("轮换任务诊断失败：{error}"))?;
            }
        }
        bytes.push(b'\n');
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut file| file.write_all(&bytes).and_then(|_| file.sync_data()))
            .map_err(|error| format!("写入任务诊断失败：{error}"))
    }
}

fn sanitize(value: &str, max_bytes: usize) -> String {
    let normalized = value.replace('\0', "");
    let lower = normalized.to_ascii_lowercase();
    if [
        "authorization:",
        "bearer ",
        "api_key",
        "api key",
        "password",
        "data:image",
        "base64,",
        "-----begin private key-----",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return "[已隐藏敏感诊断内容]".to_string();
    }
    truncate_utf8(normalized.trim(), max_bytes)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    value
        .char_indices()
        .take_while(|(index, character)| index + character.len_utf8() <= max_bytes)
        .map(|(_, character)| character)
        .collect()
}

fn sanitize_metadata(value: Value, depth: usize) -> Value {
    if depth >= 6 {
        return Value::String("[诊断层级已截断]".to_string());
    }
    match value {
        Value::String(value) => Value::String(sanitize(&value, 1_000)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .take(32)
                .map(|value| sanitize_metadata(value, depth + 1))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .take(64)
                .map(|(key, value)| (sanitize(&key, 120), sanitize_metadata(value, depth + 1)))
                .collect(),
        ),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::TaskDiagnosticLog;

    #[test]
    fn writes_bounded_redacted_task_diagnostic() {
        let root = std::env::temp_dir().join(format!("mnemora-task-log-{}", Uuid::new_v4()));
        let log = TaskDiagnosticLog::new(root.clone());
        log.record_note_pipeline(
            "forcedAbort",
            "deep-note-analysis",
            "run-1",
            "authorization: Bearer secret",
            serde_json::json!({ "phase": "cancelling" }),
        )
        .unwrap();
        let content = fs::read_to_string(root.join("task-diagnostics.jsonl")).unwrap();
        assert!(content.contains("forcedAbort"));
        assert!(content.contains("已隐藏敏感诊断内容"));
        assert!(!content.contains("secret"));
        fs::remove_dir_all(root).unwrap();
    }
}
