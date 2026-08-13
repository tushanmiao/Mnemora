use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{ipc::Channel, AppHandle, State};
use tauri_plugin_updater::UpdaterExt;

use crate::{app_update, state::AppState};

#[tauri::command]
pub async fn check_application_update(
    state: State<'_, AppState>,
) -> Result<app_update::UpdateCheckResult, String> {
    let _operation_guard = state.update_operations.lock().await;
    let proxy_settings = state
        .app_settings
        .read()
        .map_err(|_| "应用设置暂时不可用。".to_string())?
        .update_proxy
        .clone();
    let client = app_update::build_update_client(&proxy_settings)?;
    let cancellation = state.start_update_check().await;
    let check = app_update::check_latest_release(&client);
    let result = tokio::select! {
        result = check => result,
        _ = cancellation.cancelled() => Err("更新检查已取消。".to_string()),
    };
    state.finish_update_check().await;
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedUpdateInfo {
    current_version: String,
    version: String,
    date: String,
    body: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadProgress {
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    finished: bool,
}

/** 使用 Tauri Updater 读取并验证 latest.json，安装包本身在用户确认后才下载。 */
#[tauri::command]
pub async fn check_signed_application_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<SignedUpdateInfo>, String> {
    let _operation_guard = state.update_operations.lock().await;
    state.discard_pending_signed_update().await;
    let proxy_settings = state
        .app_settings
        .read()
        .map_err(|_| "应用设置暂时不可用。".to_string())?
        .update_proxy
        .clone();
    let updater_builder = app.updater_builder().timeout(Duration::from_secs(120));
    let updater = app_update::configure_signed_updater(updater_builder, &proxy_settings)?
        .build()
        .map_err(|error| format!("无法初始化签名更新检查：{error}"))?;
    let cancellation = state.start_update_check().await;
    let result = tokio::select! {
        result = updater.check() => result.map_err(|error| format!("签名更新检查失败：{error}")),
        _ = cancellation.cancelled() => Err("签名更新检查已取消。".to_string()),
    };
    state.finish_update_check().await;

    let Some(update) = result? else {
        return Ok(None);
    };
    let info = SignedUpdateInfo {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        date: update.date.map(|date| date.to_string()).unwrap_or_default(),
        body: update.body.clone().unwrap_or_default(),
    };
    *state.pending_signed_update.lock().await = Some(update);
    Ok(Some(info))
}

/** 下载完成后先停止后台任务，再由已验证的安装包执行覆盖升级。 */
#[tauri::command]
pub async fn download_and_install_application_update(
    app: AppHandle,
    state: State<'_, AppState>,
    on_progress: Channel<UpdateDownloadProgress>,
) -> Result<(), String> {
    let _operation_guard = state.update_operations.lock().await;
    let update = state
        .pending_signed_update
        .lock()
        .await
        .take()
        .ok_or_else(|| "更新信息已失效，请重新检查更新。".to_string())?;
    let cancellation = state.start_update_check().await;
    let mut downloaded_bytes = 0u64;
    let mut last_progress_emit = Instant::now() - Duration::from_millis(100);
    let progress_channel = on_progress.clone();
    let download = update.download(
        move |chunk_length, total_bytes| {
            downloaded_bytes = downloaded_bytes.saturating_add(chunk_length as u64);
            let download_finished = total_bytes.is_some_and(|total| downloaded_bytes >= total);
            if !download_finished && last_progress_emit.elapsed() < Duration::from_millis(100) {
                return;
            }
            last_progress_emit = Instant::now();
            let _ = progress_channel.send(UpdateDownloadProgress {
                downloaded_bytes,
                total_bytes,
                finished: false,
            });
        },
        || {},
    );
    let result = tokio::select! {
        result = download => result.map_err(|error| format!("更新下载或签名校验失败：{error}")),
        _ = cancellation.cancelled() => Err("更新下载已取消。".to_string()),
    };
    state.finish_update_check().await;

    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => return Err(error),
    };
    let _ = on_progress.send(UpdateDownloadProgress {
        downloaded_bytes: bytes.len() as u64,
        total_bytes: Some(bytes.len() as u64),
        finished: true,
    });

    let cancelled_chat_runs = state.cancel_all_chat_runs().await;
    let cancelled_approvals = state.cancel_all_tool_approvals().await;
    let cancelled_sync = state.cancel_sync_run().await;
    let cancelled_attachment_tasks = state.cancel_all_attachment_tasks();
    let removed_staged_attachments = state.cleanup_current_staged_attachments();
    crate::html_preview::destroy_all(&app);

    eprintln!(
        "Prepared update install: chats={cancelled_chat_runs}, approvals={cancelled_approvals}, attachments={cancelled_attachment_tasks}, sync={cancelled_sync}, staged={removed_staged_attachments}."
    );

    update
        .install(&bytes)
        .map_err(|error| format!("无法启动更新安装器：{error}"))?;
    #[cfg(not(target_os = "windows"))]
    app.restart();
    Ok(())
}

#[tauri::command]
pub async fn discard_signed_application_update(state: State<'_, AppState>) -> Result<(), String> {
    state.cancel_update_check().await;
    state.discard_pending_signed_update().await;
    Ok(())
}
