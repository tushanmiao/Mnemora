mod ai;
mod app_update;
mod chat;
mod commands;
#[cfg(feature = "memory-diagnostics")]
mod diagnostics;
mod english;
mod html_preview;
mod library;
mod mcp;
mod memory;
mod network;
mod packages;
mod plugins;
mod request_debug;
mod settings;
mod skills;
mod startup_log;
mod state;
mod storage;
mod sync;
mod task_diagnostics;
mod task_runtime;
mod usage;
mod window_lifecycle;

#[cfg(feature = "deep-note-e2e")]
#[doc(hidden)]
pub mod deep_note_e2e;

use tauri::Manager;

const AUTOSTART_ARG: &str = "--from-autostart";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
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
            tauri::WindowEvent::CloseRequested { api, .. }
                if window.label() == window_lifecycle::PET_WINDOW_LABEL =>
            {
                let locked = window
                    .app_handle()
                    .state::<state::AppState>()
                    .app_settings
                    .read()
                    .map(|settings| settings.pet.locked)
                    .unwrap_or(true);
                if locked {
                    api.prevent_close();
                }
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
            let default_data_dir = app.path().app_data_dir().map_err(std::io::Error::other)?;
            let resource_dir = app.path().resource_dir().map_err(std::io::Error::other)?;
            let log_dir = app.path().app_log_dir().map_err(std::io::Error::other)?;
            let storage_manager =
                storage::StorageManager::bootstrap(config_dir.clone(), default_data_dir)
                    .map_err(std::io::Error::other)?;
            let app_data_dir = storage_manager.runtime_data_dir().to_path_buf();
            let app_state = state::AppState::new(
                config_dir,
                app_data_dir,
                resource_dir,
                log_dir,
                storage_manager,
            )
            .map_err(std::io::Error::other)?;
            app_state.task_diagnostic_log.install_panic_hook();
            if app_state.storage.is_available() {
                let scope = app.asset_protocol_scope();
                scope
                    .allow_directory(
                        app_state.storage.current_data_dir().join("conversations"),
                        true,
                    )
                    .map_err(std::io::Error::other)?;
                scope
                    .allow_directory(
                        app_state
                            .storage
                            .current_data_dir()
                            .join("english")
                            .join("audio-cache"),
                        true,
                    )
                    .map_err(std::io::Error::other)?;
                scope
                    .allow_directory(
                        app_state
                            .storage
                            .current_data_dir()
                            .join("library")
                            .join("notes"),
                        true,
                    )
                    .map_err(std::io::Error::other)?;
            }
            app.manage(app_state);
            app.manage(html_preview::HtmlPreviewState::default());
            window_lifecycle::setup_tray(app.handle()).map_err(std::io::Error::other)?;
            let pet_settings = app
                .state::<state::AppState>()
                .app_settings
                .read()
                .map_err(|_| std::io::Error::other("App settings lock is unavailable"))?
                .pet
                .clone();

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
            if pet_settings.enabled && (!launched_from_autostart || pet_settings.show_on_startup) {
                window_lifecycle::sync_pet_window(app.handle(), &pet_settings)
                    .map_err(std::io::Error::other)?;
            }
            Ok(())
        });
    #[cfg(feature = "memory-diagnostics")]
    let builder = builder.plugin(diagnostics::plugin());
    builder
        .invoke_handler(tauri::generate_handler![
            commands::app_settings::load_application_settings,
            commands::app_settings::save_application_settings,
            commands::app_settings::export_settings_bundle,
            commands::app_settings::inspect_settings_bundle,
            commands::app_settings::import_settings_bundle,
            commands::network::test_web_network_connection,
            commands::storage::storage_get_status,
            commands::storage::storage_open_directory,
            commands::storage::storage_migrate_data,
            commands::app_update::check_application_update,
            commands::app_update::check_signed_application_update,
            commands::app_update::download_and_install_application_update,
            commands::app_update::discard_signed_application_update,
            commands::chat::chat_complete,
            commands::chat::chat_agent_run_get,
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
            commands::conversations::rename_conversation,
            commands::conversations::delete_conversation,
            commands::conversations::clear_conversations,
            commands::conversations::export_conversation,
            commands::conversations::save_conversation_as_note,
            commands::conversations::prepare_local_note_source,
            commands::conversations::discard_local_note_source,
            commands::html_preview::html_preview_open,
            commands::html_preview::html_preview_get,
            commands::english::english_dictionary_status,
            commands::english::english_dictionary_download,
            commands::english::english_dictionary_search,
            commands::english::english_dictionary_get,
            commands::english::english_dictionary_delete,
            commands::english::english_dictionary_release,
            commands::english_learning::english_learning_overview,
            commands::english_learning::english_learning_create_plan,
            commands::english_learning::english_learning_update_plan,
            commands::english_learning::english_learning_add_word,
            commands::english_learning::english_learning_pause_plan,
            commands::english_learning::english_learning_next_batch,
            commands::english_learning::english_learning_get_item,
            commands::english_learning::english_learning_submit_attempt,
            commands::english_learning::english_learning_mark_mastered,
            commands::english_learning::english_learning_archive_item,
            commands::english_learning::english_learning_restore_item,
            commands::english_learning::english_learning_list_archived,
            commands::english_learning::english_learning_stats,
            commands::english_learning::english_learning_list_history,
            commands::english_learning::english_learning_export_book,
            commands::english_learning::english_learning_import_book,
            commands::english_learning::english_learning_cache_audio,
            commands::english_learning::english_learning_audio_cache_status,
            commands::english_learning::english_learning_clear_audio_cache,
            commands::english_learning::english_learning_prefetch_audio,
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
            commands::library::library_export_note,
            commands::library::library_create_note,
            commands::library::library_create_note_with_sources,
            commands::library::library_list_note_sources,
            commands::library::library_import_markdown_notes,
            commands::library::library_update_note,
            commands::library::library_rename_note,
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
            commands::mcp::mcp_list_servers,
            commands::mcp::mcp_upsert_server,
            commands::mcp::mcp_set_server_enabled,
            commands::mcp::mcp_refresh_server,
            commands::mcp::mcp_remove_server,
            commands::note_pipeline::note_pipeline_start,
            commands::note_pipeline::note_pipeline_inspect_start,
            commands::note_pipeline::note_pipeline_adjust,
            commands::note_pipeline::note_pipeline_confirm,
            commands::note_pipeline::note_pipeline_resume,
            commands::note_pipeline::note_pipeline_retry,
            commands::note_pipeline::note_pipeline_restart,
            commands::note_pipeline::note_pipeline_cancel,
            commands::note_pipeline::note_pipeline_diagnostic_path,
            commands::note_pipeline::note_pipeline_abandon,
            commands::note_pipeline::note_pipeline_abandon_for_conversation,
            commands::note_pipeline::note_pipeline_pause,
            commands::note_pipeline::note_pipeline_list_resumable,
            commands::note_pipeline::note_pipeline_get,
            commands::note_pipeline::note_pipeline_get_detail,
            commands::note_pipeline::note_edit_prepare,
            commands::note_pipeline::note_edit_resolve,
            commands::note_pipeline::note_edit_resolve_content,
            commands::pet::pet_set_enabled,
            commands::pet::pet_set_locked,
            commands::pet::pet_update_position,
            commands::pet::pet_open_main,
            commands::pet::pet_list,
            commands::pet::pet_import,
            commands::pet::pet_import_archive,
            commands::pet::pet_import_codex,
            commands::pet::pet_delete,
            commands::pet::pet_open_directory,
            commands::plugins::plugins_list,
            commands::plugins::plugins_install,
            commands::packages::packages_search_remote,
            commands::packages::packages_fetch_remote,
            commands::packages::packages_install_remote,
            commands::plugins::plugins_set_enabled,
            commands::plugins::plugins_rollback,
            commands::plugins::plugins_uninstall,
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
            commands::skills::skills_set_all_enabled,
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
                let cancelled_note_pipelines =
                    tauri::async_runtime::block_on(state.cancel_all_note_pipeline_runs());
                let approvals =
                    tauri::async_runtime::block_on(state.cancel_all_tool_approvals());
                let cancelled_sync = tauri::async_runtime::block_on(state.cancel_sync_run());
                let cancelled_update =
                    tauri::async_runtime::block_on(state.cancel_update_check());
                tauri::async_runtime::block_on(state.discard_pending_signed_update());
                if cancelled > 0 {
                    eprintln!("Cancelled {cancelled} active chat run(s) on application exit.");
                }
                if cancelled_note_pipelines > 0 {
                    eprintln!(
                        "Cancelled {cancelled_note_pipelines} active note pipeline run(s) on application exit."
                    );
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
