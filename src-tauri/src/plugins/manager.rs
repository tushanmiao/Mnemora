use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    mcp::{McpManager, McpTransportConfig},
    skills::{stage_package_source, SkillRepository},
};

use super::types::{
    PluginCapabilities, PluginInstallRequest, PluginManifest, PluginOverview, PluginPermissions,
    PluginSignatureStatus, PluginSkillContribution, PluginStateEntry, PluginStateFile,
    PluginSummary,
};

const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

#[derive(Clone)]
pub struct PluginManager {
    root: PathBuf,
    packages: PathBuf,
    staging: PathBuf,
    backups: PathBuf,
    state_path: PathBuf,
    skills: SkillRepository,
    mcp: McpManager,
}

impl PluginManager {
    pub fn new(data_dir: PathBuf, skills: SkillRepository, mcp: McpManager) -> Self {
        let root = data_dir.join("plugins");
        Self {
            packages: root.join("packages"),
            staging: root.join("staging"),
            backups: root.join("backups"),
            state_path: root.join("state.json"),
            root,
            skills,
            mcp,
        }
    }

    pub fn list(&self) -> Result<PluginOverview, String> {
        self.ensure_directories()?;
        let state = self.read_state()?;
        let mut plugins = Vec::new();
        let mut warnings = Vec::new();
        for (id, entry) in state.plugins {
            match self.load_manifest(&self.packages.join(&id)) {
                Ok(manifest) => match self.summary(&manifest, &entry) {
                    Ok(summary) => plugins.push(summary),
                    Err(error) => warnings.push(format!("Plugin {id}: {error}")),
                },
                Err(error) => warnings.push(format!("Plugin {id}: {error}")),
            }
        }
        plugins.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(PluginOverview { plugins, warnings })
    }

    pub fn install(
        &self,
        source: &Path,
        request: PluginInstallRequest,
    ) -> Result<PluginSummary, String> {
        self.ensure_directories()?;
        if !source.is_absolute() {
            return Err("Plugin source path must be absolute".to_string());
        }
        let operation = self.staging.join(Uuid::new_v4().to_string());
        fs::create_dir(&operation)
            .map_err(|error| format!("Failed to create plugin staging directory: {error}"))?;
        let extracted = operation.join("extracted");
        let result = (|| {
            stage_package_source(source, request.kind, &extracted)?;
            let package_root = prepare_plugin_package(&extracted)?;
            let manifest = self.load_manifest(&package_root)?;
            self.validate_package(&package_root, &manifest, request.allow_unsigned)?;
            let mut state = self.read_state()?;
            if state
                .plugins
                .get(&manifest.id)
                .is_some_and(|entry| entry.enabled)
            {
                return Err("Disable the plugin before replacing it".to_string());
            }
            let destination = self.packages.join(&manifest.id);
            if destination.exists() && !request.replace_existing {
                return Err("Plugin is already installed".to_string());
            }
            let backup = self.backups.join(&manifest.id);
            if destination.exists() {
                if backup.exists() {
                    checked_remove_dir(&self.backups, &backup)?;
                }
                fs::rename(&destination, &backup)
                    .map_err(|error| format!("Failed to save plugin rollback copy: {error}"))?;
            }
            if let Err(error) = fs::rename(&package_root, &destination) {
                if backup.exists() {
                    let _ = fs::rename(&backup, &destination);
                }
                return Err(format!("Failed to install plugin: {error}"));
            }
            let installed_at = now_ms();
            state.plugins.insert(
                manifest.id.clone(),
                PluginStateEntry {
                    enabled: false,
                    installed_at,
                },
            );
            if let Err(error) = self.write_state(&state) {
                let _ = checked_remove_dir(&self.packages, &destination);
                if backup.exists() {
                    let _ = fs::rename(&backup, &destination);
                }
                return Err(error);
            }
            self.summary(
                &manifest,
                &PluginStateEntry {
                    enabled: false,
                    installed_at,
                },
            )
        })();
        let _ = fs::remove_dir_all(&operation);
        result
    }

    pub fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<PluginSummary, String> {
        validate_id("Plugin ID", plugin_id)?;
        let package = self.packages.join(plugin_id);
        let manifest = self.load_manifest(&package)?;
        let mut state = self.read_state()?;
        let entry = state
            .plugins
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| format!("Unknown plugin: {plugin_id}"))?;
        if entry.enabled == enabled {
            return self.summary(&manifest, &entry);
        }
        if enabled {
            let mut installed_skills = Vec::new();
            for contribution in &manifest.capabilities.skills {
                let source = resolve_package_path(&package, &contribution.path)?;
                match self.skills.install_plugin_skill(plugin_id, &source) {
                    Ok(skill) => installed_skills.push(skill.id),
                    Err(error) => {
                        let _ = self.skills.remove_plugin_skills(plugin_id);
                        return Err(error);
                    }
                }
            }
            let servers = namespaced_servers(&manifest)?;
            if let Err(error) = self.mcp.replace_plugin_servers(plugin_id, servers) {
                let _ = self.skills.remove_plugin_skills(plugin_id);
                return Err(error);
            }
        } else {
            self.mcp.remove_plugin_servers(plugin_id)?;
            self.skills.remove_plugin_skills(plugin_id)?;
        }
        let next = PluginStateEntry {
            enabled,
            installed_at: entry.installed_at,
        };
        state.plugins.insert(plugin_id.to_string(), next.clone());
        self.write_state(&state)?;
        self.summary(&manifest, &next)
    }

    pub fn rollback(&self, plugin_id: &str) -> Result<PluginSummary, String> {
        validate_id("Plugin ID", plugin_id)?;
        let mut state = self.read_state()?;
        let entry = state
            .plugins
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| format!("Unknown plugin: {plugin_id}"))?;
        if entry.enabled {
            return Err("Disable the plugin before rollback".to_string());
        }
        let destination = self.packages.join(plugin_id);
        let backup = self.backups.join(plugin_id);
        if !backup.is_dir() {
            return Err("No rollback version is available".to_string());
        }
        let swap = self.staging.join(format!("rollback-{}", Uuid::new_v4()));
        fs::rename(&destination, &swap)
            .map_err(|error| format!("Failed to stage current plugin version: {error}"))?;
        if let Err(error) = fs::rename(&backup, &destination) {
            let _ = fs::rename(&swap, &destination);
            return Err(format!(
                "Failed to restore plugin rollback version: {error}"
            ));
        }
        if let Err(error) = fs::rename(&swap, &backup) {
            let _ = fs::rename(&destination, &swap);
            let _ = fs::rename(&backup, &destination);
            let _ = fs::rename(&swap, &backup);
            return Err(format!(
                "Failed to preserve replaced plugin version: {error}"
            ));
        }
        state.plugins.insert(plugin_id.to_string(), entry.clone());
        self.write_state(&state)?;
        self.summary(&self.load_manifest(&destination)?, &entry)
    }

    pub fn uninstall(&self, plugin_id: &str) -> Result<bool, String> {
        validate_id("Plugin ID", plugin_id)?;
        let mut state = self.read_state()?;
        let Some(entry) = state.plugins.get(plugin_id).cloned() else {
            return Ok(false);
        };
        if entry.enabled {
            return Err("Disable the plugin before uninstalling it".to_string());
        }
        self.mcp.remove_plugin_servers(plugin_id)?;
        self.skills.remove_plugin_skills(plugin_id)?;
        let destination = self.packages.join(plugin_id);
        if destination.exists() {
            checked_remove_dir(&self.packages, &destination)?;
        }
        let backup = self.backups.join(plugin_id);
        if backup.exists() {
            checked_remove_dir(&self.backups, &backup)?;
        }
        state.plugins.remove(plugin_id);
        self.write_state(&state)?;
        Ok(true)
    }

    fn validate_package(
        &self,
        root: &Path,
        manifest: &PluginManifest,
        allow_unsigned: bool,
    ) -> Result<(), String> {
        if manifest.schema_version != 1 {
            return Err("Only plugin manifest schemaVersion 1 is supported".to_string());
        }
        validate_id("Plugin ID", &manifest.id)?;
        validate_text("Plugin name", &manifest.name, 100)?;
        validate_text("Plugin version", &manifest.version, 64)?;
        validate_text("Plugin publisher", &manifest.publisher, 200)?;
        if manifest.signature.is_none() && !allow_unsigned {
            return Err("Unsigned plugins require explicit confirmation".to_string());
        }
        if manifest.capabilities.skills.len() > 64
            || manifest.capabilities.mcp_servers.len() > 64
            || manifest.artifacts.len() > 1_024
        {
            return Err("Plugin manifest exceeds capability limits".to_string());
        }
        let mut skill_ids = HashSet::new();
        for contribution in &manifest.capabilities.skills {
            let directory = resolve_package_path(root, &contribution.path)?;
            if !directory.is_dir() {
                return Err(format!(
                    "Plugin skill path is not a directory: {}",
                    contribution.path
                ));
            }
            let skill = self.skills.inspect_plugin_skill(&directory)?;
            if !skill_ids.insert(skill.id.clone()) {
                return Err(format!("Duplicate plugin skill ID: {}", skill.id));
            }
        }
        let domains = manifest
            .permissions
            .network_domains
            .iter()
            .map(|domain| domain.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        for server in &manifest.capabilities.mcp_servers {
            let normalized = server.clone().normalize_and_validate()?;
            match &normalized.transport {
                McpTransportConfig::StreamableHttp { url, .. } => {
                    let host = reqwest::Url::parse(url)
                        .ok()
                        .and_then(|value| value.host_str().map(str::to_ascii_lowercase))
                        .ok_or_else(|| "Plugin MCP URL has no host".to_string())?;
                    if !domains.contains(&host) {
                        return Err(format!(
                            "Plugin must declare MCP network domain permission: {host}"
                        ));
                    }
                }
                McpTransportConfig::Stdio { .. } => {
                    return Err(
                        "Declarative plugins cannot contribute executable stdio MCP servers"
                            .to_string(),
                    )
                }
            }
        }
        for artifact in &manifest.artifacts {
            let path = resolve_package_path(root, &artifact.path)?;
            if !path.is_file() {
                return Err(format!("Plugin artifact does not exist: {}", artifact.path));
            }
            let actual = format!(
                "{:x}",
                Sha256::digest(
                    fs::read(&path)
                        .map_err(|error| format!("Failed to hash plugin artifact: {error}"))?
                )
            );
            if actual != artifact.sha256.to_ascii_lowercase() {
                return Err(format!("Plugin artifact hash mismatch: {}", artifact.path));
            }
        }
        Ok(())
    }

    fn summary(
        &self,
        manifest: &PluginManifest,
        state: &PluginStateEntry,
    ) -> Result<PluginSummary, String> {
        let package = self.packages.join(&manifest.id);
        let mut skill_ids = Vec::new();
        for contribution in &manifest.capabilities.skills {
            skill_ids.push(
                self.skills
                    .inspect_plugin_skill(&resolve_package_path(&package, &contribution.path)?)?
                    .id,
            );
        }
        let rollback_version = self
            .load_manifest(&self.backups.join(&manifest.id))
            .ok()
            .map(|value| value.version);
        Ok(PluginSummary {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            publisher: manifest.publisher.clone(),
            enabled: state.enabled,
            signature_status: if manifest.signature.is_some() {
                PluginSignatureStatus::Unverified
            } else {
                PluginSignatureStatus::Unsigned
            },
            skill_ids,
            mcp_server_ids: manifest
                .capabilities
                .mcp_servers
                .iter()
                .map(|server| format!("{}.{}", manifest.id, server.id))
                .collect(),
            permissions: manifest.permissions.clone(),
            installed_at: state.installed_at,
            rollback_version,
        })
    }

    fn load_manifest(&self, root: &Path) -> Result<PluginManifest, String> {
        let path = root.join("plugin.json");
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Failed to read plugin manifest metadata: {error}"))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
            return Err("plugin.json is empty or too large".to_string());
        }
        serde_json::from_slice(
            &fs::read(path).map_err(|error| format!("Failed to read plugin.json: {error}"))?,
        )
        .map_err(|error| format!("Failed to parse plugin.json: {error}"))
    }

    fn ensure_directories(&self) -> Result<(), String> {
        for directory in [&self.root, &self.packages, &self.staging, &self.backups] {
            fs::create_dir_all(directory)
                .map_err(|error| format!("Failed to create plugin directory: {error}"))?;
        }
        Ok(())
    }

    fn read_state(&self) -> Result<PluginStateFile, String> {
        match fs::read(&self.state_path) {
            Ok(bytes) => {
                let state: PluginStateFile = serde_json::from_slice(&bytes)
                    .map_err(|error| format!("Plugin state file is damaged: {error}"))?;
                if state.version != 1 {
                    return Err("Unsupported plugin state version".to_string());
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(PluginStateFile::default())
            }
            Err(error) => Err(format!("Failed to read plugin state: {error}")),
        }
    }

    fn write_state(&self, state: &PluginStateFile) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|error| format!("Failed to serialize plugin state: {error}"))?;
        let temporary = self.state_path.with_extension("tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Failed to create plugin state: {error}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("Failed to persist plugin state: {error}"))?;
        drop(file);
        if self.state_path.exists() {
            fs::remove_file(&self.state_path)
                .map_err(|error| format!("Failed to replace plugin state: {error}"))?;
        }
        fs::rename(temporary, &self.state_path)
            .map_err(|error| format!("Failed to commit plugin state: {error}"))
    }
}

fn namespaced_servers(
    manifest: &PluginManifest,
) -> Result<Vec<crate::mcp::McpServerConfig>, String> {
    manifest
        .capabilities
        .mcp_servers
        .iter()
        .cloned()
        .map(|mut server| {
            server.id = format!("{}.{}", manifest.id, server.id);
            server.name = format!("{} · {}", manifest.name, server.name);
            server.plugin_id = Some(manifest.id.clone());
            server.enabled = false;
            server.normalize_and_validate()
        })
        .collect()
}

pub(crate) fn prepare_plugin_package(extracted: &Path) -> Result<PathBuf, String> {
    let root = find_package_root(extracted)?;
    materialize_codex_manifest(&root)?;
    Ok(root)
}

fn has_plugin_manifest(root: &Path) -> bool {
    root.join("plugin.json").is_file() || root.join(".codex-plugin").join("plugin.json").is_file()
}

fn find_package_root(extracted: &Path) -> Result<PathBuf, String> {
    if has_plugin_manifest(extracted) {
        return Ok(extracted.to_path_buf());
    }
    let roots = fs::read_dir(extracted)
        .map_err(|error| format!("Failed to scan plugin package: {error}"))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        })
        .filter(|path| has_plugin_manifest(path))
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err(
            "Plugin package must contain exactly one plugin.json or .codex-plugin/plugin.json root"
                .to_string(),
        );
    }
    Ok(roots[0].clone())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexPluginManifest {
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: Option<CodexPluginAuthor>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    skills: Option<CodexSkillLocations>,
    #[serde(default)]
    interface: Option<CodexPluginInterface>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CodexPluginAuthor {
    Name(String),
    Detail { name: String },
}

impl CodexPluginAuthor {
    fn name(self) -> String {
        match self {
            Self::Name(value) => value,
            Self::Detail { name } => name,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CodexSkillLocations {
    One(String),
    Many(Vec<String>),
}

impl CodexSkillLocations {
    fn values(self) -> Vec<String> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexPluginInterface {
    #[serde(default)]
    display_name: String,
}

/// Codex 插件把清单放在 `.codex-plugin/plugin.json`，并用 `skills` 指向
/// Skill 目录。Mnemora 的运行时清单更严格，因此只在安装暂存区生成一份
/// 等价的声明式清单；上游文件保持原样，连接器、Hooks、UI 等未支持能力
/// 不会被当作本地代码执行。
fn materialize_codex_manifest(root: &Path) -> Result<(), String> {
    if root.join("plugin.json").is_file() {
        return Ok(());
    }
    let codex_path = root.join(".codex-plugin").join("plugin.json");
    let metadata = fs::metadata(&codex_path)
        .map_err(|error| format!("Failed to read Codex plugin manifest metadata: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(".codex-plugin/plugin.json is empty or too large".to_string());
    }
    let source: CodexPluginManifest = serde_json::from_slice(
        &fs::read(&codex_path)
            .map_err(|error| format!("Failed to read .codex-plugin/plugin.json: {error}"))?,
    )
    .map_err(|error| format!("Failed to parse .codex-plugin/plugin.json: {error}"))?;

    let locations = source
        .skills
        .map(CodexSkillLocations::values)
        .unwrap_or_else(|| vec!["skills".to_string()]);
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve Codex plugin root: {error}"))?;
    let mut contributions = Vec::new();
    let mut seen = HashSet::new();
    for location in locations {
        let normalized = location.trim().trim_start_matches("./");
        if normalized.is_empty() {
            continue;
        }
        let directory = resolve_package_path(root, normalized)?;
        if !directory.is_dir() {
            return Err(format!(
                "Codex plugin skill path is not a directory: {location}"
            ));
        }
        for skill_root in crate::skills::find_skill_roots(&directory, 0)? {
            let relative = skill_root
                .strip_prefix(&canonical_root)
                .map_err(|_| "Codex plugin skill path escapes the package root".to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if seen.insert(relative.clone()) {
                contributions.push(PluginSkillContribution { path: relative });
            }
        }
    }
    if contributions.is_empty() {
        return Err(
            "该 Codex 插件没有可由 Mnemora 运行的 Skill；连接器、Hooks 或专用 UI 不能作为本地 Skill 安装。"
                .to_string(),
        );
    }
    if contributions.len() > 64 {
        return Err("Codex plugin contains more than 64 skills".to_string());
    }

    let display_name = source
        .interface
        .as_ref()
        .map(|value| value.display_name.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(source.name.as_str())
        .to_string();
    let manifest = PluginManifest {
        schema_version: 1,
        id: source.name.clone(),
        name: display_name,
        version: source.version,
        description: source.description,
        publisher: source
            .author
            .map(CodexPluginAuthor::name)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Codex plugin author".to_string()),
        license: source.license,
        compatibility: Some(
            "从 Codex 插件清单兼容导入；Mnemora 仅启用其中的 Agent Skills。".to_string(),
        ),
        capabilities: PluginCapabilities {
            skills: contributions,
            mcp_servers: Vec::new(),
        },
        permissions: PluginPermissions::default(),
        artifacts: Vec::new(),
        signature: None,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("Failed to convert Codex plugin manifest: {error}"))?;
    fs::write(root.join("plugin.json"), bytes)
        .map_err(|error| format!("Failed to stage converted Codex plugin manifest: {error}"))
}

fn resolve_package_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Plugin manifest contains an unsafe relative path".to_string());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve plugin root: {error}"))?;
    let target = root.join(relative);
    let canonical = target
        .canonicalize()
        .map_err(|error| format!("Failed to resolve plugin path: {error}"))?;
    if !canonical.starts_with(&root) {
        return Err("Plugin path escapes the package root".to_string());
    }
    Ok(canonical)
}

fn checked_remove_dir(root: &Path, target: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("Failed to resolve plugin storage root: {error}"))?;
    let target = target
        .canonicalize()
        .map_err(|error| format!("Failed to resolve plugin removal target: {error}"))?;
    if target.parent() != Some(root.as_path()) {
        return Err("Plugin removal target is outside its storage root".to_string());
    }
    fs::remove_dir_all(target)
        .map_err(|error| format!("Failed to remove plugin directory: {error}"))
}

fn validate_id(label: &str, value: &str) -> Result<(), String> {
    crate::mcp::types::validate_stable_id(label, value.trim())
}

fn validate_text(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > max {
        return Err(format!("{label} is empty or too long"));
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::{SkillImportKind, SkillSource};

    #[test]
    fn manifest_paths_cannot_escape_package() {
        let root = std::env::temp_dir().join(format!("mnemora-plugin-path-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        assert!(resolve_package_path(&root, "../outside").is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn install_enable_disable_and_uninstall_are_separate_lifecycle_steps() {
        let root = std::env::temp_dir().join(format!("mnemora-plugin-life-{}", Uuid::new_v4()));
        let source = root.join("source");
        let skill = source.join("skills").join("demo-skill");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nid: demo-skill\nname: Demo skill\ndescription: A plugin lifecycle test skill.\n---\nUse this test skill.\n",
        )
        .unwrap();
        fs::write(
            source.join("plugin.json"),
            r#"{
              "schemaVersion": 1,
              "id": "demo-plugin",
              "name": "Demo plugin",
              "version": "1.0.0",
              "publisher": "Mnemora tests",
              "capabilities": {
                "skills": [{ "path": "skills/demo-skill" }],
                "mcpServers": [{
                  "id": "remote",
                  "name": "Remote tools",
                  "enabled": false,
                  "transport": {
                    "type": "streamableHttp",
                    "url": "https://example.com/mcp",
                    "hasBearerToken": false
                  },
                  "allowedTools": [],
                  "autoApproveTools": [],
                  "startupTimeoutMs": 15000,
                  "callTimeoutMs": 90000,
                  "maxOutputChars": 20000,
                  "maxConcurrency": 1,
                  "pluginId": null
                }]
              },
              "permissions": { "networkDomains": ["example.com"], "secrets": [] },
              "artifacts": []
            }"#,
        )
        .unwrap();

        let data = root.join("data");
        let skills =
            SkillRepository::new(root.join("resources").join("skills"), data.join("skills"));
        let mcp = McpManager::new(root.join("config"), data.clone()).unwrap();
        let manager = PluginManager::new(data, skills.clone(), mcp.clone());
        let installed = manager
            .install(
                &source,
                PluginInstallRequest {
                    kind: SkillImportKind::Directory,
                    replace_existing: false,
                    allow_unsigned: true,
                },
            )
            .unwrap();
        assert!(!installed.enabled);
        assert!(skills
            .list()
            .unwrap()
            .skills
            .iter()
            .all(|value| value.id != "demo-skill"));

        assert!(manager.set_enabled("demo-plugin", true).unwrap().enabled);
        assert_eq!(
            skills
                .list()
                .unwrap()
                .skills
                .iter()
                .find(|value| value.id == "demo-skill")
                .unwrap()
                .source,
            SkillSource::Plugin
        );
        let servers = mcp.overview().unwrap().servers;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].config.id, "demo-plugin.remote");
        assert!(!servers[0].config.enabled);

        assert!(!manager.set_enabled("demo-plugin", false).unwrap().enabled);
        assert!(mcp.overview().unwrap().servers.is_empty());
        assert!(skills
            .list()
            .unwrap()
            .skills
            .iter()
            .all(|value| value.id != "demo-skill"));
        assert!(manager.uninstall("demo-plugin").unwrap());
        assert!(manager.list().unwrap().plugins.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn installs_skill_based_codex_plugin_manifest() {
        let root = std::env::temp_dir().join(format!("mnemora-codex-plugin-{}", Uuid::new_v4()));
        let source = root.join("source");
        let skill = source.join("skills").join("question-helper");
        fs::create_dir_all(source.join(".codex-plugin")).unwrap();
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            source.join(".codex-plugin").join("plugin.json"),
            r#"{
              "name": "codex-helper",
              "version": "1.2.3",
              "description": "A Codex-compatible skills plugin.",
              "author": { "name": "Example" },
              "license": "MIT",
              "skills": "./skills/",
              "interface": { "displayName": "Codex Helper" }
            }"#,
        )
        .unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: question-helper\ndescription: Frame a question.\n---\nUse this skill.\n",
        )
        .unwrap();

        let data = root.join("data");
        let skills = SkillRepository::new(root.join("builtin"), data.join("skills"));
        let mcp = McpManager::new(root.join("config"), data.clone()).unwrap();
        let manager = PluginManager::new(data, skills.clone(), mcp);
        let installed = manager
            .install(
                &source,
                PluginInstallRequest {
                    kind: SkillImportKind::Directory,
                    replace_existing: false,
                    allow_unsigned: true,
                },
            )
            .unwrap();
        assert_eq!(installed.id, "codex-helper");
        assert_eq!(installed.name, "Codex Helper");
        assert_eq!(installed.skill_ids, vec!["question-helper"]);
        assert!(!installed.enabled);

        manager.set_enabled("codex-helper", true).unwrap();
        assert!(skills
            .list()
            .unwrap()
            .skills
            .iter()
            .any(|value| value.id == "question-helper"));
        let _ = fs::remove_dir_all(root);
    }
}
