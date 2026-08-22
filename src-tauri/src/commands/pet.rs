use tauri::{AppHandle, Emitter, Manager, State};

use crate::{settings::app_types::AppSettings, state::AppState, window_lifecycle};

fn save_pet_settings(
    state: &State<'_, AppState>,
    update: impl FnOnce(&mut AppSettings),
) -> Result<AppSettings, String> {
    let mut settings = state
        .app_settings
        .read()
        .map_err(|_| "App settings lock is unavailable".to_string())?
        .clone();
    update(&mut settings);
    settings = settings.normalize_and_validate()?;
    state.app_settings_repository.save(&settings)?;
    *state
        .app_settings
        .write()
        .map_err(|_| "App settings lock is unavailable".to_string())? = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub async fn pet_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSettings, String> {
    let settings = save_pet_settings(&state, |settings| settings.pet.enabled = enabled)?;
    let _ = app.emit_to("main", "mnemora://app-settings-updated", &settings);
    if enabled {
        if app
            .get_webview_window(window_lifecycle::PET_WINDOW_LABEL)
            .is_some()
        {
            window_lifecycle::update_pet_window_runtime(&app, &settings.pet)?;
        } else {
            window_lifecycle::sync_pet_window(&app, &settings.pet)?;
        }
    } else {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            let _ = window_lifecycle::destroy_pet_window(&app);
        });
    }
    Ok(settings)
}

#[tauri::command]
pub async fn pet_update_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    save_pet_settings(&state, |settings| {
        settings.pet.position_x = Some(x);
        settings.pet.position_y = Some(y);
    })?;
    Ok(())
}

#[tauri::command]
pub async fn pet_open_main(app: AppHandle) -> Result<(), String> {
    window_lifecycle::open_main_window(&app)
}
