//! Durable PDF extraction worker.
//!
//! The worker is intentionally hosted in the Tauri process.  The renderer can
//! enqueue or cancel work, but it never receives a MinerU token and it never
//! gets to decide whether a cloud upload is authorized.  A claimed job keeps a
//! lease in SQLite; every progress and terminal write carries the lease
//! identity, which makes a late provider response harmless after cancellation
//! or worker takeover.

use std::{
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};

use futures::{
    stream::{FuturesUnordered, StreamExt},
    FutureExt,
};
use serde_json::json;
use tauri::{async_runtime::JoinHandle, AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    knowledge::{
        mineru::{extract_local_text_pdf, MineruClient, MineruError, MineruProgress},
        repository::{KnowledgeJobClaim, KnowledgeRepository, PdfCommitResult},
    },
    settings::app_types::KnowledgeSettings,
    state::AppState,
};

const WORKER_IDLE_WAIT: Duration = Duration::from_secs(2);
const WORKER_RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
const LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Start one process-wide worker supervisor.  It is called after `AppState`
/// has been managed by Tauri, so the supervisor can safely resolve state from
/// the handle for its whole lifetime.
pub(crate) fn start(app: &AppHandle) {
    super::embedding_worker::start(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        worker_loop(app).await;
    });
}

async fn worker_loop(app: AppHandle) {
    let state = app.state::<AppState>();
    let shutdown = state.knowledge_worker_shutdown.clone();
    let notify = state.knowledge_worker_notify.clone();
    let runtime_id = format!("knowledge-worker-{}", Uuid::new_v4());
    let mut tasks = FuturesUnordered::<WorkerTaskFuture>::new();
    let mut last_recovery = Instant::now()
        .checked_sub(WORKER_RECOVERY_INTERVAL)
        .unwrap_or_else(Instant::now);

    loop {
        if shutdown.is_cancelled() {
            break;
        }

        let settings = knowledge_settings(&app);
        if last_recovery.elapsed() >= WORKER_RECOVERY_INTERVAL {
            let _ =
                locked_knowledge_call(&state, |repository| repository.recover_stale_jobs()).await;
            last_recovery = Instant::now();
        }

        if settings.enabled {
            let concurrency = usize::from(settings.index_concurrency.clamp(1, 4));
            while tasks.len() < concurrency && !shutdown.is_cancelled() {
                let claim = match locked_knowledge_call(&state, {
                    let runtime_id = runtime_id.clone();
                    let allow_local_fallback = settings.allow_local_text_fallback;
                    move |repository| {
                        repository
                            .claim_next_extract_job_with_fallback(&runtime_id, allow_local_fallback)
                    }
                })
                .await
                {
                    Ok(claim) => claim,
                    Err(error) => {
                        eprintln!("Knowledge worker could not claim a job: {error}");
                        break;
                    }
                };
                let Some(claim) = claim else {
                    break;
                };

                let cancellation = state.register_knowledge_job(claim.job_id.clone()).await;
                tasks.push(spawn_worker_task(app.clone(), claim, cancellation));
            }
        }

        if tasks.is_empty() {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = notify.notified() => {},
                _ = tokio::time::sleep(WORKER_IDLE_WAIT) => {},
            }
        } else {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                Some(outcome) = tasks.next() => {
                    handle_worker_task(&state, outcome).await;
                },
                _ = notify.notified() => {},
                _ = tokio::time::sleep(WORKER_IDLE_WAIT) => {},
            }
        }
    }

    // The application exit path normally persists cancellation before it
    // reaches this point.  Repeat the in-memory signal here for the small
    // race in which shutdown happens immediately after a claim.
    state.cancel_registered_knowledge_tokens().await;
    let drain = tokio::time::sleep(SHUTDOWN_DRAIN_TIMEOUT);
    tokio::pin!(drain);
    while !tasks.is_empty() {
        tokio::select! {
            _ = &mut drain => break,
            Some(outcome) = tasks.next() => {
                handle_worker_task(&state, outcome).await;
            },
        }
    }
}

type WorkerTaskFuture = Pin<Box<dyn Future<Output = WorkerTaskOutcome> + Send>>;

struct WorkerTaskOutcome {
    claim: KnowledgeJobClaim,
    result: Result<(), String>,
}

fn spawn_worker_task(
    app: AppHandle,
    claim: KnowledgeJobClaim,
    cancellation: CancellationToken,
) -> WorkerTaskFuture {
    let outcome_claim = claim.clone();
    Box::pin(async move {
        // Keep the actual job on a Tauri task so one blocked provider request
        // cannot monopolize the supervisor.  The inner future catches a
        // panic; the outer join result still handles an aborted/join-failed
        // task explicitly.
        let joined = tauri::async_runtime::spawn(async move {
            AssertUnwindSafe(process_claim(app, claim, cancellation))
                .catch_unwind()
                .await
        })
        .await;
        let result = match joined {
            Ok(Ok(())) => Ok(()),
            Ok(Err(payload)) => Err(format!(
                "worker task panicked: {}",
                panic_payload_message(payload)
            )),
            Err(error) => Err(format!("worker task join failed: {error}")),
        };
        WorkerTaskOutcome {
            claim: outcome_claim,
            result,
        }
    })
}

async fn handle_worker_task(state: &AppState, outcome: WorkerTaskOutcome) {
    let job_id = outcome.claim.job_id.clone();
    if let Err(reason) = outcome.result {
        let claim = outcome.claim.clone();
        let message = reason.clone();
        let stale = locked_knowledge_call(state, move |repository| {
            repository.mark_claim_stale(&claim, "WORKER_PANIC", &message)
        })
        .await;
        if let Err(error) = stale {
            eprintln!("Knowledge worker could not converge failed task {job_id}: {error}");
        }
    }
    // `process_claim` removes this entry on every normal path.  Keeping this
    // cleanup here makes panic, JoinError, and shutdown-drain paths converge
    // as well and is intentionally idempotent.
    state.finish_knowledge_job(&job_id).await;
}

fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_string()
}

async fn process_claim(app: AppHandle, claim: KnowledgeJobClaim, cancellation: CancellationToken) {
    let job_id = claim.job_id.clone();
    let state = app.state::<AppState>();
    let settings = knowledge_settings(&app);

    let pdf_path = match locked_library_call(&state, {
        let source_id = claim.source_id.clone();
        move |library| library.primary_file_path(&source_id)
    })
    .await
    {
        Ok(path) => path,
        Err(error) => {
            finish_with_error(
                &app,
                &claim,
                &cancellation,
                WorkerError::new("PDF_SOURCE_UNAVAILABLE", error, false),
            )
            .await;
            state.finish_knowledge_job(&job_id).await;
            return;
        }
    };

    let progress = Arc::new(StdMutex::new(ProgressSnapshot::default()));
    let keeper = LeaseKeeper::start(&app, &claim, &cancellation, progress.clone());
    let result = process_pdf(
        &app,
        &state,
        &claim,
        &cancellation,
        &settings,
        pdf_path,
        progress,
    )
    .await;
    keeper.stop().await;

    match result {
        Ok(commit) => {
            emit_job_event(
                &app,
                &claim,
                json!({
                    "state": if commit.partial { "partial" } else { "succeeded" },
                    "revisionId": commit.revision_id,
                }),
            );
        }
        Err(error) => finish_with_error(&app, &claim, &cancellation, error).await,
    }
    state.finish_knowledge_job(&job_id).await;
}

async fn process_pdf(
    app: &AppHandle,
    state: &AppState,
    claim: &KnowledgeJobClaim,
    cancellation: &CancellationToken,
    settings: &KnowledgeSettings,
    pdf_path: std::path::PathBuf,
    progress: Arc<StdMutex<ProgressSnapshot>>,
) -> Result<PdfCommitResult, WorkerError> {
    let cloud_allowed = claim.cloud_consent_granted && settings.mineru_cloud_enabled;
    if cloud_allowed {
        let secrets = state.secrets;
        let token = tauri::async_runtime::spawn_blocking(move || secrets.get_mineru_token())
            .await
            .map_err(|error| WorkerError::new("MINERU_TOKEN_READ_FAILED", error.to_string(), true))?
            .map_err(|error| WorkerError::new("MINERU_TOKEN_READ_FAILED", error, true))?;
        if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
            match process_cloud_pdf(
                app,
                state,
                claim,
                cancellation,
                settings,
                &pdf_path,
                &token,
                progress.clone(),
            )
            .await
            {
                Ok(result) => return Ok(result),
                Err(cloud_error) => {
                    if is_cancelled(app, state, claim, cancellation).await {
                        return Err(cloud_error);
                    }
                    if settings.allow_local_text_fallback {
                        report_progress(
                            app,
                            state,
                            claim,
                            cancellation,
                            &progress,
                            "cloud_failed_local_fallback",
                            0,
                            0,
                            Some(cloud_error.code.as_str()),
                        )
                        .await?;
                        return process_local_pdf(
                            app,
                            state,
                            claim,
                            cancellation,
                            settings,
                            &pdf_path,
                            Some(&cloud_error),
                            progress,
                        )
                        .await;
                    }
                    return Err(cloud_error);
                }
            }
        } else if !settings.allow_local_text_fallback {
            return Err(WorkerError::new(
                "MINERU_TOKEN_MISSING",
                "MinerU token is not configured.",
                false,
            ));
        }
    }

    if !settings.allow_local_text_fallback {
        return Err(if claim.cloud_consent_granted {
            WorkerError::new(
                "MINERU_CLOUD_DISABLED",
                "MinerU Cloud is disabled and local text fallback is disabled.",
                false,
            )
        } else {
            WorkerError::new(
                "MINERU_CONSENT_REQUIRED",
                "Cloud consent is required before this PDF can be uploaded.",
                false,
            )
        });
    }

    process_local_pdf(
        app,
        state,
        claim,
        cancellation,
        settings,
        &pdf_path,
        None,
        progress,
    )
    .await
}

async fn process_cloud_pdf(
    app: &AppHandle,
    state: &AppState,
    claim: &KnowledgeJobClaim,
    cancellation: &CancellationToken,
    settings: &KnowledgeSettings,
    pdf_path: &std::path::Path,
    token: &str,
    progress: Arc<StdMutex<ProgressSnapshot>>,
) -> Result<PdfCommitResult, WorkerError> {
    let config = MineruConfigForWorker::from_settings(settings);
    report_progress(
        app,
        state,
        claim,
        cancellation,
        &progress,
        "validating",
        0,
        0,
        None,
    )
    .await?;
    let client = MineruClient::new().map_err(worker_error_from_mineru)?;
    let callback_app = app.clone();
    let callback_state = progress.clone();
    let callback_repository = state.knowledge_repository.clone();
    let callback_claim = claim.clone();
    let callback_cancellation = cancellation.clone();
    let callback = move |update: MineruProgress| {
        let stage = provider_stage(&update.stage);
        let completed = i64::from(update.extracted_pages.unwrap_or(0));
        let total = i64::from(
            update
                .total_pages
                .unwrap_or(0)
                .max(update.extracted_pages.unwrap_or(0)),
        );
        if let Ok(mut snapshot) = callback_state.lock() {
            snapshot.stage = stage.to_string();
            snapshot.completed_units = completed;
            snapshot.total_units = total;
            snapshot.provider_state = Some(update.stage.clone());
        }
        let changed = callback_repository
            .heartbeat_claim(
                &callback_claim,
                stage,
                completed,
                total,
                Some(update.stage.as_str()),
            )
            .unwrap_or(false);
        if !changed {
            callback_cancellation.cancel();
        }
        emit_job_event(
            &callback_app,
            &callback_claim,
            json!({
                "stage": stage,
                "completedUnits": completed,
                "totalUnits": total,
                "attempt": update.attempt,
            }),
        );
    };
    let extraction = client
        .extract_pdf(
            pdf_path,
            &claim.original_name,
            token,
            &config.0,
            cancellation,
            Some(&callback),
        )
        .await
        .map_err(worker_error_from_mineru)?;
    if is_cancelled(app, state, claim, cancellation).await {
        return Err(WorkerError::cancelled());
    }
    report_progress(
        app,
        state,
        claim,
        cancellation,
        &progress,
        "validating_archive",
        i64::from(extraction.preflight.page_count),
        i64::from(extraction.preflight.page_count),
        Some("archive"),
    )
    .await?;
    report_progress(
        app,
        state,
        claim,
        cancellation,
        &progress,
        "committing",
        i64::from(extraction.preflight.page_count),
        i64::from(extraction.preflight.page_count),
        Some("commit"),
    )
    .await?;
    report_progress(
        app,
        state,
        claim,
        cancellation,
        &progress,
        "writing_revision",
        i64::from(extraction.preflight.page_count),
        i64::from(extraction.preflight.page_count),
        Some("revision"),
    )
    .await?;
    let claim_for_commit = claim.clone();
    let config_for_commit = config.0.clone();
    let target = usize::try_from(settings.chunk_target_chars).unwrap_or(1_600);
    let max = usize::try_from(settings.chunk_max_chars).unwrap_or(2_400);
    let commit = locked_knowledge_call(state, move |repository| {
        repository.commit_mineru_extraction(
            &claim_for_commit,
            &extraction,
            &config_for_commit,
            target,
            max,
        )
    })
    .await
    .map_err(|error| WorkerError::new("PDF_COMMIT_FAILED", error, true))?;
    Ok(commit)
}

async fn process_local_pdf(
    app: &AppHandle,
    state: &AppState,
    claim: &KnowledgeJobClaim,
    cancellation: &CancellationToken,
    settings: &KnowledgeSettings,
    pdf_path: &std::path::Path,
    cloud_error: Option<&WorkerError>,
    progress: Arc<StdMutex<ProgressSnapshot>>,
) -> Result<PdfCommitResult, WorkerError> {
    report_progress(
        app,
        state,
        claim,
        cancellation,
        &progress,
        "local_text_fallback",
        0,
        0,
        Some("local"),
    )
    .await?;
    let path = pdf_path.to_path_buf();
    let fallback = tauri::async_runtime::spawn_blocking(move || extract_local_text_pdf(&path))
        .await
        .map_err(|error| WorkerError::new("MINERU_LOCAL_FALLBACK_FAILED", error.to_string(), true))?
        .map_err(worker_error_from_mineru)?;
    if is_cancelled(app, state, claim, cancellation).await {
        return Err(WorkerError::cancelled());
    }
    report_progress(
        app,
        state,
        claim,
        cancellation,
        &progress,
        "committing",
        0,
        0,
        Some("local_commit"),
    )
    .await?;
    report_progress(
        app,
        state,
        claim,
        cancellation,
        &progress,
        "writing_revision",
        0,
        0,
        Some("local_revision"),
    )
    .await?;
    let cloud_error_text = cloud_error.map(WorkerError::storage_string);
    let claim_for_commit = claim.clone();
    let target = usize::try_from(settings.chunk_target_chars).unwrap_or(1_600);
    let max = usize::try_from(settings.chunk_max_chars).unwrap_or(2_400);
    locked_knowledge_call(state, move |repository| {
        repository.commit_local_pdf_fallback(
            &claim_for_commit,
            &fallback,
            cloud_error_text.as_deref(),
            target,
            max,
        )
    })
    .await
    .map_err(|error| WorkerError::new("PDF_LOCAL_COMMIT_FAILED", error, true))
}

async fn report_progress(
    app: &AppHandle,
    state: &AppState,
    claim: &KnowledgeJobClaim,
    cancellation: &CancellationToken,
    snapshot: &Arc<StdMutex<ProgressSnapshot>>,
    stage: &str,
    completed_units: i64,
    total_units: i64,
    provider_state: Option<&str>,
) -> Result<(), WorkerError> {
    let stage = stage.to_string();
    let provider_state = provider_state.map(str::to_string);
    if let Ok(mut current) = snapshot.lock() {
        current.stage = stage.clone();
        current.completed_units = completed_units.max(0);
        current.total_units = total_units.max(completed_units).max(0);
        current.provider_state = provider_state.clone();
    }
    let changed = locked_knowledge_call(state, {
        let claim = claim.clone();
        let stage = stage.clone();
        move |repository| {
            repository.heartbeat_claim(
                &claim,
                &stage,
                completed_units,
                total_units,
                provider_state.as_deref(),
            )
        }
    })
    .await
    .map_err(|error| WorkerError::new("KNOWLEDGE_HEARTBEAT_FAILED", error, true))?;
    if !changed {
        cancellation.cancel();
        return Err(WorkerError::cancelled());
    }
    emit_job_event(
        app,
        claim,
        json!({
            "stage": stage,
            "completedUnits": completed_units.max(0),
            "totalUnits": total_units.max(completed_units).max(0),
        }),
    );
    Ok(())
}

async fn is_cancelled(
    _app: &AppHandle,
    state: &AppState,
    claim: &KnowledgeJobClaim,
    cancellation: &CancellationToken,
) -> bool {
    if cancellation.is_cancelled() {
        return true;
    }
    locked_knowledge_call(state, {
        let claim = claim.clone();
        move |repository| repository.claim_cancel_requested(&claim)
    })
    .await
    .unwrap_or(true)
}

async fn finish_with_error(
    _app: &AppHandle,
    claim: &KnowledgeJobClaim,
    cancellation: &CancellationToken,
    error: WorkerError,
) {
    let state = _app.state::<AppState>();
    let requested = cancellation.is_cancelled()
        || locked_knowledge_call(&state, {
            let claim = claim.clone();
            move |repository| repository.claim_cancel_requested(&claim)
        })
        .await
        .unwrap_or(true);
    if requested {
        if cancellation.is_cancelled() {
            let _ = locked_knowledge_call(&state, {
                let job_id = claim.job_id.clone();
                move |repository| repository.cancel_job(&job_id)
            })
            .await;
        }
        let _ = locked_knowledge_call(&state, {
            let job_id = claim.job_id.clone();
            move |repository| repository.finalize_cancelled_job(&job_id, &error.code)
        })
        .await;
        return;
    }
    let _ = locked_knowledge_call(&state, {
        let claim = claim.clone();
        let code = error.code.clone();
        let message = error.message.clone();
        move |repository| repository.fail_claimed_job(&claim, &code, &message)
    })
    .await;
}

async fn locked_knowledge_call<T, F>(state: &AppState, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(KnowledgeRepository) -> Result<T, String> + Send + 'static,
{
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || operation(repository))
        .await
        .map_err(|error| format!("Knowledge worker task failed: {error}"))?
}

async fn locked_library_call<T, F>(state: &AppState, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(crate::library::LibraryRepository) -> Result<T, String> + Send + 'static,
{
    let _guard = state.library_operations.lock().await;
    let repository = state.library_repository.clone();
    tauri::async_runtime::spawn_blocking(move || operation(repository))
        .await
        .map_err(|error| format!("Knowledge worker library task failed: {error}"))?
}

fn knowledge_settings(app: &AppHandle) -> KnowledgeSettings {
    app.state::<AppState>()
        .app_settings
        .read()
        .map(|settings| settings.knowledge.clone())
        .unwrap_or_default()
}

fn provider_stage(stage: &str) -> &'static str {
    match stage {
        "requestingUploadUrl" => "requesting_upload_url",
        "uploading" => "uploading",
        "remotePending" => "remote_pending",
        "remoteRunning" => "remote_running",
        "downloading" => "downloading",
        "done" => "validating_archive",
        "validating" => "validating",
        _ => "remote_running",
    }
}

fn emit_job_event(app: &AppHandle, claim: &KnowledgeJobClaim, payload: serde_json::Value) {
    let _ = app.emit(
        "mnemora://knowledge-job-updated",
        json!({ "jobId": claim.job_id, "documentId": claim.document_id, "payload": payload }),
    );
}

fn worker_error_from_mineru(error: MineruError) -> WorkerError {
    let code = error.code().to_string();
    let retryable = error.is_retryable();
    WorkerError::new(code, error.message, retryable)
}

#[derive(Debug, Clone)]
struct WorkerError {
    code: String,
    message: String,
    retryable: bool,
}

impl WorkerError {
    fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }

    fn cancelled() -> Self {
        Self::new("CANCELLED", "Knowledge job was cancelled.", false)
    }

    fn storage_string(&self) -> String {
        if self.retryable {
            format!("{}: {}", self.code, self.message)
        } else {
            format!("{}: {}", self.code, self.message)
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ProgressSnapshot {
    stage: String,
    completed_units: i64,
    total_units: i64,
    provider_state: Option<String>,
}

struct LeaseKeeper {
    stop: CancellationToken,
    handle: Option<JoinHandle<()>>,
}

impl Drop for LeaseKeeper {
    fn drop(&mut self) {
        // A panic in the owning worker must not leave a heartbeat task alive
        // until its next twenty-second tick.  The durable CAS still protects
        // against a final race, while this token makes the normal drop path
        // prompt and quiet.
        self.stop.cancel();
    }
}

impl LeaseKeeper {
    fn start(
        app: &AppHandle,
        claim: &KnowledgeJobClaim,
        cancellation: &CancellationToken,
        progress: Arc<StdMutex<ProgressSnapshot>>,
    ) -> Self {
        let stop = CancellationToken::new();
        let task_app = app.clone();
        let task_claim = claim.clone();
        let task_cancellation = cancellation.clone();
        let task_stop = stop.clone();
        let handle = tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_stop.cancelled() => break,
                    _ = task_cancellation.cancelled() => break,
                    _ = tokio::time::sleep(LEASE_HEARTBEAT_INTERVAL) => {}
                }
                if task_stop.is_cancelled() || task_cancellation.is_cancelled() {
                    break;
                }
                let snapshot = progress
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_default();
                let state = task_app.state::<AppState>();
                let changed = locked_knowledge_call(&state, {
                    let claim = task_claim.clone();
                    move |repository| {
                        repository.heartbeat_claim(
                            &claim,
                            if snapshot.stage.is_empty() {
                                "validating"
                            } else {
                                snapshot.stage.as_str()
                            },
                            snapshot.completed_units,
                            snapshot.total_units,
                            snapshot.provider_state.as_deref(),
                        )
                    }
                })
                .await
                .unwrap_or(false);
                if !changed {
                    task_cancellation.cancel();
                    break;
                }
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }

    async fn stop(mut self) {
        self.stop.cancel();
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

/// Keep the concrete configuration conversion in one place so the worker's
/// policy snapshot cannot accidentally be rebuilt from mutable renderer data
/// halfway through a provider request.
struct MineruConfigForWorker(crate::knowledge::mineru::MineruConfig);

impl MineruConfigForWorker {
    fn from_settings(settings: &KnowledgeSettings) -> Self {
        Self(crate::knowledge::mineru::MineruConfig::from_knowledge_settings(settings))
    }
}
