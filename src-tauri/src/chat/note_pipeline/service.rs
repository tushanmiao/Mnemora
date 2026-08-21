use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use sha2::{Digest, Sha256};

use tauri::{ipc::Channel, AppHandle, Manager, Runtime};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    ai::{
        error::{ModelError, ModelErrorKind},
        types::{ModelOptions, ModelRole},
    },
    chat::{
        conversation_types::{MessageStatus, StoredChatMessage, StoredConversation},
        service as chat_service,
        types::{ChatCompletionRequest, ChatModelMessage, ChatWorkspaceMode},
    },
    library::types::{
        LibraryNoteCreate, NoteEditProposalCreate, NotePipelinePhase, NotePipelineRun,
        NotePipelineRunCreate, NotePipelineSection, NotePipelineSectionCreate,
        NotePipelineSectionStatus, NoteSourceCreate, NoteSourceOrigin,
    },
    settings::types::ModelSettings,
    state::AppState,
};

use super::{
    merge::{apply_note_patches, compact_diff},
    prompts::{
        ANALYST_SYSTEM_PROMPT, CHUNK_ANALYST_SYSTEM_PROMPT, NOTE_EDIT_PATCH_PROMPT,
        NOTE_EDIT_PLAN_PROMPT, SECTION_REVISION_SYSTEM_PROMPT, SECTION_SYSTEM_PROMPT,
        STRICT_JSON_SUFFIX,
    },
    scheduler::{stable_topological_sections, DeepNoteDagScheduler},
    types::{
        compile_plan, DeepNoteBudget, DeepNoteCapabilities, DeepNoteContextBudget, DeepNoteDagNode,
        DeepNoteInputSnapshot, DeepNoteLedger, DeepNoteModelSnapshot, DeepNoteNodeStatus,
        DeepNoteOutline, DeepNotePlanVersion, DeepNotePreflight, DeepNoteRunDetail,
        DeepNoteRuntimeState, DeepNoteSection, DeepNoteSectionProgress, DeepNoteSourceChunk,
        DeepNoteSourceKind, DeepNoteValidationReport, NoteEditPrepareRequest,
        NoteEditPrepareResult, NoteMergePlan, NotePatchSet, NotePipelineActivity,
        NotePipelineAdjustRequest, NotePipelineConfirmRequest, NotePipelineProgress,
        NotePipelineStartRequest,
    },
};

const NODE_ATTEMPT_LIMIT: u8 = 5;
const SECTION_REVISION_LIMIT: u8 = 5;
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
const FAST_PLANNER_SYSTEM_PROMPT: &str = r#"You are the outline planner for a study note. Return exactly one valid JSON object and no markdown. Use only the supplied ledger and message IDs. Keep the outline concise: 4 to 8 sections. Required shape: {"goal":"","audience":"","scope":"","title":"","summary":"","weakPoints":[],"allowAiSupplement":false,"evidencePolicy":"","sourceIds":[],"sections":[{"id":"sec-1","heading":"","kind":"prerequisite|concept|comparison|pitfall|example|summary|selfcheck","purpose":"","brief":"","dependsOn":[],"evidenceRequirements":[],"successCriteria":[],"sourceScope":[],"targetDepth":"standard","allowAiSupplement":false,"needsSupplement":false,"sourceMessageIds":[]}]}. Every sourceMessageIds value must be copied from the ledger. Do not invent facts or IDs."#;
const OUTLINE_SIZE_SUFFIX: &str =
    "Prefer 6 to 12 sections and never exceed 12 sections. Keep every field concise.";
const DEFAULT_CHUNK_TARGET_TOKENS: u64 = 16_000;
const UNKNOWN_CONTEXT_CHUNK_TOKENS: u64 = 8_000;
const PLANNER_PROMPT_OVERHEAD_TOKENS: u64 = 4_096;
const MAX_ANALYSIS_CHUNKS: usize = 96;
const PIPELINE_STOP_WAIT_ATTEMPTS: usize = 100;
const PIPELINE_STOP_WAIT_INTERVAL: Duration = Duration::from_millis(25);

fn budget_for_drafting(previous: &DeepNoteBudget, section_count: usize) -> DeepNoteBudget {
    let mut budget = DeepNoteBudget::for_section_count(section_count);
    budget.semantic_calls_used = previous.semantic_calls_used;
    let revision_calls = section_count as u32 * (1 + u32::from(SECTION_REVISION_LIMIT));
    budget.semantic_call_limit = budget
        .semantic_call_limit
        .max(previous.semantic_calls_used.saturating_add(revision_calls));
    budget
}

#[derive(Debug, Clone)]
struct ConversationChunk {
    source: DeepNoteSourceChunk,
    message_ids: Vec<String>,
    estimated_tokens: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
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

fn progress(
    state: &AppState,
    channel: &Channel<NotePipelineProgress>,
    run_id: &str,
    phase: NotePipelinePhase,
    current: Option<usize>,
    total: Option<usize>,
    message: impl Into<String>,
) {
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

fn truncate_chars(value: String) -> String {
    value
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

fn transcript(conversation: &StoredConversation, include_reasoning: bool) -> String {
    truncate_chars(
        noteworthy_messages(conversation)
            .into_iter()
            .map(|message| {
                let anchor = format!("<!-- message-id: {} -->\n", message.id);
                format!("{anchor}{}", message_text(message, include_reasoning))
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}

fn estimate_text_tokens(value: &str) -> u64 {
    let mut ascii = 0u64;
    let mut non_ascii = 0u64;
    for character in value.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else {
            non_ascii += 1;
        }
    }
    ascii.div_ceil(4) + non_ascii
}

fn context_budget(
    conversation: &StoredConversation,
    model: &DeepNoteModelSnapshot,
    max_output_tokens: u32,
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
                usable.min(DIRECT_PLANNER_TOKEN_LIMIT),
                usable
                    .saturating_sub(1_024)
                    .min(DEFAULT_CHUNK_TARGET_TOKENS)
                    .max(2_048),
            )
        } else {
            (
                4_096,
                UNKNOWN_CONTEXT_CHUNK_TOKENS,
                UNKNOWN_CONTEXT_CHUNK_TOKENS,
                UNKNOWN_CONTEXT_CHUNK_TOKENS,
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
    for paragraph in value.split_inclusive("\n\n") {
        let paragraph_units = paragraph.chars().fold(0u64, |total, character| {
            total + if character.is_ascii() { 1 } else { 4 }
        });
        if !current.is_empty() && current_units.saturating_add(paragraph_units) > target_units {
            chunks.push(std::mem::take(&mut current));
            current_units = 0;
        }
        if paragraph_units <= target_units {
            current.push_str(paragraph);
            current_units += paragraph_units;
            continue;
        }
        for character in paragraph.chars() {
            let units = if character.is_ascii() { 1 } else { 4 };
            if !current.is_empty() && current_units.saturating_add(units) > target_units {
                chunks.push(std::mem::take(&mut current));
                current_units = 0;
            }
            current.push(character);
            current_units += units;
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

fn push_conversation_chunk(
    chunks: &mut Vec<ConversationChunk>,
    conversation_id: &str,
    excerpt: String,
    mut message_ids: Vec<String>,
) {
    if excerpt.trim().is_empty() {
        return;
    }
    message_ids.sort();
    message_ids.dedup();
    let chunk_id = format!("chunk-{:04}", chunks.len() + 1);
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
}

fn conversation_chunks(
    conversation: &StoredConversation,
    target_tokens: u64,
) -> Vec<ConversationChunk> {
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
                );
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
    push_conversation_chunk(&mut chunks, &conversation.id, current, current_ids);
    chunks
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
) -> (String, HashSet<String>, Option<String>) {
    let messages = noteworthy_messages(conversation);
    let start = summarized_until
        .and_then(|anchor| messages.iter().position(|message| message.id == anchor))
        .map_or(0, |index| index + 1);
    let selected = &messages[start..];
    let ids = selected
        .iter()
        .map(|message| message.id.clone())
        .collect::<HashSet<_>>();
    let last = selected.last().map(|message| message.id.clone());
    let value = truncate_chars(
        selected
            .iter()
            .map(|message| {
                format!(
                    "<!-- message-id: {} -->\n{}",
                    message.id,
                    message_text(message, true)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    );
    (value, ids, last)
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
        .or_else(|| crate::ai::model::database_supports_function_calling(&model.api_model))
        .unwrap_or(false);
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
    let requires_tools = attachments
        .iter()
        .any(|attachment| attachment.kind == "file");
    let mut missing_capabilities = Vec::new();
    if requires_vision && model.capabilities.vision != Some(true) {
        missing_capabilities.push("当前模型未明确支持图片识别".to_string());
    }
    if requires_tools && !model.capabilities.tools {
        missing_capabilities.push("当前模型不支持 Tool，无法读取文档附件".to_string());
    }
    let mut warnings = Vec::new();
    if !model.capabilities.tools && !requires_tools {
        warnings
            .push("当前模型不支持 Tool，本次仅使用已存储的文本、文献引用和笔记引用。".to_string());
    }
    if !model.capabilities.structured_outputs {
        warnings.push("当前模型使用严格 JSON 兼容模式，所有计划均由 Rust 校验。".to_string());
    }
    Ok(DeepNotePreflight {
        ready: missing_capabilities.is_empty(),
        model,
        requires_tools,
        requires_vision,
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
) -> DeepNoteInputSnapshot {
    let messages = noteworthy_messages(conversation);
    let message_ids = messages
        .iter()
        .map(|message| message.id.clone())
        .collect::<Vec<_>>();
    let attachments = messages
        .iter()
        .flat_map(|message| message.attachments.iter())
        .collect::<Vec<_>>();
    let attachment_ids = attachments
        .iter()
        .map(|attachment| attachment.id.clone())
        .collect::<Vec<_>>();
    let attachment_content_hashes = attachments
        .iter()
        .map(|attachment| {
            stable_hash(format!(
                "{}:{}:{}:{}",
                attachment.id, attachment.name, attachment.size_bytes, attachment.path
            ))
        })
        .collect::<Vec<_>>();
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
        attachment_ids,
        attachment_content_hashes,
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
) -> Result<(), String> {
    let current = input_snapshot(conversation, snapshot.model.clone(), snapshot.created_at);
    let unchanged = current.conversation_revision == snapshot.conversation_revision
        && current.message_ids == snapshot.message_ids
        && current.attachment_ids == snapshot.attachment_ids
        && current.attachment_content_hashes == snapshot.attachment_content_hashes
        && current.selected_literature_ids == snapshot.selected_literature_ids
        && current.selected_note_ids == snapshot.selected_note_ids;
    if unchanged {
        Ok(())
    } else {
        Err("会话内容或附件在任务创建后已经变化，不能混用旧检查点。请使用当前内容重新生成深度笔记。".to_string())
    }
}

fn extend_manual_recovery_budget(runtime: &mut DeepNoteRuntimeState) {
    runtime.budget.semantic_call_limit = runtime.budget.semantic_call_limit.max(
        runtime
            .budget
            .semantic_calls_used
            .saturating_add(12)
            .min(200),
    );
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

fn save_runtime_state(
    state: &AppState,
    run_id: &str,
    runtime: &DeepNoteRuntimeState,
) -> Result<(), String> {
    let runtime_json = serde_json::to_string(runtime)
        .map_err(|error| format!("序列化深度笔记运行状态失败：{error}"))?;
    let budget_json = serde_json::to_string(&runtime.budget)
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
    if runtime.budget.semantic_calls_used >= runtime.budget.semantic_call_limit {
        return Err(format!(
            "深度笔记语义调用预算已用尽（{}/{}）。",
            runtime.budget.semantic_calls_used, runtime.budget.semantic_call_limit
        ));
    }
    runtime.budget.semantic_calls_used += 1;
    save_runtime_state(state, run_id, runtime)
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
    for node in scheduler.nodes() {
        state.library_repository.update_note_pipeline_node_state(
            run_id,
            version,
            &node.node_id,
            node.status.as_str(),
            node.attempt_count,
            node.output_ref.as_deref(),
            &node.validation_json,
            node.error_message.as_deref(),
        )?;
    }
    save_runtime_state(state, run_id, runtime)
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

fn should_retry_note_model_call(operation: &str, error: &ModelError) -> bool {
    // A gateway timeout on an outline aggregation request means the current
    // payload did not finish inside the upstream gateway window. Return to the
    // pipeline immediately so it can reduce the payload before trying again.
    if error.kind == ModelErrorKind::UpstreamTimeout
        && matches!(
            operation,
            "deepNote" | "deepNoteOutline" | "deepNoteOutlineFallback" | "deepNoteOutlineDirect"
        )
    {
        return false;
    }
    !matches!(error.kind, ModelErrorKind::ProviderUnavailable)
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
    let started_at = crate::usage::now_ms();
    let call_id = Uuid::new_v4().to_string();
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
                "durationMs": 0,
                "responseChars": text.chars().count(),
                "inputChars": user_prompt.chars().count(),
                "systemPromptChars": system_prompt.chars().count(),
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
        }
    };
    let execution = chat_service::CompleteExecution {
        cancellation,
        max_retries: Some(max_retries),
        attempt_timeout: Some(Duration::from_millis(timeout_ms)),
        retry_predicate: Some(&|error| should_retry_note_model_call(operation, error)),
        on_progress: Some(&observer),
    };
    let input_chars = user_prompt.chars().count();
    let system_prompt_chars = system_prompt.chars().count();
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
                attachments: Vec::new(),
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
            let text = response.response.text;
            let _ = state.library_repository.append_note_pipeline_event(
                &run.id,
                "modelCallCompleted",
                None,
                &serde_json::json!({
                    "callId": call_id,
                    "operation": operation,
                    "phase": phase.as_str(),
                    "durationMs": crate::usage::now_ms().saturating_sub(started_at),
                    "responseChars": text.chars().count(),
                    "inputChars": input_chars,
                    "systemPromptChars": system_prompt_chars,
                    "maxOutputTokens": max_output_tokens,
                    "maxRetries": max_retries,
                    "timeoutMs": timeout_ms,
                })
                .to_string(),
            );
            Ok(text)
        }
        Err(error) => {
            let _ = state.library_repository.append_note_pipeline_event(
                &run.id,
                "modelCallFailed",
                None,
                &serde_json::json!({
                    "callId": call_id,
                    "operation": operation,
                    "phase": phase.as_str(),
                    "durationMs": crate::usage::now_ms().saturating_sub(started_at),
                    "errorKind": format!("{:?}", error.kind),
                    "message": error.message,
                    "statusCode": error.status_code,
                    "providerCode": error.provider_code,
                    "retryAfterMs": error.retry_after_ms,
                    "inputChars": input_chars,
                    "systemPromptChars": system_prompt_chars,
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
        "分块：{}\n预计输入：{} Token\n来源消息 ID：{}\n\n{}",
        chunk.source.chunk_id,
        chunk.estimated_tokens,
        chunk.message_ids.join(", "),
        chunk.source.excerpt
    )
}

fn ledger_analysis_prompt(
    ledger: &DeepNoteLedger,
    budget: &DeepNoteContextBudget,
    adjustment: &str,
) -> Result<String, String> {
    let ledger_json = serde_json::to_string_pretty(ledger)
        .map_err(|error| format!("序列化深度笔记知识账本失败：{error}"))?;
    Ok([
        (!adjustment.trim().is_empty())
            .then(|| format!("用户对提纲的补充要求：\n{}", adjustment.trim())),
        Some(format!(
            "以下知识账本由 {}/{} 个来源分块提取，覆盖 {}/{} 条消息，coverageComplete={}。请基于账本生成提纲，并把账本中真实的消息 ID 分配到 sourceMessageIds；不得宣称使用未覆盖内容。\n\n{}",
            budget.processed_chunk_count,
            budget.chunk_count,
            budget.processed_message_count,
            budget.total_message_count,
            budget.coverage_complete,
            ledger_json
        )),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n\n"))
}

fn compact_ledger_for_planner(ledger: &DeepNoteLedger) -> DeepNoteLedger {
    fn take(values: &[String], limit: usize, max_chars: usize) -> Vec<String> {
        values
            .iter()
            .take(limit)
            .map(|value| value.chars().take(max_chars).collect())
            .collect()
    }

    DeepNoteLedger {
        note_goal: ledger.note_goal.chars().take(1_000).collect(),
        audience: ledger.audience.chars().take(500).collect(),
        canonical_terms: take(&ledger.canonical_terms, 16, 80),
        verified_facts: take(&ledger.verified_facts, 16, 180),
        evidence_claim_links: take(&ledger.evidence_claim_links, 8, 160),
        covered_topics: take(&ledger.covered_topics, 16, 80),
        open_questions: take(&ledger.open_questions, 8, 140),
        conflicts: take(&ledger.conflicts, 8, 140),
        ai_supplements: take(&ledger.ai_supplements, 8, 140),
        section_summaries: take(&ledger.section_summaries, 6, 360),
        global_constraints: take(&ledger.global_constraints, 8, 140),
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
    let chunks = conversation_chunks(conversation, target_tokens);
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
    let can_resume = runtime.context_budget.chunk_count == chunks.len()
        && runtime.context_budget.chunk_target_tokens == target_tokens
        && runtime.context_budget.processed_chunk_count <= chunks.len()
        && !runtime.ledger.section_summaries.is_empty();
    if !can_resume {
        runtime.ledger = DeepNoteLedger::default();
        runtime.context_budget.processed_chunk_count = 0;
        runtime.context_budget.processed_message_count = 0;
    }
    runtime.context_budget.chunk_target_tokens = target_tokens;
    runtime.context_budget.chunk_count = chunks.len();
    runtime.context_budget.coverage_complete = false;
    runtime.context_budget.omitted_message_ids.clear();
    runtime.budget.semantic_call_limit = runtime
        .budget
        .semantic_call_limit
        .max(((chunks.len() as u32).saturating_mul(2) + 2).min(200));

    let mut processed_ids = chunks
        .iter()
        .take(runtime.context_budget.processed_chunk_count)
        .flat_map(|chunk| chunk.message_ids.iter().cloned())
        .collect::<HashSet<_>>();
    for (index, chunk) in chunks
        .iter()
        .enumerate()
        .skip(runtime.context_budget.processed_chunk_count)
    {
        if cancellation.is_cancelled() {
            return Err("操作已取消。".to_string());
        }
        progress(
            state,
            channel,
            &run.id,
            NotePipelinePhase::Analyzing,
            Some(index + 1),
            Some(chunks.len()),
            format!("正在提取来源分块 {}/{}", index + 1, chunks.len()),
        );
        let prompt = chunk_analysis_prompt(chunk);
        consume_semantic_call(state, &run.id, runtime)?;
        let raw = model_call_with_runtime(
            state,
            run,
            "deepNoteChunk",
            NotePipelinePhase::Analyzing,
            CHUNK_ANALYST_SYSTEM_PROMPT.to_string(),
            prompt.clone(),
            run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
            run.retry_attempts,
            cancellation,
            Some(channel),
        )
        .await
        .map_err(|error| error.message)?;
        let digest = match parse_json_object::<ChunkDigest>(&raw).and_then(ChunkDigest::validate) {
            Ok(digest) => digest,
            Err(_) => {
                consume_semantic_call(state, &run.id, runtime)?;
                let repaired = model_call_with_runtime(
                    state,
                    run,
                    "deepNoteChunkRepair",
                    NotePipelinePhase::Analyzing,
                    format!("{CHUNK_ANALYST_SYSTEM_PROMPT}\n\n{STRICT_JSON_SUFFIX}"),
                    prompt,
                    run.max_output_tokens.min(CHUNK_OUTPUT_TOKEN_LIMIT),
                    run.retry_attempts,
                    cancellation,
                    Some(channel),
                )
                .await
                .map_err(|error| error.message)?;
                parse_json_object::<ChunkDigest>(&repaired).and_then(ChunkDigest::validate)?
            }
        };
        merge_chunk_digest(&mut runtime.ledger, chunk, digest);
        processed_ids.extend(chunk.message_ids.iter().cloned());
        runtime.context_budget.processed_chunk_count = index + 1;
        runtime.context_budget.processed_message_count = processed_ids.len();
        save_runtime_state(state, &run.id, runtime)?;
        let _ = state.library_repository.append_note_pipeline_event(
            &run.id,
            "contextChunkCompleted",
            None,
            &serde_json::json!({
                "chunkIndex": index + 1,
                "chunkCount": chunks.len(),
                "processedMessageCount": processed_ids.len(),
                "totalMessageCount": runtime.context_budget.total_message_count,
            })
            .to_string(),
        );
    }
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
    if !raw.trim().is_empty() && estimate_text_tokens(&raw) <= raw_limit {
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
            "原始来源超过单章输入预算，以下内容来自已完成且可追溯的来源分块提取；来源消息 ID 保留在每条摘要中。\n\n{}",
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
        "全局标题：{}\n全局概览：{}\n薄弱点：{}\n全部章节：\n{}\n\n当前章节：{}\n{}\n\n全局知识账本：\n{}\n\n当前章节来源：\n{}",
        outline.title,
        outline.summary,
        outline.weak_points.join("；"),
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
        body.push(format!("{}\n\n> 来源：{}", markdown.trim(), sources));
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

fn validate_section_markdown(
    section: &DeepNoteSection,
    markdown: &str,
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
    DeepNoteValidationReport {
        passed: errors.is_empty(),
        errors,
        warnings,
        checked_evidence_ids: section.source_message_ids.clone(),
        criteria_coverage,
    }
}

fn sidecar_json(
    run: &NotePipelineRun,
    plan: &DeepNotePlanVersion,
    sections: &[(DeepNoteSection, String, bool)],
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
        "sections": sections.iter().map(|(section, markdown, failed)| serde_json::json!({
            "sectionId": section.id,
            "heading": section.heading,
            "dependsOn": section.depends_on,
            "sourceMessageIds": section.source_message_ids,
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

async fn persist_error(
    state: &AppState,
    run_id: &str,
    channel: &Channel<NotePipelineProgress>,
    error: String,
) {
    let run = {
        let _guard = state.library_operations.lock().await;
        match state.library_repository.get_note_pipeline_run(run_id) {
            Ok(current) if current.phase == NotePipelinePhase::Paused => Ok(current),
            Ok(_) => state.library_repository.update_note_pipeline_phase(
                run_id,
                NotePipelinePhase::Error,
                None,
                &[],
                Some(&error),
            ),
            Err(error) => Err(error),
        }
    };
    if let Ok(run) = &run {
        if run.phase == NotePipelinePhase::Paused {
            send(channel, NotePipelineProgress::Paused { run: run.clone() });
            return;
        }
    }
    let _ = state.library_repository.append_note_pipeline_event(
        run_id,
        "runFailed",
        None,
        &serde_json::json!({ "message": error }).to_string(),
    );
    send(
        channel,
        NotePipelineProgress::Error {
            run_id: run_id.to_string(),
            message: error,
        },
    );
    if run.is_err() {
        eprintln!("Failed to persist note pipeline error for {run_id}");
    }
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
    let calculated = context_budget(
        conversation,
        &runtime.input_snapshot.model,
        run.max_output_tokens,
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
            runtime.context_budget.chunk_target_tokens = previous.chunk_target_tokens;
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
    if runtime.context_budget.estimated_input_tokens
        <= runtime.context_budget.direct_input_limit_tokens
        && !runtime.context_budget.coverage_complete
    {
        let direct_chunks = conversation_chunks(
            conversation,
            runtime.context_budget.direct_input_limit_tokens.max(2_048),
        );
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
            format!("{ANALYST_SYSTEM_PROMPT}\n\n{OUTLINE_SIZE_SUFFIX}"),
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
        FAST_PLANNER_SYSTEM_PROMPT.to_string(),
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
                FAST_PLANNER_SYSTEM_PROMPT.to_string(),
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
        let conversation = state.conversation_repository.load(&run.conversation_id)?;
        let mut runtime = runtime_state(&run)?;
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
        let mut plan_version = compile_plan(
            &run_id,
            saved.current_plan_version.saturating_add(1).max(1),
            outline,
            &run.input_snapshot_hash,
            if adjustment.trim().is_empty() {
                "initial-plan"
            } else {
                adjustment.trim()
            },
        )?;
        plan_version.created_at = saved.updated_at;
        runtime.plan_version = Some(plan_version.clone());
        runtime.budget = budget_for_drafting(&runtime.budget, plan_version.plan.sections.len());
        save_runtime_state(&state, &run_id, &runtime)?;
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
        persist_error(&state, &run_id, &channel, error).await;
    }
    state.finish_note_pipeline_run(&run_id).await;
}

async fn execute_dag_section(
    state: &AppState,
    run: &NotePipelineRun,
    runtime: &mut DeepNoteRuntimeState,
    selected_outline: &DeepNoteOutline,
    section: &DeepNoteSection,
    ledger_context: &str,
    dependency_outputs: &str,
    channel: &Channel<NotePipelineProgress>,
    cancellation: &CancellationToken,
    persisted: Option<&NotePipelineSection>,
) -> Result<Option<(String, DeepNoteValidationReport, u8, u8)>, String> {
    let conversation = state.conversation_repository.load(&run.conversation_id)?;
    let (source_context, using_ledger_summary) = section_source_context(
        &conversation,
        section,
        &runtime.ledger,
        &runtime.context_budget,
    )?;
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
        checked_evidence_ids: Vec::new(),
        criteria_coverage: Vec::new(),
    };
    let mut attempts = persisted.map(|value| value.attempt_count).unwrap_or(0);
    let mut revisions = persisted.map(|value| value.revision_count).unwrap_or(0);
    'attempts: while attempts < NODE_ATTEMPT_LIMIT {
        if cancellation.is_cancelled() {
            break;
        }
        attempts += 1;
        consume_semantic_call(state, &run.id, runtime)?;
        match model_call_with_runtime(
            state,
            run,
            "deepNote",
            NotePipelinePhase::Drafting,
            SECTION_SYSTEM_PROMPT.to_string(),
            prompt.clone(),
            run.max_output_tokens.min(SECTION_OUTPUT_TOKEN_LIMIT),
            run.retry_attempts,
            cancellation,
            Some(channel),
        )
        .await
        {
            Ok(value) if !value.trim().is_empty() => {
                let mut candidate = value.trim().to_string();
                validation = validate_section_markdown(section, &candidate);
                while !validation.passed && revisions < SECTION_REVISION_LIMIT {
                    revisions += 1;
                    consume_semantic_call(state, &run.id, runtime)?;
                    let revision_prompt = format!(
                        "章节计划：\n{}\n\n当前正文：\n{}\n\n验证报告：\n{}",
                        serde_json::to_string(section).map_err(|error| error.to_string())?,
                        candidate,
                        serde_json::to_string(&validation).map_err(|error| error.to_string())?,
                    );
                    let revision_result = model_call_with_runtime(
                        state,
                        run,
                        "deepNote",
                        NotePipelinePhase::Validating,
                        SECTION_REVISION_SYSTEM_PROMPT.to_string(),
                        revision_prompt,
                        run.max_output_tokens.min(SECTION_OUTPUT_TOKEN_LIMIT),
                        run.retry_attempts,
                        cancellation,
                        Some(channel),
                    )
                    .await;
                    if cancellation.is_cancelled() {
                        attempts = attempts.saturating_sub(1);
                        revisions = revisions.saturating_sub(1);
                        break 'attempts;
                    }
                    candidate = revision_result
                        .map_err(|error| error.message)?
                        .trim()
                        .to_string();
                    validation = validate_section_markdown(section, &candidate);
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
    let Some(markdown) = markdown else {
        if cancellation.is_cancelled() {
            return Ok(None);
        }
        let validation_json =
            serde_json::to_string(&validation).map_err(|error| error.to_string())?;
        let _guard = state.library_operations.lock().await;
        state
            .library_repository
            .save_note_pipeline_section_checkpoint(
                &run.id,
                &section.id,
                "",
                NotePipelineSectionStatus::Failed,
                attempts,
                revisions,
                &section.source_message_ids,
                &validation_json,
                Some(&last_error),
            )?;
        return Err(format!(
            "章节“{}”在 {} 次节点尝试和 {} 次语义修订后仍未通过验证：{}",
            section.heading, attempts, revisions, last_error
        ));
    };
    let validation_json = serde_json::to_string(&validation).map_err(|error| error.to_string())?;
    let _guard = state.library_operations.lock().await;
    state
        .library_repository
        .save_note_pipeline_section_checkpoint(
            &run.id,
            &section.id,
            &markdown,
            NotePipelineSectionStatus::Completed,
            attempts,
            revisions,
            &section.source_message_ids,
            &validation_json,
            None,
        )?;
    Ok(Some((markdown, validation, attempts, revisions)))
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
        let conversation = state.conversation_repository.load(&run.conversation_id)?;
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
        let mut scheduler = DeepNoteDagScheduler::new(plan_version.compiled_dag.clone())?;
        scheduler.complete_preparation();
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
            let ready = scheduler.ready_section_ids(runtime.budget.max_parallel_nodes as usize);
            if ready.is_empty() {
                if scheduler.has_section_failures() {
                    break;
                }
                return Err("DAG 调度器无法释放下一个章节节点，可能存在未满足的依赖。".to_string());
            }
            for section_id in ready {
                if cancellation.is_cancelled() {
                    break;
                }
                let Some(section) = selected_outline
                    .sections
                    .iter()
                    .find(|value| value.id == section_id)
                    .cloned()
                else {
                    return Err(format!("DAG 节点引用了不存在的章节：{section_id}"));
                };
                scheduler.transition(
                    &format!("draft:{section_id}"),
                    DeepNoteNodeStatus::InProgress,
                )?;
                if let Ok(node) = scheduler.node_mut(&format!("draft:{section_id}")) {
                    node.attempt_count = persisted_sections
                        .get(&section_id)
                        .map(|value| value.attempt_count)
                        .unwrap_or(0);
                    node.error_message = None;
                }
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                state.library_repository.append_note_pipeline_event(
                    &run_id,
                    "dagNodeStarted",
                    Some(&format!("draft:{section_id}")),
                    &serde_json::json!({
                        "nodeId": format!("draft:{section_id}"),
                        "sectionId": section_id,
                        "nodeType": "draftSection",
                    })
                    .to_string(),
                )?;
                let completed_count = drafts_by_id.len();
                progress(
                    &state,
                    &channel,
                    &run_id,
                    NotePipelinePhase::Drafting,
                    Some(completed_count.saturating_add(1)),
                    Some(total),
                    format!(
                        "正在按依赖执行章节 {}/{}：{}",
                        completed_count.saturating_add(1),
                        total,
                        section.heading
                    ),
                );
                let dependency_outputs = dependency_context(&section, &drafts_by_id);
                let result = match execute_dag_section(
                    &state,
                    &run,
                    &mut runtime,
                    &selected_outline,
                    &section,
                    &ledger_context,
                    &dependency_outputs,
                    &channel,
                    &cancellation,
                    persisted_sections.get(&section_id),
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        let draft_node_id = format!("draft:{section_id}");
                        scheduler.transition(&draft_node_id, DeepNoteNodeStatus::Failed)?;
                        if let Ok(node) = scheduler.node_mut(&draft_node_id) {
                            node.attempt_count = persisted_sections
                                .get(&section_id)
                                .map(|value| value.attempt_count)
                                .unwrap_or(0)
                                .saturating_add(1);
                            node.error_message = Some(error.clone());
                        }
                        persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
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
                        scheduler.refresh_ready();
                        persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                        continue;
                    }
                };
                let Some((markdown, validation, attempts, revisions)) = result else {
                    scheduler.interrupt_running();
                    persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                    cancelled = true;
                    break;
                };
                let validation_json =
                    serde_json::to_string(&validation).map_err(|error| error.to_string())?;
                let draft_node_id = format!("draft:{section_id}");
                scheduler.transition(&draft_node_id, DeepNoteNodeStatus::Completed)?;
                if let Ok(node) = scheduler.node_mut(&draft_node_id) {
                    node.attempt_count = attempts;
                    node.output_ref = Some(format!("section:{section_id}"));
                    node.validation_json = validation_json.clone();
                    node.error_message = None;
                }
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                state.library_repository.append_note_pipeline_event(
                    &run_id,
                    "dagNodeCompleted",
                    Some(&draft_node_id),
                    &serde_json::json!({
                        "nodeId": draft_node_id,
                        "sectionId": section_id,
                        "attemptCount": attempts,
                        "revisionCount": revisions,
                        "markdownChars": markdown.chars().count(),
                    })
                    .to_string(),
                )?;
                scheduler.refresh_ready();
                let validate_node_id = format!("validate:{section_id}");
                scheduler.transition(&validate_node_id, DeepNoteNodeStatus::InProgress)?;
                scheduler.transition(&validate_node_id, DeepNoteNodeStatus::Completed)?;
                if let Ok(node) = scheduler.node_mut(&validate_node_id) {
                    node.attempt_count = revisions;
                    node.output_ref = Some(format!("validation:{section_id}"));
                    node.validation_json = validation_json;
                    node.error_message = None;
                }
                persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
                state.library_repository.append_note_pipeline_event(
                    &run_id,
                    "dagNodeCompleted",
                    Some(&validate_node_id),
                    &serde_json::json!({
                        "nodeId": validate_node_id,
                        "sectionId": section_id,
                        "nodeType": "validateSection",
                    })
                    .to_string(),
                )?;
                drafts_by_id.insert(section_id, markdown);
            }
        }
        if cancellation.is_cancelled() {
            scheduler.interrupt_running();
            cancelled = true;
        }
        if cancelled {
            scheduler.skip_unfinished_sections();
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
        }
        if scheduler.has_section_failures() && !cancelled {
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
            finish_interrupted_run(&state, &run_id, &channel).await?;
            return Ok(());
        }
        scheduler.refresh_ready();
        if scheduler
            .node("validate-global")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::Ready)
        {
            scheduler.transition("validate-global", DeepNoteNodeStatus::InProgress)?;
            scheduler.transition("validate-global", DeepNoteNodeStatus::Completed)?;
            if let Ok(node) = scheduler.node_mut("validate-global") {
                node.output_ref = Some("global-validation".to_string());
            }
            persist_scheduler_state(&state, &run_id, &mut runtime, &scheduler)?;
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
        let (mut title, content, mut warnings) = assemble(&effective_outline, &drafts, false);
        if cancelled {
            title = format!("{title}（部分完成）");
            warnings.push(format!(
                "任务已取消；已保存 {} 个完成章节，另有 {} 个章节未生成。",
                drafts.len(),
                total.saturating_sub(drafts.len())
            ));
        }
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
        let sidecar = sidecar_json(&run, &plan_version, &drafts)?;
        progress(
            &state,
            &channel,
            &run_id,
            NotePipelinePhase::Persisting,
            None,
            None,
            if cancelled {
                "正在保存已完成章节为草稿。"
            } else {
                "正在保存笔记与来源。"
            },
        );
        if scheduler
            .node("persist-note")
            .is_some_and(|node| node.status == DeepNoteNodeStatus::Ready)
        {
            scheduler.transition("persist-note", DeepNoteNodeStatus::InProgress)?;
        }
        let sources = note_sources(&conversation.id, last_message_id.as_deref(), &drafts);
        let note = {
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
            state.library_repository.create_note_with_sources(
                LibraryNoteCreate {
                    item_id: None,
                    title,
                    content,
                    group_name: None,
                },
                sources,
            )?
        };
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
        let completed = {
            let _guard = state.library_operations.lock().await;
            let completed = state.library_repository.update_note_pipeline_phase(
                &run_id,
                NotePipelinePhase::Done,
                Some(&note.id),
                &warnings,
                None,
            )?;
            state.library_repository.append_note_pipeline_event(
                &run_id,
                "runCompleted",
                None,
                &serde_json::json!({
                    "noteId": note.id,
                    "completedSectionCount": completed.completed_section_ids.len(),
                    "failedSectionCount": completed.failed_section_ids.len(),
                    "degraded": cancelled,
                })
                .to_string(),
            )?;
            completed
        };
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
    state.finish_note_pipeline_run(&run_id).await;
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
        let conversation = state.conversation_repository.load(&run.conversation_id)?;
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
            'attempts: while attempts < NODE_ATTEMPT_LIMIT {
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
                        let mut candidate = value.trim().to_string();
                        validation = validate_section_markdown(section, &candidate);
                        while !validation.passed && revisions < SECTION_REVISION_LIMIT {
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
                            candidate = revision_result
                                .map_err(|error| error.message)?
                                .trim()
                                .to_string();
                            validation = validate_section_markdown(section, &candidate);
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
        let sidecar = sidecar_json(&run, &plan_version, &drafts)?;
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
        let note = {
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
            state.library_repository.create_note_with_sources(
                LibraryNoteCreate {
                    item_id: None,
                    title,
                    content,
                    group_name: None,
                },
                sources,
            )?
        };
        let phase = NotePipelinePhase::Done;
        let completed = {
            let _guard = state.library_operations.lock().await;
            let completed = state.library_repository.update_note_pipeline_phase(
                &run_id,
                phase,
                Some(&note.id),
                &warnings,
                None,
            )?;
            state.library_repository.append_note_pipeline_event(
                &run_id,
                "runCompleted",
                None,
                &serde_json::json!({
                    "noteId": note.id,
                    "completedSectionCount": completed.completed_section_ids.len(),
                    "failedSectionCount": completed.failed_section_ids.len(),
                    "degraded": cancelled,
                })
                .to_string(),
            )?;
            completed
        };
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
    state.finish_note_pipeline_run(&run_id).await;
}

async fn spawn_analysis<R: Runtime>(
    app: &AppHandle<R>,
    run_id: String,
    adjustment: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<(), String> {
    let cancellation = CancellationToken::new();
    let state = app.state::<AppState>();
    if !state
        .register_note_pipeline_run(run_id.clone(), cancellation.clone())
        .await
    {
        return Err("深度笔记任务已经在运行。".to_string());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(run_analysis_task(
        app,
        run_id,
        adjustment,
        channel,
        cancellation,
    ));
    Ok(())
}

async fn spawn_drafting<R: Runtime>(
    app: &AppHandle<R>,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<(), String> {
    let cancellation = CancellationToken::new();
    let state = app.state::<AppState>();
    if !state
        .register_note_pipeline_run(run_id.clone(), cancellation.clone())
        .await
    {
        return Err("深度笔记任务已经在运行。".to_string());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(run_drafting_task(app, run_id, channel, cancellation));
    Ok(())
}

pub async fn start<R: Runtime>(
    app: &AppHandle<R>,
    request: NotePipelineStartRequest,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let conversation = state
        .conversation_repository
        .load(request.conversation_id.trim())?;
    let (provider_id, model_id, preflight) = {
        let settings = state
            .model_settings
            .read()
            .map_err(|_| "模型设置锁不可用。".to_string())?;
        let (provider_id, model_id) = resolve_note_model(&settings, &conversation)?;
        let preflight = preflight(&settings, &conversation, &provider_id, &model_id)?;
        (provider_id, model_id, preflight)
    };
    if !preflight.ready {
        return Err(format!(
            "当前模型无法启动深度笔记：{}。请切换模型、移除不支持的附件或返回设置。",
            preflight.missing_capabilities.join("；")
        ));
    }
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
    let snapshot = input_snapshot(&conversation, preflight.model.clone(), created_at);
    let snapshot_json = serde_json::to_string(&snapshot)
        .map_err(|error| format!("序列化深度笔记输入快照失败：{error}"))?;
    let snapshot_hash = stable_hash(&snapshot_json);
    let runtime = DeepNoteRuntimeState {
        preflight: preflight.clone(),
        input_snapshot: snapshot,
        plan_version: None,
        budget: DeepNoteBudget::for_section_count(1),
        ledger: DeepNoteLedger::default(),
        context_budget: DeepNoteContextBudget::default(),
    };
    let runtime_json = serde_json::to_string(&runtime)
        .map_err(|error| format!("序列化深度笔记运行状态失败：{error}"))?;
    let budget_json = serde_json::to_string(&runtime.budget)
        .map_err(|error| format!("序列化深度笔记预算失败：{error}"))?;
    let run_id = Uuid::new_v4().to_string();
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
                idempotency_key: stable_hash(format!("deep-note-output:{run_id}")),
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
    let run = {
        let _guard = state.library_operations.lock().await;
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
        NotePipelinePhase::Compiling
        | NotePipelinePhase::Queued
        | NotePipelinePhase::Drafting
        | NotePipelinePhase::Validating
        | NotePipelinePhase::Replanning
        | NotePipelinePhase::Assembling
        | NotePipelinePhase::Persisting
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
    validate_recovery_snapshot(&conversation, &runtime.input_snapshot)?;
    extend_manual_recovery_budget(&mut runtime);
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
        validate_recovery_snapshot(&conversation, &runtime.input_snapshot)?;
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

pub async fn cancel<R: Runtime>(app: &AppHandle<R>, run_id: &str) -> Result<bool, String> {
    let state = app.state::<AppState>();
    if state.cancel_note_pipeline_run(run_id).await {
        return Ok(true);
    }
    let run = state.library_repository.get_note_pipeline_run(run_id)?;
    if run.abandoned {
        return Ok(false);
    }
    if matches!(
        run.phase,
        NotePipelinePhase::Done | NotePipelinePhase::Cancelled
    ) {
        return Ok(false);
    }
    let _guard = state.library_operations.lock().await;
    state.library_repository.update_note_pipeline_phase(
        run_id,
        NotePipelinePhase::Cancelled,
        None,
        &run.warnings,
        None,
    )?;
    Ok(true)
}

/// 用户明确删除来源会话时调用。先停止后台请求，再将任务永久标记为已遗弃；
/// 与普通“停止”不同，遗弃任务不会出现在恢复列表，也不能继续、重试或重新生成。
pub async fn abandon<R: Runtime>(
    app: &AppHandle<R>,
    run_id: &str,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let _ = state.cancel_note_pipeline_run(run_id).await;
    if !wait_for_pipeline_task_to_stop(&state, run_id).await {
        return Err("深度笔记后台任务仍在结束处理中，请稍后再试。".to_string());
    }
    let _guard = state.library_operations.lock().await;
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
    let source_chunk_count = runtime.context_budget.chunk_count;
    Ok(DeepNoteRunDetail {
        run,
        preflight: Some(runtime.preflight),
        input_snapshot: Some(runtime.input_snapshot),
        plan_version: runtime.plan_version.clone(),
        budget: runtime.budget,
        context_budget: runtime.context_budget,
        source_chunk_count,
        nodes: runtime
            .plan_version
            .map(|plan| plan.compiled_dag)
            .unwrap_or_default(),
        sections: section_progress,
        source_chunks: Vec::new(),
        evidence: Vec::new(),
        ledger: runtime.ledger,
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
    let (provider_id, model_id) = {
        let settings = state
            .model_settings
            .read()
            .map_err(|_| "模型设置锁不可用。".to_string())?;
        resolve_note_model(&settings, &conversation)?
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
    let run = NotePipelineRun {
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
        budget_json: "{}".to_string(),
        preflight_json: "{}".to_string(),
        sidecar_json: String::new(),
        idempotency_key: String::new(),
        completed_section_ids: Vec::new(),
        failed_section_ids: Vec::new(),
        warnings: Vec::new(),
        error_message: None,
        abandoned: false,
        created_at: 0,
        updated_at: 0,
    };
    let summarized_until = state
        .library_repository
        .latest_summarized_message_id(&note.id, &conversation.id)?;
    let (mut new_transcript, valid_ids, last_message_id) =
        incremental_transcript(&conversation, summarized_until.as_deref());
    if new_transcript.trim().is_empty() {
        if request.selected_text.trim().is_empty() {
            return Err("这段对话没有尚未合入目标笔记的新内容。".to_string());
        }
        new_transcript = "（没有新的对话增量；请只按选中文本和用户要求生成局部修改。）".to_string();
    }
    let context = format!(
        "目标笔记：\n{}\n\n新对话增量：\n{}\n\n选中文本：\n{}\n\n所属章节：{}\n\n用户要求：{}",
        note.content,
        new_transcript,
        request.selected_text.trim(),
        request.section_heading.trim(),
        request.requirement.trim(),
    );
    let raw_plan = model_call(
        state,
        &run,
        "noteEdit",
        NOTE_EDIT_PLAN_PROMPT.to_string(),
        context.clone(),
        max_output_tokens.min(8_192),
    )
    .await?;
    let plan = match parse_json_object::<NoteMergePlan>(&raw_plan) {
        Ok(plan) => plan,
        Err(_) => {
            let raw_plan = model_call(
                state,
                &run,
                "noteEdit",
                format!("{NOTE_EDIT_PLAN_PROMPT}\n\n{STRICT_JSON_SUFFIX}"),
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
        "目标笔记：\n{}\n\n新对话增量：\n{}\n\n合并计划：\n{}",
        note.content,
        new_transcript,
        serde_json::to_string(&plan).map_err(|error| error.to_string())?,
    );
    let raw_patches = model_call(
        state,
        &run,
        "noteEdit",
        NOTE_EDIT_PATCH_PROMPT.to_string(),
        patch_prompt.clone(),
        max_output_tokens.min(16_384),
    )
    .await?;
    let mut patch_set = match parse_json_object::<NotePatchSet>(&raw_patches) {
        Ok(patches) => patches,
        Err(_) => {
            let raw_patches = model_call(
                state,
                &run,
                "noteEdit",
                format!("{NOTE_EDIT_PATCH_PROMPT}\n\n{STRICT_JSON_SUFFIX}"),
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
        patch.source_message_ids.retain(|id| valid_ids.contains(id));
        patch.source_message_ids.sort();
        patch.source_message_ids.dedup();
    }
    let (new_content, warnings) = apply_note_patches(&note.content, &patch_set.patches)?;
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
    let sources = patch_set
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
        .collect();
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
            })?
    };
    Ok(NoteEditPrepareResult { proposal, warnings })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::{
        ai::{
            error::{ModelError, ModelErrorKind},
            types::ModelRole,
        },
        chat::conversation_types::{
            AiPermissionMode, MessageStatus, StoredChatMessage, StoredConversation,
        },
        library::types::NotePipelinePhase,
    };

    use super::{
        can_pause_phase, compact_ledger_for_planner, context_budget, conversation_chunks,
        input_snapshot, merge_chunk_digest, reset_failed_nodes, should_fallback_to_chunked_planner,
        should_retry_note_model_call, validate_recovery_snapshot, ChunkDigest, ConversationChunk,
    };
    use crate::chat::note_pipeline::types::{
        DeepNoteCapabilities, DeepNoteDagNode, DeepNoteLedger, DeepNoteModelSnapshot,
        DeepNoteNodeStatus, DeepNoteNodeType, DeepNoteSourceChunk, DeepNoteSourceKind,
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
            agent_run_id: None,
            workflow_summary: None,
            error_message: None,
        }
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
                tools: true,
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
        let budget = context_budget(&conversation, &model(Some(128_000)), 16_384);
        assert_eq!(budget.planner_output_reserve_tokens, 2_048);
        assert_eq!(budget.prompt_overhead_tokens, 4_096);
        assert_eq!(budget.safety_margin_tokens, 128_000 / 12);
        assert_eq!(budget.direct_input_limit_tokens, 3_000);
        assert_eq!(budget.chunk_target_tokens, 16_000);
        assert!(budget.usable_input_tokens < 128_000);

        let unknown = context_budget(&conversation, &model(None), 16_384);
        assert_eq!(unknown.direct_input_limit_tokens, 8_000);
        assert_eq!(unknown.chunk_target_tokens, 8_000);
    }

    #[test]
    fn recovery_rejects_a_changed_conversation_snapshot() {
        let mut conversation = conversation(vec![message(
            "message-1",
            ModelRole::User,
            "original context".to_string(),
        )]);
        let snapshot = input_snapshot(&conversation, model(Some(128_000)), 1);
        assert!(validate_recovery_snapshot(&conversation, &snapshot).is_ok());

        conversation.messages.push(message(
            "message-2",
            ModelRole::User,
            "new context".to_string(),
        ));
        conversation.updated_at += 1;
        let error = validate_recovery_snapshot(&conversation, &snapshot).unwrap_err();
        assert!(error.contains("不能混用旧检查点"));
        assert!(error.contains("重新生成"));
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
        assert_eq!(compact.section_summaries.len(), 6);
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
    fn conversation_chunks_preserve_every_message_and_long_message_character() {
        let conversation = conversation(vec![
            message("message-1", ModelRole::User, "x".repeat(700)),
            message("message-2", ModelRole::Assistant, "界".repeat(200)),
        ]);
        let chunks = conversation_chunks(&conversation, 64);
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

    #[test]
    fn outline_gateway_timeouts_reduce_payload_instead_of_repeating_the_same_request() {
        let gateway_timeout = ModelError {
            kind: ModelErrorKind::UpstreamTimeout,
            message: "gateway timeout".to_string(),
            status_code: Some(504),
            provider_code: None,
            retry_after_ms: None,
        };
        assert!(!should_retry_note_model_call(
            "deepNoteOutline",
            &gateway_timeout
        ));
        assert!(!should_retry_note_model_call(
            "deepNoteOutlineFallback",
            &gateway_timeout
        ));
        assert!(should_retry_note_model_call(
            "deepNoteChunk",
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
}
