//! Mnemora 通用基础设置。
//!
//! 这里保存外观、行为、个人资料和 Chat 默认行为，不保存 Provider API Key。
//! `ModelSettings` 负责模型服务结构；`AppSettings` 负责应用级体验，两者版本独立。

use reqwest::Url;
use serde::{Deserialize, Serialize};

pub const CURRENT_APP_SETTINGS_VERSION: u32 = 4;
const MAX_AVATAR_DATA_URL_BYTES: usize = 3 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterfaceLanguage {
    #[default]
    Zh,
    En,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeColor {
    #[default]
    Neutral,
    Warm,
    Cool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseLanguage {
    #[default]
    FollowInput,
    Zh,
    ZhHant,
    En,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub interface_language: InterfaceLanguage,
    #[serde(default)]
    pub theme: ThemeMode,
    #[serde(default)]
    pub theme_color: ThemeColor,
    #[serde(default = "default_font_size")]
    pub font_size: u8,
    #[serde(default)]
    pub launch_at_startup: bool,
    #[serde(default = "default_true")]
    pub retry_enabled: bool,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u8,
    #[serde(default)]
    pub user_display_name: String,
    #[serde(default)]
    pub user_avatar: String,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default = "default_true")]
    pub stream_enabled: bool,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
    #[serde(default)]
    pub response_language: ResponseLanguage,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub request_debug_enabled: bool,
}

fn current_version() -> u32 {
    CURRENT_APP_SETTINGS_VERSION
}

fn default_true() -> bool {
    true
}

fn default_retry_attempts() -> u8 {
    5
}

fn default_font_size() -> u8 {
    14
}

fn default_max_output_tokens() -> u32 {
    32_768
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: CURRENT_APP_SETTINGS_VERSION,
            interface_language: InterfaceLanguage::Zh,
            theme: ThemeMode::System,
            theme_color: ThemeColor::Neutral,
            font_size: 14,
            launch_at_startup: false,
            retry_enabled: true,
            retry_attempts: 5,
            user_display_name: String::new(),
            user_avatar: String::new(),
            working_directory: String::new(),
            stream_enabled: true,
            thinking_enabled: false,
            max_output_tokens: 32_768,
            response_language: ResponseLanguage::FollowInput,
            system_prompt: String::new(),
            request_debug_enabled: false,
        }
    }
}

impl AppSettings {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        let source_version = self.version;
        if self.version > CURRENT_APP_SETTINGS_VERSION {
            return Err(format!(
                "App settings version {} is newer than supported version {}",
                self.version, CURRENT_APP_SETTINGS_VERSION
            ));
        }
        if source_version < 3 && self.retry_attempts == 1 {
            self.retry_attempts = 5;
        }
        self.version = CURRENT_APP_SETTINGS_VERSION;
        self.user_display_name = self.user_display_name.trim().to_string();
        self.user_avatar = self.user_avatar.trim().to_string();
        self.working_directory = self.working_directory.trim().to_string();
        self.system_prompt = self.system_prompt.trim().to_string();

        if self.user_display_name.chars().count() > 100 {
            return Err("User display name is too long".to_string());
        }
        if self.user_avatar.len() > MAX_AVATAR_DATA_URL_BYTES {
            return Err("User avatar image is too large".to_string());
        }
        if !self.user_avatar.is_empty() {
            let safe_data_image = [
                "data:image/png;base64,",
                "data:image/jpeg;base64,",
                "data:image/webp;base64,",
                "data:image/gif;base64,",
            ]
            .iter()
            .any(|prefix| self.user_avatar.starts_with(prefix));
            let legacy_http_image = Url::parse(&self.user_avatar)
                .ok()
                .is_some_and(|url| matches!(url.scheme(), "http" | "https"));
            if !safe_data_image && !legacy_http_image {
                return Err(
                    "User avatar must be an uploaded PNG, JPEG, WebP or GIF image".to_string(),
                );
            }
        }
        if self.working_directory.len() > 32_768 {
            return Err("Working directory is too long".to_string());
        }
        if !(12..=20).contains(&self.font_size) {
            return Err("Font size must be between 12 and 20".to_string());
        }
        if !(1..=5).contains(&self.retry_attempts) {
            return Err("Retry attempts must be between 1 and 5".to_string());
        }
        if !(256..=131_072).contains(&self.max_output_tokens) {
            return Err("Maximum output tokens must be between 256 and 131072".to_string());
        }
        if self.system_prompt.len() > 256 * 1024 {
            return Err("System Prompt is too long".to_string());
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, ThemeMode, CURRENT_APP_SETTINGS_VERSION};

    #[test]
    fn defaults_are_versioned_and_streaming_is_enabled() {
        let settings = AppSettings::default();
        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
        assert_eq!(settings.theme, ThemeMode::System);
        assert!(settings.stream_enabled);
        assert!(!settings.request_debug_enabled);
        assert_eq!(settings.retry_attempts, 5);
        assert_eq!(settings.font_size, 14);
    }

    #[test]
    fn normalizes_user_input_and_rejects_invalid_avatar() {
        let mut settings = AppSettings {
            user_display_name: "  Mnemora  ".to_string(),
            ..AppSettings::default()
        };
        settings = settings.normalize_and_validate().unwrap();
        assert_eq!(settings.user_display_name, "Mnemora");

        settings.user_avatar = "file:///secret".to_string();
        assert!(settings.normalize_and_validate().is_err());
    }

    #[test]
    fn migrates_retry_default_and_accepts_uploaded_avatar() {
        let settings = AppSettings {
            version: 2,
            retry_attempts: 1,
            user_avatar: "data:image/png;base64,iVBORw0KGgo=".to_string(),
            ..AppSettings::default()
        }
        .normalize_and_validate()
        .unwrap();
        assert_eq!(settings.retry_attempts, 5);
        assert_eq!(settings.version, CURRENT_APP_SETTINGS_VERSION);
    }
}
