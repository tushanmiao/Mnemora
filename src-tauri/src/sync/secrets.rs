//! Notion Integration Token 的系统凭据存储。

use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

const SERVICE_NAME: &str = "com.mnemora.app.sync";
const NOTION_TOKEN_KEY: &str = "notion-integration-token";

#[derive(Clone, Copy, Default)]
pub struct SyncSecretStore;

impl SyncSecretStore {
    pub fn get_notion_token(&self) -> Result<Option<String>, String> {
        match Entry::new(SERVICE_NAME, NOTION_TOKEN_KEY)
            .map_err(|error| format!("打开 Notion 凭据失败：{error}"))?
            .get_password()
        {
            Ok(token) if !token.trim().is_empty() => Ok(Some(token)),
            Ok(_) => Ok(None),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(format!("读取 Notion 凭据失败：{error}")),
        }
    }

    pub fn has_notion_token(&self) -> Result<bool, String> {
        let token = self.get_notion_token()?.map(Zeroizing::new);
        Ok(token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()))
    }

    pub fn set_notion_token(&self, token: &str) -> Result<(), String> {
        let token = token.trim();
        if token.is_empty() || token.len() > 16_384 {
            return Err("Notion Integration Token 无效。".to_string());
        }
        Entry::new(SERVICE_NAME, NOTION_TOKEN_KEY)
            .map_err(|error| format!("打开 Notion 凭据失败：{error}"))?
            .set_password(token)
            .map_err(|error| format!("保存 Notion 凭据失败：{error}"))
    }

    pub fn delete_notion_token(&self) -> Result<bool, String> {
        match Entry::new(SERVICE_NAME, NOTION_TOKEN_KEY)
            .map_err(|error| format!("打开 Notion 凭据失败：{error}"))?
            .delete_credential()
        {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(error) => Err(format!("删除 Notion 凭据失败：{error}")),
        }
    }
}
