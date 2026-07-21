mod ai;
mod chat;
mod commands;
mod request_debug;
mod settings;
mod state;
mod usage;
mod window_lifecycle;

use tauri::Manager;

const AUTOSTART_ARG: &str = "--from-autostart";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    // 先清理后台任务，再让 Tauri 默认关闭流程销毁 WebView。
                    window_lifecycle::cleanup_before_main_window_close(window.app_handle());
                }
            }
        })
        .setup(|app| {
            let launched_from_autostart =
                std::env::args().any(|argument| argument == AUTOSTART_ARG);
            let config_dir = app.path().app_config_dir().map_err(std::io::Error::other)?;
            let app_data_dir = app.path().app_data_dir().map_err(std::io::Error::other)?;
            let app_state =
                state::AppState::new(config_dir, app_data_dir).map_err(std::io::Error::other)?;
            app.manage(app_state);
            window_lifecycle::setup_tray(&app.handle()).map_err(std::io::Error::other)?;

            // 仅普通交互式启动创建主窗口；开机自启只保留 Rust 后端和托盘，避免启动 WebView2 进程组。
            if !launched_from_autostart {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if let Err(error) = window_lifecycle::open_main_window(&app_handle) {
                        eprintln!("Failed to open Mnemora on launch: {error}");
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_settings::load_application_settings,
            commands::app_settings::save_application_settings,
            commands::app_settings::export_settings_bundle,
            commands::app_settings::import_settings_bundle,
            commands::chat::chat_complete,
            commands::chat::chat_stream_start,
            commands::chat::chat_stream_cancel,
            commands::attachments::inspect_chat_attachments,
            commands::attachments::save_pasted_chat_attachment,
            commands::attachments::discard_staged_chat_attachment,
            commands::attachments::import_chat_attachments,
            commands::attachments::read_chat_attachment_preview,
            commands::attachments::cancel_chat_attachment_task,
            commands::attachments::discard_imported_chat_attachments,
            commands::attachments::open_chat_attachment,
            commands::conversations::list_conversations,
            commands::conversations::load_conversation,
            commands::conversations::save_conversation,
            commands::conversations::delete_conversation,
            commands::conversations::clear_conversations,
            commands::providers::fetch_provider_models,
            commands::providers::test_provider_connection,
            commands::settings::load_model_settings,
            commands::settings::save_model_settings,
            commands::settings::set_provider_api_key,
            commands::settings::delete_provider_api_key,
            usage::usage_get_stats,
            usage::usage_clear,
            request_debug::request_debug_get_records,
            request_debug::request_debug_clear,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                    return;
                }
                let state = app_handle.state::<state::AppState>();
                let cancelled = tauri::async_runtime::block_on(state.cancel_all_chat_runs());
                if cancelled > 0 {
                    eprintln!("Cancelled {cancelled} active chat run(s) on application exit.");
                }
                let attachment_tasks = state.cancel_all_attachment_tasks();
                let staged_attachments = state.cleanup_current_staged_attachments();
                if attachment_tasks > 0 || staged_attachments > 0 {
                    eprintln!(
                        "Cancelled {attachment_tasks} attachment task(s) and removed {staged_attachments} staged attachment(s) on application exit."
                    );
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                if !has_visible_windows {
                    if let Err(error) = window_lifecycle::open_main_window(app_handle) {
                        eprintln!("Failed to reopen Mnemora: {error}");
                    }
                }
            }
            _ => {}
        });
}
