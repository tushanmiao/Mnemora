use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{mcp::McpServerConfig, skills::types::SkillImportKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub publisher: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub artifacts: Vec<PluginArtifact>,
    #[serde(default)]
    pub signature: Option<PluginSignature>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub skills: Vec<PluginSkillContribution>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginSkillContribution {
    pub path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPermissions {
    #[serde(default)]
    pub network_domains: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginArtifact {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginSignature {
    pub algorithm: String,
    pub key_id: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallRequest {
    pub kind: SkillImportKind,
    #[serde(default)]
    pub replace_existing: bool,
    #[serde(default)]
    pub allow_unsigned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub publisher: String,
    pub enabled: bool,
    pub signature_status: PluginSignatureStatus,
    pub skill_ids: Vec<String>,
    pub mcp_server_ids: Vec<String>,
    pub permissions: PluginPermissions,
    pub installed_at: u64,
    pub rollback_version: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginSignatureStatus {
    Unsigned,
    Unverified,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOverview {
    pub plugins: Vec<PluginSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginStateFile {
    pub version: u32,
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginStateEntry>,
}

impl Default for PluginStateFile {
    fn default() -> Self {
        Self {
            version: 1,
            plugins: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginStateEntry {
    pub enabled: bool,
    pub installed_at: u64,
}
