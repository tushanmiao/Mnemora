//! 只允许主窗口创建 HTML 预览，只允许对应预览窗口读取自己的短期内容。

use tauri::{AppHandle, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use uuid::Uuid;

use crate::html_preview::{
    destroy_all, HtmlPreviewState, HTML_PREVIEW_LABEL_PREFIX, MAX_HTML_PREVIEW_BYTES,
};

#[tauri::command]
pub async fn html_preview_open(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, HtmlPreviewState>,
    html: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main window can open an HTML preview.".to_string());
    }
    if html.trim().is_empty() {
        return Err("HTML preview content cannot be empty.".to_string());
    }
    if html.len() > MAX_HTML_PREVIEW_BYTES {
        return Err("HTML preview content cannot exceed 1 MB.".to_string());
    }

    destroy_all(&app);
    let token = Uuid::new_v4().simple().to_string();
    let label = format!("{HTML_PREVIEW_LABEL_PREFIX}{token}");
    state.replace(token.clone(), html)?;

    let url = WebviewUrl::App(format!("index.html#html-preview/{token}").into());
    let preview = match WebviewWindowBuilder::new(&app, label, url)
        .title("Mnemora HTML Preview")
        .inner_size(980.0, 720.0)
        .min_inner_size(480.0, 360.0)
        .resizable(true)
        .visible(false)
        .center()
        .build()
    {
        Ok(window) => window,
        Err(error) => {
            state.remove(&token);
            return Err(format!("Failed to create HTML preview window: {error}"));
        }
    };

    if let Err(error) = preview.show().and_then(|_| preview.set_focus()) {
        state.remove(&token);
        let _ = preview.destroy();
        return Err(format!("Failed to show HTML preview window: {error}"));
    }
    Ok(())
}

#[tauri::command]
pub fn html_preview_get(
    window: WebviewWindow,
    state: State<'_, HtmlPreviewState>,
    token: String,
) -> Result<String, String> {
    let expected_label = format!("{HTML_PREVIEW_LABEL_PREFIX}{token}");
    if window.label() != expected_label {
        return Err("HTML preview content does not belong to this window.".to_string());
    }
    state.get(&token)
}
