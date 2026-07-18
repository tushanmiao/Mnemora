mod ai;
mod chat;
mod commands;
mod settings;
mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir().map_err(std::io::Error::other)?;
            let app_state = state::AppState::new(config_dir).map_err(std::io::Error::other)?;
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat::chat_complete,
            commands::providers::fetch_provider_models,
            commands::providers::test_provider_connection,
            commands::settings::load_model_settings,
            commands::settings::save_model_settings,
            commands::settings::set_provider_api_key,
            commands::settings::delete_provider_api_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
