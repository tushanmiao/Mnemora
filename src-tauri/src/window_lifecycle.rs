//! 主窗口与系统托盘的生命周期。
//!
//! 主窗口关闭后立即销毁 WebView，仅保留 Rust 后端和托盘；用户从托盘重新打开时，
//! 再根据 `tauri.conf.json` 中的 `main` 窗口配置创建新的 WebView。

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::{settings::app_types::PetSettings, state::AppState};

const MAIN_WINDOW_LABEL: &str = "main";
pub const PET_WINDOW_LABEL: &str = "pet";
pub const QUICK_CHAT_WINDOW_LABEL: &str = "quick-chat";
const TRAY_ID: &str = "main-tray";
const MENU_OPEN_ID: &str = "open-main-window";
const MENU_QUIT_ID: &str = "quit-application";
pub static NOTE_EDITOR_CLOSE_GUARD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn request_editor_close(app: &AppHandle, exit: bool) -> bool {
    if !NOTE_EDITOR_CLOSE_GUARD.load(std::sync::atomic::Ordering::Acquire) {
        return false;
    }
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return false;
    };
    window.emit("mnemora://note-editor-close", exit).is_ok()
}

pub fn ensure_main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        return Ok(window);
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN_WINDOW_LABEL)
        .ok_or_else(|| "Main window configuration was not found.".to_string())?;
    WebviewWindowBuilder::from_config(app, config)
        .map_err(|error| format!("Failed to prepare the main window: {error}"))?
        .build()
        .map_err(|error| format!("Failed to create the main window: {error}"))
}

pub fn open_main_window(app: &AppHandle) -> Result<(), String> {
    let window = ensure_main_window(app)?;
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Failed to show the main window: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Failed to focus the main window: {error}"))?;
    let pet_settings = app
        .state::<AppState>()
        .app_settings
        .read()
        .map_err(|_| "App settings lock is unavailable".to_string())?
        .pet
        .clone();
    if pet_settings.enabled {
        sync_pet_window(app, &pet_settings)?;
    }
    Ok(())
}

pub fn sync_pet_window(app: &AppHandle, settings: &PetSettings) -> Result<(), String> {
    if !settings.enabled {
        return destroy_pet_window(app);
    }

    let window = if let Some(window) = app.get_webview_window(PET_WINDOW_LABEL) {
        window
    } else {
        let logical_size = f64::from(settings.size);
        let mut builder = WebviewWindowBuilder::new(
            app,
            PET_WINDOW_LABEL,
            WebviewUrl::App("index.html#pet".into()),
        )
        .title("Mnemora Pet")
        .inner_size(logical_size + 96.0, logical_size + 72.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(settings.always_on_top)
        .skip_taskbar(true)
        .focused(false)
        .visible(false);
        if let (Some(x), Some(y)) = (settings.position_x, settings.position_y) {
            builder = builder.position(x, y);
        } else {
            builder = builder.center();
        }
        builder
            .build()
            .map_err(|error| format!("Failed to create the desktop pet window: {error}"))?
    };

    let logical_size = f64::from(settings.size);
    window
        .set_size(LogicalSize::new(logical_size + 96.0, logical_size + 72.0))
        .map_err(|error| format!("Failed to resize the desktop pet window: {error}"))?;
    window
        .set_always_on_top(settings.always_on_top)
        .map_err(|error| format!("Failed to update desktop pet stacking: {error}"))?;
    window
        .set_ignore_cursor_events(settings.click_through)
        .map_err(|error| format!("Failed to update desktop pet click-through: {error}"))?;
    if let (Some(x), Some(y)) = (settings.position_x, settings.position_y) {
        let _ = window.set_position(LogicalPosition::new(x, y));
    }
    window
        .show()
        .map_err(|error| format!("Failed to show the desktop pet window: {error}"))?;
    let _ = app.emit_to(PET_WINDOW_LABEL, "mnemora://pet-settings", settings);
    Ok(())
}

pub fn update_pet_window_runtime(app: &AppHandle, settings: &PetSettings) -> Result<(), String> {
    let Some(window) = app.get_webview_window(PET_WINDOW_LABEL) else {
        return Ok(());
    };
    let logical_size = f64::from(settings.size);
    window
        .set_size(LogicalSize::new(logical_size + 96.0, logical_size + 72.0))
        .map_err(|error| format!("Failed to resize the desktop pet window: {error}"))?;
    window
        .set_always_on_top(settings.always_on_top)
        .map_err(|error| format!("Failed to update desktop pet stacking: {error}"))?;
    window
        .set_ignore_cursor_events(settings.click_through)
        .map_err(|error| format!("Failed to update desktop pet click-through: {error}"))?;
    let _ = app.emit_to(PET_WINDOW_LABEL, "mnemora://pet-settings", settings);
    Ok(())
}

pub fn destroy_pet_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(PET_WINDOW_LABEL) {
        window
            .destroy()
            .map_err(|error| format!("Failed to destroy the desktop pet window: {error}"))?;
    }
    Ok(())
}

pub fn destroy_quick_chat_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(QUICK_CHAT_WINDOW_LABEL) {
        window
            .destroy()
            .map_err(|error| format!("Failed to destroy the quick chat window: {error}"))?;
    }
    Ok(())
}

pub fn cleanup_before_main_window_close(app: &AppHandle) {
    if let Err(error) = destroy_pet_window(app) {
        eprintln!("{error}");
    }
    if let Err(error) = destroy_quick_chat_window(app) {
        eprintln!("{error}");
    }
    let state = app.state::<AppState>();
    let cancelled_knowledge = tauri::async_runtime::block_on(state.cancel_all_knowledge_jobs());
    let cancelled_chat_runs = tauri::async_runtime::block_on(state.cancel_all_chat_runs());
    let cancelled_approvals = tauri::async_runtime::block_on(state.cancel_all_tool_approvals());
    let cancelled_sync = tauri::async_runtime::block_on(state.cancel_sync_run());
    let cancelled_update = tauri::async_runtime::block_on(state.cancel_update_check());
    tauri::async_runtime::block_on(state.discard_pending_signed_update());
    let cancelled_attachment_tasks = state.cancel_all_attachment_tasks();
    let removed_staged_attachments = state.cleanup_current_staged_attachments();
    if cancelled_chat_runs > 0
        || cancelled_approvals > 0
        || cancelled_attachment_tasks > 0
        || removed_staged_attachments > 0
        || cancelled_sync
        || cancelled_update
        || cancelled_knowledge > 0
    {
        eprintln!(
            "Background cleanup cancelled {cancelled_chat_runs} chat run(s), {cancelled_approvals} tool approval(s), {cancelled_knowledge} knowledge job(s), {cancelled_attachment_tasks} attachment task(s), sync={cancelled_sync}, update={cancelled_update}, and removed {removed_staged_attachments} staged attachment(s)."
        );
    }
}

pub fn setup_tray(app: &AppHandle) -> Result<(), String> {
    let open_item = MenuItem::with_id(app, MENU_OPEN_ID, "打开 Mnemora", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT_ID, "退出", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let menu =
        Menu::with_items(app, &[&open_item, &quit_item]).map_err(|error| error.to_string())?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "Default application icon is unavailable.".to_string())?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Mnemora")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN_ID => {
                if let Err(error) = open_main_window(app) {
                    eprintln!("Failed to open Mnemora from the tray: {error}");
                }
            }
            MENU_QUIT_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(error) = open_main_window(tray.app_handle()) {
                    eprintln!("Failed to open Mnemora from the tray: {error}");
                }
            }
        })
        .build(app)
        .map_err(|error| format!("Failed to create the system tray: {error}"))?;
    Ok(())
}
