//! 英语词库的按需下载和本地查询命令。

use std::time::{Duration, Instant};

use tauri::{ipc::Channel, State};

use crate::{
    english::{
        download_source_with_progress,
        types::{
            EnglishDictionaryStatus, EnglishDownloadProgress, EnglishSearchResult, EnglishWordEntry,
        },
    },
    state::AppState,
};

#[tauri::command]
pub async fn english_dictionary_status(
    state: State<'_, AppState>,
) -> Result<EnglishDictionaryStatus, String> {
    let _guard = state.english_operations.lock().await;
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.status())
        .await
        .map_err(|error| format!("读取英语词库状态失败：{error}"))?
}

/// 只在用户点击下载时访问来源站点；不会在启动或进入 Chat 时执行。
#[tauri::command]
pub async fn english_dictionary_download(
    state: State<'_, AppState>,
    on_progress: Channel<EnglishDownloadProgress>,
) -> Result<EnglishDictionaryStatus, String> {
    let _guard = state.english_operations.lock().await;
    let progress_channel = on_progress.clone();
    let mut last_download_progress = Instant::now() - Duration::from_millis(100);
    let bundled_backup = state.english_repository.bundled_backup_path();
    let payload = download_source_with_progress(
        &state.http,
        &bundled_backup,
        move |phase, downloaded, total| {
            let finished = total.is_some_and(|value| downloaded >= value);
            if !finished && last_download_progress.elapsed() < Duration::from_millis(100) {
                return;
            }
            last_download_progress = Instant::now();
            let progress = total
                .map(|value| (((downloaded as f64 / value.max(1) as f64) * 70.0) as u8).min(70));
            let _ = progress_channel.send(EnglishDownloadProgress {
                phase: phase.to_string(),
                downloaded_bytes: downloaded,
                total_bytes: total,
                indexed_words: 0,
                total_words: 0,
                progress,
                finished: false,
            });
        },
    )
    .await?;
    let _ = on_progress.send(EnglishDownloadProgress {
        phase: "decode".to_string(),
        downloaded_bytes: payload.len() as u64,
        total_bytes: Some(payload.len() as u64),
        indexed_words: 0,
        total_words: 0,
        progress: Some(70),
        finished: false,
    });
    let repository = state.english_repository.clone();
    let progress_channel = on_progress.clone();
    let mut last_index_progress = Instant::now() - Duration::from_millis(100);
    tauri::async_runtime::spawn_blocking(move || {
        repository.install_payload_with_progress(payload, move |indexed, total| {
            if indexed < total && last_index_progress.elapsed() < Duration::from_millis(100) {
                return;
            }
            last_index_progress = Instant::now();
            let progress = if total == 0 {
                Some(100)
            } else {
                Some(70 + ((indexed as f64 / total as f64) * 29.0) as u8)
            };
            let _ = progress_channel.send(EnglishDownloadProgress {
                phase: "index".to_string(),
                downloaded_bytes: 0,
                total_bytes: None,
                indexed_words: indexed,
                total_words: total,
                progress,
                finished: false,
            });
        })
    })
    .await
    .map_err(|error| format!("安装英语词库失败：{error}"))??;
    let _ = on_progress.send(EnglishDownloadProgress {
        phase: "complete".to_string(),
        downloaded_bytes: 0,
        total_bytes: None,
        indexed_words: 0,
        total_words: 0,
        progress: Some(100),
        finished: true,
    });
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.status())
        .await
        .map_err(|error| format!("读取英语词库状态失败：{error}"))?
}

#[tauri::command]
pub async fn english_dictionary_search(
    state: State<'_, AppState>,
    query: String,
    group_id: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<EnglishSearchResult, String> {
    let _guard = state.english_operations.lock().await;
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.search(&query, group_id, limit.unwrap_or(20), offset.unwrap_or(0))
    })
    .await
    .map_err(|error| format!("搜索英语词库失败：{error}"))?
}

#[tauri::command]
pub async fn english_dictionary_get(
    state: State<'_, AppState>,
    word_id: u32,
) -> Result<EnglishWordEntry, String> {
    let _guard = state.english_operations.lock().await;
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.get_entry(word_id))
        .await
        .map_err(|error| format!("读取英语单词失败：{error}"))?
}

#[tauri::command]
pub async fn english_dictionary_delete(state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.english_operations.lock().await;
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.delete())
        .await
        .map_err(|error| format!("删除英语词库失败：{error}"))?
}

/// 释放内存中的英语索引；不会删除用户已经下载的本地词库文件。
#[tauri::command]
pub async fn english_dictionary_release(state: State<'_, AppState>) -> Result<(), String> {
    let _guard = state.english_operations.lock().await;
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.clear_cache())
        .await
        .map_err(|error| format!("释放英语词库缓存失败：{error}"))?;
    Ok(())
}
