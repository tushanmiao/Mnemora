use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    future::Future,
    io::Read,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    time::Duration,
};

use futures::{stream, StreamExt};
use sha2::{Digest, Sha256};

use tauri::{ipc::Channel, AppHandle, Manager, Runtime};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    ai::{
        error::{ModelError, ModelErrorKind},
        types::{ModelOptions, ModelRole, ModelToolCall},
    },
    chat::{
        attachment_formats::{self, AttachmentReadKind},
        conversation_types::{MessageStatus, StoredChatMessage, StoredConversation},
        service as chat_service,
        types::{ChatCompletionRequest, ChatModelMessage, ChatWorkspaceMode},
    },
    library::{
        note_files::NoteAttachmentSource,
        types::{
            LibraryNoteCreate, NoteEditProposalCreate, NotePipelinePhase, NotePipelineRun,
            NotePipelineRunCreate, NotePipelineSection, NotePipelineSectionCreate,
            NotePipelineSectionStatus, NoteSourceCreate, NoteSourceOrigin,
        },
    },
    settings::types::ModelSettings,
    state::AppState,
    task_diagnostics::{current_task_instance_id, scope_task_diagnostics, TaskDiagnosticContext},
};

use super::{
    adaptive_volume::{
        AdaptiveVolumeOutcome, AdaptiveVolumeProfile, DeepNoteRouteIdentity, RouteAvailability,
        MAX_ADAPTIVE_CHUNK_TOKENS, MIN_ADAPTIVE_CHUNK_TOKENS,
    },
    merge::{apply_note_patches, compact_diff},
    prompts::{
        ANALYST_SYSTEM_PROMPT, CHUNK_ANALYST_SYSTEM_PROMPT, NOTE_ATTACHMENT_EDIT_PATCH_PROMPT,
        NOTE_ATTACHMENT_EDIT_PLAN_PROMPT, NOTE_ATTACHMENT_REVIEW_PROMPT, NOTE_EDIT_PATCH_PROMPT,
        NOTE_EDIT_PLAN_PROMPT, SECTION_REVISION_SYSTEM_PROMPT, SECTION_SYSTEM_PROMPT,
        STRICT_JSON_SUFFIX,
    },
    run_machine::{DeepNoteRunEffect, DeepNoteRunMachine},
    scheduler::{stable_topological_sections, DeepNoteDagScheduler},
    types::{
        compile_plan, DeepNoteBudget, DeepNoteCapabilities, DeepNoteContextBudget, DeepNoteDagNode,
        DeepNoteEvidenceArtifact, DeepNoteEvidenceStatus, DeepNoteInputSnapshot, DeepNoteLedger,
        DeepNoteLocalReaderCapabilities, DeepNoteModelSnapshot, DeepNoteNodeStatus,
        DeepNoteOutline, DeepNotePlanVersion, DeepNotePreflight, DeepNoteRunDetail,
        DeepNoteRuntimeState, DeepNoteSection, DeepNoteSectionProgress, DeepNoteSkillProfileKind,
        DeepNoteSkillProfiles, DeepNoteSkillSnapshot, DeepNoteSourceChunk, DeepNoteSourceKind,
        DeepNoteSourceUnit, DeepNoteSourceUnitKind, DeepNoteSourceUnitStatus,
        DeepNoteStartInspection, DeepNoteSupportLevel, DeepNoteValidationReport,
        NoteEditPrepareRequest, NoteEditPrepareResult, NoteMergePlan, NotePatchSet,
        NotePipelineActivity, NotePipelineAdjustRequest, NotePipelineCancelResult,
        NotePipelineConfirmRequest, NotePipelineProgress, NotePipelineStartRequest,
        DEEP_NOTE_FAILURE_PREFIX, MAX_DEEP_NOTE_SOURCE_CHUNKS, MAX_DEEP_NOTE_UPSTREAM_REQUESTS,
    },
};

#[cfg(test)]
use super::adaptive_volume::INITIAL_ADAPTIVE_CHUNK_TOKENS;

// Direct mode is only safe when the same raw input can also be reused by a
// section writer. Larger inputs first build a traceable ledger so drafting
// never reaches an outline with no bounded source context.
const DIRECT_PLANNER_TOKEN_LIMIT: u64 = 3_000;
// 提纲是受 Rust schema 校验的短 JSON。较小的输出上限可以显著降低中转网关
// 对推理模型的首字节等待时间，避免 504；章节正文仍使用独立的输出预算。
const PLANNER_OUTPUT_TOKEN_LIMIT: u32 = 2_048;
// Chunk digests contain evidence links and message IDs. Keep a larger bound than
// the outline planner so a valid JSON object is not cut off mid-string by a
// provider that spends tokens on structured output.
const CHUNK_OUTPUT_TOKEN_LIMIT: u32 = 4_096;
const PLANNER_FALLBACK_RETRIES: u8 = 1;
const FAST_PLANNER_OUTPUT_TOKENS: u32 = 1_024;
const SECTION_OUTPUT_TOKEN_LIMIT: u32 = 2_048;
const SECTION_SOURCE_TOKEN_LIMIT: u64 = 3_000;
const FAST_PLANNER_SYSTEM_PROMPT: &str = r#"You are the outline planner for a study note. Return exactly one valid JSON object and no markdown. Use only the supplied ledger and message IDs. Identify the unspoken question, missing prerequisite, confused causal link, or misconception that actually blocks understanding. Prefer causal mechanisms over empty abstraction. Record Mermaid opportunities as 'diagram type | cognitive question | target section'. Choose by semantics instead of defaulting to flowchart: mindmap for hierarchy, stateDiagram-v2 for lifecycle, sequenceDiagram for interactions, erDiagram for entities, classDiagram for types, gantt/timeline for real schedules, journey for execution experience, requirementDiagram for requirements, and xychart-beta/pie only for sourced numeric data. Keep the outline concise: 4 to 8 sections. Required shape: {"goal":"","audience":"","scope":"","title":"","summary":"","weakPoints":[],"hiddenQuestions":[],"knowledgeGaps":[],"misconceptions":[],"causalChains":[],"visualizationOpportunities":[],"allowAiSupplement":false,"evidencePolicy":"","sourceIds":[],"sections":[{"id":"sec-1","heading":"","kind":"prerequisite|concept|comparison|pitfall|example|summary|selfcheck","purpose":"","brief":"","dependsOn":[],"evidenceRequirements":[],"successCriteria":[],"sourceScope":[],"targetDepth":"standard","allowAiSupplement":false,"needsSupplement":false,"sourceMessageIds":[]}]}. Section ids must be unique and use only ASCII letters, digits, hyphens, and underscores (like sec-1) — never CJK characters or spaces. Every sourceMessageIds value must be copied from the ledger. Do not invent facts, IDs, schedules, or numbers."#;
const OUTLINE_SIZE_SUFFIX: &str =
    "Prefer 6 to 12 sections and never exceed 12 sections. Keep every field concise.";
const DEFAULT_CHUNK_TARGET_TOKENS: u64 = 16_000;
const UNKNOWN_CONTEXT_CHUNK_TOKENS: u64 = 8_000;
const PLANNER_PROMPT_OVERHEAD_TOKENS: u64 = 4_096;
const MAX_ANALYSIS_CHUNKS: usize = MAX_DEEP_NOTE_SOURCE_CHUNKS;
const MAX_INCREMENTAL_ATTACHMENT_CHUNKS: usize = 24;
const MAX_UNPACKED_ATTACHMENT_CHUNKS: usize = 1_024;
const PIPELINE_STOP_WAIT_ATTEMPTS: usize = 80;
const PIPELINE_STOP_WAIT_INTERVAL: Duration = Duration::from_millis(50);
const PIPELINE_ABORT_WAIT_ATTEMPTS: usize = 20;
/// 纯文本请求的请求体字节上限。
///
/// 按字节而非 token 设闸：网关限的是 body 大小，而一个中文字符是 1 token、3 字节，
/// token 估算会系统性低估。2 MiB 约等于 70 万中文字符 —— 正常单次调用远达不到，
/// 所以这道闸只会在载荷失控时响（例如超长对话直出规划），不会误伤正常请求。
const REQUEST_BYTE_LIMIT: usize = 2 * 1024 * 1024;
/// 带图请求的请求体字节上限。
///
/// base64 图片比原文件再大约 1/3，且在 token 估算里几乎不占位，是最容易悄悄超限的
/// 一类载荷，所以单独给一档更宽的上限，而不是让它去挤文本档。
const VISION_REQUEST_BYTE_LIMIT: usize = 16 * 1024 * 1024;
const RUN_BUDGET_EXHAUSTED_PREFIX: &str = "deep-note-run-budget-exhausted:";

#[derive(Debug, Clone)]
struct AdaptiveBudgetSnapshot {
    limit_tokens: u64,
    route_key: String,
    route_state: String,
    profile_samples: u64,
}

impl AdaptiveBudgetSnapshot {
    #[cfg(test)]
    fn cold_start() -> Self {
        Self {
            limit_tokens: INITIAL_ADAPTIVE_CHUNK_TOKENS,
            route_key: String::new(),
            route_state: "unknown".to_string(),
            profile_samples: 0,
        }
    }

    fn from_profile(profile: &AdaptiveVolumeProfile) -> Self {
        Self {
            limit_tokens: profile.effective_target_tokens(crate::usage::now_ms()),
            route_key: profile.identity.route_key.clone(),
            route_state: profile.availability.as_str().to_string(),
            profile_samples: profile.sample_count,
        }
    }
}

fn deep_note_route_profile(
    state: &AppState,
    provider_id: &str,
    model_id: &str,
) -> Result<(DeepNoteRouteIdentity, AdaptiveVolumeProfile), String> {
    let streaming_preferred = state
        .app_settings
        .read()
        .map_err(|_| "应用设置锁不可用。".to_string())?
        .deep_note_stream_keepalive;
    let identity = {
        let settings = state
            .model_settings
            .read()
            .map_err(|_| "模型设置锁不可用。".to_string())?;
        DeepNoteRouteIdentity::resolve(&settings, provider_id, model_id, streaming_preferred)?
    };
    let profile = state
        .library_repository
        .get_or_create_deep_note_route_profile(&identity)?;
    Ok((identity, profile))
}

fn blocked_route_error(profile: &AdaptiveVolumeProfile) -> Option<ModelError> {
    let reason = profile.blocked_reason(crate::usage::now_ms())?;
    Some(if profile.availability == RouteAvailability::Unsupported {
        ModelError::model_not_found(reason)
    } else {
        ModelError::route_unavailable(reason)
    })
}

fn record_adaptive_route_outcome(
    state: &AppState,
    run_id: &str,
    identity: &DeepNoteRouteIdentity,
    previous: &AdaptiveVolumeProfile,
    outcome: &AdaptiveVolumeOutcome,
) -> AdaptiveVolumeProfile {
    match state
        .library_repository
        .record_deep_note_route_outcome(identity, outcome)
    {
        Ok(profile) => profile,
        Err(error) => {
            let _ = state.library_repository.append_note_pipeline_event(
                run_id,
                "routeProfileUpdateFailed",
                None,
                &serde_json::json!({
                    "routeKey": identity.route_key.as_str(),
                    "message": error,
                })
                .to_string(),
            );
            previous.clone()
        }
    }
}

/// 按本次调用是否带图选择字节上限档位。
///
/// 只看 `kind == "image"`：非图片附件走的是读取工具、内容以文本进入 prompt，已经被
/// 文本档覆盖；只有图片会被内联成 base64 直接撑大请求体。
fn request_byte_limit(
    attachments: &[crate::chat::conversation_types::StoredChatAttachment],
) -> usize {
    if attachments.iter().any(|item| item.kind == "image") {
        VISION_REQUEST_BYTE_LIMIT
    } else {
        REQUEST_BYTE_LIMIT
    }
}

fn budget_for_drafting(previous: &DeepNoteBudget, section_count: usize) -> DeepNoteBudget {
    let mut budget = DeepNoteBudget::for_section_count(section_count);
    budget.semantic_calls_used = previous.semantic_calls_used;
    budget.upstream_request_limit = previous.upstream_request_limit;
    budget.upstream_requests_used = previous.upstream_requests_used;
    budget.replans_used = previous.replans_used;
    budget.upstream_wall_clock_ms = previous.upstream_wall_clock_ms;
    budget
}

#[derive(Debug, Clone)]
struct ConversationChunk {
    source: DeepNoteSourceChunk,
    message_ids: Vec<String>,
    estimated_tokens: u64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunkDigest {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    canonical_terms: Vec<String>,
    #[serde(default)]
    verified_facts: Vec<String>,
    #[serde(default)]
    covered_topics: Vec<String>,
    #[serde(default)]
    open_questions: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    global_constraints: Vec<String>,
    #[serde(default)]
    source_message_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct ChunkDigestJob {
    index: usize,
    chunk: ConversationChunk,
    system_prompt: String,
    user_prompt: String,
    prompt_hash: String,
}

#[derive(Debug)]
struct ChunkDigestJobResult {
    index: usize,
    chunk_id: String,
    prompt_hash: String,
    semantic_calls: u32,
    result: Result<ChunkDigest, String>,
    /// 失败是否由超时类错误（本地超时或网关 504）引起。
    ///
    /// `should_retry_note_model_call` 刻意否决了这类错误的 HTTP 层重试，理由是
    /// 「交还给管线缩小载荷后再试」。orchestrator 需要这一位才能兑现那半个承诺：
    /// 只有超时才值得切小重投，格式错误再切也还是错。
    timed_out: bool,
}

#[derive(Debug, Clone)]
struct SectionDagJob {
    section: DeepNoteSection,
    dependency_outputs: String,
    evidence_ids: Vec<String>,
    persisted: Option<NotePipelineSection>,
    writer_system_prompt: String,
    reviewer_system_prompt: String,
    reserved_semantic_calls: u32,
}

#[derive(Debug)]
struct SectionDagJobResult {
    job: SectionDagJob,
    result: Result<(Option<(String, DeepNoteValidationReport, u8, u8)>, u32), String>,
    /// 本轮这个 section 实际执行了多久（毫秒），用于累加 section 级墙钟预算。
    ///
    /// 在任务内部量而不是由调用方按批次估算：并行批次里各 section 的起止时刻不同，
    /// 用批次时间会把等其他 section 的时间摊到每一个身上。三条结果分支（成功、
    /// 取消、失败）都要累加 —— 时间是实际花掉的，失败不退款。
    active_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentUpdateReview {
    #[serde(default)]
    passed: bool,
    #[serde(default)]
    requires_full_rebuild: bool,
    #[serde(default)]
    warnings: Vec<String>,
}

impl ChunkDigest {
    fn validate(self) -> Result<Self, String> {
        if self.summary.trim().is_empty() {
            return Err("来源分块摘要为空，不能把该分块标记为已覆盖。".to_string());
        }
        Ok(self)
    }
}

fn send(channel: &Channel<NotePipelineProgress>, event: NotePipelineProgress) {
    let _ = channel.send(event);
}

fn pipeline_progress_suppressed(state: &AppState, run_id: &str) -> bool {
    state
        .library_repository
        .get_note_pipeline_run(run_id)
        .is_ok_and(|run| {
            matches!(
                run.phase,
                NotePipelinePhase::Cancelling
                    | NotePipelinePhase::Cancelled
                    | NotePipelinePhase::Paused
                    | NotePipelinePhase::Done
                    | NotePipelinePhase::Error
            )
        })
}

fn send_paused_if_requested(
    state: &AppState,
    run_id: &str,
    channel: &Channel<NotePipelineProgress>,
) -> Result<bool, String> {
    let run = state.library_repository.get_note_pipeline_run(run_id)?;
    if run.phase != NotePipelinePhase::Paused {
        return Ok(false);
    }
    send(channel, NotePipelineProgress::Paused { run });
    Ok(true)
}

async fn finish_interrupted_run(
    state: &AppState,
    run_id: &str,
    channel: &Channel<NotePipelineProgress>,
) -> Result<(), String> {
    let (run, paused) = {
        let _guard = state.library_operations.lock().await;
        let current = state.library_repository.get_note_pipeline_run(run_id)?;
        if current.phase == NotePipelinePhase::Paused {
            (current, true)
        } else {
            let warnings = current.warnings.clone();
            (
                state.library_repository.update_note_pipeline_phase(
                    run_id,
                    NotePipelinePhase::Cancelled,
                    None,
                    &warnings,
                    None,
                )?,
                false,
            )
        }
    };
    if paused {
        send(channel, NotePipelineProgress::Paused { run });
    } else {
        send(channel, NotePipelineProgress::Cancelled { run });
    }
    Ok(())
}

async fn wait_for_pipeline_task_to_stop(state: &AppState, run_id: &str) -> bool {
    for _ in 0..PIPELINE_STOP_WAIT_ATTEMPTS {
        if !state.is_note_pipeline_run_active(run_id).await {
            return true;
        }
        tokio::time::sleep(PIPELINE_STOP_WAIT_INTERVAL).await;
    }
    !state.is_note_pipeline_run_active(run_id).await
}

async fn wait_for_pipeline_task_abort(state: &AppState, run_id: &str) -> bool {
    for _ in 0..PIPELINE_ABORT_WAIT_ATTEMPTS {
        if !state.is_note_pipeline_run_active(run_id).await {
            return true;
        }
        tokio::time::sleep(PIPELINE_STOP_WAIT_INTERVAL).await;
    }
    !state.is_note_pipeline_run_active(run_id).await
}

async fn await_note_pipeline_cancellable<T, F>(
    cancellation: &CancellationToken,
    future: F,
) -> Result<T, String>
where
    F: Future<Output = Result<T, String>>,
{
    tokio::select! {
        _ = cancellation.cancelled() => Err("操作已取消。".to_string()),
        result = future => result,
    }
}

fn can_pause_phase(phase: NotePipelinePhase) -> bool {
    matches!(
        phase,
        NotePipelinePhase::Analyzing
            | NotePipelinePhase::Compiling
            | NotePipelinePhase::Queued
            | NotePipelinePhase::Drafting
            | NotePipelinePhase::Validating
            | NotePipelinePhase::Replanning
    )
}

fn phase_expects_background_worker(phase: NotePipelinePhase) -> bool {
    matches!(
        phase,
        NotePipelinePhase::Preflight
            | NotePipelinePhase::Analyzing
            | NotePipelinePhase::Compiling
            | NotePipelinePhase::Queued
            | NotePipelinePhase::Drafting
            | NotePipelinePhase::Validating
            | NotePipelinePhase::Replanning
            | NotePipelinePhase::Assembling
            | NotePipelinePhase::Persisting
    )
}

fn progress(
    state: &AppState,
    channel: &Channel<NotePipelineProgress>,
    run_id: &str,
    phase: NotePipelinePhase,
    current: Option<usize>,
    total: Option<usize>,
    message: impl Into<String>,
) {
    if pipeline_progress_suppressed(state, run_id) {
        return;
    }
    let message = message.into();
    let _ = state.library_repository.append_note_pipeline_event(
        run_id,
        "phaseProgress",
        None,
        &serde_json::json!({
            "phase": phase.as_str(),
            "current": current,
            "total": total,
            "message": message,
        })
        .to_string(),
    );
    send(
        channel,
        NotePipelineProgress::Progress {
            run_id: run_id.to_string(),
            phase,
            current,
            total,
            message,
            activity: None,
        },
    );
}

fn progress_activity(
    state: &AppState,
    channel: &Channel<NotePipelineProgress>,
    run_id: &str,
    phase: NotePipelinePhase,
    message: impl Into<String>,
    activity: NotePipelineActivity,
) {
    if pipeline_progress_suppressed(state, run_id) {
        return;
    }
    let message = message.into();
    let event_type = if activity.kind == "retryWait" {
        "modelRetryScheduled"
    } else {
        "modelCallStarted"
    };
    let _ = state.library_repository.append_note_pipeline_event(
        run_id,
        event_type,
        None,
        &serde_json::json!({
            "phase": phase.as_str(),
            "message": message,
            "activity": activity,
        })
        .to_string(),
    );
    send(
        channel,
        NotePipelineProgress::Progress {
            run_id: run_id.to_string(),
            phase,
            current: None,
            total: None,
            message,
            activity: Some(activity),
        },
    );
}

fn append_pipeline_event_if_available(
    state: &AppState,
    run_id: &str,
    event_type: &str,
    node_id: Option<&str>,
    payload_json: &str,
) -> Result<(), String> {
    if let Some(instance_id) = current_task_instance_id() {
        if state.is_note_pipeline_instance_detached(&instance_id) {
            return Ok(());
        }
    }
    match state.library_repository.append_note_pipeline_event(
        run_id,
        event_type,
        node_id,
        payload_json,
    ) {
        Ok(_) => {}
        Err(error) if error == "深度笔记任务不存在。" => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn noteworthy_messages(conversation: &StoredConversation) -> Vec<&StoredChatMessage> {
    conversation
        .messages
        .iter()
        .filter(|message| {
            message.status == MessageStatus::Completed
                && (!message.content.trim().is_empty()
                    || !message.attachments.is_empty()
                    || !message.literature_references.is_empty()
                    || !message.note_references.is_empty())
        })
        .collect()
}

fn message_source_hash(message: &StoredChatMessage) -> String {
    stable_hash(
        serde_json::to_vec(&serde_json::json!({
            "id": message.id,
            "role": message.role,
            "content": message.content,
            "attachments": message.attachments,
            "literatureReferences": message.literature_references,
            "noteReferences": message.note_references,
            "status": message.status,
        }))
        .unwrap_or_default(),
    )
}

fn attachment_metadata_hash(
    attachment: &crate::chat::conversation_types::StoredChatAttachment,
) -> String {
    stable_hash(
        serde_json::to_vec(&serde_json::json!({
            "id": attachment.id,
            "kind": attachment.kind,
            "name": attachment.name,
            "mimeType": attachment.mime_type,
            "sizeBytes": attachment.size_bytes,
            "path": attachment.path,
            "previewPath": attachment.preview_path,
            "width": attachment.width,
            "height": attachment.height,
        }))
        .unwrap_or_default(),
    )
}

fn attachment_snapshot_hash(
    repository: &crate::chat::storage::ConversationRepository,
    conversation_id: &str,
    attachment: &crate::chat::conversation_types::StoredChatAttachment,
) -> Result<String, String> {
    let path = repository.resolve_attachment_path(conversation_id, &attachment.path)?;
    let mut file =
        File::open(&path).map_err(|error| format!("读取附件“{}”失败：{error}", attachment.name))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("校验附件“{}”失败：{error}", attachment.name))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let byte_hash = format!("{:x}", hasher.finalize());
    Ok(stable_hash(format!(
        "{}:{byte_hash}",
        attachment_metadata_hash(attachment)
    )))
}

fn attachment_content_hashes(
    repository: &crate::chat::storage::ConversationRepository,
    conversation: &StoredConversation,
) -> Result<Vec<String>, String> {
    noteworthy_messages(conversation)
        .into_iter()
        .flat_map(|message| message.attachments.iter())
        .map(|attachment| attachment_snapshot_hash(repository, &conversation.id, attachment))
        .collect()
}

fn is_supported_source_attachment(
    attachment: &crate::chat::conversation_types::StoredChatAttachment,
) -> bool {
    attachment_formats::is_supported_deep_note_attachment(attachment)
}

fn message_text(message: &StoredChatMessage, include_reasoning: bool) -> String {
    let role = match message.role {
        ModelRole::User => "用户",
        ModelRole::Assistant => "助手",
        ModelRole::Tool => "工具",
    };
    let mut parts = vec![format!("### {role}"), message.content.trim().to_string()];
    if !message.literature_references.is_empty() {
        parts.push(
            message
                .literature_references
                .iter()
                .map(|reference| {
                    format!(
                        "文献引用：{} 第 {} 页\n{}",
                        reference.title,
                        reference.page_index + 1,
                        reference.text
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !message.note_references.is_empty() {
        parts.push(
            message
                .note_references
                .iter()
                .map(|reference| {
                    format!(
                        "笔记引用：{}\n{}",
                        reference.note_title, reference.selected_text
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if include_reasoning {
        if let Some(reasoning) = message
            .reasoning
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(format!(
                "### 助手推理（仅供分析，不得写入笔记正文）\n{}",
                reasoning.trim()
            ));
        }
    }
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 不做长度控制：这里一旦截断就是静默丢失来源，而后续引用校验会把丢掉的
/// message-id 当成模型编造。超长由两道机制处理 —— 规划阶段的
/// `should_fallback_to_chunked_planner` 转分块，发出前的字节硬闸兜底。
fn transcript(conversation: &StoredConversation, include_reasoning: bool) -> String {
    noteworthy_messages(conversation)
        .into_iter()
        .map(|message| {
            let anchor = format!("<!-- message-id: {} -->\n", message.id);
            format!("{anchor}{}", message_text(message, include_reasoning))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn estimate_text_tokens(value: &str) -> u64 {
    token_estimate_units(value).div_ceil(4)
}

fn context_budget(
    conversation: &StoredConversation,
    model: &DeepNoteModelSnapshot,
    max_output_tokens: u32,
    adaptive: &AdaptiveBudgetSnapshot,
) -> DeepNoteContextBudget {
    let messages = noteworthy_messages(conversation);
    let estimated_input_tokens = messages
        .iter()
        .map(|message| {
            estimate_text_tokens(&message_text(message, false))
                + estimate_text_tokens(&message.id)
                + 12
        })
        .sum();
    // The outline schema is deliberately bounded. Reserving the full user output
    // budget here can force otherwise safe conversations into unnecessary chunking.
    let planner_output_reserve_tokens =
        u64::from(max_output_tokens.min(PLANNER_OUTPUT_TOKEN_LIMIT));
    let adaptive_limit = adaptive
        .limit_tokens
        .clamp(MIN_ADAPTIVE_CHUNK_TOKENS, MAX_ADAPTIVE_CHUNK_TOKENS);
    let (safety_margin_tokens, usable_input_tokens, direct_input_limit_tokens, chunk_target_tokens) =
        if let Some(window) = model.context_window_tokens {
            let safety = (window / 12).max(4_096);
            let usable = window
                .saturating_sub(planner_output_reserve_tokens)
                .saturating_sub(PLANNER_PROMPT_OVERHEAD_TOKENS)
                .saturating_sub(safety);
            (
                safety,
                usable,
                usable.min(DIRECT_PLANNER_TOKEN_LIMIT).min(adaptive_limit),
                usable
                    .saturating_sub(1_024)
                    .min(DEFAULT_CHUNK_TARGET_TOKENS)
                    .min(adaptive_limit)
                    .max(2_048),
            )
        } else {
            (
                4_096,
                UNKNOWN_CONTEXT_CHUNK_TOKENS,
                UNKNOWN_CONTEXT_CHUNK_TOKENS.min(adaptive_limit),
                UNKNOWN_CONTEXT_CHUNK_TOKENS.min(adaptive_limit),
            )
        };
    DeepNoteContextBudget {
        context_window_tokens: model.context_window_tokens,
        estimated_input_tokens,
        planner_output_reserve_tokens,
        prompt_overhead_tokens: PLANNER_PROMPT_OVERHEAD_TOKENS,
        safety_margin_tokens,
        usable_input_tokens,
        direct_input_limit_tokens,
        chunk_target_tokens,
        adaptive_chunk_limit_tokens: adaptive_limit,
        adaptive_route_key: adaptive.route_key.clone(),
        adaptive_route_state: adaptive.route_state.clone(),
        adaptive_profile_samples: adaptive.profile_samples,
        chunk_count: 0,
        processed_chunk_count: 0,
        total_message_count: messages.len(),
        processed_message_count: 0,
        coverage_complete: false,
        omitted_message_ids: Vec::new(),
    }
}

fn split_text_by_token_budget(value: &str, target_tokens: u64) -> Vec<String> {
    let target_units = target_tokens.max(1).saturating_mul(4);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_units = 0u64;
    for block in semantic_text_blocks(value) {
        let block_units = token_estimate_units(&block);
        if !current.is_empty() && current_units.saturating_add(block_units) > target_units {
            chunks.push(std::mem::take(&mut current));
            current_units = 0;
        }
        if block_units <= target_units {
            current.push_str(&block);
            current_units += block_units;
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_units = 0;
        }
        for segment in split_oversized_text_block(&block, target_units) {
            chunks.push(segment);
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Token 估算的唯一底层标度：ASCII=1 unit，非 ASCII=4 units，4 units=1 token。
/// 切块需要字符级累计，因此保留 unit 精度；所有对外预算一律经
/// `estimate_text_tokens` 向上取整，不能把 unit 当 token 使用。
fn token_estimate_units(value: &str) -> u64 {
    value
        .chars()
        .map(character_token_units)
        .fold(0u64, u64::saturating_add)
}

fn character_token_units(character: char) -> u64 {
    if character.is_ascii() {
        1
    } else {
        4
    }
}

fn semantic_text_blocks(value: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut fence: Option<(char, usize)> = None;
    for line in value.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let heading = fence.is_none() && trimmed.starts_with('#');
        if heading && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        if let Some((marker, length)) = fence {
            let candidate_length = trimmed.chars().take_while(|value| *value == marker).count();
            if candidate_length >= length && trimmed[candidate_length..].trim().is_empty() {
                fence = None;
            }
        } else {
            let marker = trimmed.chars().next().unwrap_or_default();
            if matches!(marker, '`' | '~') {
                let length = trimmed.chars().take_while(|value| *value == marker).count();
                if length >= 3 {
                    fence = Some((marker, length));
                }
            }
        }
        if fence.is_none() && (line.trim().is_empty() || heading) {
            blocks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn split_oversized_text_block(value: &str, target_units: u64) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_units = 0u64;
    for line in value.split_inclusive('\n') {
        let line_units = token_estimate_units(line);
        if !current.is_empty() && current_units.saturating_add(line_units) > target_units {
            chunks.push(std::mem::take(&mut current));
            current_units = 0;
        }
        if line_units <= target_units {
            current.push_str(line);
            current_units = current_units.saturating_add(line_units);
            continue;
        }
        for character in line.chars() {
            let units = character_token_units(character);
            if !current.is_empty() && current_units.saturating_add(units) > target_units {
                chunks.push(std::mem::take(&mut current));
                current_units = 0;
            }
            current.push(character);
            current_units = current_units.saturating_add(units);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn source_chunk_limit_error() -> String {
    format!(
        "当前来源需要超过单次深度笔记允许的 {MAX_ANALYSIS_CHUNKS} 个分块。请缩小会话或附件范围后重试；系统不会静默丢弃内容。"
    )
}

fn content_addressed_chunk_id(excerpt: &str) -> String {
    format!("chunk-{}", stable_hash(excerpt))
}

fn push_conversation_chunk(
    chunks: &mut Vec<ConversationChunk>,
    conversation_id: &str,
    excerpt: String,
    mut message_ids: Vec<String>,
) -> Result<(), String> {
    if excerpt.trim().is_empty() {
        return Ok(());
    }
    if chunks.len() >= MAX_ANALYSIS_CHUNKS {
        return Err(source_chunk_limit_error());
    }
    message_ids.sort();
    message_ids.dedup();
    let chunk_id = content_addressed_chunk_id(&excerpt);
    let location = if message_ids.len() == 1 {
        format!("消息 {}", message_ids[0])
    } else {
        format!("{} 条消息", message_ids.len())
    };
    chunks.push(ConversationChunk {
        estimated_tokens: estimate_text_tokens(&excerpt),
        source: DeepNoteSourceChunk {
            chunk_id,
            source_kind: DeepNoteSourceKind::Conversation,
            source_id: conversation_id.to_string(),
            message_id: (message_ids.len() == 1).then(|| message_ids[0].clone()),
            attachment_id: None,
            library_item_id: None,
            location,
            content_hash: stable_hash(&excerpt),
            excerpt,
            ocr_confidence: None,
        },
        message_ids,
    });
    Ok(())
}

fn conversation_chunks(
    conversation: &StoredConversation,
    target_tokens: u64,
) -> Result<Vec<ConversationChunk>, String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_ids = Vec::new();
    let mut current_tokens = 0u64;
    for message in noteworthy_messages(conversation) {
        let block = format!(
            "<!-- message-id: {} -->\n{}",
            message.id,
            message_text(message, false)
        );
        for segment in split_text_by_token_budget(&block, target_tokens) {
            let segment_tokens = estimate_text_tokens(&segment);
            let separator_tokens = (!current.is_empty())
                .then(|| estimate_text_tokens("\n\n"))
                .unwrap_or_default();
            if !current.is_empty()
                && current_tokens
                    .saturating_add(separator_tokens)
                    .saturating_add(segment_tokens)
                    > target_tokens
            {
                push_conversation_chunk(
                    &mut chunks,
                    &conversation.id,
                    std::mem::take(&mut current),
                    std::mem::take(&mut current_ids),
                )?;
                current_tokens = 0;
            }
            if !current.is_empty() {
                current.push_str("\n\n");
                current_tokens = current_tokens.saturating_add(separator_tokens);
            }
            current.push_str(&segment);
            current_tokens += segment_tokens;
            current_ids.push(message.id.clone());
        }
    }
    push_conversation_chunk(&mut chunks, &conversation.id, current, current_ids)?;
    Ok(chunks)
}

const SOURCE_TEXT_WINDOW_LINES: usize = 100;
const SOURCE_OFFICE_WINDOW_ITEMS: usize = 50;
const SOURCE_READER_CALL_LIMIT: usize = 256;
const SOURCE_PDF_PAGES_PER_CALL: usize = 2;
const VISION_SOURCE_SYSTEM_PROMPT: &str = r#"你是深度笔记的只读视觉来源分析器。只描述图片中真实可见的信息，并区分文字、结构、关系、趋势与不确定项。不得根据文件名猜测，不得补写图片外事实。输出可直接作为 Source Chunk 的中文文本；保留关键标签、箭头方向、层次和数值。若图片无法辨认，明确说明无法辨认的区域。"#;

fn source_chunk_message_ids(chunk: &DeepNoteSourceChunk) -> Vec<String> {
    if let Some(message_id) = chunk.message_id.as_ref() {
        return vec![message_id.clone()];
    }
    chunk
        .excerpt
        .match_indices("<!-- message-id: ")
        .filter_map(|(start, _)| {
            let suffix = &chunk.excerpt[start + "<!-- message-id: ".len()..];
            suffix.find(" -->").map(|end| suffix[..end].to_string())
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

fn evidence_terms(value: &str) -> HashSet<String> {
    let mut terms = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|term| term.len() >= 2)
        .map(|term| term.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let cjk = value
        .chars()
        .filter(|character| !character.is_ascii() && character.is_alphanumeric())
        .collect::<Vec<_>>();
    for pair in cjk.windows(2) {
        terms.insert(pair.iter().collect());
    }
    terms
}

fn evidence_relevance_score(query: &str, chunk: &DeepNoteSourceChunk) -> usize {
    let query_terms = evidence_terms(query);
    if query_terms.is_empty() {
        return 0;
    }
    let source_terms = evidence_terms(&chunk.excerpt);
    query_terms.intersection(&source_terms).count()
}

fn evidence_for_plan(
    run: &NotePipelineRun,
    plan: &DeepNotePlanVersion,
    chunks: &[DeepNoteSourceChunk],
) -> Vec<DeepNoteEvidenceArtifact> {
    let created_at = run.updated_at.max(1);
    plan.plan
        .sections
        .iter()
        .flat_map(|section| {
            let scoped = chunks
                .iter()
                .filter(|chunk| {
                    section.source_message_ids.is_empty()
                        || source_chunk_message_ids(chunk)
                            .iter()
                            .any(|message_id| section.source_message_ids.contains(message_id))
                })
                .collect::<Vec<_>>();
            let candidates = if section.source_message_ids.is_empty() {
                chunks.iter().collect::<Vec<_>>()
            } else {
                scoped
            };
            let requirements = if section.evidence_requirements.is_empty() {
                vec![format!("支撑章节“{}”的已确认来源", section.heading)]
            } else {
                section.evidence_requirements.clone()
            };
            requirements
                .into_iter()
                .enumerate()
                .map(|(index, requirement)| {
                    let query = format!(
                        "{} {} {} {}",
                        section.heading, section.purpose, section.brief, requirement
                    );
                    let mut ranked = candidates
                        .iter()
                        .map(|chunk| (*chunk, evidence_relevance_score(&query, chunk)))
                        .collect::<Vec<_>>();
                    ranked.sort_by(|left, right| {
                        right
                            .1
                            .cmp(&left.1)
                            .then_with(|| left.0.chunk_id.cmp(&right.0.chunk_id))
                    });
                    let best_score = ranked.first().map(|(_, score)| *score).unwrap_or(0);
                    let selected = ranked
                        .into_iter()
                        .filter(|(_, score)| *score > 0 || !section.source_message_ids.is_empty())
                        .take(4)
                        .map(|(chunk, _)| chunk)
                        .collect::<Vec<_>>();
                    let source_chunk_ids = selected
                        .iter()
                        .map(|chunk| chunk.chunk_id.clone())
                        .collect::<Vec<_>>();
                    let source_excerpt = selected
                        .iter()
                        .take(4)
                        .map(|chunk| {
                            format!(
                                "[{} · {}]\n{}",
                                chunk.source_kind.as_str(),
                                chunk.location,
                                chunk.excerpt.chars().take(1_200).collect::<String>()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let status = if source_chunk_ids.is_empty() {
                        DeepNoteEvidenceStatus::Insufficient
                    } else {
                        DeepNoteEvidenceStatus::Verified
                    };
                    let content_hash = stable_hash(format!(
                        "{}:{}:{}:{}",
                        section.id,
                        index,
                        requirement,
                        source_chunk_ids.join(":")
                    ));
                    DeepNoteEvidenceArtifact {
                        evidence_id: format!("evidence-{}", &content_hash[..20]),
                        section_id: section.id.clone(),
                        source_chunk_ids,
                        claim: requirement.clone(),
                        model_synthesis: format!(
                            "章节“{}”的证据要求已按来源文本相关度绑定；最高匹配分 {}：{}",
                            section.heading, best_score, requirement
                        ),
                        source_excerpt,
                        support_level: if status == DeepNoteEvidenceStatus::Verified
                            && best_score >= 2
                        {
                            DeepNoteSupportLevel::Direct
                        } else {
                            DeepNoteSupportLevel::Partial
                        },
                        status,
                        content_hash,
                        created_at,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn evidence_ids_by_section(evidence: &[DeepNoteEvidenceArtifact]) -> HashMap<String, Vec<String>> {
    let mut result = HashMap::<String, Vec<String>>::new();
    for item in evidence {
        if item.status != DeepNoteEvidenceStatus::Verified {
            continue;
        }
        result
            .entry(item.section_id.clone())
            .or_default()
            .push(item.evidence_id.clone());
    }
    result
}

fn ledger_has_real_output(ledger: &DeepNoteLedger, coverage_complete: bool) -> bool {
    coverage_complete
        && (!ledger.section_summaries.is_empty()
            || !ledger.verified_facts.is_empty()
            || !ledger.covered_topics.is_empty()
            || !ledger.open_questions.is_empty()
            || !ledger.conflicts.is_empty())
}

fn push_attachment_source_chunks(
    chunks: &mut Vec<ConversationChunk>,
    max_chunks: usize,
    attachment: &crate::chat::conversation_types::StoredChatAttachment,
    message_id: &str,
    source_kind: DeepNoteSourceKind,
    location: &str,
    content: String,
    target_tokens: u64,
) -> Result<(), String> {
    for excerpt in split_text_by_token_budget(&content, target_tokens) {
        if excerpt.trim().is_empty() {
            continue;
        }
        if chunks.len() >= max_chunks {
            return Err(source_chunk_limit_error());
        }
        let content_hash = stable_hash(&excerpt);
        let chunk_id = content_addressed_chunk_id(&excerpt);
        chunks.push(ConversationChunk {
            estimated_tokens: estimate_text_tokens(&excerpt),
            source: DeepNoteSourceChunk {
                chunk_id,
                source_kind,
                source_id: attachment.id.clone(),
                message_id: Some(message_id.to_string()),
                attachment_id: Some(attachment.id.clone()),
                library_item_id: None,
                location: location.to_string(),
                excerpt,
                content_hash,
                ocr_confidence: None,
            },
            message_ids: vec![message_id.to_string()],
        });
    }
    Ok(())
}

fn pack_adjacent_attachment_chunks(
    chunks: Vec<ConversationChunk>,
    target_tokens: u64,
) -> Vec<ConversationChunk> {
    let target_tokens = target_tokens.max(MIN_ADAPTIVE_CHUNK_TOKENS);
    let mut packed: Vec<ConversationChunk> = Vec::new();
    for next in chunks {
        let can_merge = packed.last().is_some_and(|previous| {
            previous.source.attachment_id.is_some()
                && previous.source.attachment_id == next.source.attachment_id
                && previous.source.message_id == next.source.message_id
                && previous.source.source_kind == next.source.source_kind
                && attachment_pack_group(&previous.source.location)
                    == attachment_pack_group(&next.source.location)
                && previous
                    .estimated_tokens
                    .saturating_add(next.estimated_tokens)
                    .saturating_add(8)
                    <= target_tokens
        });
        if !can_merge {
            packed.push(next);
            continue;
        }
        let previous = packed.last_mut().expect("merge candidate checked above");
        let first_location = previous
            .source
            .location
            .split(" … ")
            .next()
            .unwrap_or(previous.source.location.as_str())
            .to_string();
        previous
            .source
            .excerpt
            .push_str("\n\n<!-- packed-source-boundary -->\n\n");
        previous.source.excerpt.push_str(&next.source.excerpt);
        previous.estimated_tokens = estimate_text_tokens(&previous.source.excerpt);
        previous.source.location = format!("{first_location} … {}", next.source.location);
        previous.source.content_hash = stable_hash(&previous.source.excerpt);
        previous.source.chunk_id = content_addressed_chunk_id(&previous.source.excerpt);
    }
    packed
}

fn attachment_pack_group(location: &str) -> &str {
    location
        .rsplit_once(" … ")
        .map(|(_, last)| last)
        .unwrap_or(location)
        .split(" 第")
        .next()
        .unwrap_or(location)
}

async fn execute_source_reader(
    state: &AppState,
    run: &NotePipelineRun,
    call: ModelToolCall,
    attachments: &[crate::chat::conversation_types::StoredChatAttachment],
    cancellation: &CancellationToken,
) -> Result<crate::chat::agent::ToolExecution, String> {
    let started_at = crate::usage::now_ms();
    append_pipeline_event_if_available(
        state,
        &run.id,
        "toolStarted",
        Some("recon-source"),
        &serde_json::json!({
            "callId": call.id,
            "toolName": call.name,
            "arguments": call.arguments,
            "readOnly": true,
        })
        .to_string(),
    )?;
    let result = crate::chat::agent::execute_bounded_attachment_reader(
        &call,
        &run.conversation_id,
        attachments,
        &state.conversation_repository,
        cancellation,
    )
    .await;
    match result {
        Ok(result) if result.output_truncated => {
            let message = format!(
                "只读来源工具 {} 的输出被截断，不能把该范围标记为覆盖完成。",
                call.name
            );
            append_pipeline_event_if_available(
                state,
                &run.id,
                "toolFailed",
                Some("recon-source"),
                &serde_json::json!({
                    "callId": call.id,
                    "toolName": call.name,
                    "message": message,
                    "outputTruncated": true,
                })
                .to_string(),
            )?;
            Err(message)
        }
        Ok(result) => {
            append_pipeline_event_if_available(
                state,
                &run.id,
                "toolCompleted",
                Some("recon-source"),
                &serde_json::json!({
                    "callId": call.id,
                    "toolName": call.name,
                    "durationMs": crate::usage::now_ms().saturating_sub(started_at),
                    "outputChars": result.output_chars,
                    "outputTruncated": false,
                    "preview": result.preview,
                })
                .to_string(),
            )?;
            Ok(result)
        }
        Err(error) => {
            append_pipeline_event_if_available(
                state,
                &run.id,
                "toolFailed",
                Some("recon-source"),
                &serde_json::json!({
                    "callId": call.id,
                    "toolName": call.name,
                    "message": error.message,
                })
                .to_string(),
            )?;
            Err(error.message)
        }
    }
}

fn xlsx_sheet_catalog(content: &str) -> Vec<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("可用工作表："))
        .map(|value| {
            value
                .split('、')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

async fn attachment_source_chunks(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    conversation: &StoredConversation,
    target_tokens: u64,
    max_chunks: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<ConversationChunk>, String> {
    let attachments = noteworthy_messages(conversation)
        .into_iter()
        .flat_map(|message| message.attachments.iter().cloned())
        .collect::<Vec<_>>();
    let mut chunks = Vec::new();
    let mut calls = 0usize;
    for message in noteworthy_messages(conversation) {
        for attachment in &message.attachments {
            if cancellation.is_cancelled() {
                return Err("操作已取消。".to_string());
            }
            if chunks.len() >= MAX_UNPACKED_ATTACHMENT_CHUNKS {
                return Err("附件预分块达到内存安全上限，请缩小附件范围。".to_string());
            }
            let read_kind = attachment_formats::deep_note_read_kind(attachment);
            if read_kind == AttachmentReadKind::Image {
                consume_semantic_call(state, &run.id, runtime)?;
                let prompt = format!(
                    "图片附件 ID：{}\n文件名：{}\n请生成可追溯的视觉 Source Chunk。",
                    attachment.id, attachment.name
                );
                let text = model_call_with_runtime_attachments(
                    state,
                    run,
                    "deepNoteVisionSource",
                    NotePipelinePhase::Analyzing,
                    system_prompt_with_skill_profile(
                        state,
                        &run.id,
                        runtime,
                        DeepNoteSkillProfileKind::Reviewer,
                        Some("recon-source"),
                        VISION_SOURCE_SYSTEM_PROMPT,
                    ),
                    prompt,
                    vec![attachment.clone()],
                    run.max_output_tokens.min(2_048),
                    run.retry_attempts,
                    cancellation,
                    None,
                )
                .await
                .map_err(|error| error.message)?;
                if text.trim().is_empty() {
                    return Err(format!(
                        "图片附件“{}”没有产生可验证的视觉描述。",
                        attachment.name
                    ));
                }
                push_attachment_source_chunks(
                    &mut chunks,
                    MAX_UNPACKED_ATTACHMENT_CHUNKS,
                    attachment,
                    &message.id,
                    DeepNoteSourceKind::Image,
                    &format!("图片 {}", attachment.name),
                    text,
                    target_tokens,
                )?;
                continue;
            }
            if read_kind == AttachmentReadKind::Text {
                let code_language = attachment_formats::code_language(&attachment.name);
                let source_kind = if code_language.is_some() {
                    DeepNoteSourceKind::Code
                } else {
                    DeepNoteSourceKind::Text
                };
                let mut start = 1usize;
                loop {
                    if chunks.len() >= MAX_UNPACKED_ATTACHMENT_CHUNKS {
                        return Err("附件预分块达到内存安全上限，请缩小附件范围。".to_string());
                    }
                    if calls >= SOURCE_READER_CALL_LIMIT {
                        return Err("来源 Reader 调用达到安全上限，附件覆盖尚未完成。".to_string());
                    }
                    let requested_end = start + SOURCE_TEXT_WINDOW_LINES - 1;
                    calls = calls.saturating_add(1);
                    let call_id = Uuid::new_v4().to_string();
                    append_pipeline_event_if_available(
                        state,
                        &run.id,
                        "toolStarted",
                        Some("recon-source"),
                        &serde_json::json!({
                            "callId": call_id,
                            "toolName": "read_attachment_text",
                            "arguments": {
                                "attachmentId": attachment.id,
                                "startLine": start,
                                "requestedEndLine": requested_end,
                                "maxBytes": 32_000,
                            },
                            "readOnly": true,
                        })
                        .to_string(),
                    )?;
                    let started_at = crate::usage::now_ms();
                    let (result, end) = match crate::chat::agent::execute_bounded_text_window(
                        &run.conversation_id,
                        attachment,
                        &state.conversation_repository,
                        start,
                        requested_end,
                        32_000,
                        cancellation,
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(error) => {
                            append_pipeline_event_if_available(
                                state,
                                &run.id,
                                "toolFailed",
                                Some("recon-source"),
                                &serde_json::json!({
                                    "callId": call_id,
                                    "toolName": "read_attachment_text",
                                    "message": error.message,
                                })
                                .to_string(),
                            )?;
                            return Err(error.message);
                        }
                    };
                    append_pipeline_event_if_available(
                        state,
                        &run.id,
                        "toolCompleted",
                        Some("recon-source"),
                        &serde_json::json!({
                            "callId": call_id,
                            "toolName": "read_attachment_text",
                            "durationMs": crate::usage::now_ms().saturating_sub(started_at),
                            "outputChars": result.output_chars,
                            "outputTruncated": false,
                            "actualEndLine": end,
                            "preview": result.preview,
                        })
                        .to_string(),
                    )?;
                    if result.content.trim().is_empty() {
                        if start == 1 {
                            push_attachment_source_chunks(
                                &mut chunks,
                                MAX_UNPACKED_ATTACHMENT_CHUNKS,
                                attachment,
                                &message.id,
                                source_kind,
                                &format!("附件 {}", attachment.name),
                                "[该附件没有可提取的非空 UTF-8 文本。]".to_string(),
                                target_tokens,
                            )?;
                        }
                        break;
                    }
                    push_attachment_source_chunks(
                        &mut chunks,
                        MAX_UNPACKED_ATTACHMENT_CHUNKS,
                        attachment,
                        &message.id,
                        source_kind,
                        &match code_language {
                            Some(language) => format!(
                                "代码附件 {} · {} · 第 {start}-{end} 行",
                                attachment.name, language
                            ),
                            None => format!("文本附件 {} 第 {start}-{end} 行", attachment.name),
                        },
                        match code_language {
                            Some(language) => format!(
                                "[代码来源；语言={language}；文件={}；仅做静态文本读取，未执行代码]\n{}",
                                attachment.name, result.content
                            ),
                            None => result.content,
                        },
                        target_tokens,
                    )?;
                    start = end + 1;
                }
            } else if read_kind == AttachmentReadKind::Pdf {
                let path = state
                    .conversation_repository
                    .resolve_attachment_path(&conversation.id, &attachment.path)?;
                let page_count = tauri::async_runtime::spawn_blocking(move || {
                    lopdf::Document::load(path)
                        .map_err(|error| format!("读取 PDF 页数失败：{error}"))
                        .map(|document| document.get_pages().len())
                })
                .await
                .map_err(|error| format!("PDF 页数检查任务失败：{error}"))??;
                for start in (1..=page_count.max(1)).step_by(SOURCE_PDF_PAGES_PER_CALL) {
                    if chunks.len() >= MAX_UNPACKED_ATTACHMENT_CHUNKS {
                        return Err("附件预分块达到内存安全上限，请缩小附件范围。".to_string());
                    }
                    if calls >= SOURCE_READER_CALL_LIMIT {
                        return Err("来源 Reader 调用达到安全上限，PDF 覆盖尚未完成。".to_string());
                    }
                    let end = (start + SOURCE_PDF_PAGES_PER_CALL - 1).min(page_count);
                    let pages = (start..=end).collect::<Vec<_>>();
                    calls = calls.saturating_add(1);
                    let call = ModelToolCall {
                        id: Uuid::new_v4().to_string(),
                        name: "read_pdf_pages".to_string(),
                        arguments: serde_json::json!({ "attachmentId": attachment.id, "pages": pages }),
                        provider_signature: None,
                    };
                    let result =
                        execute_source_reader(state, run, call, &attachments, cancellation).await?;
                    push_attachment_source_chunks(
                        &mut chunks,
                        MAX_UNPACKED_ATTACHMENT_CHUNKS,
                        attachment,
                        &message.id,
                        DeepNoteSourceKind::Pdf,
                        &format!("PDF {} 第 {start}-{end} 页", attachment.name),
                        result.content,
                        target_tokens,
                    )?;
                }
            } else if read_kind == AttachmentReadKind::Docx {
                let mut start = 1usize;
                loop {
                    if chunks.len() >= MAX_UNPACKED_ATTACHMENT_CHUNKS {
                        return Err("附件预分块达到内存安全上限，请缩小附件范围。".to_string());
                    }
                    if calls >= SOURCE_READER_CALL_LIMIT {
                        return Err("来源 Reader 调用达到安全上限，DOCX 覆盖尚未完成。".to_string());
                    }
                    let end = start + SOURCE_OFFICE_WINDOW_ITEMS - 1;
                    calls = calls.saturating_add(1);
                    let call = ModelToolCall {
                        id: Uuid::new_v4().to_string(),
                        name: "read_docx_blocks".to_string(),
                        arguments: serde_json::json!({
                            "attachmentId": attachment.id,
                            "startBlock": start,
                            "endBlock": end,
                        }),
                        provider_signature: None,
                    };
                    let result =
                        execute_source_reader(state, run, call, &attachments, cancellation).await?;
                    if result.content.starts_with("DOCX 中没有第") {
                        break;
                    }
                    push_attachment_source_chunks(
                        &mut chunks,
                        MAX_UNPACKED_ATTACHMENT_CHUNKS,
                        attachment,
                        &message.id,
                        DeepNoteSourceKind::Docx,
                        &format!("DOCX {} 第 {start}-{end} 块", attachment.name),
                        result.content,
                        target_tokens,
                    )?;
                    start = end + 1;
                }
            } else if read_kind == AttachmentReadKind::Xlsx {
                calls = calls.saturating_add(1);
                let first_call = ModelToolCall {
                    id: Uuid::new_v4().to_string(),
                    name: "read_xlsx_rows".to_string(),
                    arguments: serde_json::json!({
                        "attachmentId": attachment.id,
                        "startRow": 1,
                        "endRow": SOURCE_OFFICE_WINDOW_ITEMS,
                    }),
                    provider_signature: None,
                };
                let first =
                    execute_source_reader(state, run, first_call, &attachments, cancellation)
                        .await?;
                let sheets = xlsx_sheet_catalog(&first.content);
                if sheets.is_empty() {
                    return Err(format!(
                        "XLSX 附件“{}”没有可读取的工作表。",
                        attachment.name
                    ));
                }
                for (sheet_index, sheet) in sheets.iter().enumerate() {
                    let mut start = 1usize;
                    loop {
                        if chunks.len() >= MAX_UNPACKED_ATTACHMENT_CHUNKS {
                            return Err("附件预分块达到内存安全上限，请缩小附件范围。".to_string());
                        }
                        if calls >= SOURCE_READER_CALL_LIMIT {
                            return Err(
                                "来源 Reader 调用达到安全上限，XLSX 覆盖尚未完成。".to_string()
                            );
                        }
                        let end = start + SOURCE_OFFICE_WINDOW_ITEMS - 1;
                        let result = if sheet_index == 0 && start == 1 {
                            first.clone()
                        } else {
                            calls = calls.saturating_add(1);
                            let call = ModelToolCall {
                                id: Uuid::new_v4().to_string(),
                                name: "read_xlsx_rows".to_string(),
                                arguments: serde_json::json!({
                                    "attachmentId": attachment.id,
                                    "sheetName": sheet,
                                    "startRow": start,
                                    "endRow": end,
                                }),
                                provider_signature: None,
                            };
                            execute_source_reader(state, run, call, &attachments, cancellation)
                                .await?
                        };
                        if result.content.contains("中没有第") {
                            break;
                        }
                        push_attachment_source_chunks(
                            &mut chunks,
                            MAX_UNPACKED_ATTACHMENT_CHUNKS,
                            attachment,
                            &message.id,
                            DeepNoteSourceKind::Xlsx,
                            &format!("XLSX {} / {} 第 {start}-{end} 行", attachment.name, sheet),
                            result.content,
                            target_tokens,
                        )?;
                        start = end + 1;
                    }
                }
            } else {
                return Err(format!(
                    "附件“{}”不在深度笔记只读来源白名单中。",
                    attachment.name
                ));
            }
        }
    }
    let unpacked_count = chunks.len();
    let chunks = pack_adjacent_attachment_chunks(chunks, target_tokens);
    if chunks.len() > max_chunks {
        return Err(source_chunk_limit_error());
    }
    append_pipeline_event_if_available(
        state,
        &run.id,
        "attachmentChunksPacked",
        Some("recon-source"),
        &serde_json::json!({
            "unpackedChunkCount": unpacked_count,
            "packedChunkCount": chunks.len(),
            "targetTokens": target_tokens,
            "savedChunkCount": unpacked_count.saturating_sub(chunks.len()),
        })
        .to_string(),
    )?;
    Ok(chunks)
}

async fn incremental_attachment_source_chunks(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    conversation: &StoredConversation,
    message_ids: &HashSet<String>,
    cancellation: &CancellationToken,
) -> Result<Vec<ConversationChunk>, String> {
    let mut projected = conversation.clone();
    projected.messages = noteworthy_messages(conversation)
        .into_iter()
        .filter(|message| message_ids.contains(&message.id))
        .map(|message| (*message).clone())
        .collect();
    projected.updated_at = conversation.updated_at;
    if projected.messages.is_empty() {
        return Ok(Vec::new());
    }
    attachment_source_chunks(
        state,
        run,
        runtime,
        &projected,
        runtime
            .context_budget
            .chunk_target_tokens
            .max(2_048)
            .min(DEFAULT_CHUNK_TARGET_TOKENS),
        MAX_INCREMENTAL_ATTACHMENT_CHUNKS,
        cancellation,
    )
    .await
    .map_err(|error| {
        if error == source_chunk_limit_error() {
            format!(
                "新增附件需要超过 {MAX_INCREMENTAL_ATTACHMENT_CHUNKS} 个来源分块，不能在有界上下文中安全增量更新；请缩小附件范围或执行完整重建。"
            )
        } else {
            error
        }
    })
}

fn parser_contract_for_attachment(
    attachment: &crate::chat::conversation_types::StoredChatAttachment,
) -> (&'static str, &'static str) {
    match attachment_formats::deep_note_read_kind(attachment) {
        AttachmentReadKind::Text => ("read_attachment_text", "1"),
        AttachmentReadKind::Pdf => ("read_pdf_pages", "1"),
        AttachmentReadKind::Docx => ("read_docx_blocks", "1"),
        AttachmentReadKind::Xlsx => ("read_xlsx_rows", "1"),
        AttachmentReadKind::Image => ("vision_source", "1"),
        AttachmentReadKind::Unsupported => ("unsupported", "1"),
    }
}

fn incremental_source_units(
    note_id: &str,
    conversation: &StoredConversation,
    message_ids: &HashSet<String>,
    snapshot: &DeepNoteInputSnapshot,
    chunks: &[ConversationChunk],
    created_at: u64,
) -> Vec<DeepNoteSourceUnit> {
    let attachment_hashes = snapshot
        .attachment_ids
        .iter()
        .cloned()
        .zip(snapshot.attachment_content_hashes.iter().cloned())
        .collect::<HashMap<_, _>>();
    let message_hashes = snapshot
        .message_ids
        .iter()
        .cloned()
        .zip(snapshot.message_content_hashes.iter().cloned())
        .collect::<HashMap<_, _>>();
    let mut units = noteworthy_messages(conversation)
        .into_iter()
        .filter(|message| message_ids.contains(&message.id))
        .map(|message| DeepNoteSourceUnit {
            unit_id: format!("{}:body:{}", note_id, message.id),
            note_id: note_id.to_string(),
            conversation_id: conversation.id.clone(),
            message_id: message.id.clone(),
            kind: DeepNoteSourceUnitKind::Body,
            attachment_id: None,
            content_hash: message_hashes
                .get(&message.id)
                .cloned()
                .unwrap_or_else(|| message_source_hash(message)),
            parser_id: "conversation-body".to_string(),
            parser_version: "1".to_string(),
            status: DeepNoteSourceUnitStatus::Covered,
            chunk_ids: Vec::new(),
            evidence_ids: Vec::new(),
            error_message: None,
            created_at,
            updated_at: created_at,
        })
        .collect::<Vec<_>>();
    units.extend(
        noteworthy_messages(conversation)
            .into_iter()
            .filter(|message| message_ids.contains(&message.id))
            .flat_map(|message| {
                message.attachments.iter().map(|attachment| {
                    let unit_chunks = chunks
                        .iter()
                        .filter(|chunk| {
                            chunk.source.attachment_id.as_deref() == Some(&attachment.id)
                        })
                        .map(|chunk| chunk.source.chunk_id.clone())
                        .collect::<Vec<_>>();
                    let (parser_id, parser_version) = parser_contract_for_attachment(attachment);
                    DeepNoteSourceUnit {
                        unit_id: format!("{}:attachment:{}", note_id, attachment.id),
                        note_id: note_id.to_string(),
                        conversation_id: conversation.id.clone(),
                        message_id: message.id.clone(),
                        kind: DeepNoteSourceUnitKind::Attachment,
                        attachment_id: Some(attachment.id.clone()),
                        content_hash: attachment_hashes
                            .get(&attachment.id)
                            .cloned()
                            .unwrap_or_else(|| stable_hash(&attachment.id)),
                        parser_id: parser_id.to_string(),
                        parser_version: parser_version.to_string(),
                        status: if unit_chunks.is_empty() {
                            DeepNoteSourceUnitStatus::Failed
                        } else {
                            DeepNoteSourceUnitStatus::Covered
                        },
                        chunk_ids: unit_chunks,
                        evidence_ids: Vec::new(),
                        error_message: None,
                        created_at,
                        updated_at: created_at,
                    }
                })
            })
            .collect::<Vec<_>>(),
    );
    units
}

async fn digest_incremental_chunks(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    chunks: &[ConversationChunk],
    cancellation: &CancellationToken,
) -> Result<DeepNoteLedger, String> {
    let candidates = chunks
        .iter()
        .map(|chunk| {
            let system_prompt = system_prompt_with_skill_profile(
                state,
                &run.id,
                runtime,
                DeepNoteSkillProfileKind::Planner,
                Some("incremental-recon"),
                CHUNK_ANALYST_SYSTEM_PROMPT,
            );
            let user_prompt = chunk_analysis_prompt(chunk);
            let prompt_hash = stable_hash(format!(
                "chunk-digest-v4\0{}\0{}\0{}\0{}\0{}",
                run.provider_id,
                run.model_id,
                run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
                system_prompt,
                user_prompt
            ));
            (system_prompt, user_prompt, prompt_hash)
        })
        .collect::<Vec<_>>();
    let cache_keys = chunks
        .iter()
        .zip(&candidates)
        .map(|(chunk, (_, _, prompt_hash))| {
            (chunk.source.content_hash.clone(), prompt_hash.clone())
        })
        .collect::<Vec<_>>();
    let mut resolved = state
        .library_repository
        .find_note_pipeline_chunk_digests(&cache_keys, &run.provider_id, &run.model_id)?
        .into_iter()
        .filter_map(|checkpoint| {
            let digest = parse_json_object::<ChunkDigest>(&checkpoint.digest_json)
                .and_then(ChunkDigest::validate)
                .ok()?;
            Some(((checkpoint.content_hash, checkpoint.prompt_hash), digest))
        })
        .collect::<HashMap<_, _>>();
    let mut ledger = DeepNoteLedger::default();
    for (chunk, (system_prompt, prompt, prompt_hash)) in chunks.iter().zip(candidates) {
        let cache_key = (chunk.source.content_hash.clone(), prompt_hash.clone());
        if let Some(digest) = resolved.get(&cache_key).cloned() {
            merge_chunk_digest(&mut ledger, chunk, digest);
            continue;
        }
        consume_semantic_call(state, &run.id, runtime)?;
        let raw = await_note_pipeline_cancellable(cancellation, async {
            model_call_with_runtime(
                state,
                run,
                "deepNoteChunk",
                NotePipelinePhase::Analyzing,
                system_prompt,
                prompt.clone(),
                run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
                run.retry_attempts,
                cancellation,
                None,
            )
            .await
            .map_err(|error| error.message)
        })
        .await?;
        let mut semantic_calls = 1u32;
        let digest = match parse_json_object::<ChunkDigest>(&raw).and_then(ChunkDigest::validate) {
            Ok(digest) => digest,
            Err(_) => {
                semantic_calls = semantic_calls.saturating_add(1);
                consume_semantic_call(state, &run.id, runtime)?;
                let repaired = await_note_pipeline_cancellable(cancellation, async {
                    model_call_with_runtime(
                        state,
                        run,
                        "deepNoteChunkRepair",
                        NotePipelinePhase::Analyzing,
                        format!("{CHUNK_ANALYST_SYSTEM_PROMPT}\n\n{STRICT_JSON_SUFFIX}"),
                        prompt,
                        run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
                        run.retry_attempts,
                        cancellation,
                        None,
                    )
                    .await
                    .map_err(|error| error.message)
                })
                .await?;
                parse_json_object::<ChunkDigest>(&repaired).and_then(ChunkDigest::validate)?
            }
        };
        let digest_json = serde_json::to_string(&digest)
            .map_err(|error| format!("序列化增量 Chunk 摘要失败：{error}"))?;
        state.library_repository.save_note_pipeline_chunk_digest(
            &chunk.source.chunk_id,
            &chunk.source.content_hash,
            &prompt_hash,
            &run.provider_id,
            &run.model_id,
            &digest_json,
            semantic_calls,
        )?;
        resolved.insert(cache_key, digest.clone());
        merge_chunk_digest(&mut ledger, chunk, digest);
    }
    Ok(ledger)
}

async fn all_source_chunks(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    conversation: &StoredConversation,
    target_tokens: u64,
    cancellation: &CancellationToken,
) -> Result<Vec<ConversationChunk>, String> {
    let mut chunks = conversation_chunks(conversation, target_tokens)?;
    let remaining_chunks = MAX_ANALYSIS_CHUNKS.saturating_sub(chunks.len());
    save_runtime_state(state, &run.id, runtime)?;
    chunks.extend(
        attachment_source_chunks(
            state,
            run,
            runtime,
            conversation,
            target_tokens,
            remaining_chunks,
            cancellation,
        )
        .await?,
    );
    for chunk in &chunks {
        state.library_repository.append_note_pipeline_event(
            &run.id,
            "sourceChunkCreated",
            Some("recon-source"),
            &serde_json::json!({
                "chunkId": chunk.source.chunk_id,
                "sourceKind": chunk.source.source_kind.as_str(),
                "sourceId": chunk.source.source_id,
                "messageId": chunk.source.message_id,
                "attachmentId": chunk.source.attachment_id,
                "location": chunk.source.location,
                "contentHash": chunk.source.content_hash,
                "excerptChars": chunk.source.excerpt.chars().count(),
            })
            .to_string(),
        )?;
    }
    Ok(chunks)
}

fn extend_unique(target: &mut Vec<String>, values: Vec<String>, limit: usize) {
    for value in values {
        let value = value.trim();
        if value.is_empty() || target.iter().any(|existing| existing == value) {
            continue;
        }
        if target.len() >= limit {
            break;
        }
        target.push(value.chars().take(2_000).collect());
    }
}

fn merge_chunk_digest(
    ledger: &mut DeepNoteLedger,
    chunk: &ConversationChunk,
    mut digest: ChunkDigest,
) {
    digest
        .source_message_ids
        .retain(|id| chunk.message_ids.contains(id));
    extend_unique(&mut ledger.canonical_terms, digest.canonical_terms, 240);
    extend_unique(&mut ledger.verified_facts, digest.verified_facts, 480);
    extend_unique(&mut ledger.covered_topics, digest.covered_topics, 240);
    extend_unique(&mut ledger.open_questions, digest.open_questions, 160);
    extend_unique(&mut ledger.conflicts, digest.conflicts, 160);
    extend_unique(
        &mut ledger.global_constraints,
        digest.global_constraints,
        160,
    );
    let source_ids = if digest.source_message_ids.is_empty() {
        chunk.message_ids.join(", ")
    } else {
        digest.source_message_ids.join(", ")
    };
    let summary = digest.summary.trim();
    if !summary.is_empty() && ledger.section_summaries.len() < MAX_ANALYSIS_CHUNKS {
        ledger.section_summaries.push(format!(
            "{} | 来源消息 [{}] | {}",
            chunk.source.chunk_id,
            source_ids,
            summary.chars().take(4_000).collect::<String>()
        ));
    }
}

fn incremental_transcript(
    conversation: &StoredConversation,
    summarized_until: Option<&str>,
) -> Result<(String, HashSet<String>, Option<String>), String> {
    let messages = noteworthy_messages(conversation);
    let start = match summarized_until {
        Some(anchor) => messages
            .iter()
            .position(|message| message.id == anchor)
            .map(|index| index + 1)
            .ok_or_else(|| {
                "已有笔记的增量锚点已被删除或重排，不能从头回退生成更新。".to_string()
            })?,
        None => 0,
    };
    let selected = &messages[start..];
    let ids = selected
        .iter()
        .map(|message| message.id.clone())
        .collect::<HashSet<_>>();
    let last = selected.last().map(|message| message.id.clone());
    let value = selected
        .iter()
        .map(|message| {
            format!(
                "<!-- message-id: {} -->\n{}",
                message.id,
                message_text(message, true)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok((value, ids, last))
}

fn enabled_model(settings: &ModelSettings, provider_id: &str, model_id: &str) -> bool {
    settings.providers.iter().any(|provider| {
        provider.enabled
            && provider.id == provider_id
            && provider
                .models
                .iter()
                .any(|model| model.enabled && model.id == model_id)
    })
}

fn stable_hash(value: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(value.as_ref()))
}

/// 深度笔记请求的内容指纹。
///
/// 修复前这里是 `stable_hash("deep-note-output:{run_id}")`，而 `run_id` 是
/// `Uuid::new_v4()` —— 每次启动都是全新的随机值，`note_pipeline_output_idempotency`
/// 唯一索引因此永远不会命中，幂等键实际上没有幂等能力。
///
/// 现在由「决定输出内容的全部输入」派生：会话、输入快照哈希（它已经涵盖消息与附件）、
/// 供应商、模型、输出 Token 上限、思考开关、以及 `force_rebuild`（它改变来源口径，
/// 必须参与指纹）。`retry_attempts` **不参与**：它只影响失败后重试几次，不影响输出。
///
/// 字段之间用 `\0` 分隔，避免「ab|c」与「a|bc」这类拼接歧义。
fn deep_note_content_signature(
    conversation_id: &str,
    input_snapshot_hash: &str,
    provider_id: &str,
    model_id: &str,
    max_output_tokens: u32,
    thinking_enabled: bool,
    force_rebuild: bool,
) -> String {
    stable_hash(format!(
        "deep-note-output-v2\0{conversation_id}\0{input_snapshot_hash}\0{provider_id}\0{model_id}\0{max_output_tokens}\0{thinking_enabled}\0{force_rebuild}"
    ))
}

fn resolve_note_model_snapshot(
    settings: &ModelSettings,
    provider_id: &str,
    model_id: &str,
) -> Result<DeepNoteModelSnapshot, String> {
    let provider = settings
        .providers
        .iter()
        .find(|provider| provider.enabled && provider.id == provider_id)
        .ok_or_else(|| "没有找到深度笔记使用的模型供应商。".to_string())?;
    let model = provider
        .models
        .iter()
        .find(|model| model.enabled && model.id == model_id)
        .ok_or_else(|| "没有找到深度笔记使用的模型。".to_string())?;
    let tools = model
        .capabilities
        .and_then(|capabilities| capabilities.function_calling)
        .or_else(|| crate::ai::model::database_supports_function_calling(&model.api_model));
    let vision = model
        .capabilities
        .and_then(|capabilities| capabilities.vision)
        .or_else(|| crate::ai::model::resolve_supports_vision(&model.api_model));
    let reasoning = model
        .capabilities
        .and_then(|capabilities| capabilities.reasoning)
        .or_else(|| crate::ai::model::database_supports_reasoning(&model.api_model));
    Ok(DeepNoteModelSnapshot {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        api_model: model.api_model.clone(),
        context_window_tokens: model
            .context_window_tokens
            .or_else(|| crate::ai::model::database_context_window_tokens(&model.api_model)),
        capabilities: DeepNoteCapabilities {
            tools,
            vision,
            reasoning,
            structured_outputs: false,
        },
    })
}

fn preflight(
    settings: &ModelSettings,
    conversation: &StoredConversation,
    provider_id: &str,
    model_id: &str,
) -> Result<DeepNotePreflight, String> {
    let model = resolve_note_model_snapshot(settings, provider_id, model_id)?;
    let attachments = noteworthy_messages(conversation)
        .into_iter()
        .flat_map(|message| message.attachments.iter())
        .collect::<Vec<_>>();
    let requires_vision = attachments
        .iter()
        .any(|attachment| attachment.kind == "image");
    let file_attachments = attachments
        .iter()
        .filter(|attachment| attachment.kind == "file")
        .copied()
        .collect::<Vec<_>>();
    let requires_local_readers = !file_attachments.is_empty();
    // 深度笔记由 Rust 固定调度本地 Reader，不要求模型具备 Function Calling。
    let requires_tools = false;
    let mut missing_capabilities = Vec::new();
    if requires_vision && model.capabilities.vision != Some(true) {
        missing_capabilities.push("当前模型未明确支持图片识别".to_string());
    }
    let unsupported = file_attachments
        .iter()
        .filter(|attachment| !is_supported_source_attachment(attachment))
        .map(|attachment| {
            if attachment_formats::is_sensitive_text_name(&attachment.name) {
                format!("{}（敏感配置，禁止自动送入深度笔记）", attachment.name)
            } else {
                attachment.name.clone()
            }
        })
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        missing_capabilities.push(format!(
            "本地只读来源网关暂不支持这些附件：{}",
            unsupported.join("、")
        ));
    }
    let mut warnings = Vec::new();
    let code_files = file_attachments
        .iter()
        .filter_map(|attachment| {
            attachment_formats::code_language(&attachment.name)
                .map(|language| format!("{} ({language})", attachment.name))
        })
        .collect::<Vec<_>>();
    if !code_files.is_empty() {
        warnings.push(format!(
            "代码附件将按带行号的只读文本处理，不执行代码，也不声称具备 AST、调用图或多文件仓库语义：{}",
            code_files.join("、")
        ));
    }
    if requires_local_readers && model.capabilities.tools != Some(true) {
        warnings.push(
            "文档附件将由 Mnemora 本地只读 Reader 解析；不依赖当前模型的 Tool 能力。".to_string(),
        );
    }
    if !model.capabilities.structured_outputs {
        warnings.push("当前模型使用严格 JSON 兼容模式，所有计划均由 Rust 校验。".to_string());
    }
    Ok(DeepNotePreflight {
        ready: missing_capabilities.is_empty(),
        model,
        requires_tools,
        requires_local_readers,
        requires_vision,
        local_readers: DeepNoteLocalReaderCapabilities {
            text: true,
            pdf: true,
            docx: true,
            xlsx: true,
        },
        missing_capabilities,
        warnings,
        attachment_ids: attachments
            .into_iter()
            .map(|attachment| attachment.id.clone())
            .collect(),
    })
}

fn input_snapshot(
    conversation: &StoredConversation,
    model: DeepNoteModelSnapshot,
    created_at: u64,
    attachment_content_hashes: Vec<String>,
) -> DeepNoteInputSnapshot {
    let messages = noteworthy_messages(conversation);
    let message_ids = messages
        .iter()
        .map(|message| message.id.clone())
        .collect::<Vec<_>>();
    let message_content_hashes = messages
        .iter()
        .map(|message| message_source_hash(message))
        .collect::<Vec<_>>();
    let attachments = messages
        .iter()
        .flat_map(|message| message.attachments.iter())
        .collect::<Vec<_>>();
    let attachment_ids = attachments
        .iter()
        .map(|attachment| attachment.id.clone())
        .collect::<Vec<_>>();
    let attachment_message_ids = messages
        .iter()
        .flat_map(|message| message.attachments.iter().map(move |_| message.id.clone()))
        .collect::<Vec<_>>();
    debug_assert_eq!(attachment_content_hashes.len(), attachments.len());
    debug_assert_eq!(attachment_message_ids.len(), attachments.len());
    let mut selected_literature_ids = messages
        .iter()
        .flat_map(|message| message.literature_references.iter())
        .map(|reference| reference.library_item_id.clone())
        .collect::<Vec<_>>();
    selected_literature_ids.extend(conversation.linked_library_item_ids.clone());
    selected_literature_ids.sort();
    selected_literature_ids.dedup();
    let mut selected_note_ids = messages
        .iter()
        .flat_map(|message| message.note_references.iter())
        .map(|reference| reference.note_id.clone())
        .collect::<Vec<_>>();
    selected_note_ids.sort();
    selected_note_ids.dedup();
    DeepNoteInputSnapshot {
        conversation_revision: conversation.updated_at,
        message_ids,
        message_content_hashes,
        attachment_ids,
        attachment_content_hashes,
        attachment_message_ids,
        selected_literature_ids,
        selected_note_ids,
        model,
        permission_mode: format!("{:?}", conversation.permission_mode).to_lowercase(),
        created_at,
    }
}

fn validate_recovery_snapshot(
    conversation: &StoredConversation,
    snapshot: &DeepNoteInputSnapshot,
    current_attachment_content_hashes: Vec<String>,
) -> Result<(), String> {
    let messages = noteworthy_messages(conversation);
    if messages.len() < snapshot.message_ids.len() {
        return Err(
            "旧任务覆盖的消息已被删除，不能恢复旧快照。请使用当前内容重新生成深度笔记。"
                .to_string(),
        );
    }
    let prefix = &messages[..snapshot.message_ids.len()];
    let prefix_ids = prefix
        .iter()
        .map(|message| message.id.clone())
        .collect::<Vec<_>>();
    if prefix_ids != snapshot.message_ids {
        return Err(
            "旧任务覆盖的消息顺序或身份已经变化，不能恢复旧快照。请使用当前内容重新生成深度笔记。"
                .to_string(),
        );
    }
    if snapshot.message_content_hashes.len() != snapshot.message_ids.len() {
        return Err(
            "旧任务缺少逐消息内容 Hash，无法安全判断是否仅追加了新消息。请重新生成深度笔记。"
                .to_string(),
        );
    }
    let prefix_hashes = prefix
        .iter()
        .map(|message| message_source_hash(message))
        .collect::<Vec<_>>();
    if prefix_hashes != snapshot.message_content_hashes {
        return Err("旧任务覆盖的消息内容、引用或附件已经被编辑，不能恢复旧快照。请使用当前内容重新生成深度笔记。".to_string());
    }
    let mut projected = conversation.clone();
    projected.messages = prefix.iter().map(|message| (*message).clone()).collect();
    projected.linked_library_item_ids = conversation
        .linked_library_item_ids
        .iter()
        .filter(|id| snapshot.selected_literature_ids.contains(id))
        .cloned()
        .collect();
    let current = input_snapshot(
        &projected,
        snapshot.model.clone(),
        snapshot.created_at,
        current_attachment_content_hashes,
    );
    let unchanged_sources = current.attachment_ids == snapshot.attachment_ids
        && current.attachment_content_hashes == snapshot.attachment_content_hashes
        && (snapshot.attachment_message_ids.is_empty()
            || current.attachment_message_ids == snapshot.attachment_message_ids)
        && current.selected_literature_ids == snapshot.selected_literature_ids
        && current.selected_note_ids == snapshot.selected_note_ids;
    if !unchanged_sources {
        return Err("旧任务覆盖的附件、文献或笔记引用已经变化，不能恢复旧快照。请使用当前内容重新生成深度笔记。".to_string());
    }
    Ok(())
}

fn snapshot_conversation_after_validation(
    conversation: &StoredConversation,
    snapshot: &DeepNoteInputSnapshot,
) -> Result<StoredConversation, String> {
    let messages = noteworthy_messages(conversation);
    let mut projected = conversation.clone();
    projected.messages = messages[..snapshot.message_ids.len()]
        .iter()
        .map(|message| (*message).clone())
        .collect();
    projected.linked_library_item_ids = conversation
        .linked_library_item_ids
        .iter()
        .filter(|id| snapshot.selected_literature_ids.contains(id))
        .cloned()
        .collect();
    projected.updated_at = snapshot.conversation_revision;
    Ok(projected)
}

async fn validate_recovery_snapshot_from_storage(
    repository: &crate::chat::storage::ConversationRepository,
    conversation: &StoredConversation,
    snapshot: &DeepNoteInputSnapshot,
) -> Result<(), String> {
    let repository = repository.clone();
    let messages = noteworthy_messages(conversation);
    let mut conversation_for_hash = conversation.clone();
    if messages.len() >= snapshot.message_ids.len() {
        conversation_for_hash.messages = messages[..snapshot.message_ids.len()]
            .iter()
            .map(|message| (*message).clone())
            .collect();
    }
    let hashes = tauri::async_runtime::spawn_blocking(move || {
        attachment_content_hashes(&repository, &conversation_for_hash)
    })
    .await
    .map_err(|error| format!("附件快照校验任务失败：{error}"))??;
    validate_recovery_snapshot(conversation, snapshot, hashes)
}

async fn snapshot_conversation(
    repository: &crate::chat::storage::ConversationRepository,
    conversation: &StoredConversation,
    snapshot: &DeepNoteInputSnapshot,
) -> Result<StoredConversation, String> {
    validate_recovery_snapshot_from_storage(repository, conversation, snapshot).await?;
    snapshot_conversation_after_validation(conversation, snapshot)
}

async fn create_input_snapshot(
    repository: &crate::chat::storage::ConversationRepository,
    conversation: &StoredConversation,
    model: DeepNoteModelSnapshot,
    created_at: u64,
) -> Result<DeepNoteInputSnapshot, String> {
    let repository = repository.clone();
    let conversation_for_hash = conversation.clone();
    let hashes = tauri::async_runtime::spawn_blocking(move || {
        attachment_content_hashes(&repository, &conversation_for_hash)
    })
    .await
    .map_err(|error| format!("附件快照创建任务失败：{error}"))??;
    Ok(input_snapshot(conversation, model, created_at, hashes))
}

fn reset_failed_runtime_nodes(runtime: &mut DeepNoteRuntimeState) {
    let Some(plan_version) = runtime.plan_version.as_mut() else {
        return;
    };
    reset_failed_nodes(&mut plan_version.compiled_dag);
}

fn reset_failed_nodes(nodes: &mut [DeepNoteDagNode]) {
    for node in nodes {
        if matches!(
            node.status,
            DeepNoteNodeStatus::Failed
                | DeepNoteNodeStatus::Blocked
                | DeepNoteNodeStatus::NeedsReview
                | DeepNoteNodeStatus::NeedsRevision
                | DeepNoteNodeStatus::Interrupted
        ) {
            node.status = DeepNoteNodeStatus::Pending;
            node.attempt_count = 0;
            node.evidence_ids.clear();
            node.output_ref = None;
            node.validation_json.clear();
            node.error_message = None;
        }
    }
}

fn runtime_state(run: &NotePipelineRun) -> Result<DeepNoteRuntimeState, String> {
    serde_json::from_str(&run.preflight_json)
        .map_err(|error| format!("读取深度笔记运行快照失败：{error}"))
}

fn snapshot_skill_profiles(
    state: &AppState,
    include_visual_evidence: bool,
) -> (DeepNoteSkillProfiles, Vec<String>) {
    use crate::skills::types::{SkillMode, SkillRisk};

    let profile_ids = [
        (
            DeepNoteSkillProfileKind::Planner,
            vec!["question-framing", "knowledge-capture"],
        ),
        (
            DeepNoteSkillProfileKind::Writer,
            vec![
                "beginner-teaching",
                "document-authoring",
                "markdown-notes",
                "diagram",
            ],
        ),
        (
            DeepNoteSkillProfileKind::Reviewer,
            if include_visual_evidence {
                vec![
                    "knowledge-capture",
                    "markdown-notes",
                    "diagram",
                    "visual-evidence-analysis",
                ]
            } else {
                vec!["knowledge-capture", "markdown-notes", "diagram"]
            },
        ),
    ];
    let mut profiles = DeepNoteSkillProfiles::default();
    let mut warnings = Vec::new();
    for (profile, ids) in profile_ids {
        for skill_id in ids {
            let detail = match state.skill_repository.get_detail(skill_id) {
                Ok(detail) => detail,
                Err(error) => {
                    warnings.push(format!("深度笔记未加载 Skill {skill_id}：{error}"));
                    continue;
                }
            };
            if !detail.summary.enabled
                || !detail.summary.supported_modes.contains(&SkillMode::Notes)
                || detail.summary.risk == SkillRisk::High
                || detail.summary.disable_model_invocation
            {
                warnings.push(format!(
                    "深度笔记跳过不满足 Notes 安全条件的 Skill：{}",
                    detail.summary.name
                ));
                continue;
            }
            let rendered_prompt = match state.skill_repository.render_method_snapshot(skill_id) {
                Ok(prompt) => prompt,
                Err(error) => {
                    warnings.push(format!("深度笔记冻结 Skill {skill_id} 失败：{error}"));
                    continue;
                }
            };
            let snapshot = DeepNoteSkillSnapshot {
                profile,
                skill_id: detail.summary.id,
                name: detail.summary.name,
                version: detail.summary.version,
                content_hash: detail.summary.content_hash,
                rendered_prompt,
            };
            match profile {
                DeepNoteSkillProfileKind::Planner => profiles.planner.push(snapshot),
                DeepNoteSkillProfileKind::Writer => profiles.writer.push(snapshot),
                DeepNoteSkillProfileKind::Reviewer => profiles.reviewer.push(snapshot),
            }
        }
    }
    (profiles, warnings)
}

fn system_prompt_with_skill_profile(
    state: &AppState,
    run_id: &str,
    runtime: &DeepNoteRuntimeState,
    profile: DeepNoteSkillProfileKind,
    node_id: Option<&str>,
    base_prompt: &str,
) -> String {
    let skills = runtime.skill_profiles.for_profile(profile);
    if skills.is_empty() {
        return base_prompt.to_string();
    }
    for skill in skills {
        let _ = state.library_repository.append_note_pipeline_event(
            run_id,
            "skillApplied",
            node_id,
            &serde_json::json!({
                "profile": profile.as_str(),
                "skillId": skill.skill_id,
                "name": skill.name,
                "version": skill.version,
                "contentHash": skill.content_hash,
            })
            .to_string(),
        );
    }
    format!(
        "{base_prompt}\n\n以下方法论 Skill 已在本 Run 创建时冻结。它们只能影响分析与表达，不能改变来源、权限、DAG 或预算：\n\n{}",
        skills
            .iter()
            .map(|skill| skill.rendered_prompt.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    )
}

fn save_runtime_state(
    state: &AppState,
    run_id: &str,
    runtime: &DeepNoteRuntimeState,
) -> Result<(), String> {
    if let Some(instance_id) = current_task_instance_id() {
        if state.is_note_pipeline_instance_detached(&instance_id) {
            return Ok(());
        }
    }
    let mut persisted_runtime = runtime.clone();
    if let Ok(used) = state
        .library_repository
        .count_note_pipeline_upstream_requests(run_id)
    {
        persisted_runtime.budget.upstream_requests_used = used;
    }
    let runtime_json = serde_json::to_string(&persisted_runtime)
        .map_err(|error| format!("序列化深度笔记运行状态失败：{error}"))?;
    let budget_json = serde_json::to_string(&persisted_runtime.budget)
        .map_err(|error| format!("序列化深度笔记预算失败：{error}"))?;
    state.library_repository.update_note_pipeline_runtime_json(
        run_id,
        &budget_json,
        &runtime_json,
        None,
    )
}

fn consume_semantic_call(
    state: &AppState,
    run_id: &str,
    runtime: &mut DeepNoteRuntimeState,
) -> Result<(), String> {
    if let Some(exhaustion) = refresh_run_budget(state, run_id, runtime) {
        save_runtime_state(state, run_id, runtime)?;
        return Err(format!(
            "{RUN_BUDGET_EXHAUSTED_PREFIX}{}",
            exhaustion.message(runtime)
        ));
    }
    // 这是逻辑调用诊断计数，不再充当 provider 配额。物理请求会在 HTTP 边界逐次
    // 原子扣减，因此 retry 与 stream fallback 不会被折叠成一次。
    runtime.budget.semantic_calls_used = runtime.budget.semantic_calls_used.saturating_add(1);
    match state.library_repository.get_note_pipeline_run(run_id) {
        Ok(_) => save_runtime_state(state, run_id, runtime),
        Err(error) if error == "深度笔记任务不存在。" => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunBudgetExhaustion {
    UpstreamRequests,
    WallClock,
}

impl RunBudgetExhaustion {
    fn as_str(self) -> &'static str {
        match self {
            Self::UpstreamRequests => "upstreamRequests",
            Self::WallClock => "upstreamWallClock",
        }
    }

    fn message(self, runtime: &DeepNoteRuntimeState) -> String {
        match self {
            Self::UpstreamRequests => format!(
                "上游请求预算已用尽（{}/{}），请缩小范围或改用更稳定的模型。",
                runtime.budget.upstream_requests_used, runtime.budget.upstream_request_limit
            ),
            Self::WallClock => format!(
                "已达运行时长上限（{} 分钟），请缩小范围或改用更快的模型。",
                runtime.budget.run_wall_clock_ms / 60_000
            ),
        }
    }
}

/// 刷新 run 的物理上游请求数与累计上游墙钟，并回答「哪项预算已耗尽」。
///
/// 每次批次调度前重算一次，而不是依赖内存里的增量：section 是并行执行的，
/// 那些任务持有的是 runtime 快照而不是 `&mut runtime`（否则借用检查过不去），
/// 所以增量只能落在事件表里，由这里汇总回来。
fn refresh_run_budget(
    state: &AppState,
    run_id: &str,
    runtime: &mut DeepNoteRuntimeState,
) -> Option<RunBudgetExhaustion> {
    if let Ok(used) = state
        .library_repository
        .count_note_pipeline_upstream_requests(run_id)
    {
        runtime.budget.upstream_requests_used = used;
        if runtime.budget.upstream_request_exhausted() {
            return Some(RunBudgetExhaustion::UpstreamRequests);
        }
    }
    if let Ok(total) = state
        .library_repository
        .sum_note_pipeline_upstream_wall_clock_ms(run_id)
    {
        runtime.budget.upstream_wall_clock_ms = total;
        if runtime.budget.run_wall_clock_exhausted() {
            return Some(RunBudgetExhaustion::WallClock);
        }
    }
    // 汇总失败不能把任务判死：宁可这一轮不设对应的闸，也不要因为一次读库失败
    // 就把一个健康的 run 当成预算耗尽收掉。
    None
}

/// 回答「这个 section 的累计活跃时长是否已耗尽预算」。
///
/// 读的是累计活跃时长而不是「现在减去开始时刻」。时刻差会把关机、暂停、等并发
/// 席位的时间一并算进预算：一个跑了 3 分钟就被中断、次日才续跑的 section，时刻差
/// 是十几个小时，闸门会把它当成早已超时而跳过，用户拿到一篇静默缺章的笔记。
/// 累计活跃时长由 `record_section_active_ms` 在每个 section 任务结束时累加。
///
/// 纯读不写：登记的职责归 `record_section_active_ms`。这样闸门是个无副作用的谓词，
/// 单测里不必拼一个完整的运行时快照，也不会因为「问了一次」就改变预算。
fn section_wall_clock_exhausted(
    section_active_ms: &BTreeMap<String, u64>,
    budget: &DeepNoteBudget,
    section_id: &str,
) -> bool {
    let active = section_active_ms.get(section_id).copied().unwrap_or(0);
    budget.section_wall_clock_exhausted(active)
}

/// 把一个 section 本轮实际花掉的执行时长累加进预算。
///
/// 累加而不是覆盖：一个 section 可能跨多次 run 断续执行，预算管的是它总共花了
/// 多久。饱和加法 —— 溢出回绕会让一个严重超时的 section 看起来毫无消耗，那是
/// 最坏的失效方向。
fn record_section_active_ms(
    section_active_ms: &mut BTreeMap<String, u64>,
    section_id: &str,
    elapsed_ms: u64,
) {
    let entry = section_active_ms.entry(section_id.to_string()).or_insert(0);
    *entry = entry.saturating_add(elapsed_ms);
}

fn persist_scheduler_state(
    state: &AppState,
    run_id: &str,
    runtime: &mut DeepNoteRuntimeState,
    scheduler: &DeepNoteDagScheduler,
) -> Result<(), String> {
    let plan_version = runtime
        .plan_version
        .as_mut()
        .ok_or_else(|| "深度笔记执行图尚未编译。".to_string())?;
    plan_version.compiled_dag = scheduler.nodes().to_vec();
    let version = plan_version.version;
    state.library_repository.update_note_pipeline_nodes_state(
        run_id,
        version,
        scheduler.nodes(),
    )?;
    save_runtime_state(state, run_id, runtime)
}

fn verified_evidence_ids_for_section(
    evidence_by_section: &HashMap<String, Vec<String>>,
    section_id: &str,
) -> Vec<String> {
    evidence_by_section
        .get(section_id)
        .cloned()
        .unwrap_or_default()
}

fn dependency_context(section: &DeepNoteSection, drafts: &HashMap<String, String>) -> String {
    let mut context = Vec::new();
    for dependency in &section.depends_on {
        if let Some(markdown) = drafts.get(dependency) {
            let excerpt = markdown.chars().take(2_400).collect::<String>();
            context.push(format!("依赖章节 {dependency} 已完成内容摘录：\n{excerpt}"));
        }
    }
    context.join("\n\n")
}

fn resolve_note_model(
    settings: &ModelSettings,
    conversation: &StoredConversation,
) -> Result<(String, String), String> {
    if let (Some(provider_id), Some(model_id)) = (
        settings.note_provider_id.as_deref(),
        settings.note_model_id.as_deref(),
    ) {
        if enabled_model(settings, provider_id, model_id) {
            return Ok((provider_id.to_string(), model_id.to_string()));
        }
    }
    if let (Some(provider_id), Some(model_id)) = (
        conversation.provider_id.as_deref(),
        conversation.model_id.as_deref(),
    ) {
        if enabled_model(settings, provider_id, model_id) {
            return Ok((provider_id.to_string(), model_id.to_string()));
        }
    }
    if let (Some(provider_id), Some(model_id)) = (
        settings.default_provider_id.as_deref(),
        settings.default_model_id.as_deref(),
    ) {
        if enabled_model(settings, provider_id, model_id) {
            return Ok((provider_id.to_string(), model_id.to_string()));
        }
    }
    Err("请先在设置中配置可用的笔记或 Chat 模型。".to_string())
}

fn parse_json_object<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, String> {
    let trimmed = raw.trim();
    let without_fence = if trimmed.starts_with("```") {
        let first_line = trimmed.find('\n').map_or(0, |index| index + 1);
        let end = trimmed.rfind("```").unwrap_or(trimmed.len());
        &trimmed[first_line..end]
    } else {
        trimmed
    };
    serde_json::from_str(without_fence.trim())
        .map_err(|error| format!("模型返回的 JSON 无法解析：{error}"))
}

async fn model_call(
    state: &AppState,
    run: &NotePipelineRun,
    operation: &str,
    system_prompt: String,
    user_prompt: String,
    max_output_tokens: u32,
) -> Result<String, String> {
    let cancellation = CancellationToken::new();
    model_call_with_runtime(
        state,
        run,
        operation,
        run.phase,
        system_prompt,
        user_prompt,
        max_output_tokens,
        run.retry_attempts,
        &cancellation,
        None,
    )
    .await
    .map_err(|error| error.message)
}

fn model_stage_label(operation: &str) -> &'static str {
    match operation {
        "deepNoteChunk" => "来源分块提取",
        "deepNoteChunkRepair" => "来源分块 JSON 修复",
        "deepNoteOutlineDirect" => "直接生成提纲",
        "deepNoteOutline" => "知识账本汇总提纲",
        "deepNoteOutlineFallback" => "精简账本提纲",
        _ => "深度笔记模型调用",
    }
}

/// 超时类错误：本地 attempt 超时与网关 504 是**同一个物理原因**的两种表现形式。
///
/// `completion_attempt` 的 `tokio::time::timeout` 到期产生 `ClientTimeout`，
/// 上游网关切断连接产生 `UpstreamTimeout`。两者都意味着"这份载荷在这个时间窗口内
/// 没能生成完"，原样重投不可能变好 —— 只会把同样的墙钟再烧一遍。
fn is_timeout_like(kind: ModelErrorKind) -> bool {
    matches!(
        kind,
        ModelErrorKind::ClientTimeout | ModelErrorKind::UpstreamTimeout
    )
}

fn should_retry_note_model_call(operation: &str, error: &ModelError) -> bool {
    // 超时类错误一律否决 HTTP 层重试，交还给管线缩小载荷后再试。
    //
    // 修复前只否决 `UpstreamTimeout`，且名单里没有 `deepNoteChunk`，导致同一个
    // 物理原因走出两条相反的路：本地 300s 超时会被以 300ms 基数重试 5 次
    // （约 25 分钟耗在同一份大载荷上），网关 504 则立刻降级。
    //
    // 这里按 `deepNote` 前缀判定而不是逐个枚举 operation：管线内所有阶段都用这个
    // 前缀，枚举式名单已经漏过一次（`deepNoteChunk`、`deepNoteVisionSource`）。
    if is_timeout_like(error.kind) && operation.starts_with("deepNote") {
        return false;
    }
    !matches!(
        error.kind,
        ModelErrorKind::ProviderUnavailable
            | ModelErrorKind::ModelNotFound
            | ModelErrorKind::MissingApiKey
            | ModelErrorKind::Authentication
            | ModelErrorKind::PermissionDenied
            | ModelErrorKind::QuotaExceeded
    )
}

#[cfg(feature = "deep-note-e2e")]
fn mock_model_response(operation: &str, prompt: &str) -> String {
    let mut message_ids = Vec::new();
    for token in prompt.split_whitespace() {
        let Some(start) = token.find("message-") else {
            continue;
        };
        let id = token[start..]
            .chars()
            .take_while(|value| value.is_ascii_alphanumeric() || *value == '-')
            .collect::<String>();
        if id.len() > 10 && !message_ids.contains(&id) {
            message_ids.push(id);
        }
    }
    match operation {
        "deepNoteChunk" | "deepNoteChunkRepair" => serde_json::json!({
            "summary": "模拟来源摘要：已提取本分块中的核心概念、事实和可复习要点。",
            "canonicalTerms": ["核心概念", "实践方法"],
            "verifiedFacts": ["事实来自当前输入分块。"],
            "coveredTopics": ["主题概览"],
            "openQuestions": [],
            "conflicts": [],
            "globalConstraints": [],
            "sourceMessageIds": message_ids,
        })
        .to_string(),
        "deepNoteOutline" | "deepNoteOutlineFallback" | "deepNoteOutlineDirect" => {
            serde_json::json!({
                "goal": "建立可验证、可复习的深度笔记。",
                "audience": "当前对话的学习者",
                "scope": "仅覆盖当前输入内容",
                "title": "模拟深度笔记",
                "summary": "这是一份用于验证全链路的模拟提纲。",
                "weakPoints": [],
                "hiddenQuestions": [],
                "knowledgeGaps": [],
                "misconceptions": [],
                "causalChains": [],
                "visualizationOpportunities": ["flowchart：核心概念到实践方法"],
                "allowAiSupplement": false,
                "evidencePolicy": "以当前对话消息为依据。",
                "sourceIds": message_ids,
                "sections": [{
                    "id": "sec-1",
                    "heading": "核心概念与实践",
                    "kind": "concept",
                    "purpose": "建立核心概念并说明实践方法。",
                    "brief": "解释当前输入中的核心概念、关系和使用方式。",
                    "dependsOn": [],
                    "evidenceRequirements": ["当前对话中的事实"],
                    "successCriteria": ["说明核心概念并给出实践方法"],
                    "sourceScope": [],
                    "targetDepth": "standard",
                    "allowAiSupplement": false,
                    "needsSupplement": false,
                    "sourceMessageIds": message_ids,
                }]
            })
            .to_string()
        }
        _ => {
            let heading = prompt
                .find("\"heading\":\"")
                .and_then(|start| {
                    let value = &prompt[start + 11..];
                    value.find('"').map(|end| value[..end].to_string())
                })
                .unwrap_or_else(|| "核心概念与实践".to_string());
            format!(
                "## {heading}\n\n本节基于当前输入与已保存的来源账本，说明核心概念、它们之间的关系以及实际使用方式。内容只引用当前任务已经覆盖的消息，不把尚未验证的信息当作事实。这里补充定义、适用条件、常见误区、操作步骤和检查方法，确保读者可以独立复习并把知识迁移到新的问题中。\n\n- 核心概念：当前输入中的主要知识点，并说明它与相关概念的区别。\n- 实践方法：将概念转化为可执行步骤，说明输入、过程、结果和注意事项。\n- 常见误区：列出容易混淆的边界，说明为什么不能简单套用。\n- 自检问题：能否用自己的话解释概念并判断适用边界。\n- 复习提示：重新阅读来源消息后，检查本节是否仍然能够被证据支持。"
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn model_call_with_runtime(
    state: &AppState,
    run: &NotePipelineRun,
    operation: &str,
    phase: NotePipelinePhase,
    system_prompt: String,
    user_prompt: String,
    max_output_tokens: u32,
    max_retries: u8,
    cancellation: &CancellationToken,
    channel: Option<&Channel<NotePipelineProgress>>,
) -> Result<String, ModelError> {
    model_call_with_runtime_attachments(
        state,
        run,
        operation,
        phase,
        system_prompt,
        user_prompt,
        Vec::new(),
        max_output_tokens,
        max_retries,
        cancellation,
        channel,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn model_call_with_runtime_attachments(
    state: &AppState,
    run: &NotePipelineRun,
    operation: &str,
    phase: NotePipelinePhase,
    system_prompt: String,
    user_prompt: String,
    attachments: Vec<crate::chat::conversation_types::StoredChatAttachment>,
    max_output_tokens: u32,
    max_retries: u8,
    cancellation: &CancellationToken,
    channel: Option<&Channel<NotePipelineProgress>>,
) -> Result<String, ModelError> {
    let started_at = crate::usage::now_ms();
    let call_id = Uuid::new_v4().to_string();
    let input_chars = user_prompt.chars().count();
    let system_prompt_chars = system_prompt.chars().count();
    let estimated_input_tokens =
        estimate_text_tokens(&user_prompt).saturating_add(estimate_text_tokens(&system_prompt));
    #[cfg(feature = "deep-note-e2e")]
    if std::env::var("MNEMORA_DEEP_NOTE_MOCK").ok().as_deref() == Some("1") {
        let text = mock_model_response(operation, &user_prompt);
        let _ = state.library_repository.append_note_pipeline_event(
            &run.id,
            "modelCallCompleted",
            None,
            &serde_json::json!({
                "callId": call_id,
                "operation": operation,
                "phase": phase.as_str(),
                "providerId": run.provider_id,
                "modelId": run.model_id,
                "durationMs": 0,
                "responseChars": text.chars().count(),
                "inputChars": input_chars,
                "systemPromptChars": system_prompt_chars,
                "estimatedInputTokens": estimated_input_tokens,
                "actualAttemptCount": 0,
                "streamingAttemptCount": 0,
                "nonStreamingAttemptCount": 0,
                "requestBytes": 0,
                "maxOutputTokens": max_output_tokens,
                "maxRetries": 0,
                "timeoutMs": 0,
                "mock": true,
            })
            .to_string(),
        );
        return Ok(text);
    }
    let timeout_ms = match operation {
        "deepNoteChunk" => 300_000,
        "deepNoteChunkRepair" => 180_000,
        "deepNoteOutlineDirect" | "deepNoteOutline" => 300_000,
        "deepNoteOutlineFallback" => 180_000,
        "deepNote" => 420_000,
        _ if max_output_tokens <= 8_192 => 300_000,
        _ => 420_000,
    };
    let observer = |event: chat_service::CompletionProgress| {
        // 流式回落必须留痕，且与是否有前端通道无关：它是「这个上游不吃流式」的
        // 唯一线上证据，排查 504 时要靠它区分「没开流式」和「流式被拒」。
        if let chat_service::CompletionProgress::StreamKeepaliveFellBack { error } = &event {
            let _ = state.library_repository.append_note_pipeline_event(
                &run.id,
                "streamKeepaliveFellBack",
                None,
                &serde_json::json!({
                    "callId": call_id.clone(),
                    "operation": operation,
                    "phase": phase.as_str(),
                    "errorKind": format!("{:?}", error.kind),
                    "message": error.message.as_str(),
                })
                .to_string(),
            );
        }
        let Some(channel) = channel else {
            return;
        };
        match event {
            chat_service::CompletionProgress::AttemptStarted {
                retry_index,
                max_retries,
            } => progress_activity(
                state,
                channel,
                &run.id,
                phase,
                format!(
                    "{} · 正在等待模型响应 · 第 {} 次请求（已重试 {}/{max_retries}）",
                    model_stage_label(operation),
                    retry_index + 1,
                    retry_index
                ),
                NotePipelineActivity {
                    kind: "modelCall".to_string(),
                    call_id: call_id.clone(),
                    operation: operation.to_string(),
                    attempt: retry_index + 1,
                    max_retries,
                    started_at: crate::usage::now_ms(),
                    timeout_ms,
                    delay_ms: None,
                    last_error: None,
                },
            ),
            chat_service::CompletionProgress::RetryScheduled {
                retry_index,
                max_retries,
                delay_ms,
                error,
            } => progress_activity(
                state,
                channel,
                &run.id,
                phase,
                format!(
                    "{}请求失败，准备第 {} 次重试（{retry_index}/{max_retries}）",
                    model_stage_label(operation),
                    retry_index
                ),
                NotePipelineActivity {
                    kind: "retryWait".to_string(),
                    call_id: call_id.clone(),
                    operation: operation.to_string(),
                    attempt: retry_index + 1,
                    max_retries,
                    started_at: crate::usage::now_ms(),
                    timeout_ms,
                    delay_ms: Some(delay_ms),
                    last_error: Some(error.message),
                },
            ),
            chat_service::CompletionProgress::StreamKeepaliveFellBack { error } => {
                progress_activity(
                    state,
                    channel,
                    &run.id,
                    phase,
                    format!(
                        "{} · 上游拒绝流式，已回落非流式（长文生成可能触发网关超时）",
                        model_stage_label(operation)
                    ),
                    NotePipelineActivity {
                        kind: "streamFallback".to_string(),
                        call_id: call_id.clone(),
                        operation: operation.to_string(),
                        attempt: 1,
                        max_retries,
                        started_at: crate::usage::now_ms(),
                        timeout_ms,
                        delay_ms: None,
                        last_error: Some(error.message),
                    },
                )
            }
        }
    };
    let prefer_streaming = state
        .app_settings
        .read()
        .map(|settings| settings.deep_note_stream_keepalive)
        // 读锁不可用时保守走非流式：宁可慢，不要在这里放大一个锁故障。
        .unwrap_or(false);
    let (route_identity, route_profile) =
        deep_note_route_profile(state, &run.provider_id, &run.model_id)
            .map_err(ModelError::invalid_configuration)?;
    if let Some(error) = blocked_route_error(&route_profile) {
        let _ = state.library_repository.append_note_pipeline_event(
            &run.id,
            "routeCallSuppressed",
            None,
            &serde_json::json!({
                "callId": call_id,
                "operation": operation,
                "phase": phase.as_str(),
                "routeKey": route_identity.route_key.as_str(),
                "providerConfigEpoch": route_identity.provider_config_epoch.as_str(),
                "routeState": route_profile.availability.as_str(),
                "retryAfterUntilMs": route_profile.retry_after_until_ms,
                "message": error.message.as_str(),
            })
            .to_string(),
        );
        return Err(error);
    }
    // 增量笔记编辑会构造一个不落 `note_pipeline_runs` 的临时 run 来复用模型调用层。
    // 那条路径没有可恢复任务，也就没有 run 级事件账本；不能因为遥测表里找不到它
    // 而阻断原本合法的编辑请求。真正持久化的 DeepNote run 则始终 fail-closed。
    let enforce_upstream_budget = match state.library_repository.get_note_pipeline_run(&run.id) {
        Ok(_) => true,
        Err(error) if error == "深度笔记任务不存在。" => false,
        Err(error) => {
            return Err(ModelError::invalid_configuration(format!(
                "读取深度笔记上游请求预算失败：{error}"
            )))
        }
    };
    let budget_snapshot = if enforce_upstream_budget {
        Some(
            serde_json::from_str::<DeepNoteBudget>(&run.budget_json).map_err(|error| {
                ModelError::invalid_configuration(format!("解析深度笔记上游请求预算失败：{error}"))
            })?,
        )
    } else {
        None
    };
    let request_limit = budget_snapshot
        .as_ref()
        .map(|budget| budget.upstream_request_limit)
        .unwrap_or(MAX_DEEP_NOTE_UPSTREAM_REQUESTS);
    let run_wall_clock_limit = budget_snapshot
        .as_ref()
        .map(|budget| budget.run_wall_clock_ms)
        .unwrap_or_else(|| DeepNoteBudget::for_section_count(1).run_wall_clock_ms);
    let actual_attempt_count = AtomicU32::new(0);
    let streaming_attempt_count = AtomicU32::new(0);
    let non_streaming_attempt_count = AtomicU32::new(0);
    let max_request_bytes = AtomicUsize::new(0);
    let before_upstream_request = |attempt: chat_service::UpstreamRequestAttempt| {
        if !enforce_upstream_budget {
            return Ok(());
        }
        if state
            .library_repository
            .sum_note_pipeline_upstream_wall_clock_ms(&run.id)
            .is_ok_and(|used| used >= run_wall_clock_limit)
        {
            return Err(ModelError::invalid_configuration(format!(
                "{RUN_BUDGET_EXHAUSTED_PREFIX}已达运行时长上限（{} 分钟），已停止发出新请求。",
                run_wall_clock_limit / 60_000
            )));
        }
        let payload = serde_json::json!({
            "callId": call_id.clone(),
            "operation": operation,
            "phase": phase.as_str(),
            "providerId": run.provider_id,
            "modelId": run.model_id,
            "routeKey": route_identity.route_key.as_str(),
            "providerConfigEpoch": route_identity.provider_config_epoch.as_str(),
            "routeState": route_profile.availability.as_str(),
            "adaptiveTargetTokens": route_profile.effective_target_tokens(crate::usage::now_ms()),
            "requestIndex": attempt.request_index,
            "retryIndex": attempt.retry_index,
            "maxRetries": attempt.max_retries,
            "transport": attempt.transport.as_str(),
            "requestBytes": attempt.request_bytes,
            "estimatedInputTokens": estimated_input_tokens,
            "maxOutputTokens": max_output_tokens,
        })
        .to_string();
        state
            .library_repository
            .try_append_note_pipeline_upstream_attempt(&run.id, request_limit, &payload)
            .map_err(|error| {
                ModelError::invalid_configuration(format!("{RUN_BUDGET_EXHAUSTED_PREFIX}{error}"))
            })?;
        actual_attempt_count.fetch_add(1, Ordering::Relaxed);
        match attempt.transport {
            chat_service::CompletionTransport::Streaming => {
                streaming_attempt_count.fetch_add(1, Ordering::Relaxed);
            }
            chat_service::CompletionTransport::NonStreaming => {
                non_streaming_attempt_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        max_request_bytes.fetch_max(attempt.request_bytes, Ordering::Relaxed);
        Ok(())
    };
    let execution = chat_service::CompleteExecution {
        cancellation,
        max_retries: Some(max_retries),
        attempt_timeout: Some(Duration::from_millis(timeout_ms)),
        retry_predicate: Some(&|error| should_retry_note_model_call(operation, error)),
        on_progress: Some(&observer),
        before_upstream_request: Some(&before_upstream_request),
        prefer_streaming,
        max_request_bytes: Some(request_byte_limit(&attachments)),
    };
    let result = chat_service::complete_with_execution(
        state,
        ChatCompletionRequest {
            provider_id: run.provider_id.clone(),
            model_id: run.model_id.clone(),
            conversation_id: Some(run.conversation_id.clone()),
            message_id: Some(Uuid::new_v4().to_string()),
            // All pipeline stages are auxiliary model calls. Keep the persisted
            // request operation stable while exposing the finer stage in events.
            operation: Some("deepNote".to_string()),
            system_prompt,
            activated_skill_ids: Vec::new(),
            slash_skill_id: None,
            permission_mode: Default::default(),
            workspace_mode: ChatWorkspaceMode::Notes,
            workspace_context: None,
            messages: vec![ChatModelMessage {
                role: ModelRole::User,
                content: user_prompt,
                attachments,
                failed: false,
            }],
            options: ModelOptions {
                temperature: None,
                max_output_tokens: Some(max_output_tokens),
                thinking_enabled: run.thinking_enabled,
                reasoning_effort: None,
            },
        },
        &execution,
    )
    .await;
    match result {
        Ok(response) => {
            let usage = response.response.usage.as_ref();
            let input_tokens = usage.and_then(|value| value.input_tokens);
            let output_tokens = usage.and_then(|value| value.output_tokens);
            let time_to_first_token_ms = usage.and_then(|value| value.time_to_first_token_ms);
            let text = response.response.text;
            let request_bytes = max_request_bytes.load(Ordering::Relaxed);
            let updated_profile = record_adaptive_route_outcome(
                state,
                &run.id,
                &route_identity,
                &route_profile,
                &AdaptiveVolumeOutcome::success(
                    operation == "deepNoteChunk",
                    estimated_input_tokens,
                    request_bytes,
                ),
            );
            let _ = state.library_repository.append_note_pipeline_event(
                &run.id,
                "modelCallCompleted",
                None,
                &serde_json::json!({
                    "callId": call_id,
                    "operation": operation,
                    "phase": phase.as_str(),
                    "providerId": run.provider_id,
                    "modelId": run.model_id,
                    "routeKey": route_identity.route_key.as_str(),
                    "providerConfigEpoch": route_identity.provider_config_epoch.as_str(),
                    "routeState": updated_profile.availability.as_str(),
                    "adaptiveTargetTokens": updated_profile.effective_target_tokens(crate::usage::now_ms()),
                    "adaptiveProfileSamples": updated_profile.sample_count,
                    "durationMs": crate::usage::now_ms().saturating_sub(started_at),
                    "responseChars": text.chars().count(),
                    "inputChars": input_chars,
                    "systemPromptChars": system_prompt_chars,
                    "estimatedInputTokens": estimated_input_tokens,
                    "inputTokens": input_tokens,
                    "outputTokens": output_tokens,
                    "timeToFirstTokenMs": time_to_first_token_ms,
                    "actualAttemptCount": actual_attempt_count.load(Ordering::Relaxed),
                    "streamingAttemptCount": streaming_attempt_count.load(Ordering::Relaxed),
                    "nonStreamingAttemptCount": non_streaming_attempt_count.load(Ordering::Relaxed),
                    "requestBytes": request_bytes,
                    "maxOutputTokens": max_output_tokens,
                    "maxRetries": max_retries,
                    "timeoutMs": timeout_ms,
                })
                .to_string(),
            );
            Ok(text)
        }
        Err(error) => {
            let request_bytes = max_request_bytes.load(Ordering::Relaxed);
            let updated_profile = record_adaptive_route_outcome(
                state,
                &run.id,
                &route_identity,
                &route_profile,
                &AdaptiveVolumeOutcome::failure(
                    error.kind,
                    operation == "deepNoteChunk",
                    estimated_input_tokens,
                    request_bytes,
                    error.retry_after_ms,
                ),
            );
            let _ = state.library_repository.append_note_pipeline_event(
                &run.id,
                "modelCallFailed",
                None,
                &serde_json::json!({
                    "callId": call_id,
                    "operation": operation,
                    "phase": phase.as_str(),
                    "providerId": run.provider_id,
                    "modelId": run.model_id,
                    "routeKey": route_identity.route_key.as_str(),
                    "providerConfigEpoch": route_identity.provider_config_epoch.as_str(),
                    "routeState": updated_profile.availability.as_str(),
                    "adaptiveTargetTokens": updated_profile.effective_target_tokens(crate::usage::now_ms()),
                    "adaptiveProfileSamples": updated_profile.sample_count,
                    "durationMs": crate::usage::now_ms().saturating_sub(started_at),
                    "errorKind": format!("{:?}", error.kind),
                    "message": error.message,
                    "statusCode": error.status_code,
                    "providerCode": error.provider_code,
                    "retryAfterMs": error.retry_after_ms,
                    "inputChars": input_chars,
                    "systemPromptChars": system_prompt_chars,
                    "estimatedInputTokens": estimated_input_tokens,
                    "actualAttemptCount": actual_attempt_count.load(Ordering::Relaxed),
                    "streamingAttemptCount": streaming_attempt_count.load(Ordering::Relaxed),
                    "nonStreamingAttemptCount": non_streaming_attempt_count.load(Ordering::Relaxed),
                    "requestBytes": request_bytes,
                    "maxOutputTokens": max_output_tokens,
                    "maxRetries": max_retries,
                    "timeoutMs": timeout_ms,
                })
                .to_string(),
            );
            Err(error)
        }
    }
}

fn analysis_prompt(analysis_transcript: &str, adjustment: &str) -> String {
    [
        (!adjustment.trim().is_empty())
            .then(|| format!("用户对提纲的补充要求：\n{}", adjustment.trim())),
        Some(format!(
            "请分析以下对话转写并输出提纲 JSON：\n\n{analysis_transcript}"
        )),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn chunk_analysis_prompt(chunk: &ConversationChunk) -> String {
    format!(
        "预计输入：{} Token\n\n{}",
        chunk.estimated_tokens, chunk.source.excerpt
    )
}

fn sample_ledger_values(values: &[String], limit: usize, max_chars: usize) -> Vec<String> {
    if values.len() <= limit {
        return values
            .iter()
            .map(|value| value.chars().take(max_chars).collect())
            .collect();
    }
    if limit <= 1 {
        return values
            .last()
            .map(|value| vec![value.chars().take(max_chars).collect()])
            .unwrap_or_default();
    }
    (0..limit)
        .map(|position| position * (values.len() - 1) / (limit - 1))
        .map(|index| values[index].chars().take(max_chars).collect())
        .collect()
}

fn compact_ledger_for_planner(ledger: &DeepNoteLedger) -> DeepNoteLedger {
    DeepNoteLedger {
        note_goal: ledger.note_goal.chars().take(1_000).collect(),
        audience: ledger.audience.chars().take(500).collect(),
        canonical_terms: sample_ledger_values(&ledger.canonical_terms, 16, 80),
        verified_facts: sample_ledger_values(&ledger.verified_facts, 16, 180),
        evidence_claim_links: sample_ledger_values(&ledger.evidence_claim_links, 8, 160),
        covered_topics: sample_ledger_values(&ledger.covered_topics, 16, 80),
        open_questions: sample_ledger_values(&ledger.open_questions, 8, 140),
        conflicts: sample_ledger_values(&ledger.conflicts, 8, 140),
        ai_supplements: sample_ledger_values(&ledger.ai_supplements, 8, 140),
        section_summaries: sample_ledger_values(&ledger.section_summaries, 8, 360),
        global_constraints: sample_ledger_values(&ledger.global_constraints, 8, 140),
    }
}

fn compact_attachment_ledger(ledger: &DeepNoteLedger) -> DeepNoteLedger {
    DeepNoteLedger {
        note_goal: ledger.note_goal.chars().take(800).collect(),
        audience: ledger.audience.chars().take(400).collect(),
        canonical_terms: sample_ledger_values(&ledger.canonical_terms, 48, 120),
        verified_facts: sample_ledger_values(&ledger.verified_facts, 64, 280),
        evidence_claim_links: sample_ledger_values(&ledger.evidence_claim_links, 24, 180),
        covered_topics: sample_ledger_values(&ledger.covered_topics, 48, 160),
        open_questions: sample_ledger_values(&ledger.open_questions, 32, 220),
        conflicts: sample_ledger_values(&ledger.conflicts, 32, 220),
        ai_supplements: sample_ledger_values(&ledger.ai_supplements, 16, 200),
        section_summaries: sample_ledger_values(&ledger.section_summaries, 24, 420),
        global_constraints: sample_ledger_values(&ledger.global_constraints, 32, 220),
    }
}

fn compact_ledger_analysis_prompt(
    ledger: &DeepNoteLedger,
    budget: &DeepNoteContextBudget,
    adjustment: &str,
) -> Result<String, String> {
    let compact = compact_ledger_for_planner(ledger);
    let ledger_json = serde_json::to_string(&compact)
        .map_err(|error| format!("序列化精简知识账本失败：{error}"))?;
    let mut prompt = format!(
        "Fast outline request. Covered chunks: {}/{}; covered messages: {}/{}; coverageComplete={}.\nLedger JSON:\n{}",
        budget.processed_chunk_count,
        budget.chunk_count,
        budget.processed_message_count,
        budget.total_message_count,
        budget.coverage_complete,
        ledger_json
    );
    if !adjustment.trim().is_empty() {
        prompt.push_str(&format!(
            "\nUser outline adjustment:\n{}",
            adjustment.trim()
        ));
    }
    prompt.push_str(
        "\n\nGenerate 4 to 8 concise sections. Use only evidence retained in this ledger.",
    );
    Ok(prompt)
}

fn reserve_parallel_semantic_calls(
    state: &AppState,
    run_id: &str,
    runtime: &mut DeepNoteRuntimeState,
    calls: u32,
) -> Result<(), String> {
    if calls == 0 {
        return Ok(());
    }
    let next = runtime.budget.semantic_calls_used.saturating_add(calls);
    // 这里只记录逻辑调用的规划用量，provider 预算由每个物理 HTTP 请求发出前的
    // `try_append_note_pipeline_upstream_attempt` 原子扣减。并行 Worker 启动前仍做
    // 悲观记录，Worker 回收后归还未使用的 JSON 修复额度，便于诊断节点层放大。
    runtime.budget.semantic_calls_used = next;
    save_runtime_state(state, run_id, runtime)
}

fn release_unused_parallel_semantic_calls(
    runtime: &mut DeepNoteRuntimeState,
    reserved: u32,
    used: u32,
) {
    runtime.budget.semantic_calls_used = runtime
        .budget
        .semantic_calls_used
        .saturating_sub(reserved.saturating_sub(used));
}

async fn execute_chunk_digest_job(
    state: &AppState,
    run: &NotePipelineRun,
    job: ChunkDigestJob,
    cancellation: &CancellationToken,
    channel: Option<&Channel<NotePipelineProgress>>,
) -> ChunkDigestJobResult {
    let mut semantic_calls = 1u32;
    // 保留首个错误的 kind。JSON 修复只能修「回来了但格式不对」，修不了「没回来」：
    // 超时后再投一份同样大的载荷，只会再等一个 timeout_ms（Chunk 档位是 300s），
    // 白白多花一次语义调用额度。
    let mut initial_timed_out = false;
    let mut repair_timed_out = false;
    let initial = match model_call_with_runtime(
        state,
        run,
        "deepNoteChunk",
        NotePipelinePhase::Analyzing,
        job.system_prompt.clone(),
        job.user_prompt.clone(),
        run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
        run.retry_attempts,
        cancellation,
        channel,
    )
    .await
    {
        Ok(raw) => parse_json_object::<ChunkDigest>(&raw).and_then(ChunkDigest::validate),
        Err(error) => {
            initial_timed_out = is_timeout_like(error.kind);
            Err(error.message)
        }
    };

    let result = match initial {
        Ok(digest) => Ok(digest),
        Err(initial_error) if !initial_timed_out && !cancellation.is_cancelled() => {
            semantic_calls = semantic_calls.saturating_add(1);
            model_call_with_runtime(
                state,
                run,
                "deepNoteChunkRepair",
                NotePipelinePhase::Analyzing,
                format!("{}\n\n{}", job.system_prompt, STRICT_JSON_SUFFIX),
                job.user_prompt.clone(),
                run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
                run.retry_attempts,
                cancellation,
                channel,
            )
            .await
            .map_err(|error| {
                repair_timed_out = is_timeout_like(error.kind);
                format!("{initial_error}；JSON 修复失败：{}", error.message)
            })
            .and_then(|raw| parse_json_object::<ChunkDigest>(&raw).and_then(ChunkDigest::validate))
        }
        Err(error) => Err(error),
    };
    let timed_out = result.is_err() && (initial_timed_out || repair_timed_out);

    if let Ok(digest) = &result {
        let digest_json = serde_json::to_string(digest)
            .map_err(|error| format!("序列化 Chunk 摘要失败：{error}"));
        if let Err(error) = digest_json.and_then(|digest_json| {
            state.library_repository.save_note_pipeline_chunk_digest(
                &job.chunk.source.chunk_id,
                &job.chunk.source.content_hash,
                &job.prompt_hash,
                &run.provider_id,
                &run.model_id,
                &digest_json,
                semantic_calls,
            )
        }) {
            return ChunkDigestJobResult {
                index: job.index,
                chunk_id: job.chunk.source.chunk_id,
                prompt_hash: job.prompt_hash,
                semantic_calls,
                result: Err(error),
                // 落库失败与载荷大小无关，切小重投没有意义。
                timed_out: false,
            };
        }
    }

    ChunkDigestJobResult {
        index: job.index,
        chunk_id: job.chunk.source.chunk_id,
        prompt_hash: job.prompt_hash,
        semantic_calls,
        result,
        timed_out,
    }
}

/// 超时的分块低于这个字符数就不再切：切到几百字还超时，说明问题不在载荷大小，
/// 继续切只会把一次失败放大成很多次失败。
const MIN_SHRINKABLE_CHUNK_CHARS: usize = 2_000;

/// 把一个分块按字符数对半切开，尽量落在换行处，失败返回 `None`。
///
/// 两半都继承父块的 `message_ids`：按文本切会切断消息边界，而 `merge_chunk_digest`
/// 会用 `message_ids` 过滤模型报回的来源 ID——继承全集才不会误删证据。
fn split_chunk_in_half(
    chunk: &ConversationChunk,
) -> Option<(ConversationChunk, ConversationChunk)> {
    let excerpt = chunk.source.excerpt.as_str();
    if excerpt.chars().count() < MIN_SHRINKABLE_CHUNK_CHARS {
        return None;
    }
    let midpoint = excerpt
        .char_indices()
        .nth(excerpt.chars().count() / 2)
        .map(|(index, _)| index)?;
    // 优先切在换行，避免把一行对话劈成两半。找不到就用字符中点。
    let split_at = excerpt[..midpoint]
        .rfind('\n')
        .map(|index| index + 1)
        .filter(|index| *index > 0)
        .unwrap_or(midpoint);
    let (head, tail) = excerpt.split_at(split_at);
    if head.trim().is_empty() || tail.trim().is_empty() {
        return None;
    }
    let half = |text: &str, part: u8| {
        let excerpt = text.to_string();
        ConversationChunk {
            source: DeepNoteSourceChunk {
                chunk_id: content_addressed_chunk_id(&excerpt),
                content_hash: stable_hash(&excerpt),
                location: format!("{}（缩小重试 {}/2）", chunk.source.location, part),
                excerpt,
                ..chunk.source.clone()
            },
            message_ids: chunk.message_ids.clone(),
            // 按字符占比折算：只用于进度显示和预算估算，不参与正确性。
            estimated_tokens: chunk
                .estimated_tokens
                .saturating_mul(text.chars().count() as u64)
                .saturating_div((excerpt_chars(chunk)).max(1)),
        }
    };
    Some((half(head, 1), half(tail, 2)))
}

fn excerpt_chars(chunk: &ConversationChunk) -> u64 {
    chunk.source.excerpt.chars().count() as u64
}

/// 把两个半块的摘要并成一个，供原下标使用。
///
/// 只做字段级拼接：`merge_chunk_digest` 之后还会按父块的 `message_ids` 过滤并
/// 去重，所以这里不需要自己去重。
fn combine_chunk_digests(head: ChunkDigest, tail: ChunkDigest) -> ChunkDigest {
    let summary = match (
        head.summary.trim().is_empty(),
        tail.summary.trim().is_empty(),
    ) {
        (true, _) => tail.summary,
        (_, true) => head.summary,
        _ => format!("{}\n{}", head.summary, tail.summary),
    };
    let join = |mut first: Vec<String>, second: Vec<String>| {
        first.extend(second);
        first
    };
    ChunkDigest {
        summary,
        canonical_terms: join(head.canonical_terms, tail.canonical_terms),
        verified_facts: join(head.verified_facts, tail.verified_facts),
        covered_topics: join(head.covered_topics, tail.covered_topics),
        open_questions: join(head.open_questions, tail.open_questions),
        conflicts: join(head.conflicts, tail.conflicts),
        global_constraints: join(head.global_constraints, tail.global_constraints),
        source_message_ids: join(head.source_message_ids, tail.source_message_ids),
    }
}

/// 把超时的分块各切成两半重投一轮，返回真正救回来的原始下标。
///
/// 两半都成功才算救回：只拿到一半会让账本缺半段内容却自称完整，比继续报错更糟。
/// 每个半块按自己的内容哈希独立落检查点，所以即便这一轮又超时，下次运行仍能复用
/// 已经成功的那一半。
#[allow(clippy::too_many_arguments)]
async fn retry_timed_out_chunks_smaller(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    chunks: &[ConversationChunk],
    timed_out_indexes: &[usize],
    digests: &mut [Option<ChunkDigest>],
    cancellation: &CancellationToken,
    channel: &Channel<NotePipelineProgress>,
) -> Result<HashSet<usize>, String> {
    let mut halves = Vec::new();
    for index in timed_out_indexes {
        let Some(chunk) = chunks.get(*index) else {
            continue;
        };
        if let Some((head, tail)) = split_chunk_in_half(chunk) {
            halves.push((*index, head, tail));
        }
    }
    if halves.is_empty() {
        return Ok(HashSet::new());
    }
    let _ = state.library_repository.append_note_pipeline_event(
        &run.id,
        "chunkDigestShrinkRetry",
        Some("recon-source"),
        &serde_json::json!({
            "timedOutChunkCount": timed_out_indexes.len(),
            "shrinkableChunkCount": halves.len(),
            "reason": "upstreamTimeout",
        })
        .to_string(),
    );
    progress(
        state,
        channel,
        &run.id,
        NotePipelinePhase::Analyzing,
        None,
        None,
        format!("{} 个来源分块上游超时，正在切小重试", halves.len()),
    );

    // 每个半块两次调用（首投 + JSON 修复），两半共四次。
    let reserved = (halves.len() as u32).saturating_mul(4);
    reserve_parallel_semantic_calls(state, &run.id, runtime, reserved)?;
    let mut jobs = Vec::new();
    for (index, head, tail) in halves {
        for (part, chunk) in [(0usize, head), (1usize, tail)] {
            let user_prompt = chunk_analysis_prompt(&chunk);
            let system_prompt = system_prompt_with_skill_profile(
                state,
                &run.id,
                runtime,
                DeepNoteSkillProfileKind::Planner,
                Some("recon-source"),
                CHUNK_ANALYST_SYSTEM_PROMPT,
            );
            let prompt_hash = stable_hash(format!(
                "chunk-digest-v4\0{}\0{}\0{}\0{}\0{}",
                run.provider_id,
                run.model_id,
                run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
                system_prompt,
                user_prompt
            ));
            jobs.push((
                index,
                part,
                ChunkDigestJob {
                    // 这里的 index 只用于回填 halves 结果，重投期间不索引 digests。
                    index: jobs.len(),
                    chunk,
                    system_prompt,
                    user_prompt,
                    prompt_hash,
                },
            ));
        }
    }

    let parallelism = usize::from(runtime.budget.max_parallel_chunks.max(1));
    let mut collected = HashMap::<usize, [Option<ChunkDigest>; 2]>::new();
    let mut results = stream::iter(jobs.into_iter().map(|(index, part, job)| async move {
        let output = execute_chunk_digest_job(state, run, job, cancellation, Some(channel)).await;
        (index, part, output)
    }))
    .buffer_unordered(parallelism);
    while let Some((index, part, output)) = results.next().await {
        release_unused_parallel_semantic_calls(runtime, 2, output.semantic_calls);
        match output.result {
            Ok(digest) => {
                collected.entry(index).or_default()[part] = Some(digest);
            }
            Err(error) if error.starts_with(RUN_BUDGET_EXHAUSTED_PREFIX) => {
                drop(results);
                return Err(error);
            }
            // 切小之后仍失败：保留原始失败信息，不追加噪音。
            Err(_) => {}
        }
    }

    let mut recovered = HashSet::new();
    for (index, parts) in collected {
        let [Some(head), Some(tail)] = parts else {
            continue;
        };
        digests[index] = Some(combine_chunk_digests(head, tail));
        recovered.insert(index);
    }
    if !recovered.is_empty() {
        let _ = state.library_repository.append_note_pipeline_event(
            &run.id,
            "chunkDigestShrinkRecovered",
            Some("recon-source"),
            &serde_json::json!({
                "recoveredChunkCount": recovered.len(),
            })
            .to_string(),
        );
    }
    Ok(recovered)
}

#[allow(clippy::too_many_arguments)]
async fn build_chunked_ledger(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    conversation: &StoredConversation,
    target_tokens: u64,
    channel: &Channel<NotePipelineProgress>,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let chunks = all_source_chunks(
        state,
        run,
        runtime,
        conversation,
        target_tokens,
        cancellation,
    )
    .await?;
    if chunks.is_empty() {
        return Err("对话还没有可供深度笔记分析的来源内容。".to_string());
    }
    if chunks.len() > MAX_ANALYSIS_CHUNKS {
        return Err(format!(
            "当前对话需要拆分为 {} 个来源分块，超过单次深度笔记允许的 {} 个分块。请缩小会话范围后重试；系统不会静默丢弃内容。",
            chunks.len(),
            MAX_ANALYSIS_CHUNKS
        ));
    }
    {
        let _guard = state.library_operations.lock().await;
        state
            .library_repository
            .replace_note_pipeline_source_chunks(
                &run.id,
                &chunks
                    .iter()
                    .map(|chunk| chunk.source.clone())
                    .collect::<Vec<_>>(),
            )?;
    }
    // Chunk 的完成事实来自独立 Digest 检查点，不再依赖“前 N 个已完成”的
    // 顺序前缀。这样并行 Worker 可以乱序返回，恢复时也只重做真正缺失的 Chunk。
    runtime.ledger = DeepNoteLedger::default();
    runtime.context_budget.processed_chunk_count = 0;
    runtime.context_budget.processed_message_count = 0;
    runtime.context_budget.chunk_target_tokens = target_tokens;
    runtime.context_budget.chunk_count = chunks.len();
    runtime.context_budget.coverage_complete = false;
    runtime.context_budget.omitted_message_ids.clear();
    let mut candidates = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.iter().cloned().enumerate() {
        let user_prompt = chunk_analysis_prompt(&chunk);
        let system_prompt = system_prompt_with_skill_profile(
            state,
            &run.id,
            runtime,
            DeepNoteSkillProfileKind::Planner,
            Some("recon-source"),
            CHUNK_ANALYST_SYSTEM_PROMPT,
        );
        let prompt_hash = stable_hash(format!(
            "chunk-digest-v4\0{}\0{}\0{}\0{}\0{}",
            run.provider_id,
            run.model_id,
            run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
            system_prompt,
            user_prompt
        ));
        candidates.push(ChunkDigestJob {
            index,
            chunk,
            system_prompt,
            user_prompt,
            prompt_hash,
        });
    }
    let cache_keys = candidates
        .iter()
        .map(|job| {
            (
                job.chunk.source.content_hash.clone(),
                job.prompt_hash.clone(),
            )
        })
        .collect::<Vec<_>>();
    let cached = state
        .library_repository
        .find_note_pipeline_chunk_digests(&cache_keys, &run.provider_id, &run.model_id)?
        .into_iter()
        .map(|checkpoint| {
            (
                (
                    checkpoint.content_hash.clone(),
                    checkpoint.prompt_hash.clone(),
                ),
                checkpoint,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut digests = vec![None::<ChunkDigest>; chunks.len()];
    let mut jobs = Vec::new();
    let mut pending_by_cache_key = HashMap::<(String, String), usize>::new();
    let mut duplicate_indexes = HashMap::<usize, Vec<usize>>::new();
    for job in candidates {
        let cache_key = (
            job.chunk.source.content_hash.clone(),
            job.prompt_hash.clone(),
        );
        let restored = cached.get(&cache_key).and_then(|checkpoint| {
            (checkpoint.content_hash == job.chunk.source.content_hash
                && checkpoint.prompt_hash == job.prompt_hash
                && checkpoint.provider_id == run.provider_id
                && checkpoint.model_id == run.model_id)
                .then(|| {
                    parse_json_object::<ChunkDigest>(&checkpoint.digest_json)
                        .and_then(ChunkDigest::validate)
                        .ok()
                })
                .flatten()
        });
        if let Some(digest) = restored {
            digests[job.index] = Some(digest);
        } else if let Some(representative_index) = pending_by_cache_key.get(&cache_key) {
            duplicate_indexes
                .entry(*representative_index)
                .or_default()
                .push(job.index);
        } else {
            pending_by_cache_key.insert(cache_key, job.index);
            jobs.push(job);
        }
    }

    let cached_count = digests.iter().filter(|digest| digest.is_some()).count();
    if cached.len() > cached_count {
        let _ = state.library_repository.append_note_pipeline_event(
            &run.id,
            "chunkDigestCacheEvaluated",
            Some("recon-source"),
            &serde_json::json!({
                "policyVersion": "chunk-digest-v4",
                "candidateCheckpointCount": cached.len(),
                "reusedCheckpointCount": cached_count,
                "invalidatedCheckpointCount": cached.len().saturating_sub(cached_count),
                "possibleReasons": ["adaptiveAttachmentPacking", "content", "prompt", "model"],
            })
            .to_string(),
        );
    }
    let reserved_calls = (jobs.len() as u32).saturating_mul(2);
    reserve_parallel_semantic_calls(state, &run.id, runtime, reserved_calls)?;
    let parallelism = usize::from(runtime.budget.max_parallel_chunks.max(1));
    progress(
        state,
        channel,
        &run.id,
        NotePipelinePhase::Analyzing,
        Some(cached_count),
        Some(chunks.len()),
        format!(
            "正在并行提取来源分块 · 并发 {} · 已复用 {} 个检查点",
            parallelism, cached_count
        ),
    );

    let mut failures = Vec::<(usize, String)>::new();
    let mut timed_out_indexes = Vec::<usize>::new();
    let mut run_budget_error = None;
    let mut results = stream::iter(
        jobs.into_iter()
            .map(|job| execute_chunk_digest_job(state, run, job, cancellation, Some(channel))),
    )
    .buffer_unordered(parallelism);
    while let Some(output) = results.next().await {
        release_unused_parallel_semantic_calls(runtime, 2, output.semantic_calls);
        match output.result {
            Ok(digest) => {
                digests[output.index] = Some(digest.clone());
                if let Some(indexes) = duplicate_indexes.get(&output.index) {
                    for index in indexes {
                        digests[*index] = Some(digest.clone());
                    }
                }
                let completed = digests.iter().filter(|digest| digest.is_some()).count();
                progress(
                    state,
                    channel,
                    &run.id,
                    NotePipelinePhase::Analyzing,
                    Some(completed),
                    Some(chunks.len()),
                    format!("来源分块已完成 {completed}/{}", chunks.len()),
                );
                let _ = state.library_repository.append_note_pipeline_event(
                    &run.id,
                    "contextChunkCompleted",
                    Some(&output.chunk_id),
                    &serde_json::json!({
                        "chunkIndex": output.index + 1,
                        "chunkCount": chunks.len(),
                        "completedChunkCount": completed,
                        "parallelism": parallelism,
                        "promptHash": output.prompt_hash,
                    })
                    .to_string(),
                );
            }
            Err(error) if error.starts_with(RUN_BUDGET_EXHAUSTED_PREFIX) => {
                run_budget_error = Some(error);
                break;
            }
            Err(error) => {
                if output.timed_out {
                    // 先记下来，等这一轮全部落地再统一缩小重投：此刻并发流还在跑，
                    // 立即重投会和剩余任务抢并发额度和上游配额。
                    timed_out_indexes.push(output.index);
                }
                failures.push((output.index, format!("{}：{}", output.chunk_id, error)));
            }
        }
        runtime.context_budget.processed_chunk_count =
            digests.iter().filter(|digest| digest.is_some()).count();
        let processed_ids = chunks
            .iter()
            .enumerate()
            .filter(|(index, _)| digests[*index].is_some())
            .flat_map(|(_, chunk)| chunk.message_ids.iter().cloned())
            .collect::<HashSet<_>>();
        runtime.context_budget.processed_message_count = processed_ids.len();
        save_runtime_state(state, &run.id, runtime)?;
    }
    drop(results);
    if let Some(error) = run_budget_error {
        return Err(error);
    }
    // 兑现 `should_retry_note_model_call` 那半个承诺：超时被否决 HTTP 层重试后，
    // 由管线把载荷切小再投。只做一轮、只切一半、只针对超时——不是为了穷尽重试，
    // 而是不让一次网关 504 把其余十几块已完成的工作连坐成整次失败。
    if !timed_out_indexes.is_empty() && !cancellation.is_cancelled() {
        // 这一轮是补救，不是主路径：除了预算耗尽这类终态错误，其余失败一律咽下，
        // 让下面报出原始的分块失败原因。否则补救过程中的次生错误会顶替掉真正有
        // 诊断价值的那一条。
        let recovered = match retry_timed_out_chunks_smaller(
            state,
            run,
            runtime,
            &chunks,
            &timed_out_indexes,
            &mut digests,
            cancellation,
            channel,
        )
        .await
        {
            Ok(recovered) => recovered,
            Err(error) if error.starts_with(RUN_BUDGET_EXHAUSTED_PREFIX) => return Err(error),
            Err(_) => HashSet::new(),
        };
        if !recovered.is_empty() {
            failures.retain(|(index, _)| !recovered.contains(index));
            let processed_ids = chunks
                .iter()
                .enumerate()
                .filter(|(index, _)| digests[*index].is_some())
                .flat_map(|(_, chunk)| chunk.message_ids.iter().cloned())
                .collect::<HashSet<_>>();
            runtime.context_budget.processed_chunk_count =
                digests.iter().filter(|digest| digest.is_some()).count();
            runtime.context_budget.processed_message_count = processed_ids.len();
            save_runtime_state(state, &run.id, runtime)?;
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "{} 个来源分块生成失败；已完成分块的检查点已保留：{}",
            failures.len(),
            failures
                .into_iter()
                .map(|(_, message)| message)
                .collect::<Vec<_>>()
                .join("；")
        ));
    }

    runtime.ledger = DeepNoteLedger::default();
    let mut processed_ids = HashSet::new();
    for (index, digest) in digests.into_iter().enumerate() {
        let digest = digest.ok_or_else(|| {
            format!(
                "来源分块 {} 缺少完成的 Digest 检查点。",
                chunks[index].source.chunk_id
            )
        })?;
        merge_chunk_digest(&mut runtime.ledger, &chunks[index], digest);
        processed_ids.extend(chunks[index].message_ids.iter().cloned());
    }
    runtime.context_budget.processed_chunk_count = chunks.len();
    runtime.context_budget.processed_message_count = processed_ids.len();
    runtime.context_budget.coverage_complete = runtime.context_budget.processed_chunk_count
        == chunks.len()
        && processed_ids.len() == runtime.context_budget.total_message_count;
    if !runtime.context_budget.coverage_complete {
        let all_ids = noteworthy_messages(conversation)
            .into_iter()
            .map(|message| message.id.clone())
            .collect::<HashSet<_>>();
        runtime.context_budget.omitted_message_ids =
            all_ids.difference(&processed_ids).cloned().collect();
        runtime.context_budget.omitted_message_ids.sort();
        return Err(format!(
            "来源覆盖不完整：仍有 {} 条消息未处理。系统不会在不完整覆盖下生成完整笔记。",
            runtime.context_budget.omitted_message_ids.len()
        ));
    }
    let _ = state.library_repository.append_note_pipeline_event(
        &run.id,
        "contextCoverageCompleted",
        None,
        &serde_json::json!({
            "mode": "chunked",
            "processedMessageCount": runtime.context_budget.processed_message_count,
            "totalMessageCount": runtime.context_budget.total_message_count,
            "processedChunkCount": runtime.context_budget.processed_chunk_count,
            "chunkCount": runtime.context_budget.chunk_count,
        })
        .to_string(),
    );
    save_runtime_state(state, &run.id, runtime)
}

fn selected_message_transcript(
    conversation: &StoredConversation,
    message_ids: &HashSet<String>,
) -> String {
    noteworthy_messages(conversation)
        .into_iter()
        .filter(|message| message_ids.contains(&message.id))
        .map(|message| {
            format!(
                "<!-- message-id: {} -->\n{}",
                message.id,
                message_text(message, false)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn section_source_context(
    conversation: &StoredConversation,
    section: &DeepNoteSection,
    ledger: &DeepNoteLedger,
    budget: &DeepNoteContextBudget,
) -> Result<(String, bool), String> {
    let selected_ids = section
        .source_message_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let raw = if selected_ids.is_empty() {
        transcript(conversation, false)
    } else {
        selected_message_transcript(conversation, &selected_ids)
    };
    let raw_limit = budget
        .usable_input_tokens
        .saturating_sub(6_000)
        .min(SECTION_SOURCE_TOKEN_LIMIT)
        .max(2_048);
    let selected_has_attachments = noteworthy_messages(conversation)
        .into_iter()
        .any(|message| {
            (selected_ids.is_empty() || selected_ids.contains(&message.id))
                && !message.attachments.is_empty()
        });
    if !selected_has_attachments
        && !raw.trim().is_empty()
        && estimate_text_tokens(&raw) <= raw_limit
    {
        return Ok((raw, false));
    }
    let summaries = ledger
        .section_summaries
        .iter()
        .filter(|summary| {
            selected_ids.is_empty()
                || selected_ids
                    .iter()
                    .any(|message_id| summary.contains(message_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    if summaries.is_empty() {
        return Err(format!(
            "章节“{}”没有可加载的来源消息或分块账本。请返回计划并补充来源范围。",
            section.heading
        ));
    }
    Ok((
        format!(
            "本章使用已完成且可追溯的来源分块账本；当消息包含附件时，账本同时包含附件 Reader 或视觉分析产物，来源消息 ID 保留在每条摘要中。\n\n{}",
            summaries.join("\n\n")
        ),
        true,
    ))
}

fn section_prompt(
    outline: &DeepNoteOutline,
    section: &DeepNoteSection,
    source_context: &str,
    ledger_context: &str,
    dependency_outputs: &str,
) -> Result<String, String> {
    Ok(format!(
        "全局标题：{}\n全局概览：{}\n薄弱点：{}\n隐藏问题：{}\n知识缺口：{}\n需要辨析的误解：{}\n因果链：{}\n图形机会：{}\n全部章节：\n{}\n\n当前章节：{}\n{}\n\n全局知识账本：\n{}\n\n当前章节来源：\n{}",
        outline.title,
        outline.summary,
        outline.weak_points.join("；"),
        outline.hidden_questions.join("；"),
        outline.knowledge_gaps.join("；"),
        outline.misconceptions.join("；"),
        outline.causal_chains.join("；"),
        outline.visualization_opportunities.join("；"),
        outline
            .sections
            .iter()
            .map(|item| format!("- {} {} ({:?})", item.id, item.heading, item.kind))
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::to_string(section).map_err(|error| error.to_string())?,
        if dependency_outputs.is_empty() {
            String::new()
        } else {
            format!("已完成依赖章节产物：\n{dependency_outputs}")
        },
        ledger_context,
        source_context,
    ))
}

fn assemble(
    outline: &DeepNoteOutline,
    sections: &[(DeepNoteSection, String, bool)],
    draft: bool,
) -> (String, String, Vec<String>) {
    let title = if draft {
        format!("{}（草稿）", outline.title)
    } else {
        outline.title.clone()
    };
    let mut warnings = Vec::new();
    let mut body = Vec::new();
    for (section, markdown, failed) in sections {
        if *failed {
            warnings.push(format!("章节“{}”生成失败。", section.heading));
        }
        let sources = [
            if section.source_message_ids.is_empty() {
                "源自本次对话".to_string()
            } else {
                format!(
                    "源自本次对话（{} 个消息锚点）",
                    section.source_message_ids.len()
                )
            },
            section
                .needs_supplement
                .then(|| "AI 补充背景".to_string())
                .unwrap_or_default(),
        ]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("；");
        body.push(format!(
            "{}\n\n> 来源：{}",
            normalize_generated_markdown(markdown),
            sources
        ));
    }
    let content = [
        format!("# {title}"),
        outline.summary.trim().to_string(),
        body.join("\n\n"),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n");
    let self_checks = content
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("- ")
                || line
                    .chars()
                    .next()
                    .is_some_and(|value| value.is_ascii_digit())
        })
        .count();
    if self_checks < 3 {
        warnings.push(format!(
            "自检问题可能不足 3 题（检测到 {self_checks} 条列表项）。"
        ));
    }
    (title, content, warnings)
}

/// Convert legacy fenced math (`math`, `latex`, or `tex`) into the canonical
/// Markdown math form consumed by remark-math/KaTeX. Unclosed fences are left
/// untouched so validation can report the original malformed output.
fn normalize_math_fences(markdown: &str) -> String {
    let mut output = String::with_capacity(markdown.len());
    let mut pending: Option<Vec<String>> = None;

    for raw_line in markdown.split_inclusive('\n') {
        let (line, ending) = if let Some(line) = raw_line.strip_suffix("\r\n") {
            (line, "\r\n")
        } else if let Some(line) = raw_line.strip_suffix('\n') {
            (line, "\n")
        } else {
            (raw_line, "")
        };

        if let Some(lines) = pending.as_mut() {
            if line.trim() == "```" {
                let inner_ending = if ending.is_empty() {
                    if lines.iter().skip(1).any(|value| value.ends_with("\r\n")) {
                        "\r\n"
                    } else if lines.iter().skip(1).any(|value| value.ends_with('\n')) {
                        "\n"
                    } else {
                        ""
                    }
                } else {
                    ending
                };
                output.push_str("$$");
                output.push_str(inner_ending);
                for inner in lines.iter().skip(1) {
                    output.push_str(inner);
                }
                if !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str("$$");
                output.push_str(ending);
                pending = None;
            } else {
                lines.push(raw_line.to_string());
            }
            continue;
        }

        let trimmed = line.trim();
        let is_math_open = trimmed
            .strip_prefix("```")
            .map(|language| {
                matches!(
                    language.trim().to_ascii_lowercase().as_str(),
                    "math" | "latex" | "tex"
                )
            })
            .unwrap_or(false);
        if is_math_open {
            pending = Some(vec![raw_line.to_string()]);
        } else {
            output.push_str(raw_line);
        }
    }

    if let Some(lines) = pending {
        for line in lines {
            output.push_str(&line);
        }
    }
    output
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MarkdownFenceAnalysis {
    top_level_mermaid_blocks: usize,
    nested_mermaid_markers: usize,
    unclosed_fence: bool,
}

fn parse_fence_line(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    if line.len().saturating_sub(trimmed.len()) > 3 {
        return None;
    }
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = trimmed
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if length < 3 {
        return None;
    }
    Some((marker, length, &trimmed[length..]))
}

fn fence_language(info: &str) -> String {
    info.trim()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_markdown_source_language(language: &str) -> bool {
    matches!(
        language,
        "markdown" | "md" | "mdown" | "mkdown" | "text" | "plaintext" | "txt"
    )
}

fn line_starts_mermaid_fence(line: &str) -> bool {
    parse_fence_line(line).is_some_and(|(_, _, info)| fence_language(info) == "mermaid")
}

fn analyze_markdown_fences(markdown: &str) -> MarkdownFenceAnalysis {
    let mut analysis = MarkdownFenceAnalysis::default();
    let mut active: Option<(char, usize, String)> = None;
    for line in markdown.lines() {
        if let Some((marker, length, info)) = active.as_ref() {
            if line_starts_mermaid_fence(line) && is_markdown_source_language(info) {
                analysis.nested_mermaid_markers += 1;
            }
            if parse_fence_line(line).is_some_and(|(candidate, candidate_length, suffix)| {
                candidate == *marker && candidate_length >= *length && suffix.trim().is_empty()
            }) {
                active = None;
            }
            continue;
        }
        let Some((marker, length, info)) = parse_fence_line(line) else {
            continue;
        };
        let language = fence_language(info);
        if language == "mermaid" {
            analysis.top_level_mermaid_blocks += 1;
        }
        active = Some((marker, length, language));
    }
    analysis.unclosed_fence = active.is_some();
    analysis
}

/// 收集正文顶层的 ```mermaid 代码块内容。
///
/// `analyze_markdown_fences` 只统计数量，语法检查需要块内的正文，所以单独走
/// 一遍。嵌套在 markdown/text 源码围栏里的块不收——那种情况已经由
/// `nested_mermaid_markers` 单独报错，重复报没有意义。
fn collect_top_level_mermaid_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut active: Option<(char, usize, bool, Vec<&str>)> = None;
    for line in markdown.lines() {
        if let Some((marker, length, is_mermaid, body)) = active.as_mut() {
            if parse_fence_line(line).is_some_and(|(candidate, candidate_length, suffix)| {
                candidate == *marker && candidate_length >= *length && suffix.trim().is_empty()
            }) {
                if *is_mermaid {
                    blocks.push(body.join("\n"));
                }
                active = None;
            } else {
                body.push(line);
            }
            continue;
        }
        let Some((marker, length, info)) = parse_fence_line(line) else {
            continue;
        };
        active = Some((
            marker,
            length,
            fence_language(info) == "mermaid",
            Vec::new(),
        ));
    }
    blocks
}

/// Mermaid 图型是否使用 `ID[标签]` 这种方括号节点写法。
fn is_flowchart_block(code: &str) -> bool {
    code.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .is_some_and(|line| line.starts_with("flowchart") || line.starts_with("graph"))
}

/// 这些行首关键字后面的 `[` 不是节点标签。
fn is_mermaid_directive_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    [
        "%%",
        "click ",
        "style ",
        "classDef ",
        "class ",
        "linkStyle ",
        "direction ",
        "subgraph ",
        "accTitle",
        "accDescr",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

/// flowchart 的 `[标签]` 里是否存在会让词法分析失败的裸括号。
///
/// 判定与前端 `mermaidRepair.ts` 的 `quoteBracketLabels` 保持一致：只看单字符
/// `[` 开头的形状，标签已被引号包裹或含裸引号时放过。
fn flowchart_label_needs_quotes(line: &str) -> bool {
    if is_mermaid_directive_line(line) {
        return false;
    }
    let bytes = line.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = line[cursor..].find('[') {
        let open = cursor + offset;
        // `[[`、`[(`、`[/`、`[\` 是别的节点形状，闭合符号不同。
        if matches!(bytes.get(open + 1), Some(b'[' | b'(' | b'/' | b'\\')) {
            cursor = open + 2;
            continue;
        }
        let Some(close_offset) = line[open + 1..].find(']') else {
            return false;
        };
        let close = open + 1 + close_offset;
        let label = &line[open + 1..close];
        let trimmed = label.trim();
        let already_quoted =
            trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"');
        if !already_quoted && !label.contains('"') && label.contains(['(', ')']) {
            return true;
        }
        cursor = close + 1;
    }
    false
}

/// 边标签的竖线闭合之后紧跟引号：`A -->|"说明"|" Chat`。
fn edge_label_has_trailing_quote(line: &str) -> bool {
    let mut rest = line;
    // 找到成对竖线中的第二根，看它后面（跳过空格）是不是引号。
    while let Some(first) = rest.find('|') {
        let after_first = &rest[first + 1..];
        let Some(second) = after_first.find('|') else {
            return false;
        };
        let tail = after_first[second + 1..].trim_start();
        if tail.starts_with('"') {
            return true;
        }
        rest = &after_first[second + 1..];
    }
    false
}

/// erDiagram 属性行的复合键连写：`text run_id PK_FK`。
///
/// mermaid 只接受逗号分隔的多个键，`PK_FK` 和 `PK FK` 都实测解析失败。
fn er_attribute_has_compound_key(line: &str) -> bool {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 3 {
        return false;
    }
    let is_key = |token: &str| matches!(token, "PK" | "FK" | "UK");
    // 第三段起是键位，注释（"..."）之前的部分才算。
    let keys = fields[2..]
        .iter()
        .take_while(|token| !token.starts_with('"'))
        .collect::<Vec<_>>();
    let compound = keys.iter().any(|token| {
        token
            .split_once('_')
            .is_some_and(|(head, tail)| is_key(head) && is_key(tail))
    });
    // `PK FK` 空格连写同样解析失败，逗号是唯一合法分隔符。
    let space_separated = keys.len() >= 2 && keys.iter().all(|token| is_key(token));
    compound || space_separated
}

/// mermaid 语法警告的统一前缀。
///
/// `passed` 只看 errors，所以光把笔误塞进 warnings 等于没人读。修订循环靠这个
/// 前缀把「值得让模型再改一轮」的警告从其它警告里认出来。
const MERMAID_LINT_WARNING_PREFIX: &str = "Mermaid 语法：";

/// 对单个 mermaid 块做保守语法检查。
///
/// 每条规则都由 `scripts/probe-mermaid-errors.mjs` 用真实解析器确认过：命中
/// 即必然渲染失败。刻意不做启发式猜测——误报会白白烧掉章节的修订预算，漏报
/// 只是维持现状。误报率由 `mermaid_lint_has_no_false_positives` 在本机语料上
/// 实测，当前为 0。
fn lint_mermaid_syntax(code: &str) -> Vec<String> {
    let mut findings = Vec::new();
    if is_flowchart_block(code) {
        if code.lines().any(flowchart_label_needs_quotes) {
            findings.push(format!(
                "{MERMAID_LINT_WARNING_PREFIX}节点标签含半角圆括号却没有加引号，会被解析成圆角节点而报错；请写成 A[\"主机 (Host)\"]。"
            ));
        }
        if code.lines().any(edge_label_has_trailing_quote) {
            findings.push(format!(
                "{MERMAID_LINT_WARNING_PREFIX}边标签的竖线闭合后多了一个引号；请写成 A -->|\"说明\"| B，箭头右侧必须是不含空格的节点 ID。"
            ));
        }
    } else if code.trim_start().starts_with("erDiagram")
        && code.lines().any(er_attribute_has_compound_key)
    {
        findings.push(format!(
            "{MERMAID_LINT_WARNING_PREFIX}erDiagram 的复合键必须用英文逗号分隔，写成 PK, FK，不能写 PK_FK 或 PK FK。"
        ));
    }
    findings
}

/// 验证报告里是否存在值得再让模型改一轮的 mermaid 笔误。
///
/// 单独判定而不是并进 `passed`：`passed == false` 会在修订次数耗尽时把整章判
/// 失败、直接中断整次运行，而一处图表笔误不该有这种杀伤力——前端渲染前还有
/// 一层等价的确定性修复兜底。所以这里只驱动「再改一轮」，不参与成败。
fn has_mermaid_lint_warning(validation: &DeepNoteValidationReport) -> bool {
    validation
        .warnings
        .iter()
        .any(|warning| warning.starts_with(MERMAID_LINT_WARNING_PREFIX))
}

fn strip_outer_markdown_fence(markdown: &str) -> Option<String> {
    let lines = markdown.lines().collect::<Vec<_>>();
    let first = lines.iter().position(|line| !line.trim().is_empty())?;
    let last = lines.iter().rposition(|line| !line.trim().is_empty())?;
    let (marker, length, info) = parse_fence_line(lines[first])?;
    if !is_markdown_source_language(&fence_language(info)) {
        return None;
    }
    if !parse_fence_line(lines[last]).is_some_and(|(candidate, candidate_length, suffix)| {
        candidate == marker && candidate_length >= length && suffix.trim().is_empty()
    }) {
        return None;
    }
    let inner = lines[first + 1..last].join("\n");
    let inner_trimmed = inner.trim();
    if !inner_trimmed.starts_with('#') {
        return None;
    }
    Some(inner_trimmed.to_string())
}

fn normalize_generated_markdown(markdown: &str) -> String {
    let normalized = normalize_math_fences(markdown.trim());
    strip_outer_markdown_fence(&normalized).unwrap_or(normalized)
}

fn validate_section_markdown(
    section: &DeepNoteSection,
    markdown: &str,
    checked_evidence_ids: &[String],
) -> DeepNoteValidationReport {
    let normalized = markdown.trim();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    if normalized.is_empty() {
        errors.push("章节正文为空。".to_string());
    }
    let expected_heading = format!("## {}", section.heading);
    if !normalized
        .lines()
        .next()
        .is_some_and(|line| line.trim() == expected_heading)
    {
        errors.push(format!("章节必须以“{expected_heading}”开头。"));
    }
    let body_chars = normalized.chars().count();
    if body_chars < 180 {
        errors.push(format!("章节明显过短（{body_chars} 字符）。"));
    }
    if normalized.lines().any(|line| line.starts_with("# ")) {
        errors.push("章节正文不能包含全文一级标题。".to_string());
    }
    if section.allow_ai_supplement || section.needs_supplement {
        if !normalized.contains("AI 补充背景") {
            errors.push("计划允许或要求 AI 补充，但正文没有明确标记“AI 补充背景”。".to_string());
        }
    } else if normalized.contains("AI 补充背景") {
        warnings.push("正文包含计划未声明的 AI 补充标记。".to_string());
    }
    if normalized.contains("[本章生成失败") {
        errors.push("失败占位文本不能通过章节验证。".to_string());
    }
    let criteria_coverage = section
        .success_criteria
        .iter()
        .map(|criterion| {
            let keywords = criterion
                .split(|character: char| {
                    character.is_whitespace() || "，。；：、".contains(character)
                })
                .filter(|value| value.chars().count() >= 2)
                .take(4)
                .collect::<Vec<_>>();
            let covered = keywords.iter().any(|keyword| normalized.contains(keyword));
            if !covered {
                warnings.push(format!("成功标准可能未被明确覆盖：{criterion}"));
            }
            format!(
                "{}:{}",
                if covered { "covered" } else { "uncertain" },
                criterion
            )
        })
        .collect::<Vec<_>>();
    let fences = analyze_markdown_fences(normalized);
    let mermaid_blocks = fences.top_level_mermaid_blocks;
    if fences.unclosed_fence {
        errors.push("Markdown 代码块没有正确闭合。".to_string());
    }
    if fences.nested_mermaid_markers > 0 {
        errors.push(
            "Mermaid 被包在 Markdown/文本源码代码块中，前端只会显示源码；请移除外层源码围栏，让 ```mermaid 位于正文顶层。"
                .to_string(),
        );
    }
    for forbidden in ["click ", "javascript:", "<iframe", "<script", "<img"] {
        if normalized.to_ascii_lowercase().contains(forbidden) {
            errors.push(format!("图形或 Markdown 包含不安全内容：{forbidden}"));
        }
    }
    if mermaid_blocks > 0
        && ![
            "flowchart",
            "sequenceDiagram",
            "stateDiagram",
            "classDiagram",
            "erDiagram",
            "timeline",
            "gantt",
            "mindmap",
            "journey",
            "requirementDiagram",
            "xychart-beta",
            "pie",
            "gitGraph",
        ]
        .iter()
        .any(|kind| normalized.contains(kind))
    {
        warnings.push("检测到 Mermaid 代码块，但未识别到受支持的图型关键字。".to_string());
    }
    // 语法笔误进 warnings 而不是 errors：命中即渲染失败，值得让模型改一轮
    // （见 has_mermaid_lint_warning），但不该在修订额度耗尽时把整章判失败——
    // 前端渲染前还有一层等价的确定性修复兜底，丢掉整章更糟。
    for block in collect_top_level_mermaid_blocks(normalized) {
        warnings.extend(lint_mermaid_syntax(&block));
    }
    let relation_heavy = matches!(
        section.kind,
        super::types::DeepNoteSectionKind::Prerequisite
            | super::types::DeepNoteSectionKind::Comparison
            | super::types::DeepNoteSectionKind::Example
    ) || section.brief.contains("流程")
        || section.brief.contains("层次")
        || section.brief.contains("依赖")
        || section.brief.contains("状态")
        || section.brief.contains("时序");
    if relation_heavy && mermaid_blocks == 0 {
        warnings
            .push("本章包含明显的流程、层次或依赖关系，建议用 Mermaid 图辅助理解。".to_string());
    }
    DeepNoteValidationReport {
        passed: errors.is_empty(),
        errors,
        warnings,
        checked_evidence_ids: checked_evidence_ids.to_vec(),
        criteria_coverage,
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepNoteGlobalValidationReport {
    passed: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
    section_hashes: HashMap<String, String>,
}

fn validate_global_drafts(
    outline: &DeepNoteOutline,
    drafts: &[(DeepNoteSection, String, bool)],
    evidence_by_section: &HashMap<String, Vec<String>>,
) -> DeepNoteGlobalValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut section_hashes = HashMap::new();
    let expected = outline
        .sections
        .iter()
        .map(|section| section.id.as_str())
        .collect::<HashSet<_>>();
    let actual = drafts
        .iter()
        .map(|(section, _, _)| section.id.as_str())
        .collect::<HashSet<_>>();
    for missing in expected.difference(&actual) {
        errors.push(format!("缺少已确认章节：{missing}"));
    }
    for unexpected in actual.difference(&expected) {
        errors.push(format!("出现计划外章节：{unexpected}"));
    }

    let mut headings = HashSet::new();
    let mut content_hashes = HashMap::<String, String>::new();
    for (section, markdown, failed) in drafts {
        if *failed {
            errors.push(format!("章节“{}”仍处于失败状态。", section.heading));
        }
        let normalized_heading = section.heading.trim().to_lowercase();
        if !headings.insert(normalized_heading) {
            errors.push(format!("章节标题重复：{}", section.heading));
        }
        let evidence_ids = evidence_by_section
            .get(&section.id)
            .cloned()
            .unwrap_or_default();
        let report = validate_section_markdown(section, markdown, &evidence_ids);
        errors.extend(
            report
                .errors
                .into_iter()
                .map(|error| format!("{}：{error}", section.heading)),
        );
        warnings.extend(
            report
                .warnings
                .into_iter()
                .map(|warning| format!("{}：{warning}", section.heading)),
        );
        if evidence_ids.is_empty() && !section.allow_ai_supplement && !section.needs_supplement {
            errors.push(format!("章节“{}”没有可验证 Evidence。", section.heading));
        }
        for dependency in &section.depends_on {
            if !actual.contains(dependency.as_str()) {
                errors.push(format!(
                    "章节“{}”缺少依赖章节 {dependency} 的完成产物。",
                    section.heading
                ));
            }
        }
        let content_hash = stable_hash(markdown);
        if let Some(previous) = content_hashes.insert(content_hash.clone(), section.id.clone()) {
            errors.push(format!(
                "章节 {} 与 {} 的正文完全重复。",
                previous, section.id
            ));
        }
        section_hashes.insert(section.id.clone(), content_hash);
    }
    DeepNoteGlobalValidationReport {
        passed: errors.is_empty(),
        errors,
        warnings,
        section_hashes,
    }
}

fn sidecar_json(
    run: &NotePipelineRun,
    plan: &DeepNotePlanVersion,
    sections: &[(DeepNoteSection, String, bool)],
    evidence_by_section: &HashMap<String, Vec<String>>,
) -> Result<String, String> {
    serde_json::to_string(&serde_json::json!({
        "schemaVersion": 1,
        "runId": run.id,
        "planId": plan.plan_id,
        "planVersion": plan.version,
        "inputSnapshotHash": run.input_snapshot_hash,
        "model": {
            "providerId": run.provider_id,
            "modelId": run.model_id,
        },
        "budget": serde_json::from_str::<serde_json::Value>(&run.budget_json)
            .unwrap_or(serde_json::Value::Null),
        "sections": sections.iter().map(|(section, markdown, failed)| serde_json::json!({
            "sectionId": section.id,
            "heading": section.heading,
            "dependsOn": section.depends_on,
            "sourceMessageIds": section.source_message_ids,
            "evidenceIds": evidence_by_section.get(&section.id).cloned().unwrap_or_default(),
            "contentHash": stable_hash(markdown),
            "validated": !failed,
            "aiSupplement": section.allow_ai_supplement || section.needs_supplement,
        })).collect::<Vec<_>>(),
    }))
    .map_err(|error| format!("序列化深度笔记 Sidecar 失败：{error}"))
}

fn note_sources(
    conversation_id: &str,
    last_message_id: Option<&str>,
    sections: &[(DeepNoteSection, String, bool)],
) -> Vec<NoteSourceCreate> {
    sections
        .iter()
        .flat_map(|(section, _, _)| {
            let mut sources = if section.source_message_ids.is_empty() {
                vec![NoteSourceCreate {
                    section_id: section.id.clone(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some(conversation_id.to_string()),
                    message_id: None,
                    summarized_until_message_id: last_message_id.map(str::to_string),
                }]
            } else {
                section
                    .source_message_ids
                    .iter()
                    .map(|message_id| NoteSourceCreate {
                        section_id: section.id.clone(),
                        origin: NoteSourceOrigin::Conversation,
                        conversation_id: Some(conversation_id.to_string()),
                        message_id: Some(message_id.clone()),
                        summarized_until_message_id: last_message_id.map(str::to_string),
                    })
                    .collect()
            };
            if section.needs_supplement {
                sources.push(NoteSourceCreate {
                    section_id: section.id.clone(),
                    origin: NoteSourceOrigin::AiSupplement,
                    conversation_id: None,
                    message_id: None,
                    summarized_until_message_id: None,
                });
            }
            sources
        })
        .collect()
}

fn note_attachment_sources(
    state: &AppState,
    conversation: &StoredConversation,
) -> Result<Vec<NoteAttachmentSource>, String> {
    let mut seen = HashSet::new();
    let mut sources = Vec::new();
    for attachment in conversation
        .messages
        .iter()
        .flat_map(|message| message.attachments.iter())
    {
        if !seen.insert(attachment.path.clone()) {
            continue;
        }
        let source_path = state
            .conversation_repository
            .resolve_attachment_path(&conversation.id, &attachment.path)?;
        let metadata = std::fs::metadata(&source_path)
            .map_err(|error| format!("读取笔记来源附件“{}”失败：{error}", attachment.name))?;
        if !metadata.is_file() || metadata.len() != attachment.size_bytes {
            return Err(format!(
                "笔记来源附件“{}”缺失或大小不一致。",
                attachment.name
            ));
        }
        sources.push(NoteAttachmentSource {
            source_path,
            original_name: attachment.name.clone(),
            mime_type: Some(attachment.mime_type.clone()),
        });
    }
    Ok(sources)
}

async fn cleanup_transferred_local_source(
    state: &AppState,
    run_id: &str,
    conversation: &StoredConversation,
) {
    if conversation.source_kind.as_deref() != Some("localFiles") {
        return;
    }
    let deletion = {
        let _guard = state.conversation_writes.lock().await;
        state.conversation_repository.delete(&conversation.id)
    };
    if let Err(error) = deletion {
        let _ = append_pipeline_event_if_available(
            state,
            run_id,
            "noteAttachmentTransferCleanupFailed",
            Some("persist-note"),
            &serde_json::json!({ "error": error }).to_string(),
        );
        return;
    }
    let _guard = state.library_operations.lock().await;
    if let Err(error) = state
        .library_repository
        .detach_note_sources_for_conversation(&conversation.id)
    {
        let _ = append_pipeline_event_if_available(
            state,
            run_id,
            "noteSourceDetachFailed",
            Some("persist-note"),
            &serde_json::json!({ "error": error }).to_string(),
        );
    }
}

async fn persist_error(
    state: &AppState,
    run_id: &str,
    channel: &Channel<NotePipelineProgress>,
    error: String,
) {
    let failure = pipeline_failure_payload(&error);
    let display_message = failure
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("深度笔记运行失败。")
        .to_string();
    let failed_node_id = failure
        .get("nodeId")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let run = {
        let _guard = state.library_operations.lock().await;
        match state.library_repository.get_note_pipeline_run(run_id) {
            Ok(current)
                if matches!(
                    current.phase,
                    NotePipelinePhase::Paused | NotePipelinePhase::Cancelled
                ) =>
            {
                Ok(current)
            }
            Ok(current) if current.phase == NotePipelinePhase::Cancelling => state
                .library_repository
                .finalize_note_pipeline_cancellation(
                    run_id,
                    false,
                    "cancelled-during-error-recovery",
                    None,
                ),
            Ok(_) => state.library_repository.update_note_pipeline_phase(
                run_id,
                NotePipelinePhase::Error,
                None,
                &[],
                Some(&display_message),
            ),
            Err(error) => Err(error),
        }
    };
    if let Ok(run) = &run {
        if run.phase == NotePipelinePhase::Paused {
            send(channel, NotePipelineProgress::Paused { run: run.clone() });
            return;
        }
        if run.phase == NotePipelinePhase::Cancelled {
            send(
                channel,
                NotePipelineProgress::Cancelled { run: run.clone() },
            );
            return;
        }
    }
    let _ = state.library_repository.append_note_pipeline_event(
        run_id,
        "runFailed",
        failed_node_id.as_deref(),
        &failure.to_string(),
    );
    send(
        channel,
        NotePipelineProgress::Error {
            run_id: run_id.to_string(),
            message: display_message,
        },
    );
    if run.is_err() {
        eprintln!("Failed to persist note pipeline error for {run_id}");
    }
}

fn pipeline_failure_payload(error: &str) -> serde_json::Value {
    if let Some(raw) = error.strip_prefix(DEEP_NOTE_FAILURE_PREFIX) {
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(object) = value.as_object_mut() {
                object
                    .entry("title")
                    .or_insert_with(|| serde_json::json!("内部执行状态异常"));
                object
                    .entry("technicalDetail")
                    .or_insert_with(|| serde_json::json!(error));
            }
            return value;
        }
    }

    let normalized = error.to_ascii_lowercase();
    let (category, title, recovery, retryable) = if error.contains("DAG")
        || error.contains("状态转换")
        || error.contains("状态机")
        || error.contains("执行节点")
    {
        (
            "internalState",
            "内部执行状态异常",
            "检查点已保留。请重试任务；若仍失败，请复制诊断信息。",
            true,
        )
    } else if error.contains("预算") || error.contains("上限") {
        (
            "budget",
            "运行预算或时限已耗尽",
            "已完成的检查点会保留。可以缩小章节范围后继续，或调整模型与预算设置。",
            true,
        )
    } else if error.contains("数据库")
        || error.contains("写入")
        || error.contains("保存")
        || normalized.contains("sqlite")
    {
        (
            "storage",
            "保存运行检查点失败",
            "请确认存储目录可写且磁盘空间充足，然后重试。",
            true,
        )
    } else if error.contains("验证") || error.contains("校验") {
        (
            "validation",
            "内容验证未通过",
            "检查失败章节与证据要求；可以调整提纲后重试失败步骤。",
            true,
        )
    } else if normalized.contains("model")
        || normalized.contains("provider")
        || normalized.contains("http")
        || normalized.contains("timeout")
        || error.contains("模型")
        || error.contains("渠道")
        || error.contains("并发")
        || error.contains("超时")
    {
        (
            "model",
            "模型请求失败",
            "等待冷却后重试，或切换到可用的备用模型。",
            true,
        )
    } else if error.contains("输入") || error.contains("附件") || error.contains("来源") {
        (
            "input",
            "输入或来源不可用",
            "请检查会话消息与附件是否仍然可访问，再重新生成。",
            false,
        )
    } else {
        (
            "unknown",
            "运行步骤失败",
            "检查点已保留。请查看运行记录中的最后一项并复制诊断信息。",
            true,
        )
    };
    serde_json::json!({
        "category": category,
        "stage": "runtime",
        "title": title,
        "message": error,
        "technicalDetail": error,
        "recovery": recovery,
        "retryable": retryable,
    })
}

/// 起草前的 run 预算耗尽没有可交付章节，按状态机落到 Blocked，允许用户缩小范围后重启。
async fn persist_pre_drafting_budget_exhaustion(
    state: &AppState,
    run_id: &str,
    channel: &Channel<NotePipelineProgress>,
    raw_error: &str,
) -> Result<(), String> {
    let (run, message) = {
        let _guard = state.library_operations.lock().await;
        let current = state.library_repository.get_note_pipeline_run(run_id)?;
        let mut runtime = runtime_state(&current)?;
        let exhaustion = refresh_run_budget(state, run_id, &mut runtime);
        let transition = DeepNoteRunMachine::timeout(current.phase)
            .map_err(|error| format!("派发深度笔记预算耗尽事件失败：{error:?}"))?;
        if transition.effects != vec![DeepNoteRunEffect::PersistTimeout] {
            return Err(format!(
                "{:?} 阶段预算耗尽没有产生 PersistTimeout 效果。",
                current.phase
            ));
        }
        let message = exhaustion.map_or_else(
            || {
                raw_error
                    .strip_prefix(RUN_BUDGET_EXHAUSTED_PREFIX)
                    .unwrap_or(raw_error)
                    .to_string()
            },
            |reason| reason.message(&runtime),
        );
        save_runtime_state(state, run_id, &runtime)?;
        state.library_repository.append_note_pipeline_event(
            run_id,
            "runBudgetExhausted",
            None,
            &serde_json::json!({
                "reason": exhaustion.map(RunBudgetExhaustion::as_str),
                "phase": current.phase.as_str(),
                "upstreamRequestsUsed": runtime.budget.upstream_requests_used,
                "upstreamRequestLimit": runtime.budget.upstream_request_limit,
                "upstreamWallClockMs": runtime.budget.upstream_wall_clock_ms,
                "runWallClockMs": runtime.budget.run_wall_clock_ms,
                "deliveredSections": 0,
            })
            .to_string(),
        )?;
        let run = state.library_repository.update_note_pipeline_phase(
            run_id,
            transition.next_state,
            None,
            &[],
            Some(&message),
        )?;
        (run, message)
    };
    debug_assert_eq!(run.phase, NotePipelinePhase::Blocked);
    send(
        channel,
        NotePipelineProgress::Error {
            run_id: run_id.to_string(),
            message,
        },
    );
    Ok(())
}

async fn analyze_outline(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    conversation: &StoredConversation,
    adjustment: &str,
    channel: &Channel<NotePipelineProgress>,
    cancellation: &CancellationToken,
) -> Result<DeepNoteOutline, String> {
    let analysis_transcript = transcript(conversation, false);
    if analysis_transcript.trim().is_empty() {
        return Err("对话还没有可以生成深度笔记的消息。".to_string());
    }
    let valid_ids = noteworthy_messages(conversation)
        .into_iter()
        .map(|message| message.id.clone())
        .collect::<HashSet<_>>();
    let (_, route_profile) = deep_note_route_profile(state, &run.provider_id, &run.model_id)?;
    let adaptive_budget = AdaptiveBudgetSnapshot::from_profile(&route_profile);
    let calculated = context_budget(
        conversation,
        &runtime.input_snapshot.model,
        run.max_output_tokens,
        &adaptive_budget,
    );
    let previous = runtime.context_budget.clone();
    runtime.context_budget = calculated;
    if previous.estimated_input_tokens == runtime.context_budget.estimated_input_tokens
        && previous.total_message_count == runtime.context_budget.total_message_count
    {
        runtime.context_budget.chunk_count = previous.chunk_count;
        runtime.context_budget.processed_chunk_count = previous.processed_chunk_count;
        runtime.context_budget.processed_message_count = previous.processed_message_count;
        runtime.context_budget.coverage_complete = previous.coverage_complete;
        runtime.context_budget.omitted_message_ids = previous.omitted_message_ids;
        if previous.chunk_target_tokens > 0 {
            // 同一个 run 可以响应失败后的收缩，但不会因为其他 run 的成功在恢复时
            // 突然放大分块、令已经写好的检查点整体失效。
            runtime.context_budget.chunk_target_tokens = runtime
                .context_budget
                .chunk_target_tokens
                .min(previous.chunk_target_tokens);
        }
    }
    save_runtime_state(state, &run.id, runtime)?;
    progress(
        state,
        channel,
        &run.id,
        NotePipelinePhase::Analyzing,
        None,
        None,
        format!(
            "上下文预检完成 · 预计 {} Token · 可直接规划 {} Token",
            runtime.context_budget.estimated_input_tokens,
            runtime.context_budget.direct_input_limit_tokens
        ),
    );

    let mut planner_prompt = None;
    let has_source_attachments = noteworthy_messages(conversation)
        .into_iter()
        .any(|message| !message.attachments.is_empty());
    if !has_source_attachments
        && runtime.context_budget.estimated_input_tokens
            <= runtime.context_budget.direct_input_limit_tokens
        && !runtime.context_budget.coverage_complete
    {
        let direct_chunks = all_source_chunks(
            state,
            run,
            runtime,
            conversation,
            runtime.context_budget.direct_input_limit_tokens.max(2_048),
            cancellation,
        )
        .await?;
        {
            let _guard = state.library_operations.lock().await;
            state
                .library_repository
                .replace_note_pipeline_source_chunks(
                    &run.id,
                    &direct_chunks
                        .iter()
                        .map(|chunk| chunk.source.clone())
                        .collect::<Vec<_>>(),
                )?;
        }
        runtime.context_budget.chunk_count = direct_chunks.len();
        runtime.context_budget.processed_chunk_count = direct_chunks.len();
        runtime.context_budget.processed_message_count = valid_ids.len();
        runtime.context_budget.coverage_complete = true;
        runtime.context_budget.omitted_message_ids.clear();
        // 直接规划不需要额外 Chunk 模型调用，但执行 DAG 的 BuildLedger 节点
        // 仍必须拥有真实产物。保存有界原文索引，避免用空 Ledger 伪造完成。
        runtime.ledger = DeepNoteLedger {
            section_summaries: direct_chunks
                .iter()
                .map(|chunk| {
                    format!(
                        "{} | 来源消息 [{}] | {}",
                        chunk.source.chunk_id,
                        chunk.message_ids.join(", "),
                        chunk.source.excerpt.chars().take(4_000).collect::<String>()
                    )
                })
                .collect(),
            ..DeepNoteLedger::default()
        };
        save_runtime_state(state, &run.id, runtime)?;
        let _ = state.library_repository.append_note_pipeline_event(
            &run.id,
            "contextCoverageCompleted",
            None,
            &serde_json::json!({
                "mode": "direct",
                "processedMessageCount": runtime.context_budget.processed_message_count,
                "totalMessageCount": runtime.context_budget.total_message_count,
                "chunkCount": runtime.context_budget.chunk_count,
            })
            .to_string(),
        );
        let direct_prompt = analysis_prompt(&analysis_transcript, adjustment);
        progress(
            state,
            channel,
            &run.id,
            NotePipelinePhase::Analyzing,
            None,
            None,
            "上下文在预算内 · 正在直接生成知识结构与提纲",
        );
        consume_semantic_call(state, &run.id, runtime)?;
        match model_call_with_runtime(
            state,
            run,
            "deepNoteOutlineDirect",
            NotePipelinePhase::Analyzing,
            system_prompt_with_skill_profile(
                state,
                &run.id,
                runtime,
                DeepNoteSkillProfileKind::Planner,
                Some("analyze-input"),
                &format!("{ANALYST_SYSTEM_PROMPT}\n\n{OUTLINE_SIZE_SUFFIX}"),
            ),
            direct_prompt.clone(),
            run.max_output_tokens.min(PLANNER_OUTPUT_TOKEN_LIMIT),
            run.retry_attempts,
            cancellation,
            Some(channel),
        )
        .await
        {
            Ok(raw) => {
                if let Ok(outline) = parse_json_object::<DeepNoteOutline>(&raw)
                    .and_then(|outline| outline.validate(&valid_ids))
                {
                    return Ok(outline);
                }
                planner_prompt = Some(direct_prompt);
            }
            Err(error) if should_fallback_to_chunked_planner(&error) => {
                progress(
                    state,
                    channel,
                    &run.id,
                    NotePipelinePhase::Analyzing,
                    None,
                    None,
                    "直接规划未成功，正在缩小请求并切换到分块知识账本",
                );
                runtime.ledger = DeepNoteLedger::default();
                runtime.context_budget.processed_chunk_count = 0;
                runtime.context_budget.processed_message_count = 0;
                runtime.context_budget.coverage_complete = false;
            }
            Err(error) => return Err(error.message),
        }
    }

    if planner_prompt.is_none() {
        if !runtime.context_budget.coverage_complete || runtime.ledger.section_summaries.is_empty()
        {
            let target = if runtime.context_budget.estimated_input_tokens
                <= runtime.context_budget.direct_input_limit_tokens
            {
                runtime
                    .context_budget
                    .chunk_target_tokens
                    .min(UNKNOWN_CONTEXT_CHUNK_TOKENS)
            } else {
                runtime.context_budget.chunk_target_tokens
            };
            build_chunked_ledger(
                state,
                run,
                runtime,
                conversation,
                target.max(2_048),
                channel,
                cancellation,
            )
            .await?;
        }
        planner_prompt = Some(compact_ledger_analysis_prompt(
            &runtime.ledger,
            &runtime.context_budget,
            adjustment,
        )?);
    }

    let planner_prompt = planner_prompt.unwrap_or_default();
    progress(
        state,
        channel,
        &run.id,
        NotePipelinePhase::Analyzing,
        None,
        None,
        "来源覆盖已完成 · 正在汇总知识账本并生成提纲",
    );
    let planner_output_tokens = run.max_output_tokens.min(PLANNER_OUTPUT_TOKEN_LIMIT);
    consume_semantic_call(state, &run.id, runtime)?;
    let initial_result = model_call_with_runtime(
        state,
        run,
        "deepNoteOutline",
        NotePipelinePhase::Analyzing,
        system_prompt_with_skill_profile(
            state,
            &run.id,
            runtime,
            DeepNoteSkillProfileKind::Planner,
            Some("analyze-input"),
            FAST_PLANNER_SYSTEM_PROMPT,
        ),
        planner_prompt,
        planner_output_tokens.min(FAST_PLANNER_OUTPUT_TOKENS),
        run.retry_attempts,
        cancellation,
        Some(channel),
    )
    .await;
    let raw = match initial_result {
        Ok(raw) => raw,
        Err(error) if should_fallback_to_chunked_planner(&error) => {
            progress(
                state,
                channel,
                &run.id,
                NotePipelinePhase::Analyzing,
                None,
                None,
                "提纲请求未完成 · 正在使用精简知识账本重试",
            );
            let compact_prompt = if runtime.ledger.section_summaries.is_empty() {
                let mut prompt = analysis_prompt(&analysis_transcript, adjustment);
                prompt.push_str(
                    "\n\nGenerate a concise outline with at most 12 sections and return only valid JSON.",
                );
                prompt
            } else {
                compact_ledger_analysis_prompt(
                    &runtime.ledger,
                    &runtime.context_budget,
                    adjustment,
                )?
            };
            consume_semantic_call(state, &run.id, runtime)?;
            let initial_message = error.message.clone();
            model_call_with_runtime(
                state,
                run,
                "deepNoteOutlineFallback",
                NotePipelinePhase::Analyzing,
                system_prompt_with_skill_profile(
                    state,
                    &run.id,
                    runtime,
                    DeepNoteSkillProfileKind::Planner,
                    Some("analyze-input"),
                    FAST_PLANNER_SYSTEM_PROMPT,
                ),
                compact_prompt,
                FAST_PLANNER_OUTPUT_TOKENS,
                PLANNER_FALLBACK_RETRIES,
                cancellation,
                Some(channel),
            )
            .await
            .map_err(|fallback| {
                format!(
                    "{}；精简提纲重试仍失败：{}",
                    initial_message, fallback.message
                )
            })?
        }
        Err(error) => return Err(error.message),
    };
    parse_json_object::<DeepNoteOutline>(&raw).and_then(|outline| outline.validate(&valid_ids))
}

fn should_fallback_to_chunked_planner(error: &ModelError) -> bool {
    matches!(
        error.kind,
        ModelErrorKind::ClientTimeout
            | ModelErrorKind::UpstreamTimeout
            | ModelErrorKind::Connection
            | ModelErrorKind::Provider
            | ModelErrorKind::ContextLengthExceeded
    )
}

async fn run_analysis_task<R: Runtime>(
    app: AppHandle<R>,
    run_id: String,
    adjustment: String,
    channel: Channel<NotePipelineProgress>,
    cancellation: CancellationToken,
) {
    let state = app.state::<AppState>();
    progress(
        &state,
        &channel,
        &run_id,
        NotePipelinePhase::Analyzing,
        None,
        None,
        if adjustment.is_empty() {
            "正在分析知识结构…"
        } else {
            "正在按补充要求调整提纲…"
        },
    );
    let result = async {
        let run = state.library_repository.get_note_pipeline_run(&run_id)?;
        let mut runtime = runtime_state(&run)?;
        let live_conversation = state.conversation_repository.load(&run.conversation_id)?;
        let conversation = snapshot_conversation(
            &state.conversation_repository,
            &live_conversation,
            &runtime.input_snapshot,
        )
        .await?;
        let outline = match analyze_outline(
            &state,
            &run,
            &mut runtime,
            &conversation,
            &adjustment,
            &channel,
            &cancellation,
        )
        .await
        {
            Ok(outline) => outline,
            Err(error) if !cancellation.is_cancelled() => return Err(error),
            Err(_) => {
                finish_interrupted_run(&state, &run_id, &channel).await?;
                return Ok(());
            }
        };
        if cancellation.is_cancelled() {
            finish_interrupted_run(&state, &run_id, &channel).await?;
            return Ok(());
        }
        let outline_json = serde_json::to_string(&outline).map_err(|error| error.to_string())?;
        let sections = outline
            .sections
            .iter()
            .enumerate()
            .map(|(position, section)| {
                Ok(NotePipelineSectionCreate {
                    section_id: section.id.clone(),
                    position,
                    section_json: serde_json::to_string(section)
                        .map_err(|error| error.to_string())?,
                    input_hash: stable_hash(format!("{}:{}", run.input_snapshot_hash, section.id)),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        // 先编译并持久化计划，再把阶段翻成 awaiting_outline。
        //
        // 反过来会让阶段撒谎：`save_note_pipeline_outline` 在自己的事务里就把阶段
        // 设成 awaiting_outline，而计划此刻只在内存里。任何按阶段判断状态的消费方
        // （get_detail、e2e、崩溃后的恢复）都会看到「等待确认提纲」却拿不到计划；
        // 若进程正好在这个窗口里退出，run 会永久卡在这个自相矛盾的状态上。
        // `save_note_pipeline_outline` 不修改 current_plan_version，所以这里可以
        // 提前用 run 上的版本号编译。
        let mut plan_version = compile_plan(
            &run_id,
            run.current_plan_version.saturating_add(1).max(1),
            outline,
            &run.input_snapshot_hash,
            if adjustment.trim().is_empty() {
                "initial-plan"
            } else {
                adjustment.trim()
            },
        )?;
        runtime.plan_version = Some(plan_version.clone());
        runtime.budget = budget_for_drafting(&runtime.budget, plan_version.plan.sections.len());
        save_runtime_state(&state, &run_id, &runtime)?;
        let saved = {
            let _guard = state.library_operations.lock().await;
            let current = state.library_repository.get_note_pipeline_run(&run_id)?;
            if current.phase == NotePipelinePhase::Paused {
                send(&channel, NotePipelineProgress::Paused { run: current });
                return Ok(());
            }
            if cancellation.is_cancelled() {
                let cancelled = state.library_repository.update_note_pipeline_phase(
                    &run_id,
                    NotePipelinePhase::Cancelled,
                    None,
                    &[],
                    None,
                )?;
                send(&channel, NotePipelineProgress::Cancelled { run: cancelled });
                return Ok(());
            }
            state
                .library_repository
                .save_note_pipeline_outline(&run_id, &outline_json, sections)?
        };
        // created_at 对齐提纲落库时刻，仅用于展示排序；计划本身已经持久化。
        if plan_version.created_at != saved.updated_at {
            plan_version.created_at = saved.updated_at;
            runtime.plan_version = Some(plan_version.clone());
            save_runtime_state(&state, &run_id, &runtime)?;
        }
        let _ = state.library_repository.append_note_pipeline_event(
            &run_id,
            "outlineReady",
            None,
            &serde_json::json!({
                "planId": plan_version.plan_id,
                "version": plan_version.version,
                "sectionCount": plan_version.plan.sections.len(),
            })
            .to_string(),
        );
        send(&channel, NotePipelineProgress::OutlineReady { run: saved });
        Ok(())
    }
    .await;
    if let Err(error) = result {
        if error.starts_with(RUN_BUDGET_EXHAUSTED_PREFIX) {
            if let Err(persist_error_message) =
                persist_pre_drafting_budget_exhaustion(&state, &run_id, &channel, &error).await
            {
                persist_error(
                    &state,
                    &run_id,
                    &channel,
                    format!("{error}；预算耗尽状态持久化失败：{persist_error_message}"),
                )
                .await;
            }
        } else {
            persist_error(&state, &run_id, &channel, error).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn save_drafting_checkpoint(
    state: &AppState,
    run_id: &str,
    section_id: &str,
    markdown: &str,
    status: NotePipelineSectionStatus,
    attempts: u8,
    revisions: u8,
    evidence_ids: &[String],
    validation: &DeepNoteValidationReport,
    error_message: Option<&str>,
) -> Result<(), String> {
    let validation_json = serde_json::to_string(validation).map_err(|error| error.to_string())?;
    let _guard = state.library_operations.lock().await;
    state
        .library_repository
        .save_note_pipeline_section_checkpoint(
            run_id,
            section_id,
            markdown,
            status,
            attempts,
            revisions,
            evidence_ids,
            &validation_json,
            error_message,
        )
}

async fn execute_dag_section(
    state: &AppState,
    run: &NotePipelineRun,
    ledger: &DeepNoteLedger,
    context_budget: &DeepNoteContextBudget,
    conversation: &StoredConversation,
    selected_outline: &DeepNoteOutline,
    section: &DeepNoteSection,
    ledger_context: &str,
    dependency_outputs: &str,
    writer_system_prompt: &str,
    reviewer_system_prompt: &str,
    node_attempt_limit: u8,
    section_revision_limit: u8,
    channel: &Channel<NotePipelineProgress>,
    cancellation: &CancellationToken,
    persisted: Option<&NotePipelineSection>,
    evidence_ids: &[String],
) -> Result<(Option<(String, DeepNoteValidationReport, u8, u8)>, u32), String> {
    let (source_context, using_ledger_summary) =
        section_source_context(conversation, section, ledger, context_budget)?;
    if using_ledger_summary {
        let _ = state.library_repository.append_note_pipeline_event(
            &run.id,
            "sectionUsingChunkLedger",
            Some(&section.id),
            &serde_json::json!({
                "sectionId": section.id,
                "sourceMessageCount": section.source_message_ids.len(),
            })
            .to_string(),
        );
    }
    let prompt = section_prompt(
        selected_outline,
        section,
        &source_context,
        ledger_context,
        dependency_outputs,
    )?;
    let mut last_error = String::new();
    let mut markdown = None;
    let mut validation = DeepNoteValidationReport {
        passed: false,
        errors: vec!["章节尚未生成。".to_string()],
        warnings: Vec::new(),
        checked_evidence_ids: evidence_ids.to_vec(),
        criteria_coverage: Vec::new(),
    };
    let mut attempts = persisted.map(|value| value.attempt_count).unwrap_or(0);
    let mut revisions = persisted.map(|value| value.revision_count).unwrap_or(0);
    let mut candidate = persisted
        .filter(|value| !value.markdown.trim().is_empty())
        .map(|value| value.markdown.clone());
    let mut last_candidate = candidate.clone();
    let node_attempt_limit = node_attempt_limit.max(1);
    let mut semantic_calls = 0u32;
    'attempts: while candidate.is_some() || attempts < node_attempt_limit {
        if cancellation.is_cancelled() {
            save_drafting_checkpoint(
                state,
                &run.id,
                &section.id,
                candidate.as_deref().unwrap_or_default(),
                NotePipelineSectionStatus::Interrupted,
                attempts,
                revisions,
                evidence_ids,
                &validation,
                Some("章节执行被中断。"),
            )
            .await?;
            return Ok((None, semantic_calls));
        }
        if candidate.is_none() {
            attempts = attempts.saturating_add(1);
            semantic_calls = semantic_calls.saturating_add(1);
            match model_call_with_runtime(
                state,
                run,
                "deepNote",
                NotePipelinePhase::Drafting,
                writer_system_prompt.to_string(),
                prompt.clone(),
                run.max_output_tokens.min(SECTION_OUTPUT_TOKEN_LIMIT),
                run.retry_attempts,
                cancellation,
                Some(channel),
            )
            .await
            {
                Ok(value) if !value.trim().is_empty() => {
                    candidate = Some(normalize_generated_markdown(value.trim()));
                }
                Ok(_) => last_error = "模型返回了空章节。".to_string(),
                Err(error) => last_error = error.message,
            }
            if candidate.is_none() {
                let interrupted = cancellation.is_cancelled();
                save_drafting_checkpoint(
                    state,
                    &run.id,
                    &section.id,
                    "",
                    if interrupted {
                        NotePipelineSectionStatus::Interrupted
                    } else {
                        NotePipelineSectionStatus::InProgress
                    },
                    attempts,
                    revisions,
                    evidence_ids,
                    &validation,
                    Some(&last_error),
                )
                .await?;
                if interrupted {
                    return Ok((None, semantic_calls));
                }
                continue;
            }
        }

        let mut current = candidate.take().expect("candidate checked above");
        loop {
            validation = validate_section_markdown(section, &current, evidence_ids);
            last_error = validation.errors.join("；");
            last_candidate = Some(current.clone());
            save_drafting_checkpoint(
                state,
                &run.id,
                &section.id,
                &current,
                if validation.passed {
                    NotePipelineSectionStatus::Completed
                } else {
                    NotePipelineSectionStatus::NeedsRevision
                },
                attempts,
                revisions,
                evidence_ids,
                &validation,
                (!validation.passed).then_some(last_error.as_str()),
            )
            .await?;
            if validation.passed && !has_mermaid_lint_warning(&validation) {
                markdown = Some(current);
                break 'attempts;
            }
            if revisions >= section_revision_limit {
                // 只剩 mermaid 笔误、修订额度已用尽：接受当前稿，别为一处图表
                // 丢掉整章。渲染前的确定性修复会再兜一次。
                if validation.passed {
                    markdown = Some(current);
                    break 'attempts;
                }
                break;
            }
            revisions = revisions.saturating_add(1);
            semantic_calls = semantic_calls.saturating_add(1);
            let revision_prompt = format!(
                "章节计划：\n{}\n\n当前正文：\n{}\n\n验证报告：\n{}",
                serde_json::to_string(section).map_err(|error| error.to_string())?,
                current,
                serde_json::to_string(&validation).map_err(|error| error.to_string())?,
            );
            let revision_result = model_call_with_runtime(
                state,
                run,
                "deepNote",
                NotePipelinePhase::Validating,
                reviewer_system_prompt.to_string(),
                revision_prompt,
                run.max_output_tokens.min(SECTION_OUTPUT_TOKEN_LIMIT),
                run.retry_attempts,
                cancellation,
                Some(channel),
            )
            .await;
            if cancellation.is_cancelled() {
                save_drafting_checkpoint(
                    state,
                    &run.id,
                    &section.id,
                    &current,
                    NotePipelineSectionStatus::Interrupted,
                    attempts,
                    revisions,
                    evidence_ids,
                    &validation,
                    Some("章节修订被中断，已保留当前草稿。"),
                )
                .await?;
                return Ok((None, semantic_calls));
            }
            match revision_result {
                Ok(value) if !value.trim().is_empty() => {
                    current = normalize_generated_markdown(value.trim());
                }
                Ok(_) => {
                    last_error = "模型修订返回了空章节。".to_string();
                    // 修订请求本身失败时，别把只带 mermaid 笔误、其它验证都过的
                    // 稿子一起丢掉——那会让一处图表笔误升级成整章失败。
                    if validation.passed {
                        markdown = Some(current);
                        break 'attempts;
                    }
                    break;
                }
                Err(error) => {
                    last_error = error.message;
                    if validation.passed {
                        markdown = Some(current);
                        break 'attempts;
                    }
                    break;
                }
            }
        }
    }
    let Some(markdown) = markdown else {
        if cancellation.is_cancelled() {
            return Ok((None, semantic_calls));
        }
        save_drafting_checkpoint(
            state,
            &run.id,
            &section.id,
            last_candidate.as_deref().unwrap_or_default(),
            NotePipelineSectionStatus::Failed,
            attempts,
            revisions,
            evidence_ids,
            &validation,
            Some(&last_error),
        )
        .await?;
        return Err(format!(
            "章节“{}”在 {} 次节点尝试和 {} 次语义修订后仍未通过验证：{}",
            section.heading, attempts, revisions, last_error
        ));
    };
    save_drafting_checkpoint(
        state,
        &run.id,
        &section.id,
        &markdown,
        NotePipelineSectionStatus::Completed,
        attempts,
        revisions,
        evidence_ids,
        &validation,
        None,
    )
    .await?;
    Ok((
        Some((markdown, validation, attempts, revisions)),
        semantic_calls,
    ))
}

async fn run_drafting_task<R: Runtime>(
    app: AppHandle<R>,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
    cancellation: CancellationToken,
) {
    let state = app.state::<AppState>();
    let result = async {
        let run = state.library_repository.get_note_pipeline_run(&run_id)?;
        let mut runtime = runtime_state(&run)?;
        let plan_version = runtime
            .plan_version
            .clone()
            .ok_or_else(|| "深度笔记计划尚未编译。".to_string())?;
        if plan_version.confirmed_at.is_none() {
            return Err("深度笔记计划尚未由用户确认。".to_string());
        }
        let live_conversation = state.conversation_repository.load(&run.conversation_id)?;
        let conversation = snapshot_conversation(
            &state.conversation_repository,
            &live_conversation,
            &runtime.input_snapshot,
        )
        .await?;
        let outline = parse_json_object::<DeepNoteOutline>(&run.outline_json)?;
        let selected_ids = run
            .selected_section_ids
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let selected_outline = outline.select(&selected_ids)?;
        let persisted_sections = state
            .library_repository
            .list_note_pipeline_sections(&run_id)?
            .into_iter()
            .map(|section| (section.section_id.clone(), section))
            .collect::<HashMap<_, _>>();
        let ledger_context = serde_json::to_string(&compact_ledger_for_planner(&runtime.ledger))
            .map_err(|error| format!("读取深度笔记知识账本失败：{error}"))?;
        let last_message_id = noteworthy_messages(&conversation)
            .last()
            .map(|message| message.id.clone());
        let total = selected_outline.sections.len();
        let restored_nodes = state.library_repository.restore_note_pipeline_nodes(
            &run.id,
            plan_version.version,
            &plan_version.compiled_dag,
        )?;
        let mut scheduler = DeepNoteDagScheduler::new(restored_nodes)?;
        let source_chunks = state
            .library_repository
            .list_note_pipeline_source_chunks(&run.id)?;
        if source_chunks.is_empty() {
            return Err("深度笔记没有持久化 Source Chunk，不能推进执行 DAG。".to_string());
        }
        let mut evidence = state
            .library_repository
            .list_note_pipeline_evidence(&run.id)?;
        if evidence.is_empty() {
            evidence = evidence_for_plan(&run, &plan_version, &source_chunks);
            state
                .library_repository
                .replace_note_pipeline_evidence(&run.id, &evidence)?;
            for item in &evidence {
                state.library_repository.append_note_pipeline_event(
                    &run.id,
                    "evidenceCreated",
                    Some(&format!("evidence:{}", item.section_id)),
                    &serde_json::json!({
                        "evidenceId": item.evidence_id,
                        "sectionId": item.section_id,
                        "sourceChunkIds": item.source_chunk_ids,
                        "status": item.status.as_str(),
                        "supportLevel": item.support_level.as_str(),
                    })
                    .to_string(),
                )?;
            }
        }
        state.library_repository.save_note_pipeline_ledger(
            &run.id,
            plan_version.version,
            &runtime.ledger,
            &serde_json::json!({
                "reason": "confirmed-plan",
                "sourceChunkCount": source_chunks.len(),
                "evidenceCount": evidence.len(),
            })
            .to_string(),
        )?;
        let evidence_by_section = evidence_ids_by_section(&evidence);
        scheduler.complete_preparation(
            !source_chunks.is_empty(),
            &evidence_by_section,
            ledger_has_real_output(&runtime.ledger, runtime.context_budget.coverage_complete),
        )?;
        scheduler.prepare_for_resume();
        let mut drafts_by_id = HashMap::<String, String>::new();
        for section in &selected_outline.sections {
            if let Some(existing) = persisted_sections.get(&section.id) {
                if existing.status == NotePipelineSectionStatus::Completed {
                    drafts_by_id.insert(section.id.clone(), existing.markdown.clone());
                    scheduler.reconcile_completed_section(&section.id)?;
                }
            }
        }
        runtime.plan_version = Some(plan_version.clone());
        persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;

        let mut cancelled = false;
        // 是否因为 run 级请求数或墙钟耗尽而退出循环。用来区分收尾方式：有产出走部分交付，
        // 没产出要落到 `Blocked` 并带上超时原因，而不是和「被用户中断」混为一谈。
        let mut run_budget_exhaustion = None;
        while scheduler.has_unfinished_sections() {
            scheduler.refresh_ready();
            if cancellation.is_cancelled() {
                scheduler.interrupt_running();
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                cancelled = true;
                break;
            }
            if send_paused_if_requested(&state, &run_id, &channel)? {
                scheduler.interrupt_running();
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                return Ok(());
            }
            // run 级双维度预算闸：请求数或墙钟到点都做部分交付，而不是整体失败。
            //
            // 放在批次派发之前：一旦进了下面的 `transition(.., InProgress)`，节点就
            // 被占住且 attempt_count 已经加过，事后再拦既浪费一次尝试，又可能把节点
            // 留在非终态上，`has_unfinished_sections` 于是永远为真。
            if let Some(exhaustion) = refresh_run_budget(&state, &run_id, &mut runtime) {
                let delivered = drafts_by_id.len();
                let transition = if delivered == 0 {
                    DeepNoteRunMachine::transition_to(
                        NotePipelinePhase::Drafting,
                        NotePipelinePhase::Blocked,
                    )
                } else {
                    DeepNoteRunMachine::timeout(NotePipelinePhase::Drafting)
                }
                .map_err(|error| format!("派发深度笔记预算耗尽事件失败：{error:?}"))?;
                let expected_effect = if delivered == 0 {
                    DeepNoteRunEffect::PersistTimeout
                } else {
                    DeepNoteRunEffect::SkipUnfinishedSections
                };
                if transition.effects != vec![expected_effect] {
                    return Err("起草阶段预算耗尽产生了错误的收敛效果。".to_string());
                }
                if delivered == 0 {
                    save_runtime_state(&state, &run_id, &runtime)?;
                    state.library_repository.append_note_pipeline_event(
                        &run_id,
                        "runBudgetExhausted",
                        None,
                        &serde_json::json!({
                            "reason": exhaustion.as_str(),
                            "upstreamRequestsUsed": runtime.budget.upstream_requests_used,
                            "upstreamRequestLimit": runtime.budget.upstream_request_limit,
                            "upstreamWallClockMs": runtime.budget.upstream_wall_clock_ms,
                            "runWallClockMs": runtime.budget.run_wall_clock_ms,
                            "deliveredSections": 0,
                            "totalSections": total,
                        })
                        .to_string(),
                    )?;
                    state.library_repository.update_note_pipeline_phase(
                        &run_id,
                        transition.next_state,
                        None,
                        &[],
                        Some(&exhaustion.message(&runtime)),
                    )?;
                    progress(
                        &state,
                        &channel,
                        &run_id,
                        NotePipelinePhase::Blocked,
                        Some(0),
                        Some(total),
                        exhaustion.message(&runtime),
                    );
                    return Ok(());
                }
                run_budget_exhaustion = Some(exhaustion);
                scheduler.skip_unfinished_sections();
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                state.library_repository.append_note_pipeline_event(
                    &run_id,
                    "runBudgetExhausted",
                    None,
                    &serde_json::json!({
                        "reason": exhaustion.as_str(),
                        "upstreamRequestsUsed": runtime.budget.upstream_requests_used,
                        "upstreamRequestLimit": runtime.budget.upstream_request_limit,
                        "upstreamWallClockMs": runtime.budget.upstream_wall_clock_ms,
                        "runWallClockMs": runtime.budget.run_wall_clock_ms,
                        "deliveredSections": delivered,
                        "totalSections": total,
                    })
                    .to_string(),
                )?;
                progress(
                    &state,
                    &channel,
                    &run_id,
                    NotePipelinePhase::Drafting,
                    Some(delivered),
                    Some(total),
                    format!(
                        "{}，交付已完成的 {delivered}/{total} 个章节",
                        exhaustion.message(&runtime)
                    ),
                );
                break;
            }
            let ready = scheduler.ready_section_ids(runtime.budget.max_parallel_nodes as usize);
            if ready.is_empty() {
                if scheduler.has_section_failures() {
                    break;
                }
                return Err("DAG 调度器无法释放下一个章节节点，可能存在未满足的依赖。".to_string());
            }
            let mut jobs = Vec::with_capacity(ready.len());
            let mut batch_reserved_calls = 0u32;
            for section_id in ready {
                // section 级墙钟闸：跳过累计活跃时长已经用完预算的 section。
                //
                // **作用域是跨 run 续跑**，不是单次 run 内的抢占：`ready_section_ids`
                // 只返回 `Ready` 的节点，一个 section 在同一次 run 里派发一次就进
                // `InProgress`，重试在 `execute_dag_section` 内部循环，不会回到这里。
                // 所以首次派发时累计为 0、闸门不响；它真正生效的场景是暂停/崩溃后
                // 续跑 —— `section_active_ms` 从 runtime JSON 读回，一个已经烧掉 15
                // 分钟**执行时间**的 section 不会再被重新派发一轮。
                //
                // 判定必须在 `transition(.., InProgress)` 之前：进了那一步节点就被
                // 占住且 attempt_count 已经加过。
                if section_wall_clock_exhausted(
                    &runtime.section_active_ms,
                    &runtime.budget,
                    &section_id,
                ) {
                    for prefix in ["draft", "validate"] {
                        if let Ok(node) = scheduler.node_mut(&format!("{prefix}:{section_id}")) {
                            node.status = DeepNoteNodeStatus::Skipped;
                            node.error_message = Some("章节执行超过时长上限，已跳过。".to_string());
                        }
                    }
                    scheduler.refresh_ready();
                    state.library_repository.append_note_pipeline_event(
                        &run_id,
                        "sectionWallClockExhausted",
                        Some(&format!("draft:{section_id}")),
                        &serde_json::json!({
                            "sectionId": section_id,
                            "sectionWallClockMs": runtime.budget.section_wall_clock_ms,
                        })
                        .to_string(),
                    )?;
                    continue;
                }
                let section = selected_outline
                    .sections
                    .iter()
                    .find(|value| value.id == section_id)
                    .cloned()
                    .ok_or_else(|| format!("DAG 节点引用了不存在的章节：{section_id}"))?;
                let persisted = persisted_sections.get(&section_id).cloned();
                let remaining_attempts = runtime.budget.node_attempt_limit.max(1).saturating_sub(
                    persisted
                        .as_ref()
                        .map(|value| value.attempt_count)
                        .unwrap_or(0),
                );
                let remaining_revisions = runtime.budget.section_revision_limit.saturating_sub(
                    persisted
                        .as_ref()
                        .map(|value| value.revision_count)
                        .unwrap_or(0),
                );
                let reserved_semantic_calls =
                    u32::from(remaining_attempts) + u32::from(remaining_revisions);
                batch_reserved_calls = batch_reserved_calls.saturating_add(reserved_semantic_calls);
                jobs.push(SectionDagJob {
                    dependency_outputs: dependency_context(&section, &drafts_by_id),
                    evidence_ids: verified_evidence_ids_for_section(
                        &evidence_by_section,
                        &section_id,
                    ),
                    persisted,
                    writer_system_prompt: system_prompt_with_skill_profile(
                        &state,
                        &run.id,
                        &runtime,
                        DeepNoteSkillProfileKind::Writer,
                        Some(&format!("draft:{section_id}")),
                        SECTION_SYSTEM_PROMPT,
                    ),
                    reviewer_system_prompt: system_prompt_with_skill_profile(
                        &state,
                        &run.id,
                        &runtime,
                        DeepNoteSkillProfileKind::Reviewer,
                        Some(&format!("validate:{section_id}")),
                        SECTION_REVISION_SYSTEM_PROMPT,
                    ),
                    reserved_semantic_calls,
                    section,
                });
            }
            // 整批都被 section 闸跳过：直接进下一轮重算就绪集合，不要往下走出一条
            // 「并行执行 0 个章节」的假进度，也不要空转一次预留和并发流。
            if jobs.is_empty() {
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                continue;
            }
            reserve_parallel_semantic_calls(&state, &run_id, &mut runtime, batch_reserved_calls)?;
            for job in &jobs {
                let section_id = &job.section.id;
                scheduler.transition(
                    &format!("draft:{section_id}"),
                    DeepNoteNodeStatus::InProgress,
                )?;
                if let Ok(node) = scheduler.node_mut(&format!("draft:{section_id}")) {
                    node.attempt_count = job
                        .persisted
                        .as_ref()
                        .map(|value| value.attempt_count)
                        .unwrap_or(0);
                    node.error_message = None;
                }
                state.library_repository.append_note_pipeline_event(
                    &run_id,
                    "dagNodeStarted",
                    Some(&format!("draft:{section_id}")),
                    &serde_json::json!({
                        "nodeId": format!("draft:{section_id}"),
                        "sectionId": section_id,
                        "nodeType": "draftSection",
                        "parallelism": runtime.budget.max_parallel_nodes.max(1),
                    })
                    .to_string(),
                )?;
            }
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
            let parallelism = usize::from(runtime.budget.max_parallel_nodes.max(1));
            progress(
                &state,
                &channel,
                &run_id,
                NotePipelinePhase::Drafting,
                Some(drafts_by_id.len()),
                Some(total),
                format!("正在并行执行 {} 个依赖已满足的章节", jobs.len()),
            );
            let ledger_snapshot = runtime.ledger.clone();
            let context_budget_snapshot = runtime.context_budget.clone();
            let node_attempt_limit = runtime.budget.node_attempt_limit;
            let section_revision_limit = runtime.budget.section_revision_limit;
            let mut section_results = stream::iter(jobs.into_iter().map(|job| {
                let job_for_result = job.clone();
                let state_ref: &AppState = &state;
                let run_ref = &run;
                let ledger_ref = &ledger_snapshot;
                let context_budget_ref = &context_budget_snapshot;
                let conversation_ref = &conversation;
                let outline_ref = &selected_outline;
                let ledger_context_ref = &ledger_context;
                let channel_ref = &channel;
                let cancellation_ref = &cancellation;
                async move {
                    let started_at = crate::usage::now_ms();
                    let result = execute_dag_section(
                        state_ref,
                        run_ref,
                        ledger_ref,
                        context_budget_ref,
                        conversation_ref,
                        outline_ref,
                        &job.section,
                        ledger_context_ref,
                        &job.dependency_outputs,
                        &job.writer_system_prompt,
                        &job.reviewer_system_prompt,
                        node_attempt_limit,
                        section_revision_limit,
                        channel_ref,
                        cancellation_ref,
                        job.persisted.as_ref(),
                        &job.evidence_ids,
                    )
                    .await;
                    SectionDagJobResult {
                        job: job_for_result,
                        result,
                        // 饱和减法：时钟回拨时记 0，不要让一次系统时间调整把预算算成天文数字。
                        active_ms: crate::usage::now_ms().saturating_sub(started_at),
                    }
                }
            }))
            .buffer_unordered(parallelism);
            while let Some(output) = section_results.next().await {
                let section_id = output.job.section.id.clone();
                // 在分支之前累加：成功、取消、失败三条路都实际花掉了这段时间，
                // 预算要如实反映。放进 match 里就得写三遍，漏一处就等于该分支免费。
                record_section_active_ms(
                    &mut runtime.section_active_ms,
                    &section_id,
                    output.active_ms,
                );
                match output.result {
                    Ok((Some((markdown, validation, attempts, revisions)), used_calls)) => {
                        release_unused_parallel_semantic_calls(
                            &mut runtime,
                            output.job.reserved_semantic_calls,
                            used_calls,
                        );
                        let validation_json = serde_json::to_string(&validation)
                            .map_err(|error| error.to_string())?;
                        let draft_node_id = format!("draft:{section_id}");
                        scheduler.transition(&draft_node_id, DeepNoteNodeStatus::Completed)?;
                        if let Ok(node) = scheduler.node_mut(&draft_node_id) {
                            node.attempt_count = attempts;
                            node.evidence_ids = output.job.evidence_ids.clone();
                            node.output_ref = Some(format!("section:{section_id}"));
                            node.validation_json = validation_json.clone();
                            node.error_message = None;
                        }
                        scheduler.refresh_ready();
                        persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                        let validate_node_id = format!("validate:{section_id}");
                        scheduler.transition(&validate_node_id, DeepNoteNodeStatus::InProgress)?;
                        persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                        scheduler.transition(&validate_node_id, DeepNoteNodeStatus::Completed)?;
                        if let Ok(node) = scheduler.node_mut(&validate_node_id) {
                            node.attempt_count = revisions;
                            node.evidence_ids = output.job.evidence_ids;
                            node.output_ref = Some(format!("validation:{section_id}"));
                            node.validation_json = validation_json;
                            node.error_message = None;
                        }
                        drafts_by_id.insert(section_id.clone(), markdown.clone());
                        state.library_repository.append_note_pipeline_event(
                            &run_id,
                            "dagNodeCompleted",
                            Some(&draft_node_id),
                            &serde_json::json!({
                                "nodeId": draft_node_id,
                                "sectionId": section_id,
                                "attemptCount": attempts,
                                "revisionCount": revisions,
                                "semanticCalls": used_calls,
                                "markdownChars": markdown.chars().count(),
                            })
                            .to_string(),
                        )?;
                    }
                    Ok((None, used_calls)) => {
                        release_unused_parallel_semantic_calls(
                            &mut runtime,
                            output.job.reserved_semantic_calls,
                            used_calls,
                        );
                        cancelled = true;
                    }
                    Err(error) => {
                        let draft_node_id = format!("draft:{section_id}");
                        scheduler.transition(&draft_node_id, DeepNoteNodeStatus::Failed)?;
                        if let Ok(node) = scheduler.node_mut(&draft_node_id) {
                            node.attempt_count = output
                                .job
                                .persisted
                                .as_ref()
                                .map(|value| value.attempt_count)
                                .unwrap_or(0)
                                .saturating_add(1);
                            node.error_message = Some(error.clone());
                        }
                        state.library_repository.append_note_pipeline_event(
                            &run_id,
                            "dagNodeFailed",
                            Some(&draft_node_id),
                            &serde_json::json!({
                                "nodeId": draft_node_id,
                                "sectionId": section_id,
                                "message": error,
                            })
                            .to_string(),
                        )?;
                    }
                }
                scheduler.refresh_ready();
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                progress(
                    &state,
                    &channel,
                    &run_id,
                    NotePipelinePhase::Drafting,
                    Some(drafts_by_id.len()),
                    Some(total),
                    format!("章节已完成 {}/{}", drafts_by_id.len(), total),
                );
            }
            drop(section_results);
            if cancelled {
                scheduler.interrupt_running();
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                break;
            }
        }
        if cancellation.is_cancelled() {
            scheduler.interrupt_running();
            cancelled = true;
        }
        if cancelled {
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
            finish_interrupted_run(&state, &run_id, &channel).await?;
            return Ok(());
        }
        // 墙钟耗尽时不走这条路：此时「有 section 失败」和「有 section 没做完」是同一
        // 个原因造成的（上游太慢），把它判成组装失败会让用户连已完成的部分都拿不到。
        // 超时的收敛统一由下面的部分交付/`Blocked` 两条分支处理。
        if scheduler.has_section_failures() && !cancelled && run_budget_exhaustion.is_none() {
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
            return Err("深度笔记存在失败或被阻塞的章节节点，未继续组装不完整笔记。".to_string());
        }
        let ordered_ids = stable_topological_sections(&selected_outline.sections)?;
        let drafts = ordered_ids
            .into_iter()
            .filter_map(|id| drafts_by_id.get(&id).map(|markdown| (id, markdown.clone())))
            .filter_map(|(id, markdown)| {
                selected_outline
                    .sections
                    .iter()
                    .find(|section| section.id == id)
                    .cloned()
                    .map(|section| (section, markdown, false))
            })
            .collect::<Vec<_>>();
        if drafts.is_empty() {
            // run 预算耗尽且一个 section 都没做出来：落到 `Blocked` 并带上具体原因。
            //
            // 不复用 `finish_interrupted_run`：那条路表达的是「被中断」，用户看到的
            // 提示会指向暂停/取消，而真实原因是上游太慢。区分开之后界面才能给出
            // 「缩小范围或换更快的模型」这种能自救的建议。
            if let Some(exhaustion) = run_budget_exhaustion {
                state.library_repository.update_note_pipeline_phase(
                    &run_id,
                    NotePipelinePhase::Blocked,
                    None,
                    &[],
                    Some(&format!(
                        "{}且尚无可交付章节。",
                        exhaustion.message(&runtime)
                    )),
                )?;
                return Ok(());
            }
            finish_interrupted_run(&state, &run_id, &channel).await?;
            return Ok(());
        }
        scheduler.refresh_ready();
        let mut global_validation_warnings = Vec::new();
        if scheduler
            .node("validate-global")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::Ready)
        {
            scheduler.transition("validate-global", DeepNoteNodeStatus::InProgress)?;
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
            let report = validate_global_drafts(&selected_outline, &drafts, &evidence_by_section);
            let report_json = serde_json::to_string(&report).map_err(|error| error.to_string())?;
            global_validation_warnings = report.warnings.clone();
            if !report.passed {
                scheduler.transition("validate-global", DeepNoteNodeStatus::Failed)?;
                if let Ok(node) = scheduler.node_mut("validate-global") {
                    node.validation_json = report_json;
                    node.error_message = Some(report.errors.join("；"));
                }
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                state.library_repository.append_note_pipeline_event(
                    &run_id,
                    "globalValidationFailed",
                    Some("validate-global"),
                    &serde_json::json!({
                        "errors": report.errors,
                        "warnings": report.warnings,
                    })
                    .to_string(),
                )?;
                return Err("深度笔记没有通过跨章节全局验证。".to_string());
            }
            scheduler.transition("validate-global", DeepNoteNodeStatus::Completed)?;
            if let Ok(node) = scheduler.node_mut("validate-global") {
                node.output_ref = Some(format!("global-validation:{}", stable_hash(&report_json)));
                node.validation_json = report_json;
                node.error_message = None;
            }
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
            state.library_repository.append_note_pipeline_event(
                &run_id,
                "globalValidationCompleted",
                Some("validate-global"),
                &serde_json::json!({
                    "sectionCount": drafts.len(),
                    "warningCount": global_validation_warnings.len(),
                })
                .to_string(),
            )?;
        }
        {
            let _guard = state.library_operations.lock().await;
            let current = state.library_repository.get_note_pipeline_run(&run_id)?;
            if current.phase == NotePipelinePhase::Paused {
                send(&channel, NotePipelineProgress::Paused { run: current });
                return Ok(());
            }
            state.library_repository.update_note_pipeline_phase(
                &run_id,
                NotePipelinePhase::Assembling,
                None,
                &[],
                None,
            )?;
        }
        progress(
            &state,
            &channel,
            &run_id,
            NotePipelinePhase::Assembling,
            None,
            None,
            "正在组装与检查笔记。",
        );
        if scheduler
            .node("assemble-note")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::Ready)
        {
            scheduler.transition("assemble-note", DeepNoteNodeStatus::InProgress)?;
        }
        let effective_outline = DeepNoteOutline {
            sections: drafts
                .iter()
                .map(|(section, _, _)| section.clone())
                .collect(),
            ..selected_outline
        };
        let (title, content, mut warnings) = assemble(&effective_outline, &drafts, false);
        warnings.extend(global_validation_warnings);
        if scheduler
            .node("assemble-note")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::InProgress)
        {
            scheduler.transition("assemble-note", DeepNoteNodeStatus::Completed)?;
            if let Ok(node) = scheduler.node_mut("assemble-note") {
                node.output_ref = Some("assembled-markdown".to_string());
            }
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
        }
        let sidecar = sidecar_json(&run, &plan_version, &drafts, &evidence_by_section)?;
        progress(
            &state,
            &channel,
            &run_id,
            NotePipelinePhase::Persisting,
            None,
            None,
            "正在保存笔记与来源。",
        );
        if scheduler
            .node("persist-note")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::Ready)
        {
            scheduler.transition("persist-note", DeepNoteNodeStatus::InProgress)?;
        }
        let sources = note_sources(&conversation.id, last_message_id.as_deref(), &drafts);
        {
            let _guard = state.library_operations.lock().await;
            state.library_repository.update_note_pipeline_phase(
                &run_id,
                NotePipelinePhase::Persisting,
                None,
                &warnings,
                None,
            )?;
            state.library_repository.update_note_pipeline_runtime_json(
                &run_id,
                &serde_json::to_string(&runtime.budget).map_err(|error| error.to_string())?,
                &serde_json::to_string(&runtime).map_err(|error| error.to_string())?,
                Some(&sidecar),
            )?;
        }
        // 先把 persist-note 节点标成 InProgress 落盘，再做真正的提交。
        // 提交本身是单事务（笔记 + note_id + 终态 + runCompleted 事件），
        // 所以这里之后不再存在“笔记已建但 run 不知道”的中间态。
        if scheduler
            .node("persist-note")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::InProgress)
        {
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
        }
        let attachment_sources = note_attachment_sources(&state, &conversation)?;
        let (note, completed) = {
            let _guard = state.library_operations.lock().await;
            let create = LibraryNoteCreate {
                item_id: None,
                title,
                content,
                group_name: None,
            };
            state.library_repository.commit_deep_note_and_complete_run(
                &run_id,
                create,
                sources,
                &conversation.id,
                &runtime.input_snapshot,
                &sidecar,
                attachment_sources,
                runtime.force_rebuild,
                &warnings,
                false,
            )?
        };
        cleanup_transferred_local_source(&state, &run_id, &conversation).await;
        if scheduler
            .node("persist-note")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::InProgress)
        {
            scheduler.transition("persist-note", DeepNoteNodeStatus::Completed)?;
            if let Ok(node) = scheduler.node_mut("persist-note") {
                node.output_ref = Some(format!("note:{}", note.id));
            }
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
        }
        send(
            &channel,
            NotePipelineProgress::Done {
                run: completed,
                degraded: false,
            },
        );
        Ok(())
    }
    .await;
    if let Err(error) = result {
        persist_error(&state, &run_id, &channel, error).await;
    }
}

#[allow(dead_code)]
async fn run_drafting_task_legacy<R: Runtime>(
    app: AppHandle<R>,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
    cancellation: CancellationToken,
) {
    let state = app.state::<AppState>();
    let result = async {
        let run = state.library_repository.get_note_pipeline_run(&run_id)?;
        let mut runtime = runtime_state(&run)?;
        let plan_version = runtime
            .plan_version
            .clone()
            .ok_or_else(|| "深度笔记计划尚未编译。".to_string())?;
        if plan_version.confirmed_at.is_none() {
            return Err("深度笔记计划尚未由用户确认。".to_string());
        }
        let live_conversation = state.conversation_repository.load(&run.conversation_id)?;
        let conversation = snapshot_conversation(
            &state.conversation_repository,
            &live_conversation,
            &runtime.input_snapshot,
        )
        .await?;
        let outline = parse_json_object::<DeepNoteOutline>(&run.outline_json)?;
        let selected_ids = run
            .selected_section_ids
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let selected_outline = outline.select(&selected_ids)?;
        let persisted = state
            .library_repository
            .list_note_pipeline_sections(&run_id)?;
        let persisted = persisted
            .into_iter()
            .map(|section| (section.section_id.clone(), section))
            .collect::<HashMap<_, _>>();
        let ledger_context = serde_json::to_string(&compact_ledger_for_planner(&runtime.ledger))
            .map_err(|error| format!("读取深度笔记知识账本失败：{error}"))?;
        let last_message_id = noteworthy_messages(&conversation)
            .last()
            .map(|message| message.id.clone());
        let total = selected_outline.sections.len();
        let mut drafts: Vec<(DeepNoteSection, String, bool)> = Vec::new();
        for (index, section) in selected_outline.sections.iter().enumerate() {
            if cancellation.is_cancelled() {
                break;
            }
            if let Some(existing) = persisted.get(&section.id) {
                if existing.status == NotePipelineSectionStatus::Completed {
                    drafts.push((section.clone(), existing.markdown.clone(), false));
                    continue;
                }
            }
            progress(
                &state,
                &channel,
                &run_id,
                NotePipelinePhase::Drafting,
                Some(index + 1),
                Some(total),
                format!("正在扩写 {}/{}：{}", index + 1, total, section.heading),
            );
            let (source_context, using_ledger_summary) = section_source_context(
                &conversation,
                section,
                &runtime.ledger,
                &runtime.context_budget,
            )?;
            if using_ledger_summary {
                let _ = state.library_repository.append_note_pipeline_event(
                    &run_id,
                    "sectionUsingChunkLedger",
                    Some(&section.id),
                    &serde_json::json!({
                        "sectionId": section.id,
                        "sourceMessageCount": section.source_message_ids.len(),
                    })
                    .to_string(),
                );
            }
            let prompt = section_prompt(
                &selected_outline,
                section,
                &source_context,
                &ledger_context,
                "",
            )?;
            let mut last_error = String::new();
            let mut markdown = None;
            let mut validation = DeepNoteValidationReport {
                passed: false,
                errors: vec!["章节尚未生成。".to_string()],
                warnings: Vec::new(),
                checked_evidence_ids: Vec::new(),
                criteria_coverage: Vec::new(),
            };
            let mut attempts = persisted
                .get(&section.id)
                .map(|existing| existing.attempt_count)
                .unwrap_or(0);
            let mut revisions = persisted
                .get(&section.id)
                .map(|existing| existing.revision_count)
                .unwrap_or(0);
            let node_attempt_limit = runtime.budget.node_attempt_limit.max(1);
            let section_revision_limit = runtime.budget.section_revision_limit;
            'attempts: while attempts < node_attempt_limit {
                if cancellation.is_cancelled() {
                    break;
                }
                attempts += 1;
                consume_semantic_call(&state, &run_id, &mut runtime)?;
                match model_call_with_runtime(
                    &state,
                    &run,
                    "deepNote",
                    NotePipelinePhase::Drafting,
                    SECTION_SYSTEM_PROMPT.to_string(),
                    prompt.clone(),
                    run.max_output_tokens.min(SECTION_OUTPUT_TOKEN_LIMIT),
                    run.retry_attempts,
                    &cancellation,
                    Some(&channel),
                )
                .await
                {
                    Ok(value) if !value.trim().is_empty() => {
                        let mut candidate = normalize_generated_markdown(value.trim());
                        validation = validate_section_markdown(section, &candidate, &[]);
                        // mermaid 笔误也进这个循环：命中即渲染失败。循环退出后
                        // 下面只看 passed，所以额度用尽仍有笔误时会照常收下。
                        while (!validation.passed || has_mermaid_lint_warning(&validation))
                            && revisions < section_revision_limit
                        {
                            revisions += 1;
                            consume_semantic_call(&state, &run_id, &mut runtime)?;
                            let revision_prompt = format!(
                                "章节计划：\n{}\n\n当前正文：\n{}\n\n验证报告：\n{}",
                                serde_json::to_string(section).map_err(|error| error.to_string())?,
                                candidate,
                                serde_json::to_string(&validation)
                                    .map_err(|error| error.to_string())?,
                            );
                            let revision_result = model_call_with_runtime(
                                &state,
                                &run,
                                "deepNote",
                                NotePipelinePhase::Validating,
                                SECTION_REVISION_SYSTEM_PROMPT.to_string(),
                                revision_prompt,
                                run.max_output_tokens.min(SECTION_OUTPUT_TOKEN_LIMIT),
                                run.retry_attempts,
                                &cancellation,
                                Some(&channel),
                            )
                            .await;
                            if cancellation.is_cancelled() {
                                attempts = attempts.saturating_sub(1);
                                revisions = revisions.saturating_sub(1);
                                break 'attempts;
                            }
                            candidate = match revision_result {
                                Ok(value) => normalize_generated_markdown(value.trim()),
                                // 只剩 mermaid 笔误时，一次失败的修订请求不该把
                                // 整次运行拖死；收下已经通过验证的稿子退出循环。
                                // 笔误本身会随 validation_json 一起存进 checkpoint，
                                // 不会无声无息。
                                Err(_) if validation.passed => break,
                                Err(error) => return Err(error.message),
                            };
                            validation = validate_section_markdown(section, &candidate, &[]);
                        }
                        if validation.passed {
                            markdown = Some(candidate);
                            break;
                        }
                        last_error = validation.errors.join("；");
                    }
                    Ok(_) => last_error = "模型返回了空章节。".to_string(),
                    Err(_error) if cancellation.is_cancelled() => {
                        attempts = attempts.saturating_sub(1);
                        break;
                    }
                    Err(error) => last_error = error.message,
                }
            }
            if let Some(markdown) = markdown {
                let validation_json =
                    serde_json::to_string(&validation).map_err(|error| error.to_string())?;
                {
                    let _guard = state.library_operations.lock().await;
                    state
                        .library_repository
                        .save_note_pipeline_section_checkpoint(
                            &run_id,
                            &section.id,
                            &markdown,
                            NotePipelineSectionStatus::Completed,
                            attempts,
                            revisions,
                            &section.source_message_ids,
                            &validation_json,
                            None,
                        )?;
                    state.library_repository.append_note_pipeline_event(
                        &run_id,
                        "sectionCompleted",
                        Some(&section.id),
                        &serde_json::json!({
                            "sectionId": section.id,
                            "heading": section.heading,
                            "attemptCount": attempts,
                            "revisionCount": revisions,
                            "markdownChars": markdown.chars().count(),
                        })
                        .to_string(),
                    )?;
                }
                drafts.push((section.clone(), markdown, false));
            } else if !cancellation.is_cancelled() {
                let validation_json =
                    serde_json::to_string(&validation).map_err(|error| error.to_string())?;
                {
                    let _guard = state.library_operations.lock().await;
                    state
                        .library_repository
                        .save_note_pipeline_section_checkpoint(
                            &run_id,
                            &section.id,
                            "",
                            NotePipelineSectionStatus::Failed,
                            attempts,
                            revisions,
                            &section.source_message_ids,
                            &validation_json,
                            Some(&last_error),
                        )?;
                    state.library_repository.append_note_pipeline_event(
                        &run_id,
                        "sectionFailed",
                        Some(&section.id),
                        &serde_json::json!({
                            "sectionId": section.id,
                            "heading": section.heading,
                            "attemptCount": attempts,
                            "revisionCount": revisions,
                            "message": last_error,
                        })
                        .to_string(),
                    )?;
                }
                return Err(format!(
                    "章节“{}”在 {} 次节点尝试和 {} 次语义修订后仍未通过验证：{}",
                    section.heading, attempts, revisions, last_error
                ));
            }
        }
        if send_paused_if_requested(&state, &run_id, &channel)? {
            return Ok(());
        }
        let cancelled = cancellation.is_cancelled() || drafts.len() < total;
        if drafts.is_empty() {
            finish_interrupted_run(&state, &run_id, &channel).await?;
            return Ok(());
        }
        {
            let _guard = state.library_operations.lock().await;
            let current = state.library_repository.get_note_pipeline_run(&run_id)?;
            if current.phase == NotePipelinePhase::Paused {
                send(&channel, NotePipelineProgress::Paused { run: current });
                return Ok(());
            }
            state.library_repository.update_note_pipeline_phase(
                &run_id,
                NotePipelinePhase::Assembling,
                None,
                &[],
                None,
            )?;
        }
        progress(
            &state,
            &channel,
            &run_id,
            NotePipelinePhase::Assembling,
            None,
            None,
            "正在组装与检查笔记…",
        );
        let effective_outline = DeepNoteOutline {
            sections: drafts
                .iter()
                .map(|(section, _, _)| section.clone())
                .collect(),
            ..selected_outline
        };
        let (mut title, content, mut warnings) = assemble(&effective_outline, &drafts, false);
        if cancelled {
            title = format!("{title}（部分完成）");
            warnings.push(format!(
                "任务已取消；已保存 {} 个完成章节，另有 {} 个章节未生成。",
                drafts.len(),
                total.saturating_sub(drafts.len())
            ));
        }
        let sidecar = sidecar_json(&run, &plan_version, &drafts, &HashMap::new())?;
        progress(
            &state,
            &channel,
            &run_id,
            NotePipelinePhase::Persisting,
            None,
            None,
            if cancelled {
                "正在保存已完成章节为草稿…"
            } else {
                "正在保存笔记与来源…"
            },
        );
        let sources = note_sources(&conversation.id, last_message_id.as_deref(), &drafts);
        {
            let _guard = state.library_operations.lock().await;
            state.library_repository.update_note_pipeline_phase(
                &run_id,
                NotePipelinePhase::Persisting,
                None,
                &warnings,
                None,
            )?;
            state.library_repository.update_note_pipeline_runtime_json(
                &run_id,
                &serde_json::to_string(&runtime.budget).map_err(|error| error.to_string())?,
                &serde_json::to_string(&runtime).map_err(|error| error.to_string())?,
                Some(&sidecar),
            )?;
        }
        // 与完整路径共用同一个单事务提交方法：降级交付同样不允许留下
        // “笔记已建、note_id 未写”的中间态。
        let attachment_sources = note_attachment_sources(&state, &conversation)?;
        let (_note, completed) = {
            let _guard = state.library_operations.lock().await;
            let create = LibraryNoteCreate {
                item_id: None,
                title,
                content,
                group_name: None,
            };
            state.library_repository.commit_deep_note_and_complete_run(
                &run_id,
                create,
                sources,
                &conversation.id,
                &runtime.input_snapshot,
                &sidecar,
                attachment_sources,
                runtime.force_rebuild,
                &warnings,
                cancelled,
            )?
        };
        cleanup_transferred_local_source(&state, &run_id, &conversation).await;
        send(
            &channel,
            NotePipelineProgress::Done {
                run: completed,
                degraded: cancelled,
            },
        );
        Ok(())
    }
    .await;
    if let Err(error) = result {
        persist_error(&state, &run_id, &channel, error).await;
    }
}

fn note_pipeline_event_tail(state: &AppState, run_id: &str) -> serde_json::Value {
    let events = state
        .library_repository
        .list_note_pipeline_events(run_id, 12)
        .unwrap_or_default()
        .into_iter()
        .map(
            |(sequence, event_type, node_id, payload_json, created_at)| {
                serde_json::json!({
                    "sequence": sequence,
                    "eventType": event_type,
                    "nodeId": node_id,
                    "payload": serde_json::from_str::<serde_json::Value>(&payload_json)
                        .unwrap_or_else(|_| serde_json::json!({ "invalidPayload": true })),
                    "createdAt": created_at,
                })
            },
        )
        .collect::<Vec<_>>();
    serde_json::Value::Array(events)
}

async fn supervise_note_pipeline_task<R: Runtime>(
    app: AppHandle<R>,
    run_id: String,
    instance_id: String,
    task_kind: String,
    channel: Channel<NotePipelineProgress>,
    join: tauri::async_runtime::JoinHandle<()>,
) {
    let state = app.state::<AppState>();
    let mut join = join;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat_enabled = true;
    let joined = loop {
        tokio::select! {
            result = &mut join => break result,
            _ = heartbeat.tick(), if heartbeat_enabled => {
                if state.library_repository
                    .heartbeat_note_pipeline_runtime(&run_id, &instance_id)
                    .is_err()
                {
                    // 终态或实例已被安全接管：停止心跳，但仍等待本 Worker 退出，
                    // 后续状态写入会由 runtime_instance_id / lease CAS 拒绝。
                    heartbeat_enabled = false;
                }
            }
        }
    };
    if state
        .note_pipeline_task_snapshot(&run_id)
        .await
        .map_or(true, |snapshot| snapshot.instance_id != instance_id)
    {
        let _ = state
            .library_repository
            .release_note_pipeline_runtime(&run_id, &instance_id);
        state.clear_detached_note_pipeline_instance(&instance_id);
        return;
    }
    match joined {
        Ok(()) => {
            if let Ok(current) = state.library_repository.get_note_pipeline_run(&run_id) {
                if current.phase == NotePipelinePhase::Cancelling {
                    if let Ok(cancelled) = state
                        .library_repository
                        .finalize_note_pipeline_cancellation(
                            &run_id,
                            false,
                            "cooperative-task-exit",
                            None,
                        )
                    {
                        send(&channel, NotePipelineProgress::Cancelled { run: cancelled });
                    }
                } else if !matches!(
                    current.phase,
                    NotePipelinePhase::Done
                        | NotePipelinePhase::Cancelled
                        | NotePipelinePhase::Paused
                        | NotePipelinePhase::AwaitingOutline
                        | NotePipelinePhase::Error
                ) {
                    let diagnostic_path = state
                        .task_diagnostic_log
                        .record_note_pipeline(
                            "workerExitedWithoutTerminalState",
                            &task_kind,
                            &run_id,
                            "后台任务正常返回，但没有写入可恢复或终态阶段。",
                            serde_json::json!({
                                "phase": current.phase.as_str(),
                                "eventTail": note_pipeline_event_tail(&state, &run_id),
                            }),
                        )
                        .ok();
                    let message = diagnostic_path.as_deref().map_or_else(
                        || "深度笔记后台任务未写入终态。".to_string(),
                        |path| format!("深度笔记后台任务未写入终态。诊断日志：{path}"),
                    );
                    if let Some(path) = diagnostic_path.as_deref() {
                        let _ = state
                            .library_repository
                            .fail_note_pipeline_task(&run_id, &message, path);
                    }
                    send(
                        &channel,
                        NotePipelineProgress::Error {
                            run_id: run_id.clone(),
                            message,
                        },
                    );
                }
            }
        }
        Err(error) => {
            let (panicked, aborted, join_message) = match &error {
                tauri::Error::JoinError(join_error) => (
                    join_error.is_panic(),
                    join_error.is_cancelled(),
                    join_error.to_string(),
                ),
                _ => (false, false, error.to_string()),
            };
            let snapshot = state.note_pipeline_task_snapshot(&run_id).await;
            let diagnostic_path = state
                .task_diagnostic_log
                .record_note_pipeline(
                    if panicked {
                        "panic"
                    } else if aborted {
                        "aborted"
                    } else {
                        "joinFailure"
                    },
                    &task_kind,
                    &run_id,
                    &join_message,
                    serde_json::json!({
                        "task": snapshot.map(|value| serde_json::json!({
                            "instanceId": value.instance_id,
                            "kind": value.task_kind,
                            "startedAt": value.started_at_ms,
                            "cancellationRequested": value.cancellation_requested,
                            "abortable": value.abortable,
                        })),
                        "eventTail": note_pipeline_event_tail(&state, &run_id),
                    }),
                )
                .ok();
            let current = state.library_repository.get_note_pipeline_run(&run_id).ok();
            if current
                .as_ref()
                .is_some_and(|run| run.phase == NotePipelinePhase::Cancelling)
                || aborted
            {
                if let Ok(cancelled) = state
                    .library_repository
                    .finalize_note_pipeline_cancellation(
                        &run_id,
                        aborted,
                        if panicked {
                            "task-panicked-while-cancelling"
                        } else {
                            "task-aborted"
                        },
                        diagnostic_path.as_deref(),
                    )
                {
                    send(&channel, NotePipelineProgress::Cancelled { run: cancelled });
                }
            } else {
                let message = diagnostic_path.as_deref().map_or_else(
                    || format!("深度笔记后台任务异常终止：{join_message}。"),
                    |path| format!("深度笔记后台任务异常终止：{join_message}。诊断日志：{path}"),
                );
                if let Some(path) = diagnostic_path.as_deref() {
                    let _ = state
                        .library_repository
                        .fail_note_pipeline_task(&run_id, &message, path);
                }
                send(
                    &channel,
                    NotePipelineProgress::Error {
                        run_id: run_id.clone(),
                        message,
                    },
                );
            }
        }
    }
    state.clear_detached_note_pipeline_instance(&instance_id);
    state.finish_note_pipeline_run(&run_id, &instance_id).await;
    let _ = state
        .library_repository
        .release_note_pipeline_runtime(&run_id, &instance_id);
}

async fn spawn_analysis<R: Runtime>(
    app: &AppHandle<R>,
    run_id: String,
    adjustment: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<(), String> {
    let cancellation = CancellationToken::new();
    let instance_id = Uuid::new_v4().to_string();
    let state = app.state::<AppState>();
    if !state
        .register_note_pipeline_run(
            run_id.clone(),
            cancellation.clone(),
            "deep-note-analysis",
            instance_id.clone(),
        )
        .await
    {
        return Err("深度笔记任务已经在运行。".to_string());
    }
    if let Err(error) = state
        .library_repository
        .claim_note_pipeline_runtime(&run_id, &instance_id)
    {
        state.finish_note_pipeline_run(&run_id, &instance_id).await;
        return Err(error);
    }
    let worker_app = app.clone();
    let worker_channel = channel.clone();
    let worker_run_id = run_id.clone();
    let join = tauri::async_runtime::spawn(scope_task_diagnostics(
        TaskDiagnosticContext::note_pipeline(
            "deep-note-analysis",
            worker_run_id.clone(),
            instance_id.clone(),
        ),
        run_analysis_task(
            worker_app,
            worker_run_id,
            adjustment,
            worker_channel,
            cancellation,
        ),
    ));
    let abort_handle = join.inner().abort_handle();
    if !state
        .attach_note_pipeline_abort_handle(&run_id, &instance_id, abort_handle)
        .await
    {
        join.abort();
        state.finish_note_pipeline_run(&run_id, &instance_id).await;
        let _ = state
            .library_repository
            .release_note_pipeline_runtime(&run_id, &instance_id);
        return Err("深度笔记分析任务注册在启动期间失效。".to_string());
    }
    tauri::async_runtime::spawn(supervise_note_pipeline_task(
        app.clone(),
        run_id,
        instance_id,
        "deep-note-analysis".to_string(),
        channel,
        join,
    ));
    Ok(())
}

async fn spawn_drafting<R: Runtime>(
    app: &AppHandle<R>,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<(), String> {
    let cancellation = CancellationToken::new();
    let instance_id = Uuid::new_v4().to_string();
    let state = app.state::<AppState>();
    // 兜底守卫：所有起草 Worker 都从这里进入。run 上已有 note_id 意味着这一轮
    // 的产物早已落库，再起草一次只会产生第二篇内容几乎相同的笔记。
    // `dispatch_checkpoint` 已在 Persisting 分支单独处理，这里覆盖它之外的调用路径。
    {
        let existing = state.library_repository.get_note_pipeline_run(&run_id)?;
        if existing
            .note_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
        {
            return Err("该深度笔记任务已经产出笔记，拒绝重复起草。".to_string());
        }
    }
    if !state
        .register_note_pipeline_run(
            run_id.clone(),
            cancellation.clone(),
            "deep-note-drafting",
            instance_id.clone(),
        )
        .await
    {
        return Err("深度笔记任务已经在运行。".to_string());
    }
    if let Err(error) = state
        .library_repository
        .claim_note_pipeline_runtime(&run_id, &instance_id)
    {
        state.finish_note_pipeline_run(&run_id, &instance_id).await;
        return Err(error);
    }
    let worker_app = app.clone();
    let worker_channel = channel.clone();
    let worker_run_id = run_id.clone();
    let join = tauri::async_runtime::spawn(scope_task_diagnostics(
        TaskDiagnosticContext::note_pipeline(
            "deep-note-drafting",
            worker_run_id.clone(),
            instance_id.clone(),
        ),
        run_drafting_task(worker_app, worker_run_id, worker_channel, cancellation),
    ));
    let abort_handle = join.inner().abort_handle();
    if !state
        .attach_note_pipeline_abort_handle(&run_id, &instance_id, abort_handle)
        .await
    {
        join.abort();
        state.finish_note_pipeline_run(&run_id, &instance_id).await;
        let _ = state
            .library_repository
            .release_note_pipeline_runtime(&run_id, &instance_id);
        return Err("深度笔记章节任务注册在启动期间失效。".to_string());
    }
    tauri::async_runtime::spawn(supervise_note_pipeline_task(
        app.clone(),
        run_id,
        instance_id,
        "deep-note-drafting".to_string(),
        channel,
        join,
    ));
    Ok(())
}

fn validate_start_inspection(
    request: &NotePipelineStartRequest,
    inspection: &DeepNoteStartInspection,
) -> Result<(), String> {
    if !inspection.unsupported_attachment_names.is_empty() {
        return Err(format!(
            "当前会话包含深度笔记尚不支持或不允许自动读取的附件：{}。请先转换格式、移除附件或显式清理敏感内容。",
            inspection.unsupported_attachment_names.join("、")
        ));
    }
    match inspection.status.as_str() {
        "new" => Ok(()),
        "invalidated" if request.replace_invalidated || request.force_rebuild => Ok(()),
        "invalidated" => Err(format!(
            "{} 如需重新生成，请先明确确认替换失效快照。",
            inspection.message
        )),
        "updateAvailable" | "upToDate" if request.force_rebuild => Ok(()),
        "updateAvailable" | "upToDate" => Err(inspection.message.clone()),
        _ => Err("已有深度笔记检查返回了未知状态。".to_string()),
    }
}

pub async fn start<R: Runtime>(
    app: &AppHandle<R>,
    request: NotePipelineStartRequest,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let inspection = inspect_start(&state, request.conversation_id.trim()).await?;
    validate_start_inspection(&request, &inspection)?;
    let conversation = state
        .conversation_repository
        .load(request.conversation_id.trim())?;
    let (provider_id, model_id, mut preflight) = {
        let settings = state
            .model_settings
            .read()
            .map_err(|_| "模型设置锁不可用。".to_string())?;
        let (provider_id, model_id) = resolve_note_model(&settings, &conversation)?;
        let preflight = preflight(&settings, &conversation, &provider_id, &model_id)?;
        (provider_id, model_id, preflight)
    };
    if request.force_rebuild {
        preflight
            .warnings
            .push("本次按当前全部消息与附件执行完整来源重建；已有笔记会保留。".to_string());
    }
    if !preflight.ready {
        return Err(format!(
            "当前模型无法启动深度笔记：{}。请切换模型、移除不支持的附件或返回设置。",
            preflight.missing_capabilities.join("；")
        ));
    }
    let (skill_profiles, skill_warnings) =
        snapshot_skill_profiles(&state, preflight.requires_vision);
    preflight.warnings.extend(skill_warnings);
    let (max_output_tokens, thinking_enabled, retry_attempts) = {
        let settings = state
            .app_settings
            .read()
            .map_err(|_| "应用设置锁不可用。".to_string())?;
        (
            settings.max_output_tokens,
            settings.thinking_enabled,
            if settings.retry_enabled {
                settings.retry_attempts
            } else {
                0
            },
        )
    };
    let created_at = conversation.updated_at.max(1);
    let snapshot = create_input_snapshot(
        &state.conversation_repository,
        &conversation,
        preflight.model.clone(),
        created_at,
    )
    .await?;
    let snapshot_json = serde_json::to_string(&snapshot)
        .map_err(|error| format!("序列化深度笔记输入快照失败：{error}"))?;
    let snapshot_hash = stable_hash(&snapshot_json);
    let runtime = DeepNoteRuntimeState {
        preflight: preflight.clone(),
        input_snapshot: snapshot,
        plan_version: None,
        budget: DeepNoteBudget::for_section_count(1),
        ledger: DeepNoteLedger::default(),
        skill_profiles: skill_profiles.clone(),
        context_budget: DeepNoteContextBudget::default(),
        force_rebuild: request.force_rebuild,
        section_active_ms: BTreeMap::new(),
    };
    let runtime_json = serde_json::to_string(&runtime)
        .map_err(|error| format!("序列化深度笔记运行状态失败：{error}"))?;
    let budget_json = serde_json::to_string(&runtime.budget)
        .map_err(|error| format!("序列化深度笔记预算失败：{error}"))?;
    let run_id = Uuid::new_v4().to_string();
    let idempotency_key = deep_note_content_signature(
        &conversation.id,
        &snapshot_hash,
        &provider_id,
        &model_id,
        max_output_tokens,
        thinking_enabled,
        request.force_rebuild,
    );
    let run = {
        let _guard = state.library_operations.lock().await;
        state
            .library_repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: run_id.clone(),
                conversation_id: conversation.id,
                provider_id,
                model_id,
                max_output_tokens,
                thinking_enabled,
                retry_attempts,
                input_snapshot_hash: snapshot_hash,
                budget_json,
                preflight_json: runtime_json,
                idempotency_key,
            })?
    };
    {
        let _guard = state.library_operations.lock().await;
        state.library_repository.append_note_pipeline_event(
            &run.id,
            "preflightCompleted",
            None,
            &serde_json::to_string(&preflight).map_err(|error| error.to_string())?,
        )?;
        state.library_repository.append_note_pipeline_event(
            &run.id,
            "skillProfileLoaded",
            None,
            &serde_json::json!({
                "planner": skill_profiles.planner.iter().map(|skill| serde_json::json!({
                    "skillId": skill.skill_id,
                    "version": skill.version,
                    "contentHash": skill.content_hash,
                })).collect::<Vec<_>>(),
                "writer": skill_profiles.writer.iter().map(|skill| serde_json::json!({
                    "skillId": skill.skill_id,
                    "version": skill.version,
                    "contentHash": skill.content_hash,
                })).collect::<Vec<_>>(),
                "reviewer": skill_profiles.reviewer.iter().map(|skill| serde_json::json!({
                    "skillId": skill.skill_id,
                    "version": skill.version,
                    "contentHash": skill.content_hash,
                })).collect::<Vec<_>>(),
            })
            .to_string(),
        )?;
        state.library_repository.update_note_pipeline_phase(
            &run.id,
            NotePipelinePhase::Analyzing,
            None,
            &preflight.warnings,
            None,
        )?;
    }
    spawn_analysis(app, run.id.clone(), String::new(), channel).await?;
    Ok(run)
}

fn coverage_snapshot_for_note(
    state: &AppState,
    note_id: &str,
    conversation_id: &str,
) -> Result<Option<DeepNoteInputSnapshot>, String> {
    if let Some(snapshot) = state
        .library_repository
        .deep_note_coverage_snapshot(note_id, conversation_id)?
    {
        return Ok(Some(snapshot));
    }
    let Some(runtime_json) = state
        .library_repository
        .latest_completed_deep_note_runtime_json(note_id, conversation_id)?
    else {
        return Ok(None);
    };
    let runtime = serde_json::from_str::<DeepNoteRuntimeState>(&runtime_json)
        .map_err(|error| format!("读取已有深度笔记覆盖快照失败：{error}"))?;
    Ok(Some(runtime.input_snapshot))
}

fn inspect_attachment_delta(messages: &[&StoredChatMessage]) -> (usize, Vec<String>) {
    let attachments = messages
        .iter()
        .flat_map(|message| message.attachments.iter())
        .collect::<Vec<_>>();
    let mut unsupported = attachments
        .iter()
        .filter(|attachment| {
            attachment_formats::deep_note_read_kind(attachment) == AttachmentReadKind::Unsupported
        })
        .map(|attachment| unsupported_attachment_label(attachment))
        .collect::<Vec<_>>();
    unsupported.sort();
    unsupported.dedup();
    (attachments.len(), unsupported)
}

fn unsupported_attachment_label(
    attachment: &crate::chat::conversation_types::StoredChatAttachment,
) -> String {
    if attachment_formats::is_sensitive_text_name(&attachment.name) {
        format!("{}（敏感配置，禁止自动读取）", attachment.name)
    } else {
        attachment.name.clone()
    }
}

pub async fn inspect_start(
    state: &AppState,
    conversation_id: &str,
) -> Result<DeepNoteStartInspection, String> {
    let conversation = state.conversation_repository.load(conversation_id.trim())?;
    let Some((note, anchor)) = state
        .library_repository
        .latest_deep_note_for_conversation(&conversation.id)?
    else {
        let messages = noteworthy_messages(&conversation);
        let (new_attachment_count, unsupported_attachment_names) =
            inspect_attachment_delta(&messages);
        return Ok(DeepNoteStartInspection {
            status: "new".to_string(),
            note_id: None,
            note_title: None,
            covered_message_id: None,
            covered_message_count: 0,
            new_message_count: messages.len(),
            new_attachment_count,
            requires_full_rebuild: false,
            unsupported_attachment_names,
            message: "当前会话还没有可增量更新的深度笔记。".to_string(),
        });
    };
    let messages = noteworthy_messages(&conversation);
    let Some(anchor) = anchor else {
        return Ok(DeepNoteStartInspection {
            status: "invalidated".to_string(),
            note_id: Some(note.id),
            note_title: Some(note.title),
            covered_message_id: None,
            covered_message_count: 0,
            new_message_count: messages.len(),
            new_attachment_count: inspect_attachment_delta(&messages).0,
            requires_full_rebuild: true,
            unsupported_attachment_names: inspect_attachment_delta(&messages).1,
            message: "已有笔记缺少可靠的消息锚点，需要重新生成。".to_string(),
        });
    };
    let Some(snapshot) = coverage_snapshot_for_note(state, &note.id, &conversation.id)? else {
        return Ok(DeepNoteStartInspection {
            status: "invalidated".to_string(),
            note_id: Some(note.id),
            note_title: Some(note.title),
            covered_message_id: Some(anchor),
            covered_message_count: 0,
            new_message_count: messages.len(),
            new_attachment_count: inspect_attachment_delta(&messages).0,
            requires_full_rebuild: true,
            unsupported_attachment_names: inspect_attachment_delta(&messages).1,
            message: "已有笔记缺少逐消息与附件内容 Hash，不能安全增量更新；请重新生成。"
                .to_string(),
        });
    };
    if let Err(error) = validate_recovery_snapshot_from_storage(
        &state.conversation_repository,
        &conversation,
        &snapshot,
    )
    .await
    {
        return Ok(DeepNoteStartInspection {
            status: "invalidated".to_string(),
            note_id: Some(note.id),
            note_title: Some(note.title),
            covered_message_id: Some(anchor),
            covered_message_count: 0,
            new_message_count: messages.len(),
            new_attachment_count: inspect_attachment_delta(&messages).0,
            requires_full_rebuild: true,
            unsupported_attachment_names: inspect_attachment_delta(&messages).1,
            message: format!("已有笔记覆盖快照已失效：{error}"),
        });
    }
    if snapshot.message_ids.last() != Some(&anchor) {
        return Ok(DeepNoteStartInspection {
            status: "invalidated".to_string(),
            note_id: Some(note.id),
            note_title: Some(note.title),
            covered_message_id: Some(anchor),
            covered_message_count: snapshot.message_ids.len(),
            new_message_count: messages.len().saturating_sub(snapshot.message_ids.len()),
            new_attachment_count: inspect_attachment_delta(&messages).0,
            requires_full_rebuild: true,
            unsupported_attachment_names: inspect_attachment_delta(&messages).1,
            message: "已有笔记的来源锚点与覆盖快照不一致，不能安全增量更新；请重新生成。"
                .to_string(),
        });
    }
    let anchor_index = snapshot.message_ids.len().saturating_sub(1);
    let new_message_count = messages.len().saturating_sub(anchor_index + 1);
    let new_messages = &messages[anchor_index.saturating_add(1)..];
    let (new_attachment_count, unsupported_attachment_names) =
        inspect_attachment_delta(new_messages);
    let requires_full_rebuild = false;
    let message = if new_message_count == 0 {
        "已有深度笔记已经覆盖当前会话，没有新的消息需要合入。".to_string()
    } else if !unsupported_attachment_names.is_empty() {
        format!(
            "已有深度笔记；检测到 {new_message_count} 条未合入的新消息和 {new_attachment_count} 个附件，其中这些格式暂不支持：{}。完整重建也无法安全读取它们。",
            unsupported_attachment_names.join("、")
        )
    } else if new_attachment_count > 0 {
        format!(
            "已有深度笔记；检测到 {new_message_count} 条未合入的新消息和 {new_attachment_count} 个附件。可以只读取新增附件并生成带来源的增量更新提案。"
        )
    } else {
        format!("已有深度笔记；检测到 {new_message_count} 条未合入的新消息。")
    };
    Ok(DeepNoteStartInspection {
        status: if new_message_count == 0 {
            "upToDate".to_string()
        } else {
            "updateAvailable".to_string()
        },
        note_id: Some(note.id),
        note_title: Some(note.title),
        covered_message_id: Some(anchor),
        covered_message_count: anchor_index + 1,
        new_message_count,
        new_attachment_count,
        requires_full_rebuild,
        unsupported_attachment_names,
        message,
    })
}

pub async fn adjust<R: Runtime>(
    app: &AppHandle<R>,
    request: NotePipelineAdjustRequest,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    if request.requirement.trim().is_empty() {
        return Err("请填写提纲补充要求。".to_string());
    }
    let state = app.state::<AppState>();
    let run = state
        .library_repository
        .get_note_pipeline_run(&request.run_id)?;
    if run.phase != NotePipelinePhase::AwaitingOutline {
        return Err("当前任务不能调整提纲。".to_string());
    }
    let mut runtime = runtime_state(&run)?;
    if runtime.budget.replans_used >= runtime.budget.replan_limit {
        return Err(format!(
            "提纲调整已达到 {} 次上限；请确认当前计划，或重新生成深度笔记。",
            runtime.budget.replan_limit
        ));
    }
    runtime.budget.replans_used = runtime.budget.replans_used.saturating_add(1);
    let run = {
        let _guard = state.library_operations.lock().await;
        save_runtime_state(&state, &run.id, &runtime)?;
        state.library_repository.append_note_pipeline_event(
            &run.id,
            "outlineAdjustmentRequested",
            None,
            &serde_json::json!({
                "replansUsed": runtime.budget.replans_used,
                "replanLimit": runtime.budget.replan_limit,
            })
            .to_string(),
        )?;
        state.library_repository.update_note_pipeline_phase(
            &run.id,
            NotePipelinePhase::Analyzing,
            None,
            &[],
            None,
        )?
    };
    spawn_analysis(app, run.id.clone(), request.requirement, channel).await?;
    Ok(run)
}

pub async fn confirm<R: Runtime>(
    app: &AppHandle<R>,
    request: NotePipelineConfirmRequest,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let run = {
        let _guard = state.library_operations.lock().await;
        state
            .library_repository
            .select_note_pipeline_sections(&request.run_id, request.selected_section_ids)?
    };
    let mut runtime = runtime_state(&run)?;
    let selected = run
        .selected_section_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let mut plan_version = runtime
        .plan_version
        .clone()
        .ok_or_else(|| "深度笔记计划尚未生成。".to_string())?;
    plan_version.plan = plan_version.plan.select(&selected)?;
    plan_version = compile_plan(
        &run.id,
        plan_version.version,
        plan_version.plan,
        &run.input_snapshot_hash,
        &plan_version.revision_reason,
    )?;
    plan_version.confirmed_at = Some(run.updated_at.max(1));
    runtime.plan_version = Some(plan_version.clone());
    runtime.budget = budget_for_drafting(&runtime.budget, plan_version.plan.sections.len());
    let plan_json = serde_json::to_string(&plan_version.plan).map_err(|error| error.to_string())?;
    let dag_json =
        serde_json::to_string(&plan_version.compiled_dag).map_err(|error| error.to_string())?;
    let node_rows = plan_version
        .compiled_dag
        .iter()
        .map(|node| {
            Ok((
                node.node_id.clone(),
                serde_json::to_value(node.node_type)
                    .map_err(|error| error.to_string())?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
                node.section_id.clone(),
                serde_json::to_string(&node.depends_on).map_err(|error| error.to_string())?,
                serde_json::to_value(node.status)
                    .map_err(|error| error.to_string())?
                    .as_str()
                    .unwrap_or("pending")
                    .to_string(),
                node.input_hash.clone(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    {
        let _guard = state.library_operations.lock().await;
        state.library_repository.save_note_pipeline_plan_version(
            &run.id,
            plan_version.version,
            &plan_version.plan_id,
            &plan_json,
            &dag_json,
            &plan_version.plan_hash,
            &plan_version.revision_reason,
            plan_version.confirmed_at,
        )?;
        state.library_repository.replace_note_pipeline_nodes(
            &run.id,
            plan_version.version,
            &node_rows,
        )?;
        save_runtime_state(&state, &run.id, &runtime)?;
        state.library_repository.append_note_pipeline_event(
            &run.id,
            "planConfirmed",
            None,
            &serde_json::json!({
                "planId": plan_version.plan_id,
                "version": plan_version.version,
                "planHash": plan_version.plan_hash,
            })
            .to_string(),
        )?;
        state.library_repository.update_note_pipeline_phase(
            &run.id,
            NotePipelinePhase::Drafting,
            None,
            &run.warnings,
            None,
        )?;
    }
    let run = state.library_repository.get_note_pipeline_run(&run.id)?;
    spawn_drafting(app, run.id.clone(), channel).await?;
    Ok(run)
}

async fn dispatch_checkpoint<R: Runtime>(
    app: &AppHandle<R>,
    run: NotePipelineRun,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    match run.phase {
        NotePipelinePhase::Preflight => {
            {
                let _guard = state.library_operations.lock().await;
                state.library_repository.update_note_pipeline_phase(
                    &run.id,
                    NotePipelinePhase::Analyzing,
                    None,
                    &run.warnings,
                    None,
                )?;
            }
            spawn_analysis(app, run.id.clone(), String::new(), channel).await?;
        }
        NotePipelinePhase::AwaitingOutline => {
            send(
                &channel,
                NotePipelineProgress::OutlineReady { run: run.clone() },
            );
        }
        NotePipelinePhase::Analyzing => {
            spawn_analysis(app, run.id.clone(), String::new(), channel).await?;
        }
        // Persisting 必须先于 catch-all 分支单独判定。
        // 它是唯一一个“笔记可能已经落库”的可恢复阶段：若 note_id 已存在，
        // 说明提交事务已经成功，只是终态推进或 Worker 退出被打断。此时重新
        // spawn_drafting 会把整篇笔记重写一遍，产出第二篇几乎相同的笔记。
        NotePipelinePhase::Persisting if run.note_id.is_some() => {
            let note_id = run.note_id.clone().unwrap_or_default();
            let completed = {
                let _guard = state.library_operations.lock().await;
                let completed = state.library_repository.update_note_pipeline_phase(
                    &run.id,
                    NotePipelinePhase::Done,
                    Some(&note_id),
                    &run.warnings,
                    None,
                )?;
                state.library_repository.append_note_pipeline_event(
                    &run.id,
                    "runCompleted",
                    None,
                    &serde_json::json!({
                        "noteId": note_id,
                        "completedSectionCount": completed.completed_section_ids.len(),
                        "failedSectionCount": completed.failed_section_ids.len(),
                        "degraded": false,
                        "reusedExistingNote": true,
                    })
                    .to_string(),
                )?;
                completed
            };
            send(
                &channel,
                NotePipelineProgress::Done {
                    run: completed,
                    degraded: false,
                },
            );
        }
        NotePipelinePhase::Compiling
        | NotePipelinePhase::Queued
        | NotePipelinePhase::Drafting
        | NotePipelinePhase::Validating
        | NotePipelinePhase::Replanning
        | NotePipelinePhase::Assembling
        | NotePipelinePhase::Persisting
        | NotePipelinePhase::Cancelling
        | NotePipelinePhase::Paused
        | NotePipelinePhase::Blocked
        | NotePipelinePhase::Cancelled
        | NotePipelinePhase::Error => {
            if run.outline_json.is_empty() {
                {
                    let _guard = state.library_operations.lock().await;
                    state.library_repository.update_note_pipeline_phase(
                        &run.id,
                        NotePipelinePhase::Analyzing,
                        None,
                        &[],
                        None,
                    )?;
                }
                spawn_analysis(app, run.id.clone(), String::new(), channel).await?;
            } else if run.selected_section_ids.is_empty() {
                send(
                    &channel,
                    NotePipelineProgress::OutlineReady { run: run.clone() },
                );
            } else {
                {
                    let _guard = state.library_operations.lock().await;
                    state.library_repository.update_note_pipeline_phase(
                        &run.id,
                        NotePipelinePhase::Drafting,
                        None,
                        &run.warnings,
                        None,
                    )?;
                }
                spawn_drafting(app, run.id.clone(), channel).await?;
            }
        }
        _ => return Err("该深度笔记任务不可恢复。".to_string()),
    }
    state.library_repository.get_note_pipeline_run(&run.id)
}

async fn prepare_manual_recovery<R: Runtime>(
    app: &AppHandle<R>,
    run: &NotePipelineRun,
    reset_failed_sections: bool,
    event_type: &str,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    if !wait_for_pipeline_task_to_stop(&state, &run.id).await {
        return Err("深度笔记后台任务仍在结束处理中，请稍后再试。".to_string());
    }
    let conversation = state.conversation_repository.load(&run.conversation_id)?;
    let mut runtime = runtime_state(run)?;
    validate_recovery_snapshot_from_storage(
        &state.conversation_repository,
        &conversation,
        &runtime.input_snapshot,
    )
    .await?;
    if reset_failed_sections {
        reset_failed_runtime_nodes(&mut runtime);
    }
    let recovered = {
        let _guard = state.library_operations.lock().await;
        let recovered = state
            .library_repository
            .prepare_note_pipeline_retry(&run.id, reset_failed_sections)?;
        save_runtime_state(&state, &run.id, &runtime)?;
        state.library_repository.append_note_pipeline_event(
            &run.id,
            event_type,
            None,
            &serde_json::json!({
                "executionVersion": recovered.execution_version,
                "resetFailedSections": reset_failed_sections,
            })
            .to_string(),
        )?;
        recovered
    };
    Ok(recovered)
}

pub async fn resume<R: Runtime>(
    app: &AppHandle<R>,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let run = state.library_repository.get_note_pipeline_run(&run_id)?;
    if run.abandoned {
        return Err("该深度笔记任务已遗弃，不能继续。".to_string());
    }
    if run.phase != NotePipelinePhase::Cancelled
        && !state.is_note_pipeline_run_active(&run.id).await
    {
        let conversation = state.conversation_repository.load(&run.conversation_id)?;
        let runtime = runtime_state(&run)?;
        validate_recovery_snapshot_from_storage(
            &state.conversation_repository,
            &conversation,
            &runtime.input_snapshot,
        )
        .await?;
    }
    let run = match run.phase {
        NotePipelinePhase::Paused => {
            if !wait_for_pipeline_task_to_stop(&state, &run.id).await {
                return Err("深度笔记任务仍在暂停处理中，请稍后再继续。".to_string());
            }
            state.library_repository.append_note_pipeline_event(
                &run.id,
                "runResumed",
                None,
                "{}",
            )?;
            run
        }
        NotePipelinePhase::Cancelled => {
            prepare_manual_recovery(app, &run, false, "runContinued").await?
        }
        NotePipelinePhase::Cancelling => {
            return Err("深度笔记任务仍在停止处理中，请等待终态或再次停止。".to_string())
        }
        NotePipelinePhase::Preflight
        | NotePipelinePhase::Analyzing
        | NotePipelinePhase::AwaitingOutline
        | NotePipelinePhase::Compiling
        | NotePipelinePhase::Queued
        | NotePipelinePhase::Drafting
        | NotePipelinePhase::Validating
        | NotePipelinePhase::Replanning
        | NotePipelinePhase::Assembling
        | NotePipelinePhase::Persisting => run,
        _ => return Err("只有已暂停或已停止的深度笔记任务可以继续。".to_string()),
    };
    dispatch_checkpoint(app, run, channel).await
}

pub async fn retry<R: Runtime>(
    app: &AppHandle<R>,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let run = state.library_repository.get_note_pipeline_run(&run_id)?;
    if run.abandoned {
        return Err("该深度笔记任务已遗弃，不能重试。".to_string());
    }
    if !matches!(
        run.phase,
        NotePipelinePhase::Error | NotePipelinePhase::Blocked
    ) {
        return Err("只有失败或阻塞的深度笔记任务可以重试。".to_string());
    }
    let recovered = prepare_manual_recovery(app, &run, true, "runRetryRequested").await?;
    dispatch_checkpoint(app, recovered, channel).await
}

pub async fn restart<R: Runtime>(
    app: &AppHandle<R>,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let previous = state.library_repository.get_note_pipeline_run(&run_id)?;
    if previous.abandoned {
        return Err("该深度笔记任务已遗弃，不能重新生成。".to_string());
    }
    if !matches!(
        previous.phase,
        NotePipelinePhase::Error | NotePipelinePhase::Blocked | NotePipelinePhase::Cancelled
    ) {
        return Err("当前深度笔记任务不能重新生成。".to_string());
    }
    if state.is_note_pipeline_run_active(&previous.id).await {
        return Err("旧的深度笔记后台任务仍在结束处理中，请稍后再试。".to_string());
    }
    if previous.phase != NotePipelinePhase::Cancelled {
        let _guard = state.library_operations.lock().await;
        state.library_repository.update_note_pipeline_phase(
            &previous.id,
            NotePipelinePhase::Cancelled,
            None,
            &previous.warnings,
            None,
        )?;
    }
    let result = start(
        app,
        NotePipelineStartRequest {
            conversation_id: previous.conversation_id.clone(),
            replace_invalidated: false,
            force_rebuild: false,
        },
        channel,
    )
    .await;
    match result {
        Ok(run) => {
            let _ = state.library_repository.append_note_pipeline_event(
                &previous.id,
                "runRestarted",
                None,
                &serde_json::json!({ "newRunId": run.id }).to_string(),
            );
            Ok(run)
        }
        Err(error) => {
            if previous.phase != NotePipelinePhase::Cancelled {
                let _ = state.library_repository.update_note_pipeline_phase(
                    &previous.id,
                    previous.phase,
                    previous.note_id.as_deref(),
                    &previous.warnings,
                    previous.error_message.as_deref(),
                );
            }
            Err(error)
        }
    }
}

pub async fn cancel<R: Runtime>(
    app: &AppHandle<R>,
    run_id: &str,
) -> Result<NotePipelineCancelResult, String> {
    let state = app.state::<AppState>();
    let initial = state.library_repository.get_note_pipeline_run(run_id)?;
    let force_immediately = initial.phase == NotePipelinePhase::Cancelling;
    let cancellation_signalled = state.cancel_note_pipeline_run(run_id).await;
    let requested = state
        .library_repository
        .request_note_pipeline_cancellation(run_id)?;
    if matches!(
        requested.phase,
        NotePipelinePhase::Done | NotePipelinePhase::Cancelled
    ) {
        return Ok(NotePipelineCancelResult {
            run: requested,
            forced: false,
            diagnostic_path: None,
        });
    }
    if !cancellation_signalled {
        let diagnostic_path = phase_expects_background_worker(initial.phase)
            .then(|| {
                state.task_diagnostic_log.record_note_pipeline(
                    "orphanedTaskRegistration",
                    "deep-note-unknown",
                    run_id,
                    "数据库显示任务仍在运行，但活动任务注册中没有对应 worker。",
                    serde_json::json!({
                        "phase": initial.phase.as_str(),
                        "eventTail": note_pipeline_event_tail(&state, run_id),
                    }),
                )
            })
            .transpose()
            .ok()
            .flatten();
        let run = state
            .library_repository
            .finalize_note_pipeline_cancellation(
                run_id,
                diagnostic_path.is_some(),
                "orphaned-task-registration",
                diagnostic_path.as_deref(),
            )?;
        return Ok(NotePipelineCancelResult {
            run,
            forced: diagnostic_path.is_some(),
            diagnostic_path,
        });
    }
    if !force_immediately && wait_for_pipeline_task_to_stop(&state, run_id).await {
        let current = state.library_repository.get_note_pipeline_run(run_id)?;
        let run = if current.phase == NotePipelinePhase::Cancelling {
            state
                .library_repository
                .finalize_note_pipeline_cancellation(
                    run_id,
                    false,
                    "cooperative-cancellation",
                    None,
                )?
        } else {
            current
        };
        return Ok(NotePipelineCancelResult {
            run,
            forced: false,
            diagnostic_path: None,
        });
    }

    let snapshot = state.note_pipeline_task_snapshot(run_id).await;
    let diagnostic_path = state
        .task_diagnostic_log
        .record_note_pipeline(
            "forcedAbort",
            snapshot
                .as_ref()
                .map(|value| value.task_kind.as_str())
                .unwrap_or("deep-note-unknown"),
            run_id,
            "任务收到取消信号后 4 秒仍未退出，正在强制终止。",
            serde_json::json!({
                "task": snapshot.clone().map(|value| serde_json::json!({
                    "instanceId": value.instance_id,
                    "kind": value.task_kind,
                    "startedAt": value.started_at_ms,
                    "ageMs": crate::usage::now_ms().saturating_sub(value.started_at_ms),
                    "cancellationRequested": value.cancellation_requested,
                    "abortable": value.abortable,
                })),
                "eventTail": note_pipeline_event_tail(&state, run_id),
            }),
        )
        .ok();
    let _ = state.abort_note_pipeline_run(run_id).await;
    if !wait_for_pipeline_task_abort(&state, run_id).await {
        // Tokio cannot preempt a thread while it is inside a synchronous call.
        // Keep the registration as a tombstone until the supervisor observes
        // the real JoinHandle exit; this prevents a second worker from using
        // the same run while the old one is still unwinding.
    }
    if let Some(snapshot) = state.note_pipeline_task_snapshot(run_id).await {
        state.detach_note_pipeline_instance(&snapshot.instance_id);
        let _ = state
            .finish_note_pipeline_run(run_id, &snapshot.instance_id)
            .await;
    }
    let run = state
        .library_repository
        .finalize_note_pipeline_cancellation(
            run_id,
            true,
            "forced-after-cancellation-timeout",
            diagnostic_path.as_deref(),
        )?;
    Ok(NotePipelineCancelResult {
        run,
        forced: true,
        diagnostic_path,
    })
}

/// 用户明确删除来源会话时调用。先停止后台请求，再将任务永久标记为已遗弃；
/// 与普通“停止”不同，遗弃任务不会出现在恢复列表，也不能继续、重试或重新生成。
pub async fn abandon<R: Runtime>(
    app: &AppHandle<R>,
    run_id: &str,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let _ = cancel(app, run_id).await?;
    state.library_repository.abandon_note_pipeline_run(run_id)
}

pub async fn abandon_for_conversation<R: Runtime>(
    app: &AppHandle<R>,
    conversation_id: &str,
) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let runs = state
        .library_repository
        .list_note_pipeline_runs_for_conversation(conversation_id)?;
    let mut abandoned = 0;
    for run in runs {
        abandon(app, &run.id).await?;
        abandoned += 1;
    }
    Ok(abandoned)
}

pub async fn pause<R: Runtime>(
    app: &AppHandle<R>,
    run_id: &str,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let paused = {
        let _guard = state.library_operations.lock().await;
        let run = state.library_repository.get_note_pipeline_run(run_id)?;
        if run.phase == NotePipelinePhase::Paused {
            return Ok(run);
        }
        if !can_pause_phase(run.phase) {
            return Err("当前阶段不能暂停；可以等待进入分析或章节生成阶段后再试。".to_string());
        }
        state
            .library_repository
            .append_note_pipeline_event(run_id, "runPaused", None, "{}")?;
        state.library_repository.update_note_pipeline_phase(
            run_id,
            NotePipelinePhase::Paused,
            None,
            &run.warnings,
            None,
        )?
    };
    if state.cancel_note_pipeline_run(run_id).await {
        let _ = wait_for_pipeline_task_to_stop(&state, run_id).await;
    }
    Ok(paused)
}

pub fn list_resumable(state: &AppState) -> Result<Vec<NotePipelineRun>, String> {
    // 先收尾“笔记已落库但终态未推进”的历史 run，再列举真正需要恢复的任务。
    // 否则这类 run 会被前端当成未完成任务自动 resume，重写出第二篇笔记。
    match state
        .library_repository
        .finalize_persisted_note_pipeline_runs()
    {
        Ok(0) => {}
        Ok(count) => eprintln!("已收尾 {count} 个笔记已落库但未推进终态的深度笔记任务。"),
        Err(error) => eprintln!("收尾深度笔记任务失败：{error}"),
    }
    state.library_repository.list_resumable_note_pipeline_runs()
}

pub fn get_run(state: &AppState, run_id: &str) -> Result<NotePipelineRun, String> {
    state.library_repository.get_note_pipeline_run(run_id)
}

pub fn get_detail(state: &AppState, run_id: &str) -> Result<DeepNoteRunDetail, String> {
    let run = state.library_repository.get_note_pipeline_run(run_id)?;
    let runtime = runtime_state(&run)?;
    let sections = state
        .library_repository
        .list_note_pipeline_sections(run_id)?;
    let events = state
        .library_repository
        .list_note_pipeline_events(run_id, 500)?
        .into_iter()
        .map(
            |(sequence, event_type, node_id, payload_json, created_at)| {
                super::types::DeepNoteEventRecord {
                    sequence,
                    event_type,
                    node_id,
                    payload_json,
                    created_at,
                }
            },
        )
        .collect();
    let markdown_preview = sections
        .iter()
        .filter(|section| section.status == NotePipelineSectionStatus::Completed)
        .map(|section| section.markdown.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let section_progress = sections
        .iter()
        .map(|section| DeepNoteSectionProgress {
            section_id: section.section_id.clone(),
            position: section.position,
            status: section.status,
            attempt_count: section.attempt_count,
            revision_count: section.revision_count,
            error_message: section.error_message.clone(),
            markdown_chars: section.markdown.chars().count(),
            updated_at: section.updated_at,
        })
        .collect();
    let source_chunks = state
        .library_repository
        .list_note_pipeline_source_chunks(run_id)?;
    let evidence = state
        .library_repository
        .list_note_pipeline_evidence(run_id)?;
    let source_chunk_count = source_chunks.len();
    let persisted_ledger = state
        .library_repository
        .latest_note_pipeline_ledger(run_id)?
        .unwrap_or_else(|| runtime.ledger.clone());
    let mut plan_version = runtime.plan_version.clone();
    let nodes = if let Some(plan) = plan_version.as_mut() {
        let restored = state.library_repository.restore_note_pipeline_nodes(
            run_id,
            plan.version,
            &plan.compiled_dag,
        )?;
        plan.compiled_dag = restored.clone();
        restored
    } else {
        Vec::new()
    };
    Ok(DeepNoteRunDetail {
        run,
        preflight: Some(runtime.preflight),
        input_snapshot: Some(runtime.input_snapshot),
        plan_version,
        budget: runtime.budget,
        context_budget: runtime.context_budget,
        source_chunk_count,
        nodes,
        sections: section_progress,
        source_chunks,
        evidence,
        ledger: persisted_ledger,
        skill_profiles: runtime.skill_profiles,
        events,
        markdown_preview,
        sidecar_json: state
            .library_repository
            .get_note_pipeline_run(run_id)?
            .sidecar_json,
    })
}

pub async fn prepare_note_edit(
    state: &AppState,
    request: NoteEditPrepareRequest,
) -> Result<NoteEditPrepareResult, String> {
    let note = state.library_repository.get_note(&request.note_id)?;
    let conversation = state
        .conversation_repository
        .load(&request.conversation_id)?;
    let summarized_until = state
        .library_repository
        .latest_summarized_message_id(&note.id, &conversation.id)?;
    let mut previous_coverage_snapshot = None;
    if let Some(anchor) = summarized_until.as_ref() {
        let coverage_snapshot = coverage_snapshot_for_note(state, &note.id, &conversation.id)?
            .ok_or_else(|| {
                "目标深度笔记缺少逐消息与附件内容 Hash，不能安全生成增量更新。".to_string()
            })?;
        validate_recovery_snapshot_from_storage(
            &state.conversation_repository,
            &conversation,
            &coverage_snapshot,
        )
        .await
        .map_err(|error| format!("目标深度笔记覆盖快照已失效：{error}"))?;
        if coverage_snapshot.message_ids.last() != Some(anchor) {
            return Err("目标深度笔记的增量锚点与覆盖快照不一致，不能安全生成更新。".to_string());
        }
        previous_coverage_snapshot = Some(coverage_snapshot);
    }
    let (provider_id, model_id, model_snapshot) = {
        let settings = state
            .model_settings
            .read()
            .map_err(|_| "模型设置锁不可用。".to_string())?;
        let (provider_id, model_id) = resolve_note_model(&settings, &conversation)?;
        let model_snapshot = resolve_note_model_snapshot(&settings, &provider_id, &model_id)?;
        (provider_id, model_id, model_snapshot)
    };
    let (max_output_tokens, thinking_enabled, retry_attempts) = {
        let settings = state
            .app_settings
            .read()
            .map_err(|_| "应用设置锁不可用。".to_string())?;
        (
            settings.max_output_tokens,
            settings.thinking_enabled,
            if settings.retry_enabled {
                settings.retry_attempts
            } else {
                0
            },
        )
    };
    let mut run = NotePipelineRun {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation.id.clone(),
        note_id: Some(note.id.clone()),
        phase: NotePipelinePhase::Analyzing,
        outline_json: String::new(),
        selected_section_ids: Vec::new(),
        provider_id,
        model_id,
        max_output_tokens,
        thinking_enabled,
        retry_attempts,
        input_snapshot_hash: String::new(),
        current_plan_version: 0,
        execution_version: 1,
        state_version: 0,
        runtime_instance_id: None,
        heartbeat_at: None,
        last_event_sequence: 0,
        budget_json: "{}".to_string(),
        preflight_json: "{}".to_string(),
        sidecar_json: String::new(),
        idempotency_key: stable_hash(format!(
            "note-edit:{}:{}:{}",
            note.id,
            conversation.id,
            request
                .operation_id
                .as_deref()
                .unwrap_or_else(|| request.requirement.trim())
        )),
        completed_section_ids: Vec::new(),
        failed_section_ids: Vec::new(),
        warnings: Vec::new(),
        error_message: None,
        abandoned: false,
        created_at: 0,
        updated_at: 0,
    };
    let (mut new_transcript, valid_ids, last_message_id) =
        incremental_transcript(&conversation, summarized_until.as_deref())?;
    let updated_coverage_snapshot = create_input_snapshot(
        &state.conversation_repository,
        &conversation,
        model_snapshot.clone(),
        conversation.updated_at.max(1),
    )
    .await?;
    run.input_snapshot_hash = stable_hash(
        serde_json::to_vec(&updated_coverage_snapshot)
            .map_err(|error| format!("序列化笔记增量输入快照失败：{error}"))?,
    );
    let new_attachments = noteworthy_messages(&conversation)
        .into_iter()
        .filter(|message| valid_ids.contains(&message.id))
        .flat_map(|message| message.attachments.iter())
        .collect::<Vec<_>>();
    let attachment_count = new_attachments.len();
    let unsupported = new_attachments
        .iter()
        .filter(|attachment| {
            attachment_formats::deep_note_read_kind(attachment) == AttachmentReadKind::Unsupported
        })
        .map(|attachment| unsupported_attachment_label(attachment))
        .collect::<Vec<_>>();
    if !unsupported.is_empty() {
        return Err(format!(
            "新增附件中包含当前无法安全读取的内容：{}。请转换格式、移除附件或清理敏感配置。",
            unsupported.join("、")
        ));
    }
    let requires_vision = new_attachments
        .iter()
        .any(|attachment| attachment.kind == "image");
    if requires_vision && model_snapshot.capabilities.vision != Some(true) {
        return Err("新增附件包含图片，但当前笔记模型未明确支持视觉输入。".to_string());
    }
    let (skill_profiles, skill_warnings) = snapshot_skill_profiles(state, requires_vision);
    let (_, route_profile) = deep_note_route_profile(state, &run.provider_id, &run.model_id)?;
    let adaptive_budget = AdaptiveBudgetSnapshot::from_profile(&route_profile);
    let mut incremental_runtime = DeepNoteRuntimeState {
        preflight: DeepNotePreflight {
            ready: true,
            model: model_snapshot.clone(),
            requires_tools: false,
            requires_local_readers: attachment_count > 0,
            requires_vision,
            local_readers: DeepNoteLocalReaderCapabilities {
                text: true,
                pdf: true,
                docx: true,
                xlsx: true,
            },
            missing_capabilities: Vec::new(),
            warnings: skill_warnings.clone(),
            attachment_ids: new_attachments
                .iter()
                .map(|attachment| attachment.id.clone())
                .collect(),
        },
        input_snapshot: updated_coverage_snapshot.clone(),
        plan_version: None,
        budget: DeepNoteBudget::for_section_count(4),
        ledger: DeepNoteLedger::default(),
        skill_profiles,
        context_budget: DeepNoteContextBudget {
            context_window_tokens: model_snapshot.context_window_tokens,
            chunk_target_tokens: adaptive_budget.limit_tokens,
            adaptive_chunk_limit_tokens: adaptive_budget.limit_tokens,
            adaptive_route_key: adaptive_budget.route_key,
            adaptive_route_state: adaptive_budget.route_state,
            adaptive_profile_samples: adaptive_budget.profile_samples,
            ..DeepNoteContextBudget::default()
        },
        force_rebuild: false,
        section_active_ms: BTreeMap::new(),
    };
    let cancellation = CancellationToken::new();
    let attachment_chunks = if attachment_count > 0 {
        incremental_attachment_source_chunks(
            state,
            &run,
            &mut incremental_runtime,
            &conversation,
            &valid_ids,
            &cancellation,
        )
        .await?
    } else {
        Vec::new()
    };
    if attachment_count > 0 && attachment_chunks.is_empty() {
        return Err("新增附件没有产生可验证的 Source Chunk，覆盖快照不会推进。".to_string());
    }
    let attachment_ledger = if attachment_chunks.is_empty() {
        DeepNoteLedger::default()
    } else {
        digest_incremental_chunks(
            state,
            &run,
            &mut incremental_runtime,
            &attachment_chunks,
            &cancellation,
        )
        .await?
    };
    let source_units = incremental_source_units(
        &note.id,
        &conversation,
        &valid_ids,
        &updated_coverage_snapshot,
        &attachment_chunks,
        conversation.updated_at.max(1),
    );
    if source_units
        .iter()
        .filter(|unit| unit.kind == DeepNoteSourceUnitKind::Attachment)
        .any(|unit| unit.status != DeepNoteSourceUnitStatus::Covered)
    {
        return Err("新增附件存在未覆盖的 Source Unit，不能生成增量提案。".to_string());
    }
    if new_transcript.trim().is_empty() {
        if request.selected_text.trim().is_empty() {
            return Err("这段对话没有尚未合入目标笔记的新内容。".to_string());
        }
        new_transcript = "（没有新的对话增量；请只按选中文本和用户要求生成局部修改。）".to_string();
    }
    let attachment_context = if attachment_chunks.is_empty() {
        "（没有新增附件）".to_string()
    } else {
        format!(
            "新增附件已全部读取并形成 {} 个 Source Chunk。以下目录用于定位；正文依据来自后面的增量账本。\n{}\n\n新增附件知识账本（有界聚合）：\n{}",
            attachment_chunks.len(),
            attachment_chunks
                .iter()
                .map(|chunk| format!(
                    "- {} · {} · message={} · hash={}",
                    chunk.source.chunk_id,
                    chunk.source.location,
                    chunk.source.message_id.as_deref().unwrap_or(""),
                    chunk.source.content_hash
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            serde_json::to_string_pretty(&compact_attachment_ledger(&attachment_ledger))
                .map_err(|error| format!("序列化新增附件账本失败：{error}"))?,
        )
    };
    let context = format!(
        "目标笔记：\n{}\n\n新对话增量：\n{}\n\n{}\n\n选中文本：\n{}\n\n所属章节：{}\n\n用户要求：{}",
        note.content,
        new_transcript,
        attachment_context,
        request.selected_text.trim(),
        request.section_heading.trim(),
        request.requirement.trim(),
    );
    let plan_prompt = if attachment_count > 0 {
        NOTE_ATTACHMENT_EDIT_PLAN_PROMPT
    } else {
        NOTE_EDIT_PLAN_PROMPT
    };
    let patch_system_prompt = if attachment_count > 0 {
        NOTE_ATTACHMENT_EDIT_PATCH_PROMPT
    } else {
        NOTE_EDIT_PATCH_PROMPT
    };
    consume_semantic_call(state, &run.id, &mut incremental_runtime)?;
    let raw_plan = model_call(
        state,
        &run,
        "noteEdit",
        plan_prompt.to_string(),
        context.clone(),
        max_output_tokens.min(8_192),
    )
    .await?;
    let plan = match parse_json_object::<NoteMergePlan>(&raw_plan) {
        Ok(plan) => plan,
        Err(_) => {
            consume_semantic_call(state, &run.id, &mut incremental_runtime)?;
            let raw_plan = model_call(
                state,
                &run,
                "noteEdit",
                format!("{plan_prompt}\n\n{STRICT_JSON_SUFFIX}"),
                context.clone(),
                max_output_tokens.min(8_192),
            )
            .await?;
            parse_json_object::<NoteMergePlan>(&raw_plan)?
        }
    };
    if plan.operations.is_empty() || plan.operations.len() > 40 {
        return Err("模型返回的笔记合并计划为空或过长。".to_string());
    }
    let patch_prompt = format!(
        "目标笔记：\n{}\n\n新对话与附件增量：\n{}\n\n{}\n\n合并计划：\n{}",
        note.content,
        new_transcript,
        attachment_context,
        serde_json::to_string(&plan).map_err(|error| error.to_string())?,
    );
    consume_semantic_call(state, &run.id, &mut incremental_runtime)?;
    let raw_patches = model_call(
        state,
        &run,
        "noteEdit",
        patch_system_prompt.to_string(),
        patch_prompt.clone(),
        max_output_tokens.min(16_384),
    )
    .await?;
    let mut patch_set = match parse_json_object::<NotePatchSet>(&raw_patches) {
        Ok(patches) => patches,
        Err(_) => {
            consume_semantic_call(state, &run.id, &mut incremental_runtime)?;
            let raw_patches = model_call(
                state,
                &run,
                "noteEdit",
                format!("{patch_system_prompt}\n\n{STRICT_JSON_SUFFIX}"),
                patch_prompt,
                max_output_tokens.min(16_384),
            )
            .await?;
            parse_json_object::<NotePatchSet>(&raw_patches)?
        }
    };
    if patch_set.patches.is_empty() || patch_set.patches.len() > 40 {
        return Err("模型返回的笔记补丁为空或过长。".to_string());
    }
    for patch in &mut patch_set.patches {
        patch.markdown = normalize_generated_markdown(patch.markdown.trim());
        patch.source_message_ids.retain(|id| valid_ids.contains(id));
        patch.source_message_ids.sort();
        patch.source_message_ids.dedup();
    }
    let (new_content, mut warnings) = apply_note_patches(&note.content, &patch_set.patches)?;
    let attachment_review = if attachment_count > 0 {
        let review_context = format!(
            "旧笔记：\n{}\n\n更新后笔记：\n{}\n\n新增附件账本：\n{}",
            note.content,
            new_content,
            serde_json::to_string_pretty(&compact_attachment_ledger(&attachment_ledger))
                .map_err(|error| format!("序列化附件复核账本失败：{error}"))?,
        );
        consume_semantic_call(state, &run.id, &mut incremental_runtime)?;
        let raw_review = model_call(
            state,
            &run,
            "noteEdit",
            NOTE_ATTACHMENT_REVIEW_PROMPT.to_string(),
            review_context.clone(),
            max_output_tokens.min(2_048),
        )
        .await?;
        let review = match parse_json_object::<AttachmentUpdateReview>(&raw_review) {
            Ok(review) => review,
            Err(_) => {
                consume_semantic_call(state, &run.id, &mut incremental_runtime)?;
                let repaired = model_call(
                    state,
                    &run,
                    "noteEdit",
                    format!("{NOTE_ATTACHMENT_REVIEW_PROMPT}\n\n{STRICT_JSON_SUFFIX}"),
                    review_context,
                    max_output_tokens.min(2_048),
                )
                .await?;
                parse_json_object::<AttachmentUpdateReview>(&repaired)?
            }
        };
        warnings.extend(review.warnings.clone());
        Some(review)
    } else {
        None
    };
    let global_review_passed = attachment_review
        .as_ref()
        .is_none_or(|review| review.passed && !review.requires_full_rebuild);
    let requires_global_review = attachment_review
        .as_ref()
        .is_some_and(|review| !review.passed || review.requires_full_rebuild);
    if attachment_review
        .as_ref()
        .is_some_and(|review| review.requires_full_rebuild)
    {
        warnings.push(
            "全局复核判断新增附件可能改变核心定义、数字、时间线或结论；应用前应完整检查 Diff，必要时改用完整重建。"
                .to_string(),
        );
    }
    let new_title = [
        patch_set.title.trim(),
        plan.title.trim(),
        note.title.as_str(),
    ]
    .into_iter()
    .find(|value| !value.is_empty())
    .unwrap_or(&note.title)
    .trim_start_matches('#')
    .trim()
    .chars()
    .take(500)
    .collect::<String>();
    let diff = compact_diff(&note.content, &new_content);
    let mut sources = patch_set
        .patches
        .iter()
        .enumerate()
        .flat_map(|(index, patch)| {
            let section_id = format!("edit-{}", index + 1);
            let mut sources = if patch.source_message_ids.is_empty() {
                vec![NoteSourceCreate {
                    section_id: section_id.clone(),
                    origin: NoteSourceOrigin::Conversation,
                    conversation_id: Some(conversation.id.clone()),
                    message_id: None,
                    summarized_until_message_id: last_message_id.clone(),
                }]
            } else {
                patch
                    .source_message_ids
                    .iter()
                    .map(|message_id| NoteSourceCreate {
                        section_id: section_id.clone(),
                        origin: NoteSourceOrigin::Conversation,
                        conversation_id: Some(conversation.id.clone()),
                        message_id: Some(message_id.clone()),
                        summarized_until_message_id: last_message_id.clone(),
                    })
                    .collect()
            };
            if patch.needs_supplement {
                sources.push(NoteSourceCreate {
                    section_id,
                    origin: NoteSourceOrigin::AiSupplement,
                    conversation_id: None,
                    message_id: None,
                    summarized_until_message_id: None,
                });
            }
            sources
        })
        .collect::<Vec<_>>();
    for unit in &source_units {
        sources.push(NoteSourceCreate {
            section_id: format!("source-unit:{}", unit.unit_id),
            origin: NoteSourceOrigin::Conversation,
            conversation_id: Some(conversation.id.clone()),
            message_id: Some(unit.message_id.clone()),
            summarized_until_message_id: last_message_id.clone(),
        });
    }
    let proposal = {
        let _guard = state.library_operations.lock().await;
        state
            .library_repository
            .create_note_edit_proposal(NoteEditProposalCreate {
                id: Uuid::new_v4().to_string(),
                note_id: note.id,
                conversation_id: conversation.id,
                source_message_id: last_message_id,
                expected_note_updated_at: note.updated_at,
                old_title: note.title,
                new_title,
                old_content: note.content,
                new_content,
                diff,
                sources,
                coverage_snapshot_json: serde_json::to_string(&updated_coverage_snapshot)
                    .map_err(|error| format!("序列化更新覆盖快照失败：{error}"))?,
                source_units: source_units.clone(),
            })?
    };
    warnings.extend(skill_warnings);
    if attachment_count > 0 {
        warnings.push(format!(
            "已读取 {attachment_count} 个新增附件并生成 {} 个 Source Chunk；应用后覆盖快照才会推进。",
            attachment_chunks.len()
        ));
        if previous_coverage_snapshot.is_none() {
            warnings.push("目标笔记缺少旧 Source Unit 明细；本次只登记新增附件，旧覆盖仍由 Coverage Snapshot 保护。".to_string());
        }
    }
    Ok(NoteEditPrepareResult {
        proposal,
        warnings,
        source_units,
        attachment_count,
        requires_global_review,
        global_review_passed,
    })
}

pub async fn resolve_note_edit(
    state: &AppState,
    proposal_id: &str,
    accepted: bool,
) -> Result<Option<crate::library::types::LibraryNote>, String> {
    let _conversation_guard = if accepted {
        Some(state.conversation_writes.lock().await)
    } else {
        None
    };
    if accepted {
        let (conversation_id, snapshot_json) = state
            .library_repository
            .pending_note_edit_coverage_snapshot(proposal_id)?
            .ok_or_else(|| "笔记修改提案不存在或已经处理。".to_string())?;
        if !snapshot_json.trim().is_empty() {
            let snapshot = serde_json::from_str::<DeepNoteInputSnapshot>(&snapshot_json)
                .map_err(|error| format!("读取笔记修改覆盖快照失败：{error}"))?;
            let conversation = state.conversation_repository.load(&conversation_id)?;
            validate_recovery_snapshot_from_storage(
                &state.conversation_repository,
                &conversation,
                &snapshot,
            )
            .await
            .map_err(|error| format!("对话在提案生成后发生变化，不能应用旧提案：{error}"))?;
        }
    }
    let _guard = state.library_operations.lock().await;
    state
        .library_repository
        .resolve_note_edit_proposal(proposal_id, accepted)
}

pub async fn resolve_note_edit_with_content(
    state: &AppState,
    proposal_id: &str,
    title: String,
    content: String,
    diff: String,
) -> Result<Option<crate::library::types::LibraryNote>, String> {
    let (conversation_id, snapshot_json) = state
        .library_repository
        .pending_note_edit_coverage_snapshot(proposal_id)?
        .ok_or_else(|| "修改提案不存在或已经处理。".to_string())?;
    if !snapshot_json.trim().is_empty() {
        let snapshot = serde_json::from_str::<DeepNoteInputSnapshot>(&snapshot_json)
            .map_err(|error| format!("读取修改提案覆盖快照失败：{error}"))?;
        let conversation = state.conversation_repository.load(&conversation_id)?;
        validate_recovery_snapshot_from_storage(
            &state.conversation_repository,
            &conversation,
            &snapshot,
        )
        .await
        .map_err(|error| format!("对话在提案生成后发生变化，不能应用部分修改：{error}"))?;
    }
    let _guard = state.library_operations.lock().await;
    state
        .library_repository
        .resolve_note_edit_proposal_with_content(proposal_id, true, Some((title, content, diff)))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        time::Duration,
    };

    use tokio_util::sync::CancellationToken;

    use crate::{
        ai::{
            error::{ModelError, ModelErrorKind},
            types::ModelRole,
        },
        chat::conversation_types::{
            AiPermissionMode, MessageStatus, StoredChatAttachment, StoredChatMessage,
            StoredConversation,
        },
        library::types::NotePipelinePhase,
    };

    use super::{
        analyze_markdown_fences, await_note_pipeline_cancellable, can_pause_phase,
        chunk_analysis_prompt, collect_top_level_mermaid_blocks, combine_chunk_digests,
        compact_ledger_for_planner, content_addressed_chunk_id, context_budget,
        conversation_chunks, deep_note_content_signature, edge_label_has_trailing_quote,
        er_attribute_has_compound_key, evidence_for_plan, flowchart_label_needs_quotes,
        has_mermaid_lint_warning, input_snapshot, ledger_has_real_output, lint_mermaid_syntax,
        merge_chunk_digest, normalize_generated_markdown, normalize_math_fences,
        pack_adjacent_attachment_chunks, phase_expects_background_worker, pipeline_failure_payload,
        record_section_active_ms, request_byte_limit, reset_failed_nodes,
        section_wall_clock_exhausted, should_fallback_to_chunked_planner,
        should_retry_note_model_call, snapshot_conversation_after_validation, split_chunk_in_half,
        split_text_by_token_budget, token_estimate_units, validate_global_drafts,
        validate_recovery_snapshot, validate_section_markdown, AdaptiveBudgetSnapshot, ChunkDigest,
        ConversationChunk, REQUEST_BYTE_LIMIT, VISION_REQUEST_BYTE_LIMIT,
    };
    use crate::chat::note_pipeline::types::{
        DeepNoteCapabilities, DeepNoteDagNode, DeepNoteEvidenceStatus, DeepNoteLedger,
        DeepNoteModelSnapshot, DeepNoteNodeStatus, DeepNoteNodeType, DeepNoteSection,
        DeepNoteSectionKind, DeepNoteSourceChunk, DeepNoteSourceKind, DeepNoteStartInspection,
        DeepNoteValidationReport, NotePipelineStartRequest, DEEP_NOTE_FAILURE_PREFIX,
    };

    fn message(id: &str, role: ModelRole, content: String) -> StoredChatMessage {
        StoredChatMessage {
            id: id.to_string(),
            conversation_id: "conversation-1".to_string(),
            role,
            content,
            attachments: Vec::new(),
            literature_references: Vec::new(),
            note_references: Vec::new(),
            reasoning: Some("不应进入深度笔记来源".to_string()),
            status: MessageStatus::Completed,
            created_at: 1,
            updated_at: 1,
            model_id: None,
            model_snapshot: None,
            usage: None,
            activated_skills: Vec::new(),
            tool_traces: Vec::new(),
            agent_events: Some(Vec::new()),
            agent_run_id: None,
            workflow_summary: None,
            error_message: None,
        }
    }

    #[test]
    fn normalizes_legacy_math_fences_without_touching_unclosed_blocks() {
        let source = "前文\n```math\np = \\frac{1}{1 + e^{-x}}\n```\n\n```latex\nq=x^2\n```";
        let normalized = normalize_math_fences(source);
        assert_eq!(
            normalized,
            "前文\n$$\np = \\frac{1}{1 + e^{-x}}\n$$\n\n$$\nq=x^2\n$$"
        );

        let unclosed = "```math\np = x";
        assert_eq!(normalize_math_fences(unclosed), unclosed);
    }

    #[test]
    fn unwraps_a_generated_markdown_shell_so_nested_mermaid_becomes_top_level() {
        let fence = "````";
        let source = format!(
            "{fence}markdown\n## 依赖关系\n\n{}\n\n```mermaid\nflowchart TD\nA[基础] --> B[借用]\nB --> C[生命周期]\n```\n{fence}",
            "本节解释章节之间的依赖关系、并行条件和失败传播。".repeat(8)
        );
        let normalized = normalize_generated_markdown(&source);
        assert!(normalized.starts_with("## 依赖关系"));
        assert!(!normalized.starts_with("````markdown"));
        let fences = analyze_markdown_fences(&normalized);
        assert_eq!(fences.top_level_mermaid_blocks, 1);
        assert_eq!(fences.nested_mermaid_markers, 0);
        assert!(!fences.unclosed_fence);

        let section = DeepNoteSection {
            id: "sec-dependency".to_string(),
            heading: "依赖关系".to_string(),
            kind: DeepNoteSectionKind::Concept,
            brief: "解释依赖和并行流程".to_string(),
            purpose: "说明执行顺序".to_string(),
            depends_on: Vec::new(),
            evidence_requirements: Vec::new(),
            success_criteria: vec!["解释依赖关系".to_string()],
            source_scope: Vec::new(),
            target_depth: "standard".to_string(),
            allow_ai_supplement: false,
            needs_supplement: false,
            source_message_ids: Vec::new(),
        };
        assert!(validate_section_markdown(&section, &normalized, &[]).passed);
    }

    #[test]
    fn rejects_mermaid_that_remains_inside_a_markdown_source_example() {
        let source = format!(
            "## 依赖关系\n\n{}\n\n````markdown\n### Markdown 示例\n\n```mermaid\nflowchart TD\nA --> B\n```\n````",
            "本节包含足够长的依赖关系说明，用于验证图表必须进入真实渲染链。".repeat(8)
        );
        let fences = analyze_markdown_fences(&source);
        assert_eq!(fences.top_level_mermaid_blocks, 0);
        assert_eq!(fences.nested_mermaid_markers, 1);
        let section = DeepNoteSection {
            id: "sec-dependency".to_string(),
            heading: "依赖关系".to_string(),
            kind: DeepNoteSectionKind::Concept,
            brief: "解释依赖流程".to_string(),
            purpose: "说明执行顺序".to_string(),
            depends_on: Vec::new(),
            evidence_requirements: Vec::new(),
            success_criteria: vec!["解释依赖关系".to_string()],
            source_scope: Vec::new(),
            target_depth: "standard".to_string(),
            allow_ai_supplement: false,
            needs_supplement: false,
            source_message_ids: Vec::new(),
        };
        let report = validate_section_markdown(&section, &source, &[]);
        assert!(!report.passed);
        assert!(report.errors.iter().any(|error| error.contains("正文顶层")));
    }

    #[test]
    fn collects_only_top_level_mermaid_block_bodies() {
        let markdown = concat!(
            "## 依赖\n\n",
            "```mermaid\nflowchart TD\nA --> B\n```\n\n",
            "````markdown\n### 示例\n\n```mermaid\nflowchart TD\nC --> D\n```\n````\n\n",
            "```rust\nfn main() {}\n```\n"
        );
        let blocks = collect_top_level_mermaid_blocks(markdown);
        // 嵌套在 markdown 围栏里的那张图由 nested_mermaid_markers 单独报错，这里不重复收。
        assert_eq!(blocks, vec!["flowchart TD\nA --> B".to_string()]);
    }

    #[test]
    fn flags_unquoted_ascii_parentheses_in_flowchart_labels() {
        // 真实语料里导致渲染失败的那一行。
        assert!(flowchart_label_needs_quotes(
            "    A[MCP 主机 (Host) - AI 应用<br/>如 Claude Desktop / Visual Studio Code]"
        ));
        assert!(flowchart_label_needs_quotes("B[结果) 收尾] --> C[正常]"));
    }

    #[test]
    fn leaves_label_characters_that_mermaid_already_accepts() {
        // 这批字符由 scripts/probe-mermaid-errors.mjs 实测确认过合法，报了就是误报，
        // 会白白烧掉章节的修订预算。
        for line in [
            "A[主机（Host）]",
            "A[标签 <b>强调</b>]",
            "A[键: 值, 其他]",
            "A[结尾分号];",
            "A[\"主机 (Host)\"]",
            "A[[子程序 (Sub)]]",
            "A[(数据库 (DB))]",
            "%% A[注释里的 (括号)]",
            "click A callback \"提示 (说明)\"",
            "A[未闭合的 (标签",
        ] {
            assert!(!flowchart_label_needs_quotes(line), "误报：{line}");
        }
    }

    #[test]
    fn flags_only_a_quote_that_follows_the_closing_pipe() {
        assert!(edge_label_has_trailing_quote(
            "    A -->|\"生成结果\"|\" Chat"
        ));
        for line in [
            "A -->|说明| B",
            "A -->|\"说明\"| B",
            "A -- 说明 --> B",
            "A --> B",
        ] {
            assert!(!edge_label_has_trailing_quote(line), "误报：{line}");
        }
    }

    #[test]
    fn flags_er_compound_keys_that_are_not_comma_separated() {
        assert!(er_attribute_has_compound_key("    text run_id PK_FK"));
        assert!(er_attribute_has_compound_key("    text run_id PK FK"));
        for line in [
            "    string id PK",
            "    string id PK, FK",
            "    string id PK \"主键 (primary)\"",
            "    USER ||--o{ NOTE : writes",
            "  }",
        ] {
            assert!(!er_attribute_has_compound_key(line), "误报：{line}");
        }
    }

    #[test]
    fn mermaid_lint_stays_within_the_matching_diagram_type() {
        let flowchart = "flowchart TD\n    A[MCP 主机 (Host)] --> B[客户端]";
        let findings = lint_mermaid_syntax(flowchart);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].contains("圆括号"));

        let er = "erDiagram\n    DEEP_NOTE_RUN {\n        text run_id PK_FK\n    }";
        let er_findings = lint_mermaid_syntax(er);
        assert_eq!(er_findings.len(), 1);
        assert!(er_findings[0].contains("PK, FK"));

        // flowchart 里出现 PK_FK 只是普通文本，不该触发 erDiagram 规则。
        assert!(lint_mermaid_syntax("flowchart TD\n    A[run_id PK_FK] --> B").is_empty());
        // erDiagram 的实体名带括号不是 flowchart 的方括号标签。
        assert!(
            lint_mermaid_syntax("erDiagram\n    USER ||--o{ NOTE : \"writes (many)\"").is_empty()
        );
        assert!(lint_mermaid_syntax("flowchart TD\n    A[\"主机 (Host)\"] -->|说明| B").is_empty());
        assert!(lint_mermaid_syntax("sequenceDiagram\n    A ->> B: 调用 (同步)").is_empty());
    }

    #[test]
    fn section_validation_reports_mermaid_typos_as_warnings() {
        let source = format!(
            "## 依赖关系\n\n{}\n\n```mermaid\nflowchart TD\n    A[MCP 主机 (Host)] --> B[客户端]\n```\n",
            "本节解释章节之间的依赖关系、并行条件和失败传播，长度需要越过正文下限。".repeat(8)
        );
        let section = DeepNoteSection {
            id: "sec-dependency".to_string(),
            heading: "依赖关系".to_string(),
            kind: DeepNoteSectionKind::Concept,
            brief: "解释依赖和并行流程".to_string(),
            purpose: "说明执行顺序".to_string(),
            depends_on: Vec::new(),
            evidence_requirements: Vec::new(),
            success_criteria: vec!["解释依赖关系".to_string()],
            source_scope: Vec::new(),
            target_depth: "standard".to_string(),
            allow_ai_supplement: false,
            needs_supplement: false,
            source_message_ids: Vec::new(),
        };
        let report = validate_section_markdown(&section, &source, &[]);
        // 语法笔误只进 warnings，不进 passed：一条可能误报的规则不该在修订额度
        // 用尽时把整章判失败。但它必须能被修订循环认出来，否则等于没人读。
        assert!(report.passed);
        assert!(report.warnings.iter().any(|item| item.contains("圆括号")));
        assert!(
            has_mermaid_lint_warning(&report),
            "warnings 进不了修订循环的话，第 2 层就是死代码"
        );
    }

    fn shrinkable_chunk(excerpt: String) -> ConversationChunk {
        ConversationChunk {
            estimated_tokens: super::estimate_text_tokens(&excerpt),
            source: DeepNoteSourceChunk {
                chunk_id: super::content_addressed_chunk_id(&excerpt),
                source_kind: DeepNoteSourceKind::Text,
                source_id: "conversation-1".to_string(),
                message_id: None,
                attachment_id: None,
                library_item_id: None,
                location: "12 条消息".to_string(),
                content_hash: super::stable_hash(&excerpt),
                excerpt,
                ocr_confidence: None,
            },
            message_ids: vec!["message-1".to_string(), "message-2".to_string()],
        }
    }

    /// 网关 504 后切小重投的前提：切开不能丢字符，否则账本会缺内容却自称完整。
    #[test]
    fn splitting_a_chunk_preserves_every_character() {
        let excerpt = (0..400)
            .map(|index| format!("第 {index} 行：远程解释器与并行执行的配置说明。"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunk = shrinkable_chunk(excerpt.clone());
        let (head, tail) = split_chunk_in_half(&chunk).expect("足够长的分块必须可切");

        assert_eq!(
            format!("{}{}", head.source.excerpt, tail.source.excerpt),
            excerpt,
            "切开再拼必须与原文逐字节相同"
        );
        assert!(!head.source.excerpt.trim().is_empty());
        assert!(!tail.source.excerpt.trim().is_empty());
        // 优先切在换行：两半都不该以半行开头。
        assert!(tail.source.excerpt.starts_with("第 "));
        // 两半必须继承父块的 message_ids，否则 merge_chunk_digest 会把证据过滤掉。
        assert_eq!(head.message_ids, chunk.message_ids);
        assert_eq!(tail.message_ids, chunk.message_ids);
        // 各自内容寻址，才能独立落检查点、下次运行复用。
        assert_ne!(head.source.content_hash, tail.source.content_hash);
        assert_ne!(head.source.chunk_id, chunk.source.chunk_id);
    }

    /// 切到很小还超时说明问题不在载荷大小，继续切只会放大失败次数。
    #[test]
    fn tiny_chunks_are_not_split_further() {
        assert!(
            split_chunk_in_half(&shrinkable_chunk("短内容".repeat(20))).is_none(),
            "低于下限的分块不该再切"
        );
    }

    /// 合并两半摘要不能丢字段，否则救回来的分块内容不完整。
    #[test]
    fn combining_half_digests_keeps_both_sides() {
        let half = |suffix: &str| ChunkDigest {
            summary: format!("摘要 {suffix}"),
            canonical_terms: vec![format!("术语 {suffix}")],
            verified_facts: vec![format!("事实 {suffix}")],
            covered_topics: vec![format!("主题 {suffix}")],
            open_questions: vec![format!("问题 {suffix}")],
            conflicts: vec![format!("冲突 {suffix}")],
            global_constraints: vec![format!("约束 {suffix}")],
            source_message_ids: vec![format!("message-{suffix}")],
        };
        let merged = combine_chunk_digests(half("甲"), half("乙"));
        assert_eq!(merged.summary, "摘要 甲\n摘要 乙");
        for (field, values) in [
            ("canonicalTerms", &merged.canonical_terms),
            ("verifiedFacts", &merged.verified_facts),
            ("coveredTopics", &merged.covered_topics),
            ("openQuestions", &merged.open_questions),
            ("conflicts", &merged.conflicts),
            ("globalConstraints", &merged.global_constraints),
            ("sourceMessageIds", &merged.source_message_ids),
        ] {
            assert_eq!(values.len(), 2, "{field} 丢了一半");
        }

        // 一半为空摘要时不该留下空行。
        let mut empty = half("乙");
        empty.summary = String::new();
        assert_eq!(combine_chunk_digests(half("甲"), empty).summary, "摘要 甲");
    }

    #[test]
    fn only_mermaid_lint_warnings_trigger_a_revision() {
        let clean = DeepNoteValidationReport {
            passed: true,
            errors: Vec::new(),
            warnings: vec!["章节偏短，建议补充。".to_string()],
            checked_evidence_ids: Vec::new(),
            criteria_coverage: Vec::new(),
        };
        // 普通警告不该白烧一轮修订预算。
        assert!(!has_mermaid_lint_warning(&clean));

        let flagged = validate_section_markdown(
            &DeepNoteSection {
                id: "sec-1".to_string(),
                heading: "依赖关系".to_string(),
                kind: DeepNoteSectionKind::Concept,
                brief: "解释依赖".to_string(),
                purpose: "说明顺序".to_string(),
                depends_on: Vec::new(),
                evidence_requirements: Vec::new(),
                success_criteria: vec!["解释依赖关系".to_string()],
                source_scope: Vec::new(),
                target_depth: "standard".to_string(),
                allow_ai_supplement: false,
                needs_supplement: false,
                source_message_ids: Vec::new(),
            },
            &format!(
                "## 依赖关系\n\n{}\n\n```mermaid\nerDiagram\n    RUN {{\n        text run_id PK_FK\n    }}\n```\n",
                "本节解释章节之间的依赖关系、并行条件和失败传播，长度需要越过正文下限。".repeat(8)
            ),
            &[],
        );
        assert!(flagged.passed);
        assert!(has_mermaid_lint_warning(&flagged));
    }

    /// 用真实语料量误报率。默认跳过：语料是本机会话数据，不能进仓库。
    ///
    /// 复现方式：
    ///   node --import ./scripts/mermaid-loader.mjs scripts/dump-mermaid-corpus.mjs \
    ///     corpus.json <会话目录> <文档>
    ///   MNEMORA_MERMAID_CORPUS=corpus.json cargo test --lib mermaid_lint_has_no_false_positives \
    ///     -- --ignored --nocapture
    ///
    /// 2026-08-31 在 105 张真实图（102 张原样可解析）上的结果：误报 0，3 张真实
    /// 失败全部命中，漏报 0。加规则之后必须重跑，误报不为 0 就别上线。
    #[test]
    #[ignore = "需要本机 mermaid 语料，见 MNEMORA_MERMAID_CORPUS"]
    fn mermaid_lint_has_no_false_positives() {
        let Ok(corpus_path) = std::env::var("MNEMORA_MERMAID_CORPUS") else {
            panic!("请用 MNEMORA_MERMAID_CORPUS 指向 dump-mermaid-corpus.mjs 的输出");
        };
        let raw = std::fs::read_to_string(&corpus_path).expect("读取语料失败");
        let corpus: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("语料不是合法 json");
        let (mut false_positives, mut caught, mut missed) = (0_usize, 0_usize, 0_usize);
        for item in &corpus {
            let code = item["code"].as_str().unwrap_or_default();
            let parses = item["parses"].as_bool().unwrap_or(true);
            let findings = lint_mermaid_syntax(code);
            match (parses, findings.is_empty()) {
                (true, false) => {
                    false_positives += 1;
                    println!(
                        "误报 {}:{} -> {findings:?}\n{code}\n",
                        item["file"], item["line"]
                    );
                }
                (false, false) => {
                    caught += 1;
                    println!("命中 {}:{} -> {findings:?}", item["file"], item["line"]);
                }
                (false, true) => {
                    missed += 1;
                    println!("漏报 {}:{}\n{code}\n", item["file"], item["line"]);
                }
                (true, true) => {}
            }
        }
        println!(
            "共 {} 张图：误报 {false_positives}，命中 {caught}，漏报 {missed}",
            corpus.len()
        );
        assert_eq!(false_positives, 0, "存在误报，会白白烧掉章节修订预算");
    }

    #[test]
    fn supported_attachment_update_no_longer_requires_a_full_rebuild() {
        let inspection = DeepNoteStartInspection {
            status: "updateAvailable".to_string(),
            note_id: Some("note-1".to_string()),
            note_title: Some("旧笔记".to_string()),
            covered_message_id: Some("message-b".to_string()),
            covered_message_count: 2,
            new_message_count: 1,
            new_attachment_count: 1,
            requires_full_rebuild: false,
            unsupported_attachment_names: Vec::new(),
            message: "需要重建".to_string(),
        };
        let blocked = NotePipelineStartRequest {
            conversation_id: "conversation-1".to_string(),
            replace_invalidated: false,
            force_rebuild: false,
        };
        assert!(super::validate_start_inspection(&blocked, &inspection).is_err());
        let allowed = NotePipelineStartRequest {
            force_rebuild: true,
            ..blocked
        };
        assert!(super::validate_start_inspection(&allowed, &inspection).is_ok());
    }

    #[test]
    fn force_rebuild_still_rejects_unknown_attachment_formats() {
        let inspection = DeepNoteStartInspection {
            status: "updateAvailable".to_string(),
            note_id: Some("note-1".to_string()),
            note_title: Some("旧笔记".to_string()),
            covered_message_id: Some("message-b".to_string()),
            covered_message_count: 2,
            new_message_count: 1,
            new_attachment_count: 1,
            requires_full_rebuild: true,
            unsupported_attachment_names: vec!["slides.pptx".to_string()],
            message: "格式不支持".to_string(),
        };
        let request = NotePipelineStartRequest {
            conversation_id: "conversation-1".to_string(),
            replace_invalidated: false,
            force_rebuild: true,
        };
        assert!(super::validate_start_inspection(&request, &inspection).is_err());
    }

    fn conversation(messages: Vec<StoredChatMessage>) -> StoredConversation {
        StoredConversation {
            id: "conversation-1".to_string(),
            title: "Long conversation".to_string(),
            messages,
            assistant_id: None,
            provider_id: None,
            model_id: None,
            thinking_enabled: None,
            reasoning_effort: None,
            system_prompt: String::new(),
            context_summary: String::new(),
            compressed_until_message_id: None,
            context_compression_count: 0,
            enabled_skill_ids: Vec::new(),
            linked_library_item_ids: Vec::new(),
            permission_mode: AiPermissionMode::AskSensitive,
            project_id: None,
            collection_id: None,
            source_kind: None,
            pinned: false,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn model(context_window_tokens: Option<u64>) -> DeepNoteModelSnapshot {
        DeepNoteModelSnapshot {
            provider_id: "provider-1".to_string(),
            model_id: "model-1".to_string(),
            api_model: "test-model".to_string(),
            context_window_tokens,
            capabilities: DeepNoteCapabilities {
                tools: Some(true),
                vision: Some(true),
                reasoning: Some(true),
                structured_outputs: false,
            },
        }
    }

    #[test]
    fn context_budget_reserves_output_prompt_and_safety_tokens() {
        let conversation = conversation(vec![message(
            "message-1",
            ModelRole::User,
            "context".repeat(100),
        )]);
        let budget = context_budget(
            &conversation,
            &model(Some(128_000)),
            16_384,
            &AdaptiveBudgetSnapshot::cold_start(),
        );
        assert_eq!(budget.planner_output_reserve_tokens, 2_048);
        assert_eq!(budget.prompt_overhead_tokens, 4_096);
        assert_eq!(budget.safety_margin_tokens, 128_000 / 12);
        assert_eq!(budget.direct_input_limit_tokens, 3_000);
        assert_eq!(budget.chunk_target_tokens, 8_000);
        assert_eq!(budget.adaptive_chunk_limit_tokens, 8_000);
        assert!(budget.usable_input_tokens < 128_000);

        let unknown = context_budget(
            &conversation,
            &model(None),
            16_384,
            &AdaptiveBudgetSnapshot::cold_start(),
        );
        assert_eq!(unknown.direct_input_limit_tokens, 8_000);
        assert_eq!(unknown.chunk_target_tokens, 8_000);

        let constrained = AdaptiveBudgetSnapshot {
            limit_tokens: 2_048,
            route_key: "route-constrained".to_string(),
            route_state: "degraded".to_string(),
            profile_samples: 4,
        };
        let constrained_budget =
            context_budget(&conversation, &model(Some(128_000)), 16_384, &constrained);
        assert_eq!(constrained_budget.direct_input_limit_tokens, 2_048);
        assert_eq!(constrained_budget.chunk_target_tokens, 2_048);
        assert_eq!(constrained_budget.adaptive_route_key, "route-constrained");
    }

    #[test]
    fn token_estimator_and_splitter_share_one_unit_scale() {
        for value in ["abcde", "中文abcde", "a\n中文\nxyz"] {
            let units = token_estimate_units(value);
            let tokens = super::estimate_text_tokens(value);
            assert!(tokens.saturating_mul(4) >= units);
            assert!(tokens.saturating_mul(4).saturating_sub(units) < 4);
        }
    }

    #[test]
    fn packs_adjacent_attachment_windows_without_crossing_sources() {
        let chunk = |id: &str, attachment_id: &str, excerpt: String| ConversationChunk {
            estimated_tokens: super::estimate_text_tokens(&excerpt),
            source: DeepNoteSourceChunk {
                chunk_id: id.to_string(),
                source_kind: DeepNoteSourceKind::Text,
                source_id: attachment_id.to_string(),
                message_id: Some("message-1".to_string()),
                attachment_id: Some(attachment_id.to_string()),
                library_item_id: None,
                location: format!("{attachment_id} 第 {id} 行"),
                excerpt: excerpt.clone(),
                content_hash: super::stable_hash(excerpt),
                ocr_confidence: None,
            },
            message_ids: vec!["message-1".to_string()],
        };
        let packed = pack_adjacent_attachment_chunks(
            vec![
                chunk("a-1", "attachment-a", "a".repeat(4_000)),
                chunk("a-2", "attachment-a", "b".repeat(4_000)),
                chunk("b-1", "attachment-b", "c".repeat(400)),
            ],
            2_048,
        );
        assert_eq!(packed.len(), 2);
        assert!(packed[0].source.excerpt.contains(&"a".repeat(100)));
        assert!(packed[0].source.excerpt.contains(&"b".repeat(100)));
        assert_eq!(
            packed[1].source.attachment_id.as_deref(),
            Some("attachment-b")
        );
    }

    #[test]
    fn chunk_ids_and_digest_prompts_are_position_independent() {
        let excerpt = "相同内容应跨 run 复用".to_string();
        assert_eq!(
            content_addressed_chunk_id(&excerpt),
            content_addressed_chunk_id(&excerpt)
        );
        assert_ne!(
            content_addressed_chunk_id(&excerpt),
            content_addressed_chunk_id("不同内容")
        );
        let chunk = |message_id: &str, location: &str| ConversationChunk {
            estimated_tokens: super::estimate_text_tokens(&excerpt),
            source: DeepNoteSourceChunk {
                chunk_id: content_addressed_chunk_id(&excerpt),
                source_kind: DeepNoteSourceKind::Conversation,
                source_id: "conversation".to_string(),
                message_id: Some(message_id.to_string()),
                attachment_id: None,
                library_item_id: None,
                location: location.to_string(),
                excerpt: excerpt.clone(),
                content_hash: super::stable_hash(&excerpt),
                ocr_confidence: None,
            },
            message_ids: vec![message_id.to_string()],
        };
        assert_eq!(
            chunk_analysis_prompt(&chunk("message-1", "开头")),
            chunk_analysis_prompt(&chunk("message-9", "结尾"))
        );
    }

    #[test]
    fn recovery_allows_only_appended_messages_and_rejects_prefix_mutation() {
        let mut conversation = conversation(vec![message(
            "message-1",
            ModelRole::User,
            "original context".to_string(),
        )]);
        let snapshot = input_snapshot(&conversation, model(Some(128_000)), 1, Vec::new());
        assert!(validate_recovery_snapshot(&conversation, &snapshot, Vec::new()).is_ok());

        conversation.messages.push(message(
            "message-2",
            ModelRole::User,
            "new context".to_string(),
        ));
        conversation.updated_at += 1;
        assert!(validate_recovery_snapshot(&conversation, &snapshot, Vec::new()).is_ok());

        let projected = snapshot_conversation_after_validation(&conversation, &snapshot).unwrap();
        assert_eq!(projected.messages.len(), 1);
        assert_eq!(projected.messages[0].id, "message-1");

        conversation.messages[0].content = "edited context".to_string();
        let error = validate_recovery_snapshot(&conversation, &snapshot, Vec::new()).unwrap_err();
        assert!(error.contains("已经被编辑"));
    }

    #[test]
    fn recovery_rejects_attachment_byte_hash_changes() {
        let mut source_message = message(
            "message-1",
            ModelRole::User,
            "attachment context".to_string(),
        );
        source_message.attachments.push(StoredChatAttachment {
            id: "attachment-1".to_string(),
            kind: "file".to_string(),
            name: "source.txt".to_string(),
            mime_type: "text/plain".to_string(),
            size_bytes: 8,
            path: "attachment-1.txt".to_string(),
            preview_path: None,
            width: None,
            height: None,
        });
        let conversation = conversation(vec![source_message]);
        let snapshot = input_snapshot(
            &conversation,
            model(Some(128_000)),
            1,
            vec!["original-byte-hash".to_string()],
        );

        let error = validate_recovery_snapshot(
            &conversation,
            &snapshot,
            vec!["changed-byte-hash".to_string()],
        )
        .unwrap_err();
        assert!(error.contains("附件"));
    }

    #[test]
    fn evidence_with_explicit_missing_sources_stays_insufficient() {
        use crate::chat::note_pipeline::types::{
            compile_plan, DeepNoteOutline, DeepNoteSection, DeepNoteSectionKind,
        };
        use crate::library::types::NotePipelineRun;

        let outline = DeepNoteOutline {
            title: "Evidence".to_string(),
            sections: vec![DeepNoteSection {
                id: "sec-1".to_string(),
                heading: "Missing source".to_string(),
                kind: DeepNoteSectionKind::Concept,
                brief: "test".to_string(),
                purpose: "test".to_string(),
                depends_on: Vec::new(),
                evidence_requirements: Vec::new(),
                success_criteria: vec!["supported".to_string()],
                source_scope: Vec::new(),
                target_depth: "standard".to_string(),
                allow_ai_supplement: false,
                needs_supplement: false,
                source_message_ids: vec!["message-missing".to_string()],
            }],
            goal: String::new(),
            audience: String::new(),
            scope: String::new(),
            summary: String::new(),
            weak_points: Vec::new(),
            hidden_questions: Vec::new(),
            knowledge_gaps: Vec::new(),
            misconceptions: Vec::new(),
            causal_chains: Vec::new(),
            visualization_opportunities: Vec::new(),
            allow_ai_supplement: false,
            evidence_policy: String::new(),
            source_ids: Vec::new(),
        };
        let plan = compile_plan("run-1", 1, outline, "snapshot", "test").unwrap();
        let run = NotePipelineRun {
            id: "run-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            note_id: None,
            phase: NotePipelinePhase::Drafting,
            outline_json: String::new(),
            selected_section_ids: vec!["sec-1".to_string()],
            provider_id: "provider-1".to_string(),
            model_id: "model-1".to_string(),
            max_output_tokens: 2_048,
            thinking_enabled: false,
            retry_attempts: 1,
            input_snapshot_hash: "snapshot".to_string(),
            current_plan_version: 1,
            execution_version: 1,
            state_version: 0,
            runtime_instance_id: None,
            heartbeat_at: None,
            last_event_sequence: 0,
            budget_json: "{}".to_string(),
            preflight_json: "{}".to_string(),
            sidecar_json: String::new(),
            idempotency_key: "output-1".to_string(),
            completed_section_ids: Vec::new(),
            failed_section_ids: Vec::new(),
            warnings: Vec::new(),
            error_message: None,
            abandoned: false,
            created_at: 1,
            updated_at: 1,
        };
        let chunks = vec![DeepNoteSourceChunk {
            chunk_id: "chunk-1".to_string(),
            source_kind: DeepNoteSourceKind::Conversation,
            source_id: "conversation-1".to_string(),
            message_id: Some("message-1".to_string()),
            attachment_id: None,
            library_item_id: None,
            location: "message-1".to_string(),
            excerpt: "source".to_string(),
            content_hash: "hash".to_string(),
            ocr_confidence: None,
        }];

        let evidence = evidence_for_plan(&run, &plan, &chunks);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].status, DeepNoteEvidenceStatus::Insufficient);
        assert!(evidence[0].source_chunk_ids.is_empty());
    }

    #[test]
    fn retry_resets_failed_runtime_nodes_without_touching_completed_nodes() {
        let node = |node_id: &str, status: DeepNoteNodeStatus| DeepNoteDagNode {
            node_id: node_id.to_string(),
            node_type: DeepNoteNodeType::DraftSection,
            section_id: Some(node_id.to_string()),
            depends_on: Vec::new(),
            status,
            attempt_count: 5,
            evidence_ids: vec!["evidence-1".to_string()],
            input_hash: "input".to_string(),
            output_ref: Some("output".to_string()),
            validation_json: "{\"valid\":false}".to_string(),
            error_message: Some("timeout".to_string()),
        };
        let mut nodes = vec![
            node("completed", DeepNoteNodeStatus::Completed),
            node("failed", DeepNoteNodeStatus::Failed),
            node("review", DeepNoteNodeStatus::NeedsReview),
        ];

        reset_failed_nodes(&mut nodes);

        assert_eq!(nodes[0].status, DeepNoteNodeStatus::Completed);
        assert_eq!(nodes[0].attempt_count, 5);
        for recovered in &nodes[1..] {
            assert_eq!(recovered.status, DeepNoteNodeStatus::Pending);
            assert_eq!(recovered.attempt_count, 0);
            assert!(recovered.evidence_ids.is_empty());
            assert!(recovered.output_ref.is_none());
            assert!(recovered.validation_json.is_empty());
            assert!(recovered.error_message.is_none());
        }
    }

    #[test]
    fn compact_planner_ledger_bounds_retry_payload_without_dropping_summaries() {
        let ledger = DeepNoteLedger {
            verified_facts: (0..120)
                .map(|index| format!("fact-{index}").repeat(100))
                .collect(),
            section_summaries: (0..30)
                .map(|index| format!("chunk-{index}:{}", "summary".repeat(300)))
                .collect(),
            ..DeepNoteLedger::default()
        };
        let compact = compact_ledger_for_planner(&ledger);
        assert_eq!(compact.verified_facts.len(), 16);
        assert_eq!(compact.section_summaries.len(), 8);
        assert!(compact
            .section_summaries
            .first()
            .unwrap()
            .starts_with("chunk-0:"));
        assert!(compact
            .section_summaries
            .last()
            .unwrap()
            .starts_with("chunk-29:"));
        assert!(compact
            .verified_facts
            .iter()
            .all(|value| value.chars().count() <= 180));
        assert!(compact
            .section_summaries
            .iter()
            .all(|value| value.chars().count() <= 360));
    }

    #[test]
    fn semantic_chunking_preserves_markdown_and_keeps_bounded_fences_atomic() {
        let source = concat!(
            "# 标题\n\n",
            "解释一个需要保留结构的流程。\n\n",
            "```mermaid\n",
            "flowchart LR\n",
            "  A[输入] --> B[输出]\n",
            "```\n\n",
            "## 结论\n\n",
            "最后的说明。\n"
        );
        let chunks = split_text_by_token_budget(source, 32);
        assert_eq!(chunks.concat(), source);
        assert!(chunks.iter().any(|chunk| {
            chunk.contains("```mermaid\nflowchart LR\n  A[输入] --> B[输出]\n```")
        }));
    }

    #[test]
    fn evidence_ranking_prefers_relevant_chunks_and_rejects_unscoped_noise() {
        use crate::chat::note_pipeline::types::{
            compile_plan, DeepNoteOutline, DeepNoteSection, DeepNoteSectionKind,
        };
        use crate::library::types::NotePipelineRun;

        let section = DeepNoteSection {
            id: "sec-1".to_string(),
            heading: "SQLite 事务隔离".to_string(),
            kind: DeepNoteSectionKind::Concept,
            brief: "说明 SQLite 事务和 WAL".to_string(),
            purpose: "理解数据库并发".to_string(),
            depends_on: Vec::new(),
            evidence_requirements: vec!["SQLite WAL 事务证据".to_string()],
            success_criteria: vec!["说明事务".to_string()],
            source_scope: Vec::new(),
            target_depth: "standard".to_string(),
            allow_ai_supplement: false,
            needs_supplement: false,
            source_message_ids: Vec::new(),
        };
        let outline = DeepNoteOutline {
            title: "Evidence".to_string(),
            sections: vec![section],
            goal: String::new(),
            audience: String::new(),
            scope: String::new(),
            summary: String::new(),
            weak_points: Vec::new(),
            hidden_questions: Vec::new(),
            knowledge_gaps: Vec::new(),
            misconceptions: Vec::new(),
            causal_chains: Vec::new(),
            visualization_opportunities: Vec::new(),
            allow_ai_supplement: false,
            evidence_policy: String::new(),
            source_ids: Vec::new(),
        };
        let plan = compile_plan("run-1", 1, outline, "snapshot", "test").unwrap();
        let run = NotePipelineRun {
            id: "run-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            note_id: None,
            phase: NotePipelinePhase::Drafting,
            outline_json: String::new(),
            selected_section_ids: vec!["sec-1".to_string()],
            provider_id: "provider-1".to_string(),
            model_id: "model-1".to_string(),
            max_output_tokens: 2_048,
            thinking_enabled: false,
            retry_attempts: 1,
            input_snapshot_hash: "snapshot".to_string(),
            current_plan_version: 1,
            execution_version: 1,
            state_version: 0,
            runtime_instance_id: None,
            heartbeat_at: None,
            last_event_sequence: 0,
            budget_json: "{}".to_string(),
            preflight_json: "{}".to_string(),
            sidecar_json: String::new(),
            idempotency_key: "output-1".to_string(),
            completed_section_ids: Vec::new(),
            failed_section_ids: Vec::new(),
            warnings: Vec::new(),
            error_message: None,
            abandoned: false,
            created_at: 1,
            updated_at: 1,
        };
        let relevant = DeepNoteSourceChunk {
            chunk_id: "chunk-relevant".to_string(),
            source_kind: DeepNoteSourceKind::Conversation,
            source_id: "conversation-1".to_string(),
            message_id: Some("message-1".to_string()),
            attachment_id: None,
            library_item_id: None,
            location: "message-1".to_string(),
            excerpt: "SQLite 在 WAL 模式下允许读写并发，事务仍需控制写锁。".to_string(),
            content_hash: "relevant".to_string(),
            ocr_confidence: None,
        };
        let noise = DeepNoteSourceChunk {
            chunk_id: "chunk-noise".to_string(),
            source_kind: DeepNoteSourceKind::Conversation,
            source_id: "conversation-1".to_string(),
            message_id: Some("message-2".to_string()),
            attachment_id: None,
            library_item_id: None,
            location: "message-2".to_string(),
            excerpt: "宠物窗口支持拖动与锁定。".to_string(),
            content_hash: "noise".to_string(),
            ocr_confidence: None,
        };

        let evidence = evidence_for_plan(&run, &plan, &[noise.clone(), relevant]);
        assert_eq!(evidence[0].status, DeepNoteEvidenceStatus::Verified);
        assert_eq!(evidence[0].source_chunk_ids, vec!["chunk-relevant"]);

        let insufficient = evidence_for_plan(&run, &plan, &[noise]);
        assert_eq!(insufficient[0].status, DeepNoteEvidenceStatus::Insufficient);
        assert!(insufficient[0].source_chunk_ids.is_empty());
    }

    #[test]
    fn global_validation_rejects_duplicate_sections_and_missing_evidence() {
        use crate::chat::note_pipeline::types::{
            DeepNoteOutline, DeepNoteSection, DeepNoteSectionKind,
        };

        let section = |id: &str, heading: &str| DeepNoteSection {
            id: id.to_string(),
            heading: heading.to_string(),
            kind: DeepNoteSectionKind::Concept,
            brief: "解释核心概念".to_string(),
            purpose: "建立理解".to_string(),
            depends_on: Vec::new(),
            evidence_requirements: Vec::new(),
            success_criteria: vec!["核心概念".to_string()],
            source_scope: Vec::new(),
            target_depth: "standard".to_string(),
            allow_ai_supplement: false,
            needs_supplement: false,
            source_message_ids: Vec::new(),
        };
        let first = section("sec-1", "第一章");
        let second = section("sec-2", "第二章");
        let outline = DeepNoteOutline {
            title: "Global".to_string(),
            sections: vec![first.clone(), second.clone()],
            goal: String::new(),
            audience: String::new(),
            scope: String::new(),
            summary: String::new(),
            weak_points: Vec::new(),
            hidden_questions: Vec::new(),
            knowledge_gaps: Vec::new(),
            misconceptions: Vec::new(),
            causal_chains: Vec::new(),
            visualization_opportunities: Vec::new(),
            allow_ai_supplement: false,
            evidence_policy: String::new(),
            source_ids: Vec::new(),
        };
        let shared_body = format!("## 第一章\n\n核心概念。{}", "用于验证的正文。".repeat(30));
        let report = validate_global_drafts(
            &outline,
            &[
                (first, shared_body.clone(), false),
                (second, shared_body, false),
            ],
            &HashMap::new(),
        );

        assert!(!report.passed);
        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("没有可验证 Evidence")));
        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("正文完全重复")));
    }

    #[test]
    fn ledger_preparation_requires_complete_coverage_and_semantic_output() {
        let empty = DeepNoteLedger::default();
        assert!(!ledger_has_real_output(&empty, false));
        assert!(!ledger_has_real_output(&empty, true));

        let ledger = DeepNoteLedger {
            verified_facts: vec!["来源支持的事实".to_string()],
            ..DeepNoteLedger::default()
        };
        assert!(!ledger_has_real_output(&ledger, false));
        assert!(ledger_has_real_output(&ledger, true));
    }

    #[test]
    fn conversation_chunks_preserve_every_message_and_long_message_character() {
        let conversation = conversation(vec![
            message("message-1", ModelRole::User, "x".repeat(700)),
            message("message-2", ModelRole::Assistant, "界".repeat(200)),
        ]);
        let chunks = conversation_chunks(&conversation, 64).unwrap();
        assert!(chunks.len() > 2);
        assert!(chunks.iter().all(|chunk| chunk.estimated_tokens <= 64));
        let covered = chunks
            .iter()
            .flat_map(|chunk| chunk.message_ids.iter().cloned())
            .collect::<HashSet<_>>();
        assert_eq!(
            covered,
            HashSet::from(["message-1".to_string(), "message-2".to_string()])
        );
        let combined = chunks
            .iter()
            .map(|chunk| chunk.source.excerpt.as_str())
            .collect::<String>();
        assert_eq!(combined.matches('x').count(), 700);
        assert_eq!(combined.matches('界').count(), 200);
        assert!(!combined.contains("不应进入深度笔记来源"));
    }

    #[test]
    fn ledger_keeps_traceable_ids_and_rejects_empty_chunk_summaries() {
        let chunk = ConversationChunk {
            source: DeepNoteSourceChunk {
                chunk_id: "chunk-0001".to_string(),
                source_kind: DeepNoteSourceKind::Conversation,
                source_id: "conversation-1".to_string(),
                message_id: Some("message-1".to_string()),
                attachment_id: None,
                library_item_id: None,
                location: "消息 message-1".to_string(),
                content_hash: "hash".to_string(),
                excerpt: "source".to_string(),
                ocr_confidence: None,
            },
            message_ids: vec!["message-1".to_string()],
            estimated_tokens: 2,
        };
        let mut ledger = DeepNoteLedger::default();
        merge_chunk_digest(
            &mut ledger,
            &chunk,
            ChunkDigest {
                summary: "可追溯摘要".to_string(),
                verified_facts: vec!["事实".to_string()],
                source_message_ids: vec!["message-1".to_string(), "foreign-id".to_string()],
                canonical_terms: Vec::new(),
                covered_topics: Vec::new(),
                open_questions: Vec::new(),
                conflicts: Vec::new(),
                global_constraints: Vec::new(),
            },
        );
        assert_eq!(ledger.section_summaries.len(), 1);
        assert!(ledger.section_summaries[0].contains("message-1"));
        assert!(!ledger.section_summaries[0].contains("foreign-id"));
        assert!(ChunkDigest {
            summary: String::new(),
            canonical_terms: Vec::new(),
            verified_facts: Vec::new(),
            covered_topics: Vec::new(),
            open_questions: Vec::new(),
            conflicts: Vec::new(),
            global_constraints: Vec::new(),
            source_message_ids: Vec::new(),
        }
        .validate()
        .is_err());
    }

    #[test]
    fn direct_planner_transport_errors_fallback_without_becoming_json_repairs() {
        assert!(should_fallback_to_chunked_planner(&ModelError::timeout(
            "timeout"
        )));
        assert!(should_fallback_to_chunked_planner(
            &ModelError::context_length("too long")
        ));
        assert!(!should_fallback_to_chunked_planner(
            &ModelError::invalid_response("invalid json")
        ));
        assert!(!should_fallback_to_chunked_planner(&ModelError {
            kind: ModelErrorKind::Cancelled,
            message: "cancelled".to_string(),
            status_code: None,
            provider_code: None,
            retry_after_ms: None,
        }));
    }

    fn note_model_error(kind: ModelErrorKind) -> ModelError {
        ModelError {
            kind,
            message: "timeout".to_string(),
            status_code: None,
            provider_code: None,
            retry_after_ms: None,
        }
    }

    /// 管线内的全部 operation。新增阶段时必须补进来 —— 否决名单曾经用枚举式实现，
    /// 漏过 `deepNoteChunk` 与 `deepNoteVisionSource` 两个。
    const DEEP_NOTE_OPERATIONS: [&str; 7] = [
        "deepNote",
        "deepNoteChunk",
        "deepNoteChunkRepair",
        "deepNoteOutline",
        "deepNoteOutlineDirect",
        "deepNoteOutlineFallback",
        "deepNoteVisionSource",
    ];

    #[test]
    fn outline_gateway_timeouts_reduce_payload_instead_of_repeating_the_same_request() {
        let gateway_timeout = note_model_error(ModelErrorKind::UpstreamTimeout);
        assert!(!should_retry_note_model_call(
            "deepNoteOutline",
            &gateway_timeout
        ));
        assert!(!should_retry_note_model_call(
            "deepNoteOutlineFallback",
            &gateway_timeout
        ));
        assert!(should_retry_note_model_call(
            "deepNoteOutline",
            &ModelError {
                kind: ModelErrorKind::Connection,
                message: "temporary disconnect".to_string(),
                status_code: None,
                provider_code: None,
                retry_after_ms: None,
            }
        ));
        for kind in [
            ModelErrorKind::ModelNotFound,
            ModelErrorKind::ProviderUnavailable,
            ModelErrorKind::Authentication,
            ModelErrorKind::QuotaExceeded,
        ] {
            assert!(
                !should_retry_note_model_call("deepNoteChunk", &note_model_error(kind)),
                "{kind:?} 不应以相同路由原样重试"
            );
        }

        // 分块也被否决 HTTP 层重试，所以「缩小载荷」必须真的存在，否则一次网关
        // 504 就会直接判整次运行失败。这条断言把接盘方钉在测试里：只否决重试而
        // 不缩小载荷，是这个测试名字曾经承诺过却没兑现的事。
        assert!(!should_retry_note_model_call(
            "deepNoteChunk",
            &gateway_timeout
        ));
        let chunk = shrinkable_chunk(
            (0..200)
                .map(|index| format!("第 {index} 行：足够长的来源正文，用于触发切分。"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        assert!(
            split_chunk_in_half(&chunk).is_some(),
            "超时分块必须可以切小重投，否决 HTTP 重试才不会变成直接失败"
        );
    }

    fn signature_of(
        snapshot_hash: &str,
        model_id: &str,
        max_output_tokens: u32,
        thinking_enabled: bool,
        force_rebuild: bool,
    ) -> String {
        deep_note_content_signature(
            "conversation-1",
            snapshot_hash,
            "provider-1",
            model_id,
            max_output_tokens,
            thinking_enabled,
            force_rebuild,
        )
    }

    #[test]
    fn identical_deep_note_inputs_produce_identical_idempotency_keys() {
        // 修复前基键来自 `Uuid::new_v4()`，这个断言不可能成立。
        let first = signature_of("snapshot-a", "model-1", 8_192, false, false);
        let second = signature_of("snapshot-a", "model-1", 8_192, false, false);
        assert_eq!(first, second);
    }

    #[test]
    fn every_output_affecting_input_changes_the_idempotency_key() {
        let base = signature_of("snapshot-a", "model-1", 8_192, false, false);
        let variants = [
            signature_of("snapshot-b", "model-1", 8_192, false, false),
            signature_of("snapshot-a", "model-2", 8_192, false, false),
            signature_of("snapshot-a", "model-1", 16_384, false, false),
            signature_of("snapshot-a", "model-1", 8_192, true, false),
            // force_rebuild 改变来源口径（全量重建 vs 增量），必须改变指纹。
            signature_of("snapshot-a", "model-1", 8_192, false, true),
            deep_note_content_signature(
                "conversation-2",
                "snapshot-a",
                "provider-1",
                "model-1",
                8_192,
                false,
                false,
            ),
            deep_note_content_signature(
                "conversation-1",
                "snapshot-a",
                "provider-2",
                "model-1",
                8_192,
                false,
                false,
            ),
        ];
        for variant in variants {
            assert_ne!(base, variant);
        }
    }

    #[test]
    fn idempotency_key_field_separator_prevents_concatenation_ambiguity() {
        // 「ab + c」与「a + bc」在无分隔符拼接下会撞成同一个键。
        let left = deep_note_content_signature("ab", "c", "p", "m", 8_192, false, false);
        let right = deep_note_content_signature("a", "bc", "p", "m", 8_192, false, false);
        assert_ne!(left, right);
    }

    #[test]
    fn both_timeout_kinds_are_vetoed_on_every_deep_note_operation() {
        // 本地 `tokio::time::timeout` 到点（ClientTimeout）与网关 504（UpstreamTimeout）
        // 是同一个物理原因。修复前两者走出相反的路：前者原样重投 5 次，后者立刻降级。
        for operation in DEEP_NOTE_OPERATIONS {
            let client = should_retry_note_model_call(
                operation,
                &note_model_error(ModelErrorKind::ClientTimeout),
            );
            let upstream = should_retry_note_model_call(
                operation,
                &note_model_error(ModelErrorKind::UpstreamTimeout),
            );
            assert!(!client, "{operation} 的 ClientTimeout 应被否决");
            assert_eq!(client, upstream, "{operation} 的两种超时应当同构");
        }
    }

    #[test]
    fn non_pipeline_operations_keep_retrying_on_timeout() {
        // 前缀判定不能把普通会话的超时重试一并砍掉：那里没有「缩小载荷再来」的退路。
        for operation in ["chat", "titleSuggestion", "deepReview"] {
            assert!(should_retry_note_model_call(
                operation,
                &note_model_error(ModelErrorKind::ClientTimeout)
            ));
        }
    }

    #[test]
    fn provider_unavailable_is_never_retried() {
        for operation in DEEP_NOTE_OPERATIONS {
            assert!(!should_retry_note_model_call(
                operation,
                &note_model_error(ModelErrorKind::ProviderUnavailable)
            ));
        }
    }

    fn attachment(kind: &str) -> StoredChatAttachment {
        StoredChatAttachment {
            id: format!("attachment-{kind}"),
            kind: kind.to_string(),
            name: format!("source.{kind}"),
            mime_type: "application/octet-stream".to_string(),
            size_bytes: 8,
            path: format!("attachment-{kind}.bin"),
            preview_path: None,
            width: None,
            height: None,
        }
    }

    /// 字节硬闸报出的 kind 必须能触发缩小载荷，否则这道闸只是把「发出去被网关拒」
    /// 换成「本地直接失败」，用户体验更差。
    #[test]
    fn byte_gate_rejection_triggers_the_payload_reduction_path() {
        assert!(should_fallback_to_chunked_planner(&note_model_error(
            ModelErrorKind::ContextLengthExceeded
        )));
    }

    #[test]
    fn image_attachments_get_the_wider_byte_budget() {
        assert_eq!(request_byte_limit(&[]), REQUEST_BYTE_LIMIT);
        assert_eq!(
            request_byte_limit(&[attachment("file")]),
            REQUEST_BYTE_LIMIT,
            "非图片附件的内容以文本进入 prompt，走文本档"
        );
        assert_eq!(
            request_byte_limit(&[attachment("image")]),
            VISION_REQUEST_BYTE_LIMIT
        );
        assert_eq!(
            request_byte_limit(&[attachment("file"), attachment("image")]),
            VISION_REQUEST_BYTE_LIMIT,
            "混合附件里只要有一张图，就按带图档放宽"
        );
        assert!(VISION_REQUEST_BYTE_LIMIT > REQUEST_BYTE_LIMIT);
    }

    /// `transcript` 曾经把长度控制交给一个恒等函数 `truncate_chars`，等于没有控制。
    /// 现在明确不截断 —— 截断会静默丢掉 message-id，让后续引用校验把丢失的来源
    /// 判成模型编造。这个测试锁住「全部来源都在」这个性质。
    #[test]
    fn transcript_keeps_every_source_anchor_instead_of_truncating() {
        let messages = (1..=40)
            .map(|index| {
                message(
                    &format!("message-{index}"),
                    if index % 2 == 1 {
                        ModelRole::User
                    } else {
                        ModelRole::Assistant
                    },
                    "内容".repeat(500),
                )
            })
            .collect::<Vec<_>>();
        let conversation = conversation(messages);
        let transcript = super::transcript(&conversation, false);
        for index in 1..=40 {
            let anchor = format!("<!-- message-id: message-{index} -->");
            assert!(
                transcript.contains(&anchor),
                "转录丢了 {anchor}，长度控制不能靠截断来做"
            );
        }
    }

    /// section 的执行时长逐轮累加，重试不清零。
    ///
    /// 如果每轮重试都从零开始计时，一个反复重试的 section 永远撞不到上限 —— 预算
    /// 写了等于没写，而这正是「单个 section 卡死拖垮整个 run」的成因。
    #[test]
    fn section_wall_clock_accumulates_across_retries() {
        let mut active = std::collections::BTreeMap::new();
        let budget = crate::chat::note_pipeline::types::DeepNoteBudget::for_section_count(4);
        let limit = budget.section_wall_clock_ms;

        // 未登记过的 section 累计为 0，闸门不响。
        assert!(!section_wall_clock_exhausted(&active, &budget, "s1"));

        // 逐轮累加，差 1 毫秒时仍不该触发。
        record_section_active_ms(&mut active, "s1", limit / 2);
        assert!(!section_wall_clock_exhausted(&active, &budget, "s1"));
        record_section_active_ms(&mut active, "s1", limit / 2 - 1);
        assert!(!section_wall_clock_exhausted(&active, &budget, "s1"));
        assert_eq!(active.get("s1"), Some(&(limit - 1)));

        // 累计跨过上限：触发。
        record_section_active_ms(&mut active, "s1", 1);
        assert!(section_wall_clock_exhausted(&active, &budget, "s1"));

        // 各 section 独立计时：一个超时不牵连另一个。
        assert!(!section_wall_clock_exhausted(&active, &budget, "s2"));
    }

    /// 中断后隔很久才续跑，不该把没花掉的时间算进 section 预算。
    ///
    /// 这是这道闸门唯一真正生效的场景，也曾经是它最错的场景：闸门原先按
    /// `now - section_started_at` 判定，用户关掉应用过夜再续跑，所有在途 section
    /// 的「已耗时」都是十几个小时，会被整批跳过，产出一篇静默缺章的笔记 —— 而它们
    /// 可能一次上游调用都没跑完。预算必须只认实际执行时间，与挂钟无关。
    #[test]
    fn section_wall_clock_ignores_time_spent_paused_between_runs() {
        let mut active = std::collections::BTreeMap::new();
        let budget = crate::chat::note_pipeline::types::DeepNoteBudget::for_section_count(4);

        // 第一次 run：这个 section 只执行了 3 分钟就被中断。
        record_section_active_ms(&mut active, "s1", 3 * 60 * 1000);

        // 隔了 18 小时才续跑。累计活跃时长仍是 3 分钟，闸门不该响。
        assert!(
            !section_wall_clock_exhausted(&active, &budget, "s1"),
            "暂停时间被算进了 section 预算，续跑会静默跳过几乎没执行过的章节"
        );

        // 续跑后又执行到超过上限，这时才该跳过。
        record_section_active_ms(&mut active, "s1", budget.section_wall_clock_ms);
        assert!(section_wall_clock_exhausted(&active, &budget, "s1"));
    }

    /// 累加用饱和加法：溢出回绕会让一个严重超时的 section 看起来毫无消耗。
    #[test]
    fn section_active_time_accumulation_saturates() {
        let mut active = std::collections::BTreeMap::new();
        let budget = crate::chat::note_pipeline::types::DeepNoteBudget::for_section_count(2);
        record_section_active_ms(&mut active, "s1", u64::MAX);
        record_section_active_ms(&mut active, "s1", 5_000);
        assert_eq!(active.get("s1"), Some(&u64::MAX));
        assert!(section_wall_clock_exhausted(&active, &budget, "s1"));
    }

    #[test]
    fn pause_is_limited_to_interruptible_pipeline_phases() {
        assert!(can_pause_phase(NotePipelinePhase::Analyzing));
        assert!(can_pause_phase(NotePipelinePhase::Drafting));
        assert!(can_pause_phase(NotePipelinePhase::Validating));
        assert!(!can_pause_phase(NotePipelinePhase::AwaitingOutline));
        assert!(!can_pause_phase(NotePipelinePhase::Persisting));
        assert!(!can_pause_phase(NotePipelinePhase::Done));
        assert!(!can_pause_phase(NotePipelinePhase::Cancelled));
    }

    #[test]
    fn worker_expectation_distinguishes_active_and_user_waiting_phases() {
        assert!(phase_expects_background_worker(
            NotePipelinePhase::Analyzing
        ));
        assert!(phase_expects_background_worker(NotePipelinePhase::Drafting));
        assert!(!phase_expects_background_worker(
            NotePipelinePhase::AwaitingOutline
        ));
        assert!(!phase_expects_background_worker(NotePipelinePhase::Paused));
        assert!(!phase_expects_background_worker(
            NotePipelinePhase::Cancelled
        ));
    }

    #[test]
    fn structured_pipeline_failures_preserve_node_transition_context() {
        let raw = format!(
            "{DEEP_NOTE_FAILURE_PREFIX}{}",
            serde_json::json!({
                "category": "internalState",
                "message": "节点状态不一致",
                "nodeId": "analyze-input",
                "fromStatus": "ready",
                "toStatus": "completed",
                "retryable": true,
            })
        );
        let failure = pipeline_failure_payload(&raw);
        assert_eq!(failure["category"], "internalState");
        assert_eq!(failure["title"], "内部执行状态异常");
        assert_eq!(failure["nodeId"], "analyze-input");
        assert_eq!(failure["fromStatus"], "ready");
        assert_eq!(failure["toStatus"], "completed");
    }

    #[tokio::test]
    async fn cancellable_pipeline_wrapper_drops_a_pending_step_immediately() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result = await_note_pipeline_cancellable(&cancellation, async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok::<_, String>(())
        })
        .await;
        assert_eq!(result.unwrap_err(), "操作已取消。");
    }
}
