//! 英语词库的按需下载和本地查询命令。

use tauri::State;

use crate::{
    english::{
        download_source,
        types::{EnglishDictionaryStatus, EnglishSearchResult, EnglishWordEntry},
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
) -> Result<EnglishDictionaryStatus, String> {
    let _guard = state.english_operations.lock().await;
    let payload = download_source(&state.http).await?;
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.install_payload(payload))
        .await
        .map_err(|error| format!("安装英语词库失败：{error}"))?
}

#[tauri::command]
pub async fn english_dictionary_search(
    state: State<'_, AppState>,
    query: String,
    group_id: Option<u32>,
    limit: Option<usize>,
) -> Result<EnglishSearchResult, String> {
    let _guard = state.english_operations.lock().await;
    let repository = state.english_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.search(&query, group_id, limit.unwrap_or(40))
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
