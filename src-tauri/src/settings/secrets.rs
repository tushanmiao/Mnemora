//! API Key 系统凭据存储。
//!
//! 每个供应商使用稳定的 provider ID 作为凭据用户名，服务名固定为 Mnemora。
//! Windows 上由 `keyring` 使用 Credential Manager；本模块提供读取、状态检查、
//! 写入和删除四个操作，错误信息不会包含完整 API Key。

use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

use super::types::validate_stable_id;

const SERVICE_NAME: &str = "com.mnemora.app.model-provider";
// Keep MinerU credentials in a separate keyring namespace.  Reusing a model
// provider id would make a provider rename or settings import accidentally
// overwrite the document-processing credential.
const MINERU_SERVICE_NAME: &str = "com.mnemora.app.mineru-cloud";
const MINERU_ACCOUNT: &str = "api-token";

#[derive(Clone, Copy, Default)]
pub struct SecretStore;

impl SecretStore {
    pub fn refresh_api_key_statuses(
        &self,
        settings: &mut super::types::ModelSettings,
    ) -> Result<(), String> {
        for provider in &mut settings.providers {
            provider.has_api_key = self.has_api_key(&provider.id)?;
        }
        Ok(())
    }

    pub fn get_api_key(&self, provider_id: &str) -> Result<Option<String>, String> {
        let entry = entry(provider_id)?;
        match entry.get_password() {
            Ok(api_key) => Ok(Some(api_key)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(format!(
                "Failed to read API Key from system credentials: {error}"
            )),
        }
    }

    pub fn has_api_key(&self, provider_id: &str) -> Result<bool, String> {
        let api_key = self.get_api_key(provider_id)?.map(Zeroizing::new);
        Ok(api_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()))
    }

    pub fn set_api_key(&self, provider_id: &str, api_key: &str) -> Result<(), String> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err("API Key cannot be empty".to_string());
        }
        if api_key.len() > 16_384 {
            return Err("API Key is too long".to_string());
        }

        entry(provider_id)?
            .set_password(api_key)
            .map_err(|error| format!("Failed to save API Key to system credentials: {error}"))
    }

    pub fn delete_api_key(&self, provider_id: &str) -> Result<bool, String> {
        match entry(provider_id)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(error) => Err(format!(
                "Failed to delete API Key from system credentials: {error}"
            )),
        }
    }

    /// Read the MinerU Cloud token from the operating-system credential store.
    /// The token is deliberately not part of AppSettings or any serializable
    /// settings bundle.
    pub fn get_mineru_token(&self) -> Result<Option<String>, String> {
        let entry = mineru_entry()?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(format!(
                "Failed to read MinerU token from system credentials: {error}"
            )),
        }
    }

    pub fn has_mineru_token(&self) -> Result<bool, String> {
        let token = self.get_mineru_token()?.map(Zeroizing::new);
        Ok(token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()))
    }

    pub fn set_mineru_token(&self, token: &str) -> Result<(), String> {
        let token = token.trim();
        if token.is_empty() {
            return Err("MinerU token cannot be empty".to_string());
        }
        if token.len() > 16_384 || token.chars().any(char::is_control) {
            return Err("MinerU token is invalid or too long".to_string());
        }
        mineru_entry()?
            .set_password(token)
            .map_err(|error| format!("Failed to save MinerU token to system credentials: {error}"))
    }

    pub fn delete_mineru_token(&self) -> Result<bool, String> {
        match mineru_entry()?.delete_credential() {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(error) => Err(format!(
                "Failed to delete MinerU token from system credentials: {error}"
            )),
        }
    }
}

fn entry(provider_id: &str) -> Result<Entry, String> {
    let provider_id = provider_id.trim();
    validate_stable_id("Provider ID", provider_id)?;
    Entry::new(SERVICE_NAME, provider_id)
        .map_err(|error| format!("Failed to open system credential entry: {error}"))
}

fn mineru_entry() -> Result<Entry, String> {
    Entry::new(MINERU_SERVICE_NAME, MINERU_ACCOUNT)
        .map_err(|error| format!("Failed to open MinerU system credential entry: {error}"))
}

#[cfg(test)]
mod tests {
    use super::entry;

    #[test]
    fn rejects_invalid_provider_id_before_opening_keyring() {
        let error = match entry("../invalid") {
            Ok(_) => panic!("invalid provider ID must be rejected"),
            Err(error) => error,
        };
        assert!(error.contains("Provider ID"));
    }
}
