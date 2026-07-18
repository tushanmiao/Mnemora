mod ai;
mod commands;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = state::AppState::new().expect("failed to create application state");

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::providers::fetch_provider_models,
            commands::providers::test_provider_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
