//! 主窗口与系统托盘的生命周期。
//!
//! 主窗口关闭后立即销毁 WebView，仅保留 Rust 后端和托盘；用户从托盘重新打开时，
//! 再根据 `tauri.conf.json` 中的 `main` 窗口配置创建新的 WebView。

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewWindow, WebviewWindowBuilder,
};

use crate::state::AppState;

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "main-tray";
const MENU_OPEN_ID: &str = "open-main-window";
const MENU_QUIT_ID: &str = "quit-application";

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
        .map_err(|error| format!("Failed to focus the main window: {error}"))
}

pub fn cleanup_before_main_window_close(app: &AppHandle) {
    let state = app.state::<AppState>();
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
    {
        eprintln!(
            "Background cleanup cancelled {cancelled_chat_runs} chat run(s), {cancelled_approvals} tool approval(s), {cancelled_attachment_tasks} attachment task(s), sync={cancelled_sync}, update={cancelled_update}, and removed {removed_staged_attachments} staged attachment(s)."
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
