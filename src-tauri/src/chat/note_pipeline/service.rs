use std::collections::{HashMap, HashSet};

use tauri::{ipc::Channel, AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    ai::types::{ModelOptions, ModelRole},
    chat::{
        conversation_types::{MessageStatus, StoredChatMessage, StoredConversation},
        service as chat_service,
        types::{ChatCompletionRequest, ChatModelMessage, ChatWorkspaceMode},
    },
    library::types::{
        LibraryNoteCreate, NoteEditProposalCreate, NotePipelinePhase, NotePipelineRun,
        NotePipelineRunCreate, NotePipelineSectionCreate, NotePipelineSectionStatus,
        NoteSourceCreate, NoteSourceOrigin,
    },
    settings::types::ModelSettings,
    state::AppState,
};

use super::{
    merge::{apply_note_patches, compact_diff},
    prompts::{
        ANALYST_SYSTEM_PROMPT, NOTE_EDIT_PATCH_PROMPT, NOTE_EDIT_PLAN_PROMPT,
        SECTION_SYSTEM_PROMPT, SIMPLE_NOTE_SYSTEM_PROMPT, STRICT_JSON_SUFFIX,
    },
    types::{
        DeepNoteOutline, DeepNoteSection, NoteEditPrepareRequest, NoteEditPrepareResult,
        NoteMergePlan, NotePatchSet, NotePipelineAdjustRequest, NotePipelineConfirmRequest,
        NotePipelineProgress, NotePipelineStartRequest,
    },
};

const MAX_TRANSCRIPT_CHARS: usize = 300_000;

fn send(channel: &Channel<NotePipelineProgress>, event: NotePipelineProgress) {
    let _ = channel.send(event);
}

fn progress(
    channel: &Channel<NotePipelineProgress>,
    run_id: &str,
    phase: NotePipelinePhase,
    current: Option<usize>,
    total: Option<usize>,
    message: impl Into<String>,
) {
    send(
        channel,
        NotePipelineProgress::Progress {
            run_id: run_id.to_string(),
            phase,
            current,
            total,
            message: message.into(),
        },
    );
}

fn truncate_chars(value: String) -> String {
    if value.chars().count() <= MAX_TRANSCRIPT_CHARS {
        return value;
    }
    let mut output = value.chars().take(MAX_TRANSCRIPT_CHARS).collect::<String>();
    output.push_str("\n\n（对话过长，以上转写已截断。）");
    output
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
                let anchor = if include_reasoning {
                    format!("<!-- message-id: {} -->\n", message.id)
                } else {
                    String::new()
                };
                format!("{anchor}{}", message_text(message, include_reasoning))
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
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
    let response = chat_service::complete(
        state,
        ChatCompletionRequest {
            provider_id: run.provider_id.clone(),
            model_id: run.model_id.clone(),
            conversation_id: Some(run.conversation_id.clone()),
            message_id: Some(Uuid::new_v4().to_string()),
            operation: Some(operation.to_string()),
            system_prompt,
            activated_skill_ids: Vec::new(),
            slash_skill_id: None,
            permission_mode: Default::default(),
            workspace_mode: ChatWorkspaceMode::Notes,
            messages: vec![ChatModelMessage {
                role: ModelRole::User,
                content: user_prompt,
                attachments: Vec::new(),
            }],
            options: ModelOptions {
                temperature: None,
                max_output_tokens: Some(max_output_tokens),
                thinking_enabled: run.thinking_enabled,
            },
        },
    )
    .await
    .map_err(|error| error.message)?;
    Ok(response.response.text)
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

fn section_prompt(
    outline: &DeepNoteOutline,
    section: &DeepNoteSection,
    conversation_transcript: &str,
    previous_tail: &str,
) -> Result<String, String> {
    Ok(format!(
        "全局标题：{}\n全局概览：{}\n薄弱点：{}\n全部章节：\n{}\n\n当前章节：{}\n{}\n\n对话转写：\n{}",
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
        if previous_tail.is_empty() {
            String::new()
        } else {
            format!("前一章末段摘要：\n{previous_tail}")
        },
        conversation_transcript,
    ))
}

fn tail(value: &str) -> String {
    value
        .chars()
        .rev()
        .take(500)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn split_note_markdown(markdown: &str, fallback_title: &str) -> (String, String) {
    let content = markdown.trim();
    let title = content
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_title.trim())
        .chars()
        .take(500)
        .collect::<String>();
    let title = if title.is_empty() {
        "未命名笔记".to_string()
    } else {
        title
    };
    let content = if content.lines().any(|line| line.starts_with("# ")) {
        content.to_string()
    } else {
        format!("# {title}\n\n{content}")
    };
    (title, content)
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
        state.library_repository.update_note_pipeline_phase(
            run_id,
            NotePipelinePhase::Error,
            None,
            &[],
            Some(&error),
        )
    };
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
    conversation: &StoredConversation,
    adjustment: &str,
) -> Result<DeepNoteOutline, String> {
    let analysis_transcript = transcript(conversation, true);
    if analysis_transcript.is_empty() {
        return Err("对话还没有可以生成深度笔记的消息。".to_string());
    }
    let valid_ids = noteworthy_messages(conversation)
        .into_iter()
        .map(|message| message.id.clone())
        .collect::<HashSet<_>>();
    let user_prompt = analysis_prompt(&analysis_transcript, adjustment);
    let first = model_call(
        state,
        run,
        "deepNote",
        ANALYST_SYSTEM_PROMPT.to_string(),
        user_prompt.clone(),
        run.max_output_tokens.min(8_192),
    )
    .await;
    if let Ok(raw) = first {
        if let Ok(outline) = parse_json_object::<DeepNoteOutline>(&raw)
            .and_then(|outline| outline.validate(&valid_ids))
        {
            return Ok(outline);
        }
    }
    let raw = model_call(
        state,
        run,
        "deepNote",
        format!("{ANALYST_SYSTEM_PROMPT}\n\n{STRICT_JSON_SUFFIX}"),
        user_prompt,
        run.max_output_tokens.min(8_192),
    )
    .await?;
    parse_json_object::<DeepNoteOutline>(&raw).and_then(|outline| outline.validate(&valid_ids))
}

async fn fallback_simple_note(
    state: &AppState,
    run: &NotePipelineRun,
    conversation: &StoredConversation,
) -> Result<NotePipelineRun, String> {
    let clean_transcript = transcript(conversation, false);
    let raw = model_call(
        state,
        run,
        "noteSummary",
        SIMPLE_NOTE_SYSTEM_PROMPT.to_string(),
        clean_transcript,
        run.max_output_tokens.min(16_384),
    )
    .await?;
    let (title, content) = split_note_markdown(&raw, &conversation.title);
    let messages = noteworthy_messages(conversation);
    let last_message_id = messages.last().map(|message| message.id.clone());
    let sources = messages
        .iter()
        .map(|message| NoteSourceCreate {
            section_id: "summary".to_string(),
            origin: NoteSourceOrigin::Conversation,
            conversation_id: Some(conversation.id.clone()),
            message_id: Some(message.id.clone()),
            summarized_until_message_id: last_message_id.clone(),
        })
        .collect();
    let note = {
        let _guard = state.library_operations.lock().await;
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
    let warning = vec!["分析师提纲解析失败，已降级为简版总结。".to_string()];
    let _guard = state.library_operations.lock().await;
    state.library_repository.update_note_pipeline_phase(
        &run.id,
        NotePipelinePhase::Done,
        Some(&note.id),
        &warning,
        None,
    )
}

async fn run_analysis_task(
    app: AppHandle,
    run_id: String,
    adjustment: String,
    channel: Channel<NotePipelineProgress>,
    cancellation: CancellationToken,
) {
    let state = app.state::<AppState>();
    progress(
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
        let outline = match analyze_outline(&state, &run, &conversation, &adjustment).await {
            Ok(outline) => outline,
            Err(_) if !cancellation.is_cancelled() => {
                let completed = fallback_simple_note(&state, &run, &conversation).await?;
                send(
                    &channel,
                    NotePipelineProgress::Done {
                        run: completed,
                        degraded: true,
                    },
                );
                return Ok(());
            }
            Err(_) => {
                let cancelled = {
                    let _guard = state.library_operations.lock().await;
                    state.library_repository.update_note_pipeline_phase(
                        &run_id,
                        NotePipelinePhase::Cancelled,
                        None,
                        &[],
                        None,
                    )?
                };
                send(&channel, NotePipelineProgress::Cancelled { run: cancelled });
                return Ok(());
            }
        };
        if cancellation.is_cancelled() {
            let cancelled = {
                let _guard = state.library_operations.lock().await;
                state.library_repository.update_note_pipeline_phase(
                    &run_id,
                    NotePipelinePhase::Cancelled,
                    None,
                    &[],
                    None,
                )?
            };
            send(&channel, NotePipelineProgress::Cancelled { run: cancelled });
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
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let saved = {
            let _guard = state.library_operations.lock().await;
            state
                .library_repository
                .save_note_pipeline_outline(&run_id, &outline_json, sections)?
        };
        send(&channel, NotePipelineProgress::OutlineReady { run: saved });
        Ok(())
    }
    .await;
    if let Err(error) = result {
        persist_error(&state, &run_id, &channel, error).await;
    }
    state.finish_note_pipeline_run(&run_id).await;
}

async fn run_drafting_task(
    app: AppHandle,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
    cancellation: CancellationToken,
) {
    let state = app.state::<AppState>();
    let result = async {
        let run = state.library_repository.get_note_pipeline_run(&run_id)?;
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
        let clean_transcript = transcript(&conversation, false);
        let last_message_id = noteworthy_messages(&conversation)
            .last()
            .map(|message| message.id.clone());
        let total = selected_outline.sections.len();
        let mut drafts: Vec<(DeepNoteSection, String, bool)> = Vec::new();
        let mut previous_tail = String::new();
        for (index, section) in selected_outline.sections.iter().enumerate() {
            if cancellation.is_cancelled() {
                break;
            }
            if let Some(existing) = persisted.get(&section.id) {
                if existing.status == NotePipelineSectionStatus::Completed {
                    previous_tail = tail(&existing.markdown);
                    drafts.push((section.clone(), existing.markdown.clone(), false));
                    continue;
                }
            }
            progress(
                &channel,
                &run_id,
                NotePipelinePhase::Drafting,
                Some(index + 1),
                Some(total),
                format!("正在扩写 {}/{}：{}", index + 1, total, section.heading),
            );
            let prompt = section_prompt(
                &selected_outline,
                section,
                &clean_transcript,
                &previous_tail,
            )?;
            let mut last_error = String::new();
            let mut markdown = None;
            for _ in 0..run.retry_attempts.max(1) {
                if cancellation.is_cancelled() {
                    break;
                }
                match model_call(
                    &state,
                    &run,
                    "deepNote",
                    SECTION_SYSTEM_PROMPT.to_string(),
                    prompt.clone(),
                    run.max_output_tokens.min(16_384),
                )
                .await
                {
                    Ok(value) if !value.trim().is_empty() => {
                        markdown = Some(value.trim().to_string());
                        break;
                    }
                    Ok(_) => last_error = "模型返回了空章节。".to_string(),
                    Err(error) => last_error = error,
                }
            }
            if let Some(markdown) = markdown {
                {
                    let _guard = state.library_operations.lock().await;
                    state.library_repository.save_note_pipeline_section(
                        &run_id,
                        &section.id,
                        &markdown,
                        NotePipelineSectionStatus::Completed,
                        None,
                    )?;
                }
                previous_tail = tail(&markdown);
                drafts.push((section.clone(), markdown, false));
            } else if !cancellation.is_cancelled() {
                let placeholder = format!(
                    "## {}\n\n> [本章生成失败，可稍后重试]\n\n> 错误：{}",
                    section.heading, last_error
                );
                {
                    let _guard = state.library_operations.lock().await;
                    state.library_repository.save_note_pipeline_section(
                        &run_id,
                        &section.id,
                        &placeholder,
                        NotePipelineSectionStatus::Failed,
                        Some(&last_error),
                    )?;
                }
                previous_tail = tail(&placeholder);
                drafts.push((section.clone(), placeholder, true));
            }
        }
        let cancelled = cancellation.is_cancelled() || drafts.len() < total;
        if drafts.is_empty() {
            let cancelled_run = {
                let _guard = state.library_operations.lock().await;
                state.library_repository.update_note_pipeline_phase(
                    &run_id,
                    NotePipelinePhase::Cancelled,
                    None,
                    &[],
                    None,
                )?
            };
            send(
                &channel,
                NotePipelineProgress::Cancelled { run: cancelled_run },
            );
            return Ok(());
        }
        progress(
            &channel,
            &run_id,
            NotePipelinePhase::Assembling,
            None,
            None,
            "正在组装与检查笔记…",
        );
        {
            let _guard = state.library_operations.lock().await;
            state.library_repository.update_note_pipeline_phase(
                &run_id,
                NotePipelinePhase::Assembling,
                None,
                &[],
                None,
            )?;
        }
        let effective_outline = DeepNoteOutline {
            sections: drafts
                .iter()
                .map(|(section, _, _)| section.clone())
                .collect(),
            ..selected_outline
        };
        let (title, content, warnings) = assemble(&effective_outline, &drafts, cancelled);
        progress(
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
        let phase = if cancelled {
            NotePipelinePhase::Cancelled
        } else {
            NotePipelinePhase::Done
        };
        let completed = {
            let _guard = state.library_operations.lock().await;
            state.library_repository.update_note_pipeline_phase(
                &run_id,
                phase,
                Some(&note.id),
                &warnings,
                None,
            )?
        };
        if cancelled {
            send(&channel, NotePipelineProgress::Cancelled { run: completed });
        } else {
            send(
                &channel,
                NotePipelineProgress::Done {
                    run: completed,
                    degraded: false,
                },
            );
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        persist_error(&state, &run_id, &channel, error).await;
    }
    state.finish_note_pipeline_run(&run_id).await;
}

async fn spawn_analysis(
    app: &AppHandle,
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

async fn spawn_drafting(
    app: &AppHandle,
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

pub async fn start(
    app: &AppHandle,
    request: NotePipelineStartRequest,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let conversation = state
        .conversation_repository
        .load(request.conversation_id.trim())?;
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
                1
            },
        )
    };
    let run = {
        let _guard = state.library_operations.lock().await;
        state
            .library_repository
            .create_note_pipeline_run(NotePipelineRunCreate {
                id: Uuid::new_v4().to_string(),
                conversation_id: conversation.id,
                provider_id,
                model_id,
                max_output_tokens,
                thinking_enabled,
                retry_attempts,
            })?
    };
    spawn_analysis(app, run.id.clone(), String::new(), channel).await?;
    Ok(run)
}

pub async fn adjust(
    app: &AppHandle,
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

pub async fn confirm(
    app: &AppHandle,
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
    spawn_drafting(app, run.id.clone(), channel).await?;
    Ok(run)
}

pub async fn resume(
    app: &AppHandle,
    run_id: String,
    channel: Channel<NotePipelineProgress>,
) -> Result<NotePipelineRun, String> {
    let state = app.state::<AppState>();
    let run = state.library_repository.get_note_pipeline_run(&run_id)?;
    match run.phase {
        NotePipelinePhase::AwaitingOutline => {
            send(
                &channel,
                NotePipelineProgress::OutlineReady { run: run.clone() },
            );
        }
        NotePipelinePhase::Analyzing => {
            spawn_analysis(app, run.id.clone(), String::new(), channel).await?;
        }
        NotePipelinePhase::Drafting
        | NotePipelinePhase::Assembling
        | NotePipelinePhase::Persisting
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
    Ok(run)
}

pub async fn cancel(app: &AppHandle, run_id: &str) -> Result<bool, String> {
    let state = app.state::<AppState>();
    if state.cancel_note_pipeline_run(run_id).await {
        return Ok(true);
    }
    let run = state.library_repository.get_note_pipeline_run(run_id)?;
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

pub fn list_resumable(state: &AppState) -> Result<Vec<NotePipelineRun>, String> {
    state.library_repository.list_resumable_note_pipeline_runs()
}

pub fn get_run(state: &AppState, run_id: &str) -> Result<NotePipelineRun, String> {
    state.library_repository.get_note_pipeline_run(run_id)
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
                1
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
        completed_section_ids: Vec::new(),
        failed_section_ids: Vec::new(),
        warnings: Vec::new(),
        error_message: None,
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
