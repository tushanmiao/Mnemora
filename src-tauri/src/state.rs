use reqwest::Client;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    sync::{Mutex as StdMutex, RwLock},
    time::Duration,
};
use tokio::sync::{oneshot, Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::chat::storage::ConversationRepository;
use crate::english::{learning::EnglishLearningRepository, EnglishRepository};
use crate::library::LibraryRepository;
use crate::memory::MemoryRepository;
use crate::request_debug::RequestDebugRecord;
use crate::settings::{
    app_repository::AppSettingsRepository, app_types::AppSettings,
    repository::ModelSettingsRepository, secrets::SecretStore, types::ModelSettings,
};
use crate::skills::SkillRepository;
use crate::startup_log::StartupErrorLog;
use crate::sync::{
    mapping::SyncMappingRepository, SyncSecretStore, SyncSettings, SyncSettingsRepository,
};

/** Tauri 全局共享状态。HTTP Client、设置快照和仓库在整个应用生命周期内复用。 */
pub struct AppState {
    pub http: Client,
    pub app_settings: RwLock<AppSettings>,
    pub app_settings_repository: AppSettingsRepository,
    pub model_settings: RwLock<ModelSettings>,
    pub model_settings_repository: ModelSettingsRepository,
    pub secrets: SecretStore,
    pub active_chat_runs: Mutex<HashMap<String, CancellationToken>>,
    pub active_note_pipeline_runs: Mutex<HashMap<String, CancellationToken>>,
    pub pending_tool_approvals: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    pub active_attachment_tasks: StdMutex<HashMap<String, CancellationToken>>,
    pub attachment_preview_gate: Semaphore,
    pub staged_attachment_paths: StdMutex<HashSet<PathBuf>>,
    pub conversation_repository: ConversationRepository,
    pub english_repository: EnglishRepository,
    pub english_operations: Mutex<()>,
    pub english_learning_repository: EnglishLearningRepository,
    pub english_learning_operations: Mutex<()>,
    pub english_audio_operations: Mutex<()>,
    pub conversation_writes: Mutex<()>,
    pub library_repository: LibraryRepository,
    pub library_operations: Mutex<()>,
    pub sync_settings: RwLock<SyncSettings>,
    pub sync_settings_repository: SyncSettingsRepository,
    pub sync_secrets: SyncSecretStore,
    pub sync_mapping_repository: SyncMappingRepository,
    pub sync_operations: Mutex<()>,
    pub active_sync_run: Mutex<Option<CancellationToken>>,
    pub update_operations: Mutex<()>,
    pub active_update_check: Mutex<Option<CancellationToken>>,
    pub pending_signed_update: Mutex<Option<tauri_plugin_updater::Update>>,
    pub skill_repository: SkillRepository,
    pub memory_repository: MemoryRepository,
    pub skill_operations: Mutex<()>,
    pub usage_dir: PathBuf,
    pub usage_operations: Mutex<()>,
    pub request_debug_records: StdMutex<VecDeque<RequestDebugRecord>>,
    pub startup_error_log: StartupErrorLog,
}

fn cancel_chat_run_tokens(runs: &HashMap<String, CancellationToken>) -> usize {
    for token in runs.values() {
        token.cancel();
    }
    runs.len()
}

impl AppState {
    pub fn new(
        config_dir: PathBuf,
        app_data_dir: PathBuf,
        resource_dir: PathBuf,
        log_dir: PathBuf,
    ) -> Result<Self, String> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(600))
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
        let sync_settings_repository = SyncSettingsRepository::new(config_dir.clone());
        let sync_secrets = SyncSecretStore;
        let sync_settings = match sync_settings_repository.load() {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("{error}; using default sync settings");
                SyncSettings::default()
            }
        };
        let sync_mapping_repository = SyncMappingRepository::new(app_data_dir.clone());
        let model_settings_repository = ModelSettingsRepository::new(config_dir);
        let usage_dir = crate::usage::usage_dir(&app_data_dir);
        let skill_repository =
            SkillRepository::new(resource_dir.join("skills"), app_data_dir.join("skills"));
        let memory_repository = MemoryRepository::new(app_data_dir.clone());
        let library_repository = LibraryRepository::new(app_data_dir.clone());
        let conversation_repository = ConversationRepository::new(app_data_dir.clone());
        let english_learning_repository = EnglishLearningRepository::new(app_data_dir.clone());
        let english_repository = EnglishRepository::new(app_data_dir, resource_dir.clone());
        if let Err(error) = crate::chat::attachments::cleanup_staged_attachments_older_than(
            crate::chat::attachments::STAGED_ATTACHMENT_MAX_AGE,
        ) {
            eprintln!("Failed to clean stale staged attachments: {error}");
        }
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
            active_note_pipeline_runs: Mutex::new(HashMap::new()),
            pending_tool_approvals: Mutex::new(HashMap::new()),
            active_attachment_tasks: StdMutex::new(HashMap::new()),
            attachment_preview_gate: Semaphore::new(2),
            staged_attachment_paths: StdMutex::new(HashSet::new()),
            conversation_repository,
            english_repository,
            english_operations: Mutex::new(()),
            english_learning_repository,
            english_learning_operations: Mutex::new(()),
            english_audio_operations: Mutex::new(()),
            conversation_writes: Mutex::new(()),
            library_repository,
            library_operations: Mutex::new(()),
            sync_settings: RwLock::new(sync_settings),
            sync_settings_repository,
            sync_secrets,
            sync_mapping_repository,
            sync_operations: Mutex::new(()),
            active_sync_run: Mutex::new(None),
            update_operations: Mutex::new(()),
            active_update_check: Mutex::new(None),
            pending_signed_update: Mutex::new(None),
            skill_repository,
            memory_repository,
            skill_operations: Mutex::new(()),
            usage_dir,
            usage_operations: Mutex::new(()),
            request_debug_records: StdMutex::new(crate::request_debug::empty_store()),
            startup_error_log: StartupErrorLog::new(log_dir),
        })
    }

    /** 向所有活动 Chat 流发送取消信号；真实任务结束后仍由 service 移除注册项。 */
    pub async fn cancel_all_chat_runs(&self) -> usize {
        let runs = self.active_chat_runs.lock().await;
        cancel_chat_run_tokens(&runs)
    }

    pub async fn register_note_pipeline_run(
        &self,
        run_id: String,
        cancellation: CancellationToken,
    ) -> bool {
        let mut runs = self.active_note_pipeline_runs.lock().await;
        if !runs.is_empty() && !runs.contains_key(&run_id) {
            return false;
        }
        if runs.contains_key(&run_id) {
            return false;
        }
        runs.insert(run_id, cancellation);
        true
    }

    pub async fn finish_note_pipeline_run(&self, run_id: &str) {
        self.active_note_pipeline_runs.lock().await.remove(run_id);
    }

    pub async fn cancel_note_pipeline_run(&self, run_id: &str) -> bool {
        let runs = self.active_note_pipeline_runs.lock().await;
        let Some(token) = runs.get(run_id) else {
            return false;
        };
        token.cancel();
        true
    }

    pub async fn cancel_all_note_pipeline_runs(&self) -> usize {
        let runs = self.active_note_pipeline_runs.lock().await;
        cancel_chat_run_tokens(&runs)
    }

    /** 丢弃所有一次性审批发送端，等待中的 Agent 会收到通道关闭并按拒绝处理。 */
    pub async fn cancel_all_tool_approvals(&self) -> usize {
        let mut approvals = self.pending_tool_approvals.lock().await;
        let count = approvals.len();
        approvals.clear();
        count
    }

    pub fn register_attachment_task(
        &self,
        request_id: String,
    ) -> Result<CancellationToken, String> {
        let mut tasks = self
            .active_attachment_tasks
            .lock()
            .map_err(|_| "附件任务状态暂时不可用。".to_string())?;
        if tasks.contains_key(&request_id) {
            return Err("相同附件任务已经存在。".to_string());
        }
        let token = CancellationToken::new();
        tasks.insert(request_id, token.clone());
        Ok(token)
    }

    pub fn finish_attachment_task(&self, request_id: &str) {
        if let Ok(mut tasks) = self.active_attachment_tasks.lock() {
            tasks.remove(request_id);
        }
    }

    pub fn cancel_attachment_task(&self, request_id: &str) -> bool {
        let Ok(tasks) = self.active_attachment_tasks.lock() else {
            return false;
        };
        let Some(token) = tasks.get(request_id) else {
            return false;
        };
        token.cancel();
        true
    }

    pub fn cancel_all_attachment_tasks(&self) -> usize {
        let Ok(tasks) = self.active_attachment_tasks.lock() else {
            return 0;
        };
        for token in tasks.values() {
            token.cancel();
        }
        tasks.len()
    }

    pub fn register_staged_attachment(&self, path: PathBuf) {
        if let Ok(mut paths) = self.staged_attachment_paths.lock() {
            paths.insert(path);
        }
    }

    pub fn unregister_staged_attachment(&self, path: &PathBuf) {
        if let Ok(mut paths) = self.staged_attachment_paths.lock() {
            paths.remove(path);
        }
    }

    pub fn cleanup_current_staged_attachments(&self) -> usize {
        let Ok(mut paths) = self.staged_attachment_paths.lock() else {
            return 0;
        };
        let mut removed = 0usize;
        for path in paths.drain() {
            if fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    pub async fn start_sync_run(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self.active_sync_run.lock().await = Some(token.clone());
        token
    }

    pub async fn finish_sync_run(&self) {
        self.active_sync_run.lock().await.take();
    }

    pub async fn cancel_sync_run(&self) -> bool {
        let run = self.active_sync_run.lock().await;
        let Some(token) = run.as_ref() else {
            return false;
        };
        token.cancel();
        true
    }

    pub async fn start_update_check(&self) -> CancellationToken {
        let token = CancellationToken::new();
        *self.active_update_check.lock().await = Some(token.clone());
        token
    }

    pub async fn finish_update_check(&self) {
        self.active_update_check.lock().await.take();
    }

    pub async fn cancel_update_check(&self) -> bool {
        let check = self.active_update_check.lock().await;
        let Some(token) = check.as_ref() else {
            return false;
        };
        token.cancel();
        true
    }

    pub async fn discard_pending_signed_update(&self) {
        self.pending_signed_update.lock().await.take();
    }
}

#[cfg(test)]
mod tests {
    use super::cancel_chat_run_tokens;
    use std::collections::HashMap;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn cancels_all_registered_chat_runs_without_removing_them() {
        let first = CancellationToken::new();
        let second = CancellationToken::new();
        let runs = HashMap::from([
            ("run-1".to_string(), first.clone()),
            ("run-2".to_string(), second.clone()),
        ]);

        assert_eq!(cancel_chat_run_tokens(&runs), 2);
        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
        assert_eq!(runs.len(), 2);
        assert_eq!(cancel_chat_run_tokens(&runs), 2);
    }
}
