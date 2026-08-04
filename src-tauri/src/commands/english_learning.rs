//! 英语单词计划、训练、FSRS 复习和统计命令。

use std::fs;

use reqwest::Client;
use tauri::State;

use crate::{
    english::learning::types::{
        EnglishArchivedItem, EnglishAttemptHistoryPage, EnglishAttemptResult,
        EnglishAudioCacheStatus, EnglishCachedAudio, EnglishCreatePlanInput,
        EnglishLearningOverview, EnglishLearningStats, EnglishNextBatchInput, EnglishPlanSummary,
        EnglishQueueItem, EnglishSubmitAttemptInput, EnglishUpdatePlanInput,
    },
    english::learning::EnglishLearningRepository,
    state::AppState,
};

const MAX_AUDIO_DOWNLOAD_BYTES: u64 = 15 * 1024 * 1024;

#[tauri::command]
pub async fn english_learning_overview(
    state: State<'_, AppState>,
) -> Result<EnglishLearningOverview, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.overview())
        .await
        .map_err(|error| format!("读取英语学习概览失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_create_plan(
    state: State<'_, AppState>,
    input: EnglishCreatePlanInput,
) -> Result<EnglishPlanSummary, String> {
    let _guard = state.english_learning_operations.lock().await;
    let input = input.validate()?;
    let group_ids = input.group_ids.clone();
    let dictionary = state.english_repository.clone();
    let snapshots = tauri::async_runtime::spawn_blocking(move || {
        dictionary.learning_snapshots_for_groups(&group_ids)
    })
    .await
    .map_err(|error| format!("读取英语词书内容失败：{error}"))??;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.create_plan(input, snapshots))
        .await
        .map_err(|error| format!("创建英语学习计划失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_update_plan(
    state: State<'_, AppState>,
    input: EnglishUpdatePlanInput,
) -> Result<EnglishPlanSummary, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.update_plan(input))
        .await
        .map_err(|error| format!("更新英语学习计划失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_add_word(
    state: State<'_, AppState>,
    word_id: u32,
) -> Result<EnglishLearningOverview, String> {
    let _guard = state.english_learning_operations.lock().await;
    let dictionary = state.english_repository.clone();
    let entry = tauri::async_runtime::spawn_blocking(move || dictionary.get_entry(word_id))
        .await
        .map_err(|error| format!("读取英语单词失败：{error}"))??;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.add_word(&entry))
        .await
        .map_err(|error| format!("加入英语学习计划失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_pause_plan(
    state: State<'_, AppState>,
    plan_id: String,
    paused: bool,
) -> Result<Option<EnglishPlanSummary>, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.pause_plan(&plan_id, paused))
        .await
        .map_err(|error| format!("切换英语学习计划失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_next_batch(
    state: State<'_, AppState>,
    input: EnglishNextBatchInput,
) -> Result<Vec<EnglishQueueItem>, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.next_batch(input))
        .await
        .map_err(|error| format!("读取英语学习队列失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_get_item(
    state: State<'_, AppState>,
    progress_id: String,
) -> Result<EnglishQueueItem, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.get_item(&progress_id))
        .await
        .map_err(|error| format!("读取英语学习项失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_submit_attempt(
    state: State<'_, AppState>,
    input: EnglishSubmitAttemptInput,
) -> Result<EnglishAttemptResult, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.submit_attempt(input))
        .await
        .map_err(|error| format!("保存英语答题记录失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_mark_mastered(
    state: State<'_, AppState>,
    progress_id: String,
) -> Result<EnglishLearningOverview, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.mark_mastered(&progress_id))
        .await
        .map_err(|error| format!("标记已掌握失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_archive_item(
    state: State<'_, AppState>,
    progress_id: String,
) -> Result<EnglishLearningOverview, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.archive_item(&progress_id))
        .await
        .map_err(|error| format!("归档英语学习项失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_restore_item(
    state: State<'_, AppState>,
    progress_id: String,
) -> Result<EnglishLearningOverview, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.restore_item(&progress_id))
        .await
        .map_err(|error| format!("恢复英语学习项失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_list_archived(
    state: State<'_, AppState>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<EnglishArchivedItem>, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.list_archived(limit.unwrap_or(20), offset.unwrap_or(0))
    })
    .await
    .map_err(|error| format!("读取归档单词失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_stats(
    state: State<'_, AppState>,
) -> Result<EnglishLearningStats, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.stats())
        .await
        .map_err(|error| format!("读取英语学习统计失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_list_history(
    state: State<'_, AppState>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<EnglishAttemptHistoryPage, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        repository.list_history(limit.unwrap_or(20), offset.unwrap_or(0))
    })
    .await
    .map_err(|error| format!("读取英语答题历史失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_export_book(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let contents = repository.export_active_book()?;
        fs::write(&path, contents).map_err(|error| format!("导出英语词书失败：{error}"))
    })
    .await
    .map_err(|error| format!("导出英语词书任务失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_import_book(
    state: State<'_, AppState>,
    path: String,
) -> Result<EnglishPlanSummary, String> {
    let _guard = state.english_learning_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let metadata = fs::metadata(&path).map_err(|error| format!("读取英语词书失败：{error}"))?;
        if metadata.len() > 25 * 1024 * 1024 {
            return Err("英语词书文件不能超过 25 MB。".to_string());
        }
        let contents =
            fs::read_to_string(&path).map_err(|error| format!("读取英语词书内容失败：{error}"))?;
        repository.import_portable_book(&contents)
    })
    .await
    .map_err(|error| format!("导入英语词书任务失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_cache_audio(
    state: State<'_, AppState>,
    url: String,
) -> Result<EnglishCachedAudio, String> {
    let _guard = state.english_audio_operations.lock().await;
    cache_audio(&state.http, &state.english_learning_repository, &url).await
}

#[tauri::command]
pub async fn english_learning_audio_cache_status(
    state: State<'_, AppState>,
) -> Result<EnglishAudioCacheStatus, String> {
    let _guard = state.english_audio_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.audio_cache_status())
        .await
        .map_err(|error| format!("读取英语音频缓存状态失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_clear_audio_cache(
    state: State<'_, AppState>,
) -> Result<EnglishAudioCacheStatus, String> {
    let _guard = state.english_audio_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.clear_audio_cache())
        .await
        .map_err(|error| format!("清理英语音频缓存任务失败：{error}"))?
}

#[tauri::command]
pub async fn english_learning_prefetch_audio(
    state: State<'_, AppState>,
) -> Result<EnglishAudioCacheStatus, String> {
    let _guard = state.english_audio_operations.lock().await;
    let repository = state.english_learning_repository.clone();
    let urls = tauri::async_runtime::spawn_blocking({
        let repository = repository.clone();
        move || repository.prefetch_audio_urls()
    })
    .await
    .map_err(|error| format!("读取英语音频预下载任务失败：{error}"))??;
    for url in urls {
        let _ = cache_audio(&state.http, &repository, &url).await;
    }
    tauri::async_runtime::spawn_blocking(move || repository.audio_cache_status())
        .await
        .map_err(|error| format!("读取英语音频缓存状态失败：{error}"))?
}

async fn cache_audio(
    client: &Client,
    repository: &EnglishLearningRepository,
    url: &str,
) -> Result<EnglishCachedAudio, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "英语音频地址无效。".to_string())?;
    if parsed.scheme() != "https" {
        return Err("英语音频缓存只允许 HTTPS 地址。".to_string());
    }
    if let Some(cached) = repository.cached_audio(url) {
        return Ok(cached);
    }
    let (max_bytes, _) = repository.audio_cache_settings()?;
    if max_bytes == 0 {
        return Ok(EnglishCachedAudio {
            path: url.to_string(),
            cached: false,
        });
    }
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|error| format!("下载英语音频失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("下载英语音频失败：{error}"))?;
    if response.content_length().unwrap_or_default() > MAX_AUDIO_DOWNLOAD_BYTES {
        return Err("英语音频文件超过 15 MB。".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取英语音频失败：{error}"))?;
    if bytes.len() as u64 > MAX_AUDIO_DOWNLOAD_BYTES {
        return Err("英语音频文件超过 15 MB。".to_string());
    }
    let repository = repository.clone();
    let url = url.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        repository.store_cached_audio(&url, &bytes, max_bytes)
    })
    .await
    .map_err(|error| format!("保存英语音频缓存任务失败：{error}"))?
}
