use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rmcp::{
    model::{CallToolRequestParams, ProtocolVersion, Tool},
    transport::{
        child_process::TokioChildProcess,
        streamable_http_client::{
            StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
        },
        ConfigureCommandExt,
    },
    ClientLifecycleMode, ClientServiceExt,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::{
    repository::McpRepository,
    secrets::McpSecretStore,
    types::{
        McpCachedServer, McpCallOutput, McpCatalogCache, McpConnectionState, McpOverview,
        McpServerConfig, McpServerStatus, McpServerView, McpSettings, McpToolSnapshot,
        McpTransportConfig,
    },
};

const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_ARGUMENT_DEPTH: usize = 20;
const MAX_TOOL_COUNT: usize = 512;
const MAX_SSE_EVENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_ERROR_CHARS: usize = 800;

#[derive(Clone)]
pub struct McpManager {
    inner: Arc<McpManagerInner>,
}

struct McpManagerInner {
    repository: McpRepository,
    secrets: McpSecretStore,
    http: reqwest13::Client,
    settings: RwLock<McpSettings>,
    cache: RwLock<McpCatalogCache>,
    statuses: RwLock<BTreeMap<String, McpServerStatus>>,
    operation_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    call_gates: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl McpManager {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Result<Self, String> {
        let repository = McpRepository::new(config_dir, data_dir);
        let http = reqwest13::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .redirect(reqwest13::redirect::Policy::none())
            .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("Failed to create MCP HTTP client: {error}"))?;
        let mut settings = repository.load_settings()?;
        let secrets = McpSecretStore;
        for server in &mut settings.servers {
            if let McpTransportConfig::StreamableHttp {
                has_bearer_token, ..
            } = &mut server.transport
            {
                *has_bearer_token = secrets.has_bearer_token(&server.id).unwrap_or(false);
            }
        }
        let mut cache = repository.load_cache().unwrap_or_default();
        cache.servers.retain(|server_id, cached| {
            settings.servers.iter().any(|server| {
                server.id == *server_id && config_fingerprint(server) == cached.config_fingerprint
            })
        });
        let mut statuses = BTreeMap::new();
        for server in &settings.servers {
            let cached = cache.servers.get(&server.id);
            statuses.insert(
                server.id.clone(),
                McpServerStatus {
                    server_id: server.id.clone(),
                    state: if !server.enabled {
                        McpConnectionState::Disabled
                    } else if cached.is_some() {
                        McpConnectionState::Cached
                    } else {
                        McpConnectionState::Failed
                    },
                    tool_count: cached.map_or(0, |value| value.tools.len()),
                    catalog_revision: cached.map(|value| value.catalog_revision.clone()),
                    last_success_at: cached.map(|value| value.discovered_at),
                    last_error: None,
                    consecutive_failures: 0,
                    retry_after: None,
                },
            );
        }
        Ok(Self {
            inner: Arc::new(McpManagerInner {
                repository,
                secrets,
                http,
                settings: RwLock::new(settings),
                cache: RwLock::new(cache),
                statuses: RwLock::new(statuses),
                operation_locks: Mutex::new(HashMap::new()),
                call_gates: Mutex::new(HashMap::new()),
            }),
        })
    }

    pub fn overview(&self) -> Result<McpOverview, String> {
        let settings = self
            .inner
            .settings
            .read()
            .map_err(|_| "MCP settings lock is unavailable".to_string())?;
        let cache = self
            .inner
            .cache
            .read()
            .map_err(|_| "MCP catalog lock is unavailable".to_string())?;
        let statuses = self
            .inner
            .statuses
            .read()
            .map_err(|_| "MCP status lock is unavailable".to_string())?;
        let mut servers = settings
            .servers
            .iter()
            .map(|server| McpServerView {
                config: server.clone(),
                status: statuses
                    .get(&server.id)
                    .cloned()
                    .unwrap_or_else(|| default_status(server)),
                tools: cache
                    .servers
                    .get(&server.id)
                    .map(|value| value.tools.clone())
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        servers.sort_by(|left, right| left.config.name.cmp(&right.config.name));
        Ok(McpOverview { servers })
    }

    pub fn catalog_tools(&self) -> Vec<McpToolSnapshot> {
        let Ok(settings) = self.inner.settings.read() else {
            return Vec::new();
        };
        let Ok(cache) = self.inner.cache.read() else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for server in settings.servers.iter().filter(|server| server.enabled) {
            if let Some(cached) = cache.servers.get(&server.id) {
                result.extend(
                    cached
                        .tools
                        .iter()
                        .filter(|tool| server.permits_tool(&tool.remote_name))
                        .cloned(),
                );
            }
        }
        result.sort_by(|left, right| left.wire_name.cmp(&right.wire_name));
        result
    }

    pub fn upsert_server(
        &self,
        config: McpServerConfig,
        bearer_token: Option<String>,
    ) -> Result<McpServerView, String> {
        let mut config = config.normalize_and_validate()?;
        if let Some(token) = bearer_token.as_deref().map(str::trim) {
            if token.is_empty() {
                self.inner.secrets.delete_bearer_token(&config.id)?;
            } else {
                self.inner.secrets.set_bearer_token(&config.id, token)?;
            }
        }
        if let McpTransportConfig::StreamableHttp {
            has_bearer_token, ..
        } = &mut config.transport
        {
            *has_bearer_token = self.inner.secrets.has_bearer_token(&config.id)?;
        }
        {
            let mut settings = self
                .inner
                .settings
                .write()
                .map_err(|_| "MCP settings lock is unavailable".to_string())?;
            if let Some(index) = settings
                .servers
                .iter()
                .position(|value| value.id == config.id)
            {
                let old = &settings.servers[index];
                if old.plugin_id.is_some() && old.plugin_id != config.plugin_id {
                    return Err("Plugin-owned MCP servers cannot be replaced manually".to_string());
                }
                settings.servers[index] = config.clone();
            } else {
                settings.servers.push(config.clone());
            }
            settings
                .servers
                .sort_by(|left, right| left.id.cmp(&right.id));
            self.inner.repository.save_settings(&settings)?;
        }
        self.invalidate_if_fingerprint_changed(&config)?;
        self.server_view(&config.id)
    }

    pub fn set_enabled(&self, server_id: &str, enabled: bool) -> Result<McpServerView, String> {
        let config = {
            let mut settings = self
                .inner
                .settings
                .write()
                .map_err(|_| "MCP settings lock is unavailable".to_string())?;
            let server = settings
                .servers
                .iter_mut()
                .find(|server| server.id == server_id)
                .ok_or_else(|| format!("Unknown MCP server: {server_id}"))?;
            server.enabled = enabled;
            let config = server.clone();
            self.inner.repository.save_settings(&settings)?;
            config
        };
        let mut statuses = self
            .inner
            .statuses
            .write()
            .map_err(|_| "MCP status lock is unavailable".to_string())?;
        let status = statuses
            .entry(config.id.clone())
            .or_insert_with(|| default_status(&config));
        status.state = if enabled {
            if status.tool_count > 0 {
                McpConnectionState::Cached
            } else {
                McpConnectionState::Failed
            }
        } else {
            McpConnectionState::Disabled
        };
        drop(statuses);
        self.server_view(server_id)
    }

    pub fn remove_server(&self, server_id: &str) -> Result<bool, String> {
        let removed = {
            let mut settings = self
                .inner
                .settings
                .write()
                .map_err(|_| "MCP settings lock is unavailable".to_string())?;
            if let Some(index) = settings
                .servers
                .iter()
                .position(|value| value.id == server_id)
            {
                if settings.servers[index].plugin_id.is_some() {
                    return Err("Remove this MCP server by uninstalling its plugin".to_string());
                }
                settings.servers.remove(index);
                self.inner.repository.save_settings(&settings)?;
                true
            } else {
                false
            }
        };
        if removed {
            let _ = self.inner.secrets.delete_bearer_token(server_id);
            let mut cache = self
                .inner
                .cache
                .write()
                .map_err(|_| "MCP catalog lock is unavailable".to_string())?;
            cache.servers.remove(server_id);
            self.inner.repository.save_cache(&cache)?;
            drop(cache);
            self.inner
                .statuses
                .write()
                .map_err(|_| "MCP status lock is unavailable".to_string())?
                .remove(server_id);
        }
        Ok(removed)
    }

    pub async fn refresh_server(
        &self,
        server_id: &str,
        force: bool,
    ) -> Result<McpServerView, String> {
        let lock = self.operation_lock(server_id).await;
        let _guard = lock.lock().await;
        let config = self.server_config(server_id)?;
        if !config.enabled {
            return Err("Enable the MCP server before refreshing its catalog".to_string());
        }
        if !force {
            if let Some(retry_after) = self
                .inner
                .statuses
                .read()
                .ok()
                .and_then(|values| values.get(server_id).and_then(|value| value.retry_after))
            {
                if retry_after > now_ms() {
                    return Err(format!("MCP server is backing off until {retry_after}"));
                }
            }
        }
        self.update_status(server_id, |status| {
            status.state = McpConnectionState::Connecting;
            status.last_error = None;
        });
        let discovery = tokio::time::timeout(
            Duration::from_millis(config.startup_timeout_ms),
            self.discover(&config),
        )
        .await
        .map_err(|_| "MCP discovery timed out".to_string())
        .and_then(|result| result);
        match discovery {
            Ok(tools) => {
                let cached = build_cached_server(&config, tools)?;
                {
                    let mut cache = self
                        .inner
                        .cache
                        .write()
                        .map_err(|_| "MCP catalog lock is unavailable".to_string())?;
                    cache.servers.insert(server_id.to_string(), cached.clone());
                    self.inner.repository.save_cache(&cache)?;
                }
                self.update_status(server_id, |status| {
                    status.state = McpConnectionState::Ready;
                    status.tool_count = cached.tools.len();
                    status.catalog_revision = Some(cached.catalog_revision.clone());
                    status.last_success_at = Some(cached.discovered_at);
                    status.last_error = None;
                    status.consecutive_failures = 0;
                    status.retry_after = None;
                });
                self.server_view(server_id)
            }
            Err(error) => {
                self.record_failure(server_id, &error);
                Err(error)
            }
        }
    }

    pub async fn call_tool(
        &self,
        server_id: &str,
        remote_name: &str,
        arguments: Value,
        cancellation: &CancellationToken,
    ) -> Result<McpCallOutput, String> {
        validate_arguments(&arguments)?;
        let config = self.server_config(server_id)?;
        if !config.enabled || !config.permits_tool(remote_name) {
            return Err("MCP tool is disabled or outside the server allowlist".to_string());
        }
        let _permit = self.acquire_call_permit(&config).await?;
        let operation = self.call_once(&config, remote_name, arguments);
        let result = tokio::select! {
            _ = cancellation.cancelled() => return Err("MCP tool call was cancelled".to_string()),
            result = tokio::time::timeout(Duration::from_millis(config.call_timeout_ms), operation) => {
                result.map_err(|_| "MCP tool call timed out; outcome is unknown and was not retried".to_string())?
            }
        };
        match result {
            Ok(result) => Ok(normalize_call_result(result, config.max_output_chars)),
            Err(error) => {
                self.record_failure(server_id, &error);
                Err(format!(
                    "{error}; the call was not retried because its outcome may be unknown"
                ))
            }
        }
    }

    pub fn replace_plugin_servers(
        &self,
        plugin_id: &str,
        mut contributions: Vec<McpServerConfig>,
    ) -> Result<(), String> {
        for contribution in &mut contributions {
            contribution.plugin_id = Some(plugin_id.to_string());
            contribution.enabled = false;
            *contribution = contribution.clone().normalize_and_validate()?;
        }
        let mut settings = self
            .inner
            .settings
            .write()
            .map_err(|_| "MCP settings lock is unavailable".to_string())?;
        let retained = settings
            .servers
            .iter()
            .filter(|server| server.plugin_id.as_deref() != Some(plugin_id))
            .cloned()
            .collect::<Vec<_>>();
        for contribution in &contributions {
            if retained.iter().any(|server| server.id == contribution.id) {
                return Err(format!(
                    "Plugin MCP server ID conflicts with an existing server: {}",
                    contribution.id
                ));
            }
        }
        settings.servers = retained;
        settings.servers.extend(contributions);
        settings
            .servers
            .sort_by(|left, right| left.id.cmp(&right.id));
        self.inner.repository.save_settings(&settings)
    }

    pub fn remove_plugin_servers(&self, plugin_id: &str) -> Result<(), String> {
        let removed_ids = {
            let mut settings = self
                .inner
                .settings
                .write()
                .map_err(|_| "MCP settings lock is unavailable".to_string())?;
            let removed = settings
                .servers
                .iter()
                .filter(|server| server.plugin_id.as_deref() == Some(plugin_id))
                .map(|server| server.id.clone())
                .collect::<Vec<_>>();
            settings
                .servers
                .retain(|server| server.plugin_id.as_deref() != Some(plugin_id));
            self.inner.repository.save_settings(&settings)?;
            removed
        };
        let mut cache = self
            .inner
            .cache
            .write()
            .map_err(|_| "MCP catalog lock is unavailable".to_string())?;
        let mut statuses = self
            .inner
            .statuses
            .write()
            .map_err(|_| "MCP status lock is unavailable".to_string())?;
        for id in removed_ids {
            cache.servers.remove(&id);
            statuses.remove(&id);
            let _ = self.inner.secrets.delete_bearer_token(&id);
        }
        self.inner.repository.save_cache(&cache)
    }

    async fn discover(&self, config: &McpServerConfig) -> Result<Vec<Tool>, String> {
        match &config.transport {
            McpTransportConfig::StreamableHttp { url, .. } => {
                let transport = self.http_transport(config, url)?;
                let client = ()
                    .serve_with_lifecycle(transport, lifecycle_mode())
                    .await
                    .map_err(|error| format!("MCP initialization failed: {error}"))?;
                let result = client
                    .list_all_tools()
                    .await
                    .map_err(|error| format!("MCP tool discovery failed: {error}"));
                let _ = client.cancel().await;
                result
            }
            McpTransportConfig::Stdio {
                command,
                args,
                cwd,
                env,
            } => {
                let transport = stdio_transport(command, args, cwd.as_deref(), env)?;
                let client = ()
                    .serve_with_lifecycle(transport, lifecycle_mode())
                    .await
                    .map_err(|error| format!("MCP stdio initialization failed: {error}"))?;
                let result = client
                    .list_all_tools()
                    .await
                    .map_err(|error| format!("MCP tool discovery failed: {error}"));
                let _ = client.cancel().await;
                result
            }
        }
    }

    async fn call_once(
        &self,
        config: &McpServerConfig,
        remote_name: &str,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, String> {
        let arguments = arguments
            .as_object()
            .cloned()
            .ok_or_else(|| "MCP tool arguments must be a JSON object".to_string())?;
        let params = CallToolRequestParams::new(remote_name.to_string()).with_arguments(arguments);
        match &config.transport {
            McpTransportConfig::StreamableHttp { url, .. } => {
                let transport = self.http_transport(config, url)?;
                let client = ()
                    .serve_with_lifecycle(transport, lifecycle_mode())
                    .await
                    .map_err(|error| format!("MCP initialization failed: {error}"))?;
                let result = client
                    .call_tool(params)
                    .await
                    .map_err(|error| format!("MCP tool call failed: {error}"));
                let _ = client.cancel().await;
                result
            }
            McpTransportConfig::Stdio {
                command,
                args,
                cwd,
                env,
            } => {
                let transport = stdio_transport(command, args, cwd.as_deref(), env)?;
                let client = ()
                    .serve_with_lifecycle(transport, lifecycle_mode())
                    .await
                    .map_err(|error| format!("MCP stdio initialization failed: {error}"))?;
                let result = client
                    .call_tool(params)
                    .await
                    .map_err(|error| format!("MCP tool call failed: {error}"));
                let _ = client.cancel().await;
                result
            }
        }
    }

    fn http_transport(
        &self,
        config: &McpServerConfig,
        url: &str,
    ) -> Result<StreamableHttpClientTransport<reqwest13::Client>, String> {
        let mut transport_config = StreamableHttpClientTransportConfig::with_uri(url.to_string());
        transport_config.allow_stateless = true;
        transport_config.max_sse_event_size = MAX_SSE_EVENT_BYTES;
        // Never transparently replay a call after an expired session: the outcome may be unknown.
        transport_config.reinit_on_expired_session = false;
        if let Some(token) = self.inner.secrets.get_bearer_token(&config.id)? {
            transport_config = transport_config.auth_header(token.as_str());
        }
        Ok(StreamableHttpClientTransport::with_client(
            self.inner.http.clone(),
            transport_config,
        ))
    }

    fn server_config(&self, server_id: &str) -> Result<McpServerConfig, String> {
        self.inner
            .settings
            .read()
            .map_err(|_| "MCP settings lock is unavailable".to_string())?
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .cloned()
            .ok_or_else(|| format!("Unknown MCP server: {server_id}"))
    }

    fn server_view(&self, server_id: &str) -> Result<McpServerView, String> {
        self.overview()?
            .servers
            .into_iter()
            .find(|server| server.config.id == server_id)
            .ok_or_else(|| format!("Unknown MCP server: {server_id}"))
    }

    fn invalidate_if_fingerprint_changed(&self, config: &McpServerConfig) -> Result<(), String> {
        let fingerprint = config_fingerprint(config);
        let mut cache = self
            .inner
            .cache
            .write()
            .map_err(|_| "MCP catalog lock is unavailable".to_string())?;
        if cache
            .servers
            .get(&config.id)
            .is_some_and(|cached| cached.config_fingerprint != fingerprint)
        {
            cache.servers.remove(&config.id);
            self.inner.repository.save_cache(&cache)?;
        }
        let cached = cache.servers.get(&config.id);
        let mut statuses = self
            .inner
            .statuses
            .write()
            .map_err(|_| "MCP status lock is unavailable".to_string())?;
        statuses.insert(
            config.id.clone(),
            McpServerStatus {
                server_id: config.id.clone(),
                state: if !config.enabled {
                    McpConnectionState::Disabled
                } else if cached.is_some() {
                    McpConnectionState::Cached
                } else {
                    McpConnectionState::Failed
                },
                tool_count: cached.map_or(0, |value| value.tools.len()),
                catalog_revision: cached.map(|value| value.catalog_revision.clone()),
                last_success_at: cached.map(|value| value.discovered_at),
                last_error: None,
                consecutive_failures: 0,
                retry_after: None,
            },
        );
        Ok(())
    }

    fn update_status(&self, server_id: &str, update: impl FnOnce(&mut McpServerStatus)) {
        if let Ok(mut statuses) = self.inner.statuses.write() {
            let status = statuses
                .entry(server_id.to_string())
                .or_insert_with(|| McpServerStatus {
                    server_id: server_id.to_string(),
                    ..McpServerStatus::default()
                });
            update(status);
        }
    }

    fn record_failure(&self, server_id: &str, error: &str) {
        self.update_status(server_id, |status| {
            status.consecutive_failures = status.consecutive_failures.saturating_add(1);
            let delay_seconds = 2u64
                .saturating_pow(status.consecutive_failures.min(8))
                .min(300);
            status.state = McpConnectionState::Backoff;
            status.retry_after = Some(now_ms().saturating_add(delay_seconds * 1_000));
            status.last_error = Some(truncate_chars(error, MAX_ERROR_CHARS));
        });
    }

    async fn operation_lock(&self, server_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.inner.operation_locks.lock().await;
        locks
            .entry(server_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn acquire_call_permit(
        &self,
        config: &McpServerConfig,
    ) -> Result<OwnedSemaphorePermit, String> {
        let gate = {
            let mut gates = self.inner.call_gates.lock().await;
            gates
                .entry(config.id.clone())
                .or_insert_with(|| Arc::new(Semaphore::new(config.max_concurrency)))
                .clone()
        };
        gate.acquire_owned()
            .await
            .map_err(|_| "MCP concurrency gate is closed".to_string())
    }
}

fn lifecycle_mode() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        legacy_version: Some(ProtocolVersion::V_2025_11_25),
    }
}

fn stdio_transport(
    command: &str,
    args: &[String],
    cwd: Option<&str>,
    env: &BTreeMap<String, String>,
) -> Result<TokioChildProcess, String> {
    let cwd = cwd.map(PathBuf::from);
    if let Some(path) = &cwd {
        if !path.is_absolute() || !path.is_dir() {
            return Err(
                "MCP stdio working directory must be an existing absolute directory".to_string(),
            );
        }
    }
    TokioChildProcess::builder(tokio::process::Command::new(command).configure(|process| {
        process
            .env_clear()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        process.kill_on_drop(true);
        for key in [
            "PATH",
            "PATHEXT",
            "SYSTEMROOT",
            "WINDIR",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "HOME",
            "TMPDIR",
        ] {
            if let Some(value) = std::env::var_os(key) {
                process.env(key, value);
            }
        }
        if let Some(path) = cwd.as_ref() {
            process.current_dir(path);
        }
        for (key, value) in env {
            process.env(key, value);
        }
    }))
    .stderr(Stdio::null())
    .spawn()
    .map(|(transport, _)| transport)
    .map_err(|error| format!("Failed to spawn MCP stdio server: {error}"))
}

fn build_cached_server(
    config: &McpServerConfig,
    tools: Vec<Tool>,
) -> Result<McpCachedServer, String> {
    if tools.len() > MAX_TOOL_COUNT {
        return Err(format!(
            "MCP server exposed more than {MAX_TOOL_COUNT} tools"
        ));
    }
    let mut snapshots = Vec::new();
    for tool in tools {
        let remote_name = tool.name.to_string();
        if remote_name.trim().is_empty() || remote_name.chars().count() > 256 {
            continue;
        }
        if !config.permits_tool(&remote_name) {
            continue;
        }
        let annotations = tool.annotations.unwrap_or_default();
        snapshots.push(McpToolSnapshot {
            server_id: config.id.clone(),
            server_name: config.name.clone(),
            remote_name: remote_name.clone(),
            wire_name: wire_tool_name(&config.id, &remote_name),
            description: tool
                .description
                .map(|value| truncate_chars(&value, 2_000))
                .unwrap_or_else(|| format!("Tool provided by MCP server {}", config.name)),
            input_schema: Value::Object((*tool.input_schema).clone()),
            // Annotations are recorded for UI/audit only. Trust and approval are local policy.
            read_only_hint: annotations.read_only_hint.unwrap_or(false),
            destructive_hint: annotations.destructive_hint.unwrap_or(true),
            idempotent_hint: annotations.idempotent_hint.unwrap_or(false),
            open_world_hint: annotations.open_world_hint.unwrap_or(true),
            auto_approved: config
                .auto_approve_tools
                .iter()
                .any(|value| value == &remote_name),
            max_output_chars: config.max_output_chars,
            catalog_revision: String::new(),
            plugin_id: config.plugin_id.clone(),
        });
    }
    snapshots.sort_by(|left, right| left.remote_name.cmp(&right.remote_name));
    let revision = hash_json(&serde_json::json!({
        "serverId": config.id,
        "tools": snapshots.iter().map(|tool| serde_json::json!({
            "name": tool.remote_name,
            "schema": tool.input_schema,
        })).collect::<Vec<_>>()
    }));
    for snapshot in &mut snapshots {
        snapshot.catalog_revision = revision.clone();
    }
    Ok(McpCachedServer {
        config_fingerprint: config_fingerprint(config),
        catalog_revision: revision,
        discovered_at: now_ms(),
        tools: snapshots,
    })
}

fn normalize_call_result(result: rmcp::model::CallToolResult, max_chars: usize) -> McpCallOutput {
    let is_error = result.is_error.unwrap_or(false);
    let raw = serde_json::to_string(&result)
        .unwrap_or_else(|error| format!(r#"{{"serializationError":"{error}"}}"#));
    let tagged = format!("<mcp_result trust=\"external_untrusted\">\n{raw}\n</mcp_result>");
    let output_chars = tagged.chars().count();
    let output_truncated = output_chars > max_chars;
    let content = truncate_chars(&tagged, max_chars);
    McpCallOutput {
        content,
        is_error,
        output_chars,
        output_truncated,
    }
}

fn validate_arguments(value: &Value) -> Result<(), String> {
    if !value.is_object() {
        return Err("MCP tool arguments must be a JSON object".to_string());
    }
    if serde_json::to_vec(value)
        .map_err(|error| format!("Failed to serialize MCP arguments: {error}"))?
        .len()
        > MAX_ARGUMENT_BYTES
    {
        return Err(format!(
            "MCP tool arguments exceed {MAX_ARGUMENT_BYTES} bytes"
        ));
    }
    if json_depth(value, 0) > MAX_ARGUMENT_DEPTH {
        return Err(format!(
            "MCP tool arguments exceed nesting depth {MAX_ARGUMENT_DEPTH}"
        ));
    }
    Ok(())
}

fn json_depth(value: &Value, depth: usize) -> usize {
    match value {
        Value::Array(values) => values
            .iter()
            .map(|value| json_depth(value, depth + 1))
            .max()
            .unwrap_or(depth),
        Value::Object(values) => values
            .values()
            .map(|value| json_depth(value, depth + 1))
            .max()
            .unwrap_or(depth),
        _ => depth,
    }
}

fn wire_tool_name(server_id: &str, remote_name: &str) -> String {
    let mut readable = format!("mcp__{server_id}__{remote_name}")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let hash = hash_bytes(format!("{server_id}\0{remote_name}").as_bytes());
    readable.truncate(52);
    format!("{readable}_{}", &hash[..10])
}

fn config_fingerprint(config: &McpServerConfig) -> String {
    let mut safe = config.clone();
    if let McpTransportConfig::StreamableHttp {
        has_bearer_token, ..
    } = &mut safe.transport
    {
        *has_bearer_token = false;
    }
    hash_json(&serde_json::to_value(safe).unwrap_or(Value::Null))
}

fn hash_json(value: &Value) -> String {
    hash_bytes(&serde_json::to_vec(value).unwrap_or_default())
}

fn hash_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut result = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    result.push('…');
    result
}

fn default_status(config: &McpServerConfig) -> McpServerStatus {
    McpServerStatus {
        server_id: config.id.clone(),
        state: if config.enabled {
            McpConnectionState::Failed
        } else {
            McpConnectionState::Disabled
        },
        ..McpServerStatus::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    #[test]
    fn wire_names_are_stable_bounded_and_collision_resistant() {
        let left = wire_tool_name("server-a", "read.file");
        let same = wire_tool_name("server-a", "read.file");
        let right = wire_tool_name("server-b", "read.file");
        assert_eq!(left, same);
        assert_ne!(left, right);
        assert!(left.len() <= 63);
        assert!(left
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
    }

    #[test]
    fn argument_limits_reject_deep_or_large_payloads() {
        let mut value = Value::Object(Map::new());
        for _ in 0..=MAX_ARGUMENT_DEPTH {
            value = serde_json::json!({ "nested": value });
        }
        assert!(validate_arguments(&value).is_err());
        assert!(validate_arguments(&serde_json::json!({"ok": true})).is_ok());
    }
}
