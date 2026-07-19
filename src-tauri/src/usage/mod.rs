//! 本地模型用量模块。
//!
//! - `recorder`：把一次逻辑模型调用转换为统一记录。
//! - `storage`：JSONL 追加、读取、轮转和清空。
//! - `stats`：总览、趋势、供应商和模型聚合。
//! - `types`：Tauri 命令和内部记录共用的数据结构。
//!
//! 用量历史不进入数据库，也不常驻内存。聊天服务只在一次请求最终结束时记录一行，
//! 查询与磁盘写入都放到阻塞线程执行，避免占用异步网络运行时。

mod recorder;
mod stats;
mod storage;
mod types;

use std::path::{Path, PathBuf};

use tauri::State;

pub use recorder::{now_ms, record_model_call};
pub use types::UsageRecordInput;

use crate::state::AppState;
use types::{UsageStatsQuery, UsageStatsResponse};

#[tauri::command]
pub async fn usage_get_stats(
    state: State<'_, AppState>,
    query: Option<UsageStatsQuery>,
) -> Result<UsageStatsResponse, String> {
    let query = query.unwrap_or_default();
    let _guard = state.usage_operations.lock().await;
    let usage_dir = state.usage_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (records, skipped_records) = storage::read_records(&usage_dir);
        Ok(stats::build_response(records, skipped_records, query))
    })
    .await
    .map_err(|error| format!("用量统计后台任务失败：{error}"))?
}

#[tauri::command]
pub async fn usage_clear(state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.usage_operations.lock().await;
    let usage_dir = state.usage_dir.clone();
    tauri::async_runtime::spawn_blocking(move || storage::clear_records(&usage_dir))
        .await
        .map_err(|error| format!("清空用量记录后台任务失败：{error}"))?
}

pub fn usage_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("usage")
}
