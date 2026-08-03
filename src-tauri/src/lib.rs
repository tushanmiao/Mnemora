mod ai;
mod app_update;
mod chat;
mod commands;
mod english;
mod html_preview;
mod library;
mod memory;
mod request_debug;
mod settings;
mod skills;
mod startup_log;
mod state;
mod sync;
mod usage;
mod window_lifecycle;

use tauri::Manager;

const AUTOSTART_ARG: &str = "--from-autostart";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { .. } if window.label() == "main" => {
                // 先清理后台任务，再让 Tauri 默认关闭流程销毁 WebView。
                window_lifecycle::cleanup_before_main_window_close(window.app_handle());
            }
            tauri::WindowEvent::Destroyed => {
                if window.label() == "main" {
                    html_preview::destroy_all(window.app_handle());
                } else {
                    html_preview::cleanup_destroyed_window(window.app_handle(), window.label());
                }
            }
            _ => {}
        })
        .setup(|app| {
            let launched_from_autostart =
                std::env::args().any(|argument| argument == AUTOSTART_ARG);
            let config_dir = app.path().app_config_dir().map_err(std::io::Error::other)?;
            let app_data_dir = app.path().app_data_dir().map_err(std::io::Error::other)?;
            let resource_dir = app.path().resource_dir().map_err(std::io::Error::other)?;
            let log_dir = app.path().app_log_dir().map_err(std::io::Error::other)?;
            let app_state = state::AppState::new(config_dir, app_data_dir, resource_dir, log_dir)
                .map_err(std::io::Error::other)?;
            app.manage(app_state);
            app.manage(html_preview::HtmlPreviewState::default());
            window_lifecycle::setup_tray(app.handle()).map_err(std::io::Error::other)?;

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
            commands::app_settings::inspect_settings_bundle,
            commands::app_settings::import_settings_bundle,
            commands::app_update::check_application_update,
            commands::app_update::check_signed_application_update,
            commands::app_update::download_and_install_application_update,
            commands::app_update::discard_signed_application_update,
            commands::chat::chat_complete,
            commands::chat::chat_stream_start,
            commands::chat::chat_stream_cancel,
            commands::chat::chat_tool_approval_resolve,
            commands::attachments::inspect_chat_attachments,
            commands::attachments::save_pasted_chat_attachment,
            commands::attachments::discard_staged_chat_attachment,
            commands::attachments::import_chat_attachments,
            commands::attachments::read_chat_attachment_preview,
            commands::attachments::read_chat_attachment_image,
            commands::attachments::cancel_chat_attachment_task,
            commands::attachments::discard_imported_chat_attachments,
            commands::attachments::open_chat_attachment,
            commands::conversations::list_conversations,
            commands::conversations::load_conversation,
            commands::conversations::save_conversation,
            commands::conversations::delete_conversation,
            commands::conversations::clear_conversations,
            commands::conversations::export_conversation,
            commands::conversations::save_conversation_as_note,
            commands::html_preview::html_preview_open,
            commands::html_preview::html_preview_get,
            commands::english::english_dictionary_status,
            commands::english::english_dictionary_download,
            commands::english::english_dictionary_search,
            commands::english::english_dictionary_get,
            commands::english::english_dictionary_delete,
            commands::english::english_dictionary_release,
            commands::library::library_list_items,
            commands::library::library_get_item,
            commands::library::library_import_pdfs,
            commands::library::library_update_item,
            commands::library::library_set_favorite,
            commands::library::library_move_to_trash,
            commands::library::library_restore_item,
            commands::library::library_delete_permanently,
            commands::library::library_mark_opened,
            commands::library::library_open_item,
            commands::library::library_read_pdf_range,
            commands::library::library_get_reading_state,
            commands::library::library_save_reading_state,
            commands::library::library_list_annotations,
            commands::library::library_create_annotation,
            commands::library::library_update_annotation,
            commands::library::library_delete_annotation,
            commands::library::library_list_notes,
            commands::library::library_get_note,
            commands::library::library_create_note,
            commands::library::library_import_markdown_notes,
            commands::library::library_update_note,
            commands::library::library_delete_note,
            commands::library::library_list_note_groups,
            commands::library::library_create_note_group,
            commands::library::library_delete_note_group,
            commands::library::library_set_note_group,
            commands::library::library_list_collections,
            commands::library::library_create_collection,
            commands::library::library_rename_collection,
            commands::library::library_delete_collection,
            commands::memory::memory_load,
            commands::memory::memory_save,
            commands::memory::memory_clear,
            commands::memory::memory_get_directory,
            commands::memory::memory_open_directory,
            commands::providers::fetch_provider_models,
            commands::providers::test_provider_connection,
            commands::settings::load_model_settings,
            commands::settings::save_model_settings,
            commands::settings::set_provider_api_key,
            commands::settings::delete_provider_api_key,
            commands::skills::skills_list,
            commands::skills::skills_get_detail,
            commands::skills::skills_import,
            commands::skills::skills_set_enabled,
            commands::skills::skills_uninstall,
            commands::skills::skills_restore_builtin,
            commands::startup::record_startup_error,
            commands::sync::sync_load_settings,
            commands::sync::sync_save_settings,
            commands::sync::sync_set_notion_token,
            commands::sync::sync_delete_notion_token,
            commands::sync::sync_set_feishu_app_secret,
            commands::sync::sync_delete_feishu_app_secret,
            commands::sync::sync_run,
            usage::usage_get_summary,
            usage::usage_get_records,
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
                let approvals =
                    tauri::async_runtime::block_on(state.cancel_all_tool_approvals());
                let cancelled_sync = tauri::async_runtime::block_on(state.cancel_sync_run());
                let cancelled_update =
                    tauri::async_runtime::block_on(state.cancel_update_check());
                tauri::async_runtime::block_on(state.discard_pending_signed_update());
                if cancelled > 0 {
                    eprintln!("Cancelled {cancelled} active chat run(s) on application exit.");
                }
                if approvals > 0 {
                    eprintln!("Cancelled {approvals} pending tool approval(s) on application exit.");
                }
                if cancelled_sync {
                    eprintln!("Cancelled the active note sync on application exit.");
                }
                if cancelled_update {
                    eprintln!("Cancelled the active update check on application exit.");
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
