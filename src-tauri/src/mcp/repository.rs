use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use super::types::{McpCatalogCache, McpSettings, MCP_SETTINGS_VERSION};

#[derive(Debug, Clone)]
pub struct McpRepository {
    settings_path: PathBuf,
    cache_path: PathBuf,
}

impl McpRepository {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            settings_path: config_dir.join("mcp-servers.json"),
            cache_path: data_dir.join("mcp").join("catalog-cache.json"),
        }
    }

    pub fn load_settings(&self) -> Result<McpSettings, String> {
        if !self.settings_path.exists() {
            return Ok(McpSettings {
                version: MCP_SETTINGS_VERSION,
                servers: Vec::new(),
            });
        }
        let bytes = fs::read(&self.settings_path)
            .map_err(|error| format!("Failed to read MCP settings: {error}"))?;
        let mut settings: McpSettings = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse MCP settings: {error}"))?;
        if settings.version != MCP_SETTINGS_VERSION {
            return Err(format!(
                "Unsupported MCP settings version {}",
                settings.version
            ));
        }
        for server in &mut settings.servers {
            *server = server.clone().normalize_and_validate()?;
        }
        settings
            .servers
            .sort_by(|left, right| left.id.cmp(&right.id));
        if settings
            .servers
            .windows(2)
            .any(|pair| pair[0].id == pair[1].id)
        {
            return Err("MCP settings contain duplicate server IDs".to_string());
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &McpSettings) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|error| format!("Failed to serialize MCP settings: {error}"))?;
        atomic_write(&self.settings_path, &bytes)
    }

    pub fn load_cache(&self) -> Result<McpCatalogCache, String> {
        if !self.cache_path.exists() {
            return Ok(McpCatalogCache::default());
        }
        let bytes = fs::read(&self.cache_path)
            .map_err(|error| format!("Failed to read MCP catalog cache: {error}"))?;
        let cache: McpCatalogCache = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse MCP catalog cache: {error}"))?;
        if cache.version != 1 {
            return Ok(McpCatalogCache::default());
        }
        Ok(cache)
    }

    pub fn save_cache(&self, cache: &McpCatalogCache) -> Result<(), String> {
        let bytes = serde_json::to_vec(cache)
            .map_err(|error| format!("Failed to serialize MCP catalog cache: {error}"))?;
        atomic_write(&self.cache_path, &bytes)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "MCP storage path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create MCP storage directory: {error}"))?;
    let temporary = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|error| format!("Failed to create MCP temporary file: {error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("Failed to persist MCP temporary file: {error}"))?;
    drop(file);
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("Failed to replace MCP storage file: {error}"))?;
    }
    fs::rename(&temporary, path)
        .map_err(|error| format!("Failed to commit MCP storage file: {error}"))
}
