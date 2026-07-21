//! HTML 预览窗口的短期内容仓库和生命周期辅助函数。
//!
//! 预览内容只在窗口存活期间保留，最多一个文档且最大 1 MB；窗口销毁后立即释放。

use std::{collections::HashMap, sync::Mutex};

use tauri::{AppHandle, Manager};

pub const HTML_PREVIEW_LABEL_PREFIX: &str = "html-preview-";
pub const MAX_HTML_PREVIEW_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct HtmlPreviewState {
    documents: Mutex<HashMap<String, String>>,
}

impl HtmlPreviewState {
    pub fn replace(&self, token: String, html: String) -> Result<(), String> {
        let mut documents = self
            .documents
            .lock()
            .map_err(|_| "HTML preview state is unavailable.".to_string())?;
        documents.clear();
        documents.insert(token, html);
        Ok(())
    }

    pub fn get(&self, token: &str) -> Result<String, String> {
        self.documents
            .lock()
            .map_err(|_| "HTML preview state is unavailable.".to_string())?
            .get(token)
            .cloned()
            .ok_or_else(|| "HTML preview content is no longer available.".to_string())
    }

    pub fn remove(&self, token: &str) {
        if let Ok(mut documents) = self.documents.lock() {
            documents.remove(token);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut documents) = self.documents.lock() {
            documents.clear();
        }
    }
}

pub fn preview_token_from_label(label: &str) -> Option<&str> {
    label.strip_prefix(HTML_PREVIEW_LABEL_PREFIX)
}

pub fn destroy_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if preview_token_from_label(&label).is_some() {
            if let Err(error) = window.destroy() {
                eprintln!("Failed to destroy HTML preview window {label}: {error}");
            }
        }
    }
    app.state::<HtmlPreviewState>().clear();
}

pub fn cleanup_destroyed_window(app: &AppHandle, label: &str) {
    if let Some(token) = preview_token_from_label(label) {
        app.state::<HtmlPreviewState>().remove(token);
    }
}

#[cfg(test)]
mod tests {
    use super::{preview_token_from_label, HtmlPreviewState};

    #[test]
    fn preview_state_replaces_old_content_and_removes_destroyed_content() {
        let state = HtmlPreviewState::default();
        state.replace("first".into(), "one".into()).unwrap();
        state.replace("second".into(), "two".into()).unwrap();

        assert!(state.get("first").is_err());
        assert_eq!(state.get("second").unwrap(), "two");
        state.remove("second");
        assert!(state.get("second").is_err());
    }

    #[test]
    fn extracts_only_html_preview_labels() {
        assert_eq!(
            preview_token_from_label("html-preview-token"),
            Some("token")
        );
        assert_eq!(preview_token_from_label("main"), None);
    }
}
