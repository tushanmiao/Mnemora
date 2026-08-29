use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MCP_SETTINGS_VERSION: u32 = 1;
pub const DEFAULT_STARTUP_TIMEOUT_MS: u64 = 15_000;
pub const DEFAULT_CALL_TIMEOUT_MS: u64 = 90_000;
pub const DEFAULT_MAX_OUTPUT_CHARS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum McpTransportConfig {
    StreamableHttp {
        url: String,
        #[serde(default)]
        has_bearer_token: bool,
    },
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    pub transport: McpTransportConfig,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub auto_approve_tools: Vec<String>,
    #[serde(default = "default_startup_timeout_ms")]
    pub startup_timeout_ms: u64,
    #[serde(default = "default_call_timeout_ms")]
    pub call_timeout_ms: u64,
    #[serde(default = "default_max_output_chars")]
    pub max_output_chars: usize,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,
    #[serde(default)]
    pub plugin_id: Option<String>,
}

impl McpServerConfig {
    pub fn normalize_and_validate(mut self) -> Result<Self, String> {
        self.id = self.id.trim().to_string();
        self.name = self.name.trim().to_string();
        validate_stable_id("MCP server ID", &self.id)?;
        if self.name.is_empty() || self.name.chars().count() > 100 {
            return Err("MCP server name must contain 1 to 100 characters".to_string());
        }
        if !(1_000..=120_000).contains(&self.startup_timeout_ms) {
            return Err("MCP startup timeout must be between 1000 and 120000 ms".to_string());
        }
        if !(1_000..=600_000).contains(&self.call_timeout_ms) {
            return Err("MCP call timeout must be between 1000 and 600000 ms".to_string());
        }
        if !(1_000..=200_000).contains(&self.max_output_chars) {
            return Err("MCP output limit must be between 1000 and 200000 characters".to_string());
        }
        if !(1..=8).contains(&self.max_concurrency) {
            return Err("MCP concurrency must be between 1 and 8".to_string());
        }
        normalize_tool_list(&mut self.allowed_tools)?;
        normalize_tool_list(&mut self.auto_approve_tools)?;
        if !self.allowed_tools.is_empty()
            && self
                .auto_approve_tools
                .iter()
                .any(|name| !self.allowed_tools.contains(name))
        {
            return Err("Automatically approved MCP tools must also be allowed".to_string());
        }
        if let Some(plugin_id) = self.plugin_id.as_mut() {
            *plugin_id = plugin_id.trim().to_string();
            validate_stable_id("Plugin ID", plugin_id)?;
        }
        match &mut self.transport {
            McpTransportConfig::StreamableHttp { url, .. } => {
                *url = validate_http_url(url)?;
            }
            McpTransportConfig::Stdio {
                command,
                args,
                cwd,
                env,
            } => {
                *command = command.trim().to_string();
                if command.is_empty() || command.chars().count() > 2_000 {
                    return Err("MCP stdio command is invalid".to_string());
                }
                if args.len() > 100 || args.iter().any(|value| value.len() > 8_192) {
                    return Err("MCP stdio arguments exceed the safety limit".to_string());
                }
                if let Some(value) = cwd {
                    *value = value.trim().to_string();
                    if value.is_empty() || value.chars().count() > 2_000 {
                        return Err("MCP stdio working directory is invalid".to_string());
                    }
                }
                if env.len() > 100
                    || env.iter().any(|(key, value)| {
                        key.is_empty()
                            || key.len() > 256
                            || value.len() > 16_384
                            || !key
                                .chars()
                                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    })
                {
                    return Err("MCP stdio environment exceeds the safety limit".to_string());
                }
            }
        }
        Ok(self)
    }

    pub fn permits_tool(&self, name: &str) -> bool {
        self.allowed_tools.is_empty() || self.allowed_tools.iter().any(|value| value == name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSettings {
    #[serde(default = "settings_version")]
    pub version: u32,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolSnapshot {
    pub server_id: String,
    pub server_name: String,
    pub remote_name: String,
    pub wire_name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
    pub auto_approved: bool,
    pub max_output_chars: usize,
    pub catalog_revision: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallOutput {
    pub content: String,
    pub is_error: bool,
    pub output_chars: usize,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub server_id: String,
    pub state: McpConnectionState,
    pub tool_count: usize,
    pub catalog_revision: Option<String>,
    pub last_success_at: Option<u64>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub retry_after: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpConnectionState {
    #[default]
    Disabled,
    Cached,
    Connecting,
    Ready,
    Backoff,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    #[serde(flatten)]
    pub config: McpServerConfig,
    pub status: McpServerStatus,
    pub tools: Vec<McpToolSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOverview {
    pub servers: Vec<McpServerView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpCatalogCache {
    pub version: u32,
    #[serde(default)]
    pub servers: BTreeMap<String, McpCachedServer>,
}

impl Default for McpCatalogCache {
    fn default() -> Self {
        Self {
            version: 1,
            servers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpCachedServer {
    pub config_fingerprint: String,
    pub catalog_revision: String,
    pub discovered_at: u64,
    pub tools: Vec<McpToolSnapshot>,
}

const fn settings_version() -> u32 {
    MCP_SETTINGS_VERSION
}
const fn default_startup_timeout_ms() -> u64 {
    DEFAULT_STARTUP_TIMEOUT_MS
}
const fn default_call_timeout_ms() -> u64 {
    DEFAULT_CALL_TIMEOUT_MS
}
const fn default_max_output_chars() -> usize {
    DEFAULT_MAX_OUTPUT_CHARS
}
const fn default_max_concurrency() -> usize {
    1
}

pub fn validate_stable_id(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(format!(
            "{label} must contain only ASCII letters, digits, dots, hyphens, or underscores"
        ));
    }
    Ok(())
}

fn normalize_tool_list(values: &mut Vec<String>) -> Result<(), String> {
    if values.len() > 512 {
        return Err("MCP tool allowlist exceeds 512 entries".to_string());
    }
    for value in values.iter_mut() {
        *value = value.trim().to_string();
        if value.is_empty() || value.chars().count() > 256 {
            return Err("MCP tool name is invalid".to_string());
        }
    }
    values.sort();
    values.dedup();
    Ok(())
}

fn validate_http_url(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value.trim())
        .map_err(|error| format!("MCP URL is invalid: {error}"))?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("MCP URL must not contain credentials or a fragment".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "MCP URL must contain a host".to_string())?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback_host(host) => {}
        "http" => {
            return Err(
                "Plain HTTP MCP endpoints are allowed only on localhost or loopback addresses"
                    .to_string(),
            )
        }
        _ => return Err("MCP URL must use HTTPS or loopback HTTP".to_string()),
    }
    Ok(url.to_string())
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_transport_rejects_remote_plaintext() {
        assert!(validate_http_url("http://example.com/mcp").is_err());
        assert!(validate_http_url("http://127.0.0.1:8123/mcp").is_ok());
        assert!(validate_http_url("https://example.com/mcp").is_ok());
    }
}
