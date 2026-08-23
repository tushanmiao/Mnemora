use reqwest::Client;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, RwLock},
    time::Duration,
};
use tokio::{
    sync::{oneshot, Mutex, Semaphore},
    task::AbortHandle,
};
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
use crate::storage::StorageManager;
use crate::sync::{
    mapping::SyncMappingRepository, SyncSecretStore, SyncSettings, SyncSettingsRepository,
};
use crate::task_diagnostics::TaskDiagnosticLog;

/** Tauri 全局共享状态。HTTP Client、设置快照和仓库在整个应用生命周期内复用。 */
pub struct AppState {
    pub http: Client,
    pub app_settings: RwLock<AppSettings>,
    pub app_settings_repository: AppSettingsRepository,
    pub model_settings: RwLock<ModelSettings>,
    pub model_settings_repository: ModelSettingsRepository,
    pub secrets: SecretStore,
    pub active_chat_runs: Mutex<HashMap<String, CancellationToken>>,
    pub active_note_pipeline_runs: Mutex<HashMap<String, ActiveNotePipelineRun>>,
    pub pending_tool_approvals: Mutex<HashMap<String, PendingToolApproval>>,
    pub active_attachment_tasks: StdMutex<HashMap<String, CancellationToken>>,
    pub detached_note_pipeline_instances: StdMutex<HashSet<String>>,
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
    pub library_operations: Arc<Mutex<()>>,
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
    pub task_diagnostic_log: TaskDiagnosticLog,
    pub storage: StorageManager,
    pub storage_operations: Mutex<()>,
}

pub struct ActiveNotePipelineRun {
    instance_id: String,
    cancellation: CancellationToken,
    abort_handle: Option<AbortHandle>,
    task_kind: String,
    started_at_ms: u64,
}

/// 内存中的一次性审批通道只负责唤醒 Worker；审批身份和 CAS 版本同时持久化到 SQLite。
pub struct PendingToolApproval {
    pub sender: oneshot::Sender<bool>,
    pub run_id: String,
    pub call_id: String,
    pub execution_version: u32,
    pub state_version: u32,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct NotePipelineTaskSnapshot {
    pub instance_id: String,
    pub task_kind: String,
    pub started_at_ms: u64,
    pub cancellation_requested: bool,
    pub abortable: bool,
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
        storage: StorageManager,
    ) -> Result<Self, String> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            // 中转站可能在长时间无首字节时仍保持连接；普通 Chat 的单次
            // HTTP 请求上限从 600 秒提高到 900 秒。深度笔记各阶段仍使用
            // 自己更细的 attempt timeout，不共享这个全局上限。
            .timeout(Duration::from_secs(900))
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
        if let Err(error) = library_repository.recover_stale_cancelling_runs() {
            eprintln!("Failed to recover stale cancelling note tasks: {error}");
        }
        if let Err(error) = library_repository.recover_stale_agent_runs() {
            eprintln!("Failed to recover stale Agent runs: {error}");
        }
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
            detached_note_pipeline_instances: StdMutex::new(HashSet::new()),
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
            library_operations: Arc::new(Mutex::new(())),
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
            startup_error_log: StartupErrorLog::new(log_dir.clone()),
            task_diagnostic_log: TaskDiagnosticLog::new(log_dir),
            storage,
            storage_operations: Mutex::new(()),
        })
    }

    /** 向所有活动 Chat 流发送取消信号；真实任务结束后仍由 service 移除注册项。 */
    pub async fn cancel_all_chat_runs(&self) -> usize {
        let runs = self.active_chat_runs.lock().await;
        for run_id in runs.keys() {
            let _ = self.library_repository.transition_agent_run(
                run_id,
                crate::chat::agent::run_machine::AgentRunEvent::CancelRequested,
                Some(&format!("agent-shutdown:{run_id}")),
                r#"{"reason":"applicationShutdown"}"#,
                None,
            );
        }
        cancel_chat_run_tokens(&runs)
    }

    pub async fn register_note_pipeline_run(
        &self,
        run_id: String,
        cancellation: CancellationToken,
        task_kind: impl Into<String>,
        instance_id: String,
    ) -> bool {
        let mut runs = self.active_note_pipeline_runs.lock().await;
        if !runs.is_empty() && !runs.contains_key(&run_id) {
            return false;
        }
        if runs.contains_key(&run_id) {
            return false;
        }
        runs.insert(
            run_id,
            ActiveNotePipelineRun {
                instance_id,
                cancellation,
                abort_handle: None,
                task_kind: task_kind.into(),
                started_at_ms: crate::usage::now_ms(),
            },
        );
        true
    }

    pub async fn attach_note_pipeline_abort_handle(
        &self,
        run_id: &str,
        instance_id: &str,
        abort_handle: AbortHandle,
    ) -> bool {
        let mut runs = self.active_note_pipeline_runs.lock().await;
        let Some(run) = runs
            .get_mut(run_id)
            .filter(|run| run.instance_id == instance_id)
        else {
            return false;
        };
        run.abort_handle = Some(abort_handle);
        true
    }

    pub async fn finish_note_pipeline_run(&self, run_id: &str, instance_id: &str) -> bool {
        let mut runs = self.active_note_pipeline_runs.lock().await;
        if runs
            .get(run_id)
            .is_some_and(|run| run.instance_id == instance_id)
        {
            runs.remove(run_id);
            true
        } else {
            false
        }
    }

    pub fn detach_note_pipeline_instance(&self, instance_id: &str) {
        if let Ok(mut instances) = self.detached_note_pipeline_instances.lock() {
            instances.insert(instance_id.to_string());
        }
    }

    pub fn is_note_pipeline_instance_detached(&self, instance_id: &str) -> bool {
        self.detached_note_pipeline_instances
            .lock()
            .map(|instances| instances.contains(instance_id))
            .unwrap_or(true)
    }

    pub fn clear_detached_note_pipeline_instance(&self, instance_id: &str) {
        if let Ok(mut instances) = self.detached_note_pipeline_instances.lock() {
            instances.remove(instance_id);
        }
    }

    pub async fn cancel_note_pipeline_run(&self, run_id: &str) -> bool {
        let runs = self.active_note_pipeline_runs.lock().await;
        let Some(run) = runs.get(run_id) else {
            return false;
        };
        run.cancellation.cancel();
        true
    }

    pub async fn abort_note_pipeline_run(&self, run_id: &str) -> bool {
        let runs = self.active_note_pipeline_runs.lock().await;
        let Some(run) = runs.get(run_id) else {
            return false;
        };
        run.cancellation.cancel();
        let Some(abort_handle) = run.abort_handle.as_ref() else {
            return false;
        };
        abort_handle.abort();
        true
    }

    pub async fn note_pipeline_task_snapshot(
        &self,
        run_id: &str,
    ) -> Option<NotePipelineTaskSnapshot> {
        self.active_note_pipeline_runs
            .lock()
            .await
            .get(run_id)
            .map(|run| NotePipelineTaskSnapshot {
                instance_id: run.instance_id.clone(),
                task_kind: run.task_kind.clone(),
                started_at_ms: run.started_at_ms,
                cancellation_requested: run.cancellation.is_cancelled(),
                abortable: run.abort_handle.is_some(),
            })
    }

    pub async fn is_note_pipeline_run_active(&self, run_id: &str) -> bool {
        self.active_note_pipeline_runs
            .lock()
            .await
            .contains_key(run_id)
    }

    pub async fn cancel_all_note_pipeline_runs(&self) -> usize {
        let runs = self.active_note_pipeline_runs.lock().await;
        for run in runs.values() {
            run.cancellation.cancel();
            if let Some(abort_handle) = run.abort_handle.as_ref() {
                abort_handle.abort();
            }
        }
        runs.len()
    }

    /** 丢弃所有一次性审批发送端，等待中的 Agent 会收到通道关闭并按拒绝处理。 */
    pub async fn cancel_all_tool_approvals(&self) -> usize {
        let mut approvals = self.pending_tool_approvals.lock().await;
        let count = approvals.len();
        for approval in approvals.values() {
            let _ = self.library_repository.transition_agent_tool_call(
                &approval.run_id,
                &approval.call_id,
                crate::chat::agent::run_machine::ToolCallEvent::Cancelled,
                approval.execution_version,
                Some(approval.state_version),
                Some("应用正在退出，工具审批已取消。"),
                Some("applicationShutdown"),
            );
        }
        approvals.clear();
        count
    }

    pub async fn close_tool_approvals_for_run(&self, run_id: &str) -> usize {
        let mut approvals = self.pending_tool_approvals.lock().await;
        let ids = approvals
            .iter()
            .filter(|(_, approval)| approval.run_id == run_id)
            .map(|(approval_id, _)| approval_id.clone())
            .collect::<Vec<_>>();
        for approval_id in &ids {
            approvals.remove(approval_id);
        }
        ids.len()
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
