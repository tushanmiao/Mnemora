mod process_tree;

use std::{fs, path::Path};

use serde_json::Value;
use tauri::{plugin::TauriPlugin, Runtime};

pub use process_tree::MemoryProcessTreeSample;

const MAX_REPORT_BYTES: usize = 2 * 1024 * 1024;

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("memory-diagnostics")
        .invoke_handler(tauri::generate_handler![
            memory_diagnostics_sample,
            memory_diagnostics_export,
        ])
        .build()
}

#[tauri::command]
fn memory_diagnostics_sample() -> Result<MemoryProcessTreeSample, String> {
    process_tree::sample_current_process_tree()
}

#[tauri::command]
fn memory_diagnostics_export(path: String, report: Value) -> Result<(), String> {
    let path = Path::new(&path);
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err("Memory diagnostics reports must use the .json extension.".to_string());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Memory diagnostics report path has no parent directory.".to_string())?;
    if !parent.is_dir() {
        return Err("Memory diagnostics report directory does not exist.".to_string());
    }
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("Failed to serialize memory diagnostics report: {error}"))?;
    if bytes.len() > MAX_REPORT_BYTES {
        return Err("Memory diagnostics report exceeds the 2 MB limit.".to_string());
    }
    fs::write(path, bytes)
        .map_err(|error| format!("Failed to write memory diagnostics report: {error}"))
}
