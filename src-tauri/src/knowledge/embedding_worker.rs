//! Durable, cancellable worker for optional remote document embeddings.

use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::{
    ai::concurrency::ProviderRequestClass,
    knowledge::{
        embedding::{
            embed_documents_batched, EmbeddingError, EmbeddingProvider, EmbeddingProviderSpec,
            EmbeddingRetryPolicy, OpenAiCompatibleEmbeddingProvider,
        },
        repository::{EmbeddingJobClaim, EmbeddingWrite, KnowledgeRepository},
    },
    settings::app_types::KnowledgeSettings,
    state::AppState,
};

const IDLE_WAIT: Duration = Duration::from_secs(2);
const AUTO_QUEUE_INTERVAL: Duration = Duration::from_secs(30);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

pub(crate) fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        worker_loop(app).await;
    });
}

async fn worker_loop(app: AppHandle) {
    let state = app.state::<AppState>();
    let shutdown = state.knowledge_worker_shutdown.clone();
    let notify = state.knowledge_worker_notify.clone();
    let runtime_id = format!("embedding-worker-{}", uuid::Uuid::new_v4());
    let mut last_auto_queue = Instant::now()
        .checked_sub(AUTO_QUEUE_INTERVAL)
        .unwrap_or_else(Instant::now);

    loop {
        if shutdown.is_cancelled() {
            break;
        }
        let runtime = embedding_runtime(&state);
        if let Ok((settings, spec)) = runtime {
            if last_auto_queue.elapsed() >= AUTO_QUEUE_INTERVAL {
                let queue_spec = spec.clone();
                let _ = locked_knowledge_call(&state, move |repository| {
                    repository.enqueue_embedding_jobs(&queue_spec, None, false)
                })
                .await;
                last_auto_queue = Instant::now();
            }
            let claim = locked_knowledge_call(&state, {
                let runtime_id = runtime_id.clone();
                let embedding_key = spec.embedding_key.clone();
                move |repository| repository.claim_next_embedding_job(&runtime_id, &embedding_key)
            })
            .await;
            match claim {
                Ok(Some(claim)) => {
                    let cancellation = state
                        .register_knowledge_job(claim.common.job_id.clone())
                        .await;
                    process_claim(&app, claim, cancellation, settings, spec).await;
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!("Embedding worker could not claim a job: {error}");
                }
            }
        }

        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = notify.notified() => {},
            _ = tokio::time::sleep(IDLE_WAIT) => {},
        }
    }
}

fn embedding_runtime(
    state: &AppState,
) -> Result<(KnowledgeSettings, EmbeddingProviderSpec), EmbeddingError> {
    let settings = state
        .app_settings
        .read()
        .map(|settings| settings.knowledge.clone())
        .map_err(|_| {
            EmbeddingError::new(
                "EMBEDDING_SETTINGS_UNAVAILABLE",
                "Knowledge settings are unavailable.",
                true,
            )
        })?;
    let models = state.model_settings.read().map_err(|_| {
        EmbeddingError::new(
            "EMBEDDING_SETTINGS_UNAVAILABLE",
            "Model provider settings are unavailable.",
            true,
        )
    })?;
    let spec = EmbeddingProviderSpec::resolve(&settings, &models)?;
    Ok((settings, spec))
}

async fn process_claim(
    app: &AppHandle,
    claim: EmbeddingJobClaim,
    cancellation: CancellationToken,
    settings: KnowledgeSettings,
    spec: EmbeddingProviderSpec,
) {
    let state = app.state::<AppState>();
    let job_id = claim.common.job_id.clone();
    let result = process_embedding_job(&state, &claim, &cancellation, &settings, &spec).await;
    match result {
        Ok(()) => {
            let _ = app.emit(
                "mnemora://knowledge-job-updated",
                serde_json::json!({
                    "jobId": claim.common.job_id,
                    "documentId": claim.common.document_id,
                    "payload": { "state": "succeeded", "stage": "done" }
                }),
            );
        }
        Err(error) => finish_with_error(&state, &claim, &cancellation, error).await,
    }
    state.finish_knowledge_job(&job_id).await;
}

async fn process_embedding_job(
    state: &AppState,
    claim: &EmbeddingJobClaim,
    cancellation: &CancellationToken,
    settings: &KnowledgeSettings,
    spec: &EmbeddingProviderSpec,
) -> Result<(), EmbeddingError> {
    let provider_id = spec.provider_id.clone();
    let secrets = state.secrets;
    let key = tauri::async_runtime::spawn_blocking(move || secrets.get_api_key(&provider_id))
        .await
        .map_err(|_| {
            EmbeddingError::new(
                "EMBEDDING_API_KEY_UNAVAILABLE",
                "Could not read the embedding provider credential.",
                true,
            )
        })?
        .map_err(|_| {
            EmbeddingError::new(
                "EMBEDDING_API_KEY_UNAVAILABLE",
                "Could not read the embedding provider credential.",
                true,
            )
        })?
        .ok_or_else(|| {
            EmbeddingError::new(
                "EMBEDDING_API_KEY_MISSING",
                "The embedding provider API key is missing.",
                false,
            )
        })?;
    let provider = OpenAiCompatibleEmbeddingProvider::new(state.http.clone(), spec.clone(), key)?;
    let _permit = state
        .provider_concurrency
        .acquire(
            &spec.provider_id,
            ProviderRequestClass::Background,
            cancellation,
        )
        .await
        .map_err(|error| {
            if cancellation.is_cancelled() {
                EmbeddingError::cancelled()
            } else {
                EmbeddingError::new("EMBEDDING_CONCURRENCY_UNAVAILABLE", error.message, true)
            }
        })?;
    let chunks = locked_knowledge_call(state, {
        let claim = claim.clone();
        move |repository| repository.embedding_chunks_for_claim(&claim)
    })
    .await
    .map_err(storage_error)?;
    let total = chunks.len();
    if total == 0 {
        return complete_job(state, claim).await;
    }

    let completed = Arc::new(AtomicUsize::new(0));
    let heartbeat_stop = CancellationToken::new();
    let heartbeat = spawn_heartbeat(
        state,
        claim.clone(),
        completed.clone(),
        total,
        heartbeat_stop.clone(),
    );
    let policy = EmbeddingRetryPolicy {
        request_timeout: Duration::from_secs(u64::from(
            settings.network_timeout_seconds.clamp(30, 600),
        )),
        ..EmbeddingRetryPolicy::default()
    };
    let batch_size = provider.max_batch_size().max(1);
    let mut retry_count = 0usize;
    let mut result = Ok(());
    for batch in chunks.chunks(batch_size) {
        if cancellation.is_cancelled() {
            result = Err(EmbeddingError::cancelled());
            break;
        }
        let texts = batch
            .iter()
            .map(|chunk| chunk.search_text.clone())
            .collect::<Vec<_>>();
        let vectors =
            embed_documents_batched(&provider, &texts, cancellation, policy, |progress| {
                retry_count = progress.retries
            })
            .await;
        let vectors = match vectors {
            Ok(vectors) => vectors,
            Err(error) => {
                result = Err(error);
                break;
            }
        };
        let writes = batch
            .iter()
            .zip(vectors)
            .map(|(chunk, vector)| EmbeddingWrite {
                chunk_id: chunk.chunk_id.clone(),
                content_hash: chunk.content_hash.clone(),
                vector,
            })
            .collect::<Vec<_>>();
        let next_completed = completed
            .load(Ordering::Relaxed)
            .saturating_add(batch.len());
        let write_result = locked_knowledge_call(state, {
            let claim = claim.clone();
            let spec = spec.clone();
            move |repository| {
                repository.write_embedding_batch(
                    &claim,
                    &spec,
                    writes,
                    next_completed,
                    total,
                    retry_count,
                )
            }
        })
        .await;
        if let Err(error) = write_result {
            result = Err(storage_error(error));
            break;
        }
        completed.store(next_completed, Ordering::Relaxed);
    }
    heartbeat_stop.cancel();
    let _ = heartbeat.await;
    result?;
    complete_job(state, claim).await
}

fn spawn_heartbeat(
    state: &AppState,
    claim: EmbeddingJobClaim,
    completed: Arc<AtomicUsize>,
    total: usize,
    stop: CancellationToken,
) -> tauri::async_runtime::JoinHandle<()> {
    let repository = state.knowledge_repository.clone();
    let library_operations = state.library_operations.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = tokio::time::sleep(HEARTBEAT_INTERVAL) => {
                    let _guard = library_operations.lock().await;
                    let common = claim.common.clone();
                    let completed = completed.load(Ordering::Relaxed);
                    let repository = repository.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        repository.heartbeat_claim(
                            &common,
                            "embedding",
                            i64::try_from(completed).unwrap_or(i64::MAX),
                            i64::try_from(total).unwrap_or(i64::MAX),
                            Some("running"),
                        )
                    })
                    .await;
                }
            }
        }
    })
}

async fn complete_job(state: &AppState, claim: &EmbeddingJobClaim) -> Result<(), EmbeddingError> {
    let completed = locked_knowledge_call(state, {
        let claim = claim.clone();
        move |repository| repository.complete_embedding_claim(&claim)
    })
    .await
    .map_err(storage_error)?;
    if completed {
        Ok(())
    } else {
        Err(EmbeddingError::new(
            "EMBEDDING_LEASE_LOST",
            "Embedding job lease was replaced before completion.",
            false,
        ))
    }
}

async fn finish_with_error(
    state: &AppState,
    claim: &EmbeddingJobClaim,
    cancellation: &CancellationToken,
    error: EmbeddingError,
) {
    let cancelled = cancellation.is_cancelled() || error.code == "EMBEDDING_CANCELLED";
    if cancelled {
        let _ = locked_knowledge_call(state, {
            let job_id = claim.common.job_id.clone();
            move |repository| repository.cancel_job(&job_id)
        })
        .await;
        let _ = locked_knowledge_call(state, {
            let job_id = claim.common.job_id.clone();
            move |repository| repository.finalize_cancelled_job(&job_id, "EMBEDDING_CANCELLED")
        })
        .await;
        return;
    }
    let _ = locked_knowledge_call(state, {
        let common = claim.common.clone();
        let code = error.code;
        let message = error.message;
        move |repository| repository.fail_claimed_job(&common, &code, &message)
    })
    .await;
}

fn storage_error(message: String) -> EmbeddingError {
    EmbeddingError::new("EMBEDDING_STORAGE", message, false)
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
        .map_err(|error| format!("Embedding worker task failed: {error}"))?
}
