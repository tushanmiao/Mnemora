use reqwest::Client;
use std::{collections::HashMap, path::PathBuf, sync::RwLock, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::settings::{
    app_repository::AppSettingsRepository, app_types::AppSettings,
    repository::ModelSettingsRepository, secrets::SecretStore, types::ModelSettings,
};
use crate::chat::storage::ConversationRepository;

/** Tauri 全局共享状态。HTTP Client、设置快照和仓库在整个应用生命周期内复用。 */
pub struct AppState {
    pub http: Client,
    pub app_settings: RwLock<AppSettings>,
    pub app_settings_repository: AppSettingsRepository,
    pub model_settings: RwLock<ModelSettings>,
    pub model_settings_repository: ModelSettingsRepository,
    pub secrets: SecretStore,
    pub active_chat_runs: Mutex<HashMap<String, CancellationToken>>,
    pub conversation_repository: ConversationRepository,
    pub conversation_writes: Mutex<()>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, app_data_dir: PathBuf) -> Result<Self, String> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("Mnemora/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("Failed to create HTTP client: {error}"))?;

        let app_settings_repository = AppSettingsRepository::new(config_dir.clone());
        let app_settings = match app_settings_repository.load() {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("{error}; using default app settings");
                AppSettings::default()
            }
        };
        let model_settings_repository = ModelSettingsRepository::new(config_dir);
        let conversation_repository = ConversationRepository::new(app_data_dir);
        let secrets = SecretStore;
        let mut model_settings = match model_settings_repository.load() {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("{error}; using default model settings");
                ModelSettings::default()
            }
        };
        if let Err(error) = secrets.refresh_api_key_statuses(&mut model_settings) {
            eprintln!("{error}; API Key status will be shown as unavailable");
            for provider in &mut model_settings.providers {
                provider.has_api_key = false;
            }
        }

        Ok(Self {
            http,
            app_settings: RwLock::new(app_settings),
            app_settings_repository,
            model_settings: RwLock::new(model_settings),
            model_settings_repository,
            secrets,
            active_chat_runs: Mutex::new(HashMap::new()),
            conversation_repository,
            conversation_writes: Mutex::new(()),
        })
    }
}
