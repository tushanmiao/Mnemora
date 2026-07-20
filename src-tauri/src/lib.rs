mod ai;
mod chat;
mod commands;
mod request_debug;
mod settings;
mod state;
mod usage;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let config_dir = app.path().app_config_dir().map_err(std::io::Error::other)?;
            let app_data_dir = app.path().app_data_dir().map_err(std::io::Error::other)?;
            let app_state =
                state::AppState::new(config_dir, app_data_dir).map_err(std::io::Error::other)?;
            app.manage(app_state);
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
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let state = app_handle.state::<state::AppState>();
                let cancelled = tauri::async_runtime::block_on(state.cancel_all_chat_runs());
                if cancelled > 0 {
                    eprintln!("Cancelled {cancelled} active chat run(s) on application exit.");
                }
            }
        });
}
