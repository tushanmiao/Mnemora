//! PDF/Markdown 知识库 Tauri 命令边界。
//!
//! 所有写入都只操作可重建派生表，并复用 library 写锁。MinerU Token、正文和
//! 图片二进制不会通过这些状态命令返回。

use tauri::State;

use crate::{
    ai::concurrency::ProviderRequestClass,
    knowledge::{
        embedding::{
            embed_query_with_retry, EmbeddingProviderSpec, EmbeddingRetryPolicy,
            OpenAiCompatibleEmbeddingProvider,
        },
        types::{
            KnowledgeChunkView, KnowledgeConsentStatus, KnowledgeDocumentStatus,
            KnowledgeEmbeddingRebuildResult, KnowledgeGlobalConsentStatus, KnowledgeJobView,
            KnowledgeMineruTokenStatus, KnowledgeOverview, KnowledgeRebuildResult,
            KnowledgeSearchRequest, KnowledgeSearchResponse,
        },
    },
    settings::app_types::KnowledgeRetrievalMode,
    state::AppState,
};

/// Schedule a local Markdown projection after the library write has committed.
///
/// The library tables remain the transaction authority.  Knowledge indexing is
/// deliberately detached from the command response so a note save never waits
/// on parsing, hashing, or FTS maintenance.  The repository's content hash and
/// idempotency key make repeated notifications safe.
pub(crate) fn schedule_note_sync(state: &AppState, note_id: impl Into<String>) {
    if !knowledge_indexing_enabled(state) {
        return;
    }
    let repository = state.knowledge_repository.clone();
    let note_id = note_id.into();
    tauri::async_runtime::spawn(async move {
        let result =
            tauri::async_runtime::spawn_blocking(move || repository.sync_note(&note_id)).await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => eprintln!("Automatic Markdown knowledge indexing failed: {error}"),
            Err(error) => eprintln!("Automatic Markdown knowledge indexing task failed: {error}"),
        }
    });
}

/// Register a PDF source after import or restore.  Registration is always
/// local.  A processing job is created only when the user enabled automatic
/// PDF parsing; consent is still checked separately by the worker.
pub(crate) fn schedule_literature_enqueue(state: &AppState, item_id: impl Into<String>) {
    if !knowledge_indexing_enabled(state) {
        return;
    }
    let repository = state.knowledge_repository.clone();
    let item_id = item_id.into();
    let auto_parse = state
        .app_settings
        .read()
        .map(|settings| settings.knowledge.auto_parse_imported_pdf)
        .unwrap_or(false);
    if auto_parse {
        state.notify_knowledge_worker();
    }
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            if auto_parse {
                repository.enqueue_literature(&item_id)
            } else {
                repository.register_literature(&item_id)
            }
        })
        .await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => eprintln!("Automatic PDF knowledge registration failed: {error}"),
            Err(error) => eprintln!("Automatic PDF knowledge registration task failed: {error}"),
        }
    });
}

pub(crate) fn schedule_source_deleted(
    state: &AppState,
    source_class: &'static str,
    source_id: impl Into<String>,
) {
    let repository = state.knowledge_repository.clone();
    let source_id = source_id.into();
    tauri::async_runtime::spawn(async move {
        let result = tauri::async_runtime::spawn_blocking(move || {
            repository.mark_source_deleted(source_class, &source_id)
        })
        .await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => eprintln!("Automatic knowledge source cleanup failed: {error}"),
            Err(error) => eprintln!("Automatic knowledge source cleanup task failed: {error}"),
        }
    });
}

fn knowledge_indexing_enabled(state: &AppState) -> bool {
    state
        .app_settings
        .read()
        .map(|settings| settings.knowledge.enabled)
        .unwrap_or(false)
}

fn join_error(error: impl std::fmt::Display) -> String {
    format!("知识库后台任务失败：{error}")
}

#[tauri::command]
pub async fn knowledge_overview(state: State<'_, AppState>) -> Result<KnowledgeOverview, String> {
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.overview())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn knowledge_list_documents(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<KnowledgeDocumentStatus>, String> {
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list_documents(limit.unwrap_or(200)))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn knowledge_list_jobs(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<KnowledgeJobView>, String> {
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.list_jobs(limit.unwrap_or(100)))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn knowledge_rebuild_all(
    state: State<'_, AppState>,
) -> Result<KnowledgeRebuildResult, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    let result = tauri::async_runtime::spawn_blocking(move || repository.rebuild_all())
        .await
        .map_err(join_error)??;
    state.notify_knowledge_worker();
    Ok(result)
}

#[tauri::command]
pub async fn knowledge_rebuild_note(
    state: State<'_, AppState>,
    note_id: String,
) -> Result<KnowledgeDocumentStatus, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.sync_note(&note_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn knowledge_enqueue_literature(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<KnowledgeDocumentStatus, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || repository.enqueue_literature(&item_id))
            .await
            .map_err(join_error)??;
    state.notify_knowledge_worker();
    Ok(result)
}

#[tauri::command]
pub async fn knowledge_search(
    state: State<'_, AppState>,
    request: KnowledgeSearchRequest,
) -> Result<KnowledgeSearchResponse, String> {
    let repository = state.knowledge_repository.clone();
    let knowledge_settings = state
        .app_settings
        .read()
        .map(|settings| settings.knowledge.clone())
        .unwrap_or_default();
    let requested_mode = knowledge_settings.retrieval_mode;
    if requested_mode == KnowledgeRetrievalMode::Lexical || !knowledge_settings.embedding_enabled {
        return tauri::async_runtime::spawn_blocking(move || repository.search(request))
            .await
            .map_err(join_error)?;
    }

    let model_settings = state
        .model_settings
        .read()
        .map(|settings| settings.clone())
        .map_err(|_| "模型供应商设置暂时不可用。".to_string())?;
    let spec = match EmbeddingProviderSpec::resolve(&knowledge_settings, &model_settings) {
        Ok(spec) => spec,
        Err(error) => {
            return lexical_search_fallback(repository, request, requested_mode, error.code).await
        }
    };
    let secrets = state.secrets;
    let provider_id = spec.provider_id.clone();
    let key_result =
        tauri::async_runtime::spawn_blocking(move || secrets.get_api_key(&provider_id))
            .await
            .map_err(join_error)?;
    let key = match key_result {
        Ok(key) => key,
        Err(_) => {
            return lexical_search_fallback(
                repository,
                request,
                requested_mode,
                "EMBEDDING_API_KEY_UNAVAILABLE".to_string(),
            )
            .await;
        }
    };
    let Some(key) = key.filter(|value| !value.trim().is_empty()) else {
        return lexical_search_fallback(
            repository,
            request,
            requested_mode,
            "EMBEDDING_API_KEY_MISSING".to_string(),
        )
        .await;
    };
    let provider =
        match OpenAiCompatibleEmbeddingProvider::new(state.http.clone(), spec.clone(), key) {
            Ok(provider) => provider,
            Err(error) => {
                return lexical_search_fallback(repository, request, requested_mode, error.code)
                    .await
            }
        };
    let cancellation = tokio_util::sync::CancellationToken::new();
    let _permit = match state
        .provider_concurrency
        .acquire(
            &spec.provider_id,
            ProviderRequestClass::Interactive,
            &cancellation,
        )
        .await
    {
        Ok(permit) => permit,
        Err(_) => {
            return lexical_search_fallback(
                repository,
                request,
                requested_mode,
                "EMBEDDING_CONCURRENCY_UNAVAILABLE".to_string(),
            )
            .await
        }
    };
    let policy = EmbeddingRetryPolicy {
        request_timeout: std::time::Duration::from_secs(u64::from(
            knowledge_settings.network_timeout_seconds.clamp(30, 600),
        )),
        ..EmbeddingRetryPolicy::default()
    };
    let query_vector =
        match embed_query_with_retry(&provider, &request.query, &cancellation, policy).await {
            Ok(vector) => vector,
            Err(error) => {
                return lexical_search_fallback(repository, request, requested_mode, error.code)
                    .await
            }
        };
    let embedding_key = spec.embedding_key.clone();
    let vector_request = request.clone();
    match tauri::async_runtime::spawn_blocking(move || {
        repository.search_with_vector(vector_request, requested_mode, &embedding_key, query_vector)
    })
    .await
    .map_err(join_error)?
    {
        Ok(response) => Ok(response),
        Err(error) => {
            let repository = state.knowledge_repository.clone();
            lexical_search_fallback(repository, request, requested_mode, bounded_reason(&error))
                .await
        }
    }
}

async fn lexical_search_fallback(
    repository: crate::knowledge::KnowledgeRepository,
    request: KnowledgeSearchRequest,
    requested_mode: KnowledgeRetrievalMode,
    reason: String,
) -> Result<KnowledgeSearchResponse, String> {
    let mut response = tauri::async_runtime::spawn_blocking(move || repository.search(request))
        .await
        .map_err(join_error)??;
    response.requested_mode = requested_mode.as_str().to_string();
    response.actual_mode = "lexical".to_string();
    response.fallback_reason = Some(bounded_reason(&reason));
    Ok(response)
}

fn bounded_reason(value: &str) -> String {
    value.chars().take(240).collect()
}

#[tauri::command]
pub async fn knowledge_rebuild_embeddings(
    state: State<'_, AppState>,
    document_id: Option<String>,
    force: Option<bool>,
) -> Result<KnowledgeEmbeddingRebuildResult, String> {
    let knowledge_settings = state
        .app_settings
        .read()
        .map(|settings| settings.knowledge.clone())
        .map_err(|_| "知识库设置暂时不可用。".to_string())?;
    let model_settings = state
        .model_settings
        .read()
        .map(|settings| settings.clone())
        .map_err(|_| "模型供应商设置暂时不可用。".to_string())?;
    let spec = EmbeddingProviderSpec::resolve(&knowledge_settings, &model_settings)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let secrets = state.secrets;
    let provider_id = spec.provider_id.clone();
    let configured =
        tauri::async_runtime::spawn_blocking(move || secrets.has_api_key(&provider_id))
            .await
            .map_err(join_error)?
            .map_err(|_| "EMBEDDING_API_KEY_UNAVAILABLE: 无法读取当前供应商凭据。".to_string())?;
    if !configured {
        return Err("EMBEDDING_API_KEY_MISSING: 当前供应商尚未配置 API Key。".to_string());
    }
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        repository.enqueue_embedding_jobs(&spec, document_id.as_deref(), force.unwrap_or(false))
    })
    .await
    .map_err(join_error)??;
    state.notify_knowledge_worker();
    Ok(result)
}

#[tauri::command]
pub async fn knowledge_read_chunk(
    state: State<'_, AppState>,
    chunk_id: String,
) -> Result<KnowledgeChunkView, String> {
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.get_chunk(&chunk_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn knowledge_cancel_job(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<bool, String> {
    state
        .cancel_knowledge_job(&job_id)
        .await
        .map_err(join_error)
}

#[tauri::command]
pub async fn knowledge_get_mineru_token_status(
    state: State<'_, AppState>,
) -> Result<KnowledgeMineruTokenStatus, String> {
    let secrets = state.secrets;
    let configured = tauri::async_runtime::spawn_blocking(move || secrets.has_mineru_token())
        .await
        .map_err(join_error)??;
    Ok(KnowledgeMineruTokenStatus { configured })
}

#[tauri::command]
pub async fn knowledge_set_mineru_token(
    state: State<'_, AppState>,
    token: String,
) -> Result<KnowledgeMineruTokenStatus, String> {
    let secrets = state.secrets;
    tauri::async_runtime::spawn_blocking(move || secrets.set_mineru_token(&token))
        .await
        .map_err(join_error)??;
    let configured = tauri::async_runtime::spawn_blocking(move || secrets.has_mineru_token())
        .await
        .map_err(join_error)??;
    Ok(KnowledgeMineruTokenStatus { configured })
}

#[tauri::command]
pub async fn knowledge_delete_mineru_token(
    state: State<'_, AppState>,
) -> Result<KnowledgeMineruTokenStatus, String> {
    let secrets = state.secrets;
    tauri::async_runtime::spawn_blocking(move || secrets.delete_mineru_token())
        .await
        .map_err(join_error)??;
    let configured = tauri::async_runtime::spawn_blocking(move || secrets.has_mineru_token())
        .await
        .map_err(join_error)??;
    Ok(KnowledgeMineruTokenStatus { configured })
}

#[tauri::command]
pub async fn knowledge_grant_literature_consent(
    state: State<'_, AppState>,
    item_id: String,
    scope: String,
) -> Result<KnowledgeDocumentStatus, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        repository.grant_literature_consent(&item_id, &scope)
    })
    .await
    .map_err(join_error)??;
    state.notify_knowledge_worker();
    Ok(result)
}

#[tauri::command]
pub async fn knowledge_grant_global_literature_consent(
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    let count =
        tauri::async_runtime::spawn_blocking(move || repository.grant_global_literature_consent())
            .await
            .map_err(join_error)??;
    state.notify_knowledge_worker();
    Ok(count)
}

#[tauri::command]
pub async fn knowledge_revoke_literature_consent(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<bool, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.revoke_literature_consent(&item_id))
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn knowledge_revoke_global_literature_consent(
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let _guard = state.library_operations.lock().await;
    let repository = state.knowledge_repository.clone();
    let changed =
        tauri::async_runtime::spawn_blocking(move || repository.revoke_global_literature_consent())
            .await
            .map_err(join_error)??;
    state.notify_knowledge_worker();
    Ok(changed)
}

#[tauri::command]
pub async fn knowledge_global_literature_consent_status(
    state: State<'_, AppState>,
) -> Result<KnowledgeGlobalConsentStatus, String> {
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.global_literature_consent_status())
        .await
        .map_err(join_error)?
}

#[tauri::command]
pub async fn knowledge_literature_consent_status(
    state: State<'_, AppState>,
    item_id: String,
) -> Result<Option<KnowledgeConsentStatus>, String> {
    let repository = state.knowledge_repository.clone();
    tauri::async_runtime::spawn_blocking(move || repository.literature_consent_status(&item_id))
        .await
        .map_err(join_error)?
}
