use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

use super::types::validate_stable_id;

const SERVICE_NAME: &str = "com.mnemora.app.mcp-server";

#[derive(Debug, Clone, Copy, Default)]
pub struct McpSecretStore;

impl McpSecretStore {
    pub fn get_bearer_token(&self, server_id: &str) -> Result<Option<Zeroizing<String>>, String> {
        match entry(server_id)?.get_password() {
            Ok(value) if !value.trim().is_empty() => Ok(Some(Zeroizing::new(value))),
            Ok(_) | Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(format!("Failed to read MCP credential: {error}")),
        }
    }

    pub fn set_bearer_token(&self, server_id: &str, token: &str) -> Result<(), String> {
        let token = token.trim();
        if token.is_empty() || token.len() > 16_384 {
            return Err("MCP bearer token is empty or too long".to_string());
        }
        entry(server_id)?
            .set_password(token)
            .map_err(|error| format!("Failed to save MCP credential: {error}"))
    }

    pub fn delete_bearer_token(&self, server_id: &str) -> Result<bool, String> {
        match entry(server_id)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(error) => Err(format!("Failed to delete MCP credential: {error}")),
        }
    }

    pub fn has_bearer_token(&self, server_id: &str) -> Result<bool, String> {
        Ok(self.get_bearer_token(server_id)?.is_some())
    }
}

fn entry(server_id: &str) -> Result<Entry, String> {
    validate_stable_id("MCP server ID", server_id.trim())?;
    Entry::new(SERVICE_NAME, server_id.trim())
        .map_err(|error| format!("Failed to open MCP credential entry: {error}"))
}
