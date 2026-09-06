//! 独立快速聊天窗口。它复用同一个后端，但前端路由和会话 hook 从空白对话开始。

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const QUICK_CHAT_WINDOW_LABEL: &str = "quick-chat";

#[tauri::command]
pub fn quick_chat_open(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main window can open quick chat.".to_string());
    }

    let quick_chat = if let Some(existing) = app.get_webview_window(QUICK_CHAT_WINDOW_LABEL) {
        existing
    } else {
        WebviewWindowBuilder::new(
            &app,
            QUICK_CHAT_WINDOW_LABEL,
            WebviewUrl::App("index.html#quick-chat".into()),
        )
        .title("Mnemora Quick Chat")
        .inner_size(460.0, 640.0)
        .min_inner_size(360.0, 480.0)
        .resizable(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .center()
        .build()
        .map_err(|error| format!("Failed to create quick chat window: {error}"))?
    };

    quick_chat
        .show()
        .and_then(|_| quick_chat.set_focus())
        .map_err(|error| format!("Failed to show quick chat window: {error}"))
}
