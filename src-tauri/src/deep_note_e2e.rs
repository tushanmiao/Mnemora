//! 无窗口的真实深度笔记全链路测试入口。
//!
//! 该模块只在 `deep-note-e2e` 特性下编译。它复用用户现有的非敏感模型配置和
//! Windows Credential Manager 中的 API Key，但把会话、笔记与日志写入隔离目录。

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use tauri::{ipc::Channel, Manager};
use uuid::Uuid;

use crate::{
    ai::types::ModelRole,
    chat::{
        conversation_types::{
            AiPermissionMode, MessageStatus, StoredChatMessage, StoredConversation,
        },
        note_pipeline::{
            self, NotePipelineConfirmRequest, NotePipelineProgress, NotePipelineStartRequest,
        },
    },
    library::types::{NotePipelinePhase, NotePipelineSectionStatus},
    state::AppState,
};

const DEFAULT_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(45 * 60);
const DEFAULT_DRAFTING_TIMEOUT: Duration = Duration::from_secs(120 * 60);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

struct Options {
    input: PathBuf,
    config_source: PathBuf,
    resource_dir: PathBuf,
    output_dir: PathBuf,
    report_path: Option<PathBuf>,
    provider_id: Option<String>,
    model_id: Option<String>,
    max_messages: Option<usize>,
    analysis_timeout: Duration,
    drafting_timeout: Duration,
}

struct TestResult {
    mock_model: bool,
    markdown_path: PathBuf,
    isolation_dir: PathBuf,
    conversation_id: String,
    message_count: usize,
    source_bytes: u64,
    run_id: String,
    provider_id: String,
    model_id: String,
    note_id: String,
    note_title: String,
    note_chars: usize,
    source_count: usize,
    outline_sections: usize,
    completed_sections: usize,
    failed_sections: usize,
    source_chunk_count: usize,
    estimated_input_tokens: u64,
    processed_chunks: usize,
    processed_messages: usize,
    total_messages: usize,
    coverage_complete: bool,
    omitted_messages: usize,
    semantic_calls_used: u32,
    semantic_call_limit: u32,
    retries: u32,
    revisions: u32,
    analysis_elapsed: Duration,
    drafting_elapsed: Duration,
    total_elapsed: Duration,
    event_counts: BTreeMap<String, usize>,
    channel_event_count: usize,
    warnings: Vec<String>,
}

pub fn run_cli() -> Result<(), String> {
    let options = parse_options()?;
    let report_path = options.report_path.clone();
    let started = Instant::now();
    match tauri::async_runtime::block_on(run(options)) {
        Ok(result) => {
            let report = render_report(&result);
            if let Some(path) = report_path {
                write_report(&path, &report)?;
                println!("测试报告：{}", path.display());
            }
            println!("{report}");
            println!(
                "深度笔记真实 E2E 已通过，总耗时 {:.1} 秒。",
                started.elapsed().as_secs_f64()
            );
            Ok(())
        }
        Err(error) => {
            if let Some(path) = report_path {
                let report = format!(
                    "# 深度笔记真实长对话全链路测试报告\n\n- 结果：失败\n- 错误：{}\n- 总耗时：{:.1} 秒\n",
                    escape_markdown(&error),
                    started.elapsed().as_secs_f64()
                );
                write_report(&path, &report)?;
                eprintln!("失败报告：{}", path.display());
            }
            Err(error)
        }
    }
}

async fn run(options: Options) -> Result<TestResult, String> {
    validate_options(&options)?;
    prepare_isolation(&options)?;

    let markdown = fs::read_to_string(&options.input)
        .map_err(|error| format!("读取测试 Markdown 失败：{error}"))?;
    let source_bytes = fs::metadata(&options.input)
        .map_err(|error| format!("读取 Markdown 元数据失败：{error}"))?
        .len();
    let conversation = parse_conversation(&markdown, &options.input, options.max_messages)?;
    let conversation_id = conversation.id.clone();
    let message_count = conversation.messages.len();

    let config_dir = options.output_dir.join("config");
    let data_dir = options.output_dir.join("data");
    let log_dir = options.output_dir.join("logs");
    let storage = crate::storage::StorageManager::bootstrap(config_dir.clone(), data_dir.clone())?;
    let state = AppState::new(
        config_dir,
        data_dir.clone(),
        options.resource_dir.clone(),
        log_dir,
        storage,
    )?;
    state.conversation_repository.save(&conversation)?;

    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .map_err(|error| format!("创建无窗口 Tauri 测试运行时失败：{error}"))?;
    let app_handle = app.handle().clone();
    let channel_events = Arc::new(StdMutex::new(Vec::<Value>::new()));
    let captured = Arc::clone(&channel_events);
    let channel = Channel::<NotePipelineProgress>::new(move |body| {
        if let Ok(value) = body.deserialize::<Value>() {
            print_progress(&value);
            if let Ok(mut events) = captured.lock() {
                events.push(value);
            }
        }
        Ok(())
    });

    let total_started = Instant::now();
    let analysis_started = Instant::now();
    let started_run = note_pipeline::start(
        &app_handle,
        NotePipelineStartRequest {
            conversation_id: conversation_id.clone(),
            replace_invalidated: false,
            force_rebuild: false,
        },
        channel.clone(),
    )
    .await?;
    println!(
        "任务已创建：{}，模型 {}/{}",
        started_run.id, started_run.provider_id, started_run.model_id
    );

    let awaiting = wait_for_phase(
        &app_handle,
        &started_run.id,
        options.analysis_timeout,
        &[NotePipelinePhase::AwaitingOutline],
        "提纲生成",
    )
    .await?;
    let analysis_elapsed = analysis_started.elapsed();
    let outline_detail = {
        let state = app_handle.state::<AppState>();
        note_pipeline::get_detail(&state, &awaiting.id)?
    };
    let selected_section_ids = outline_detail
        .plan_version
        .as_ref()
        .ok_or_else(|| "任务进入提纲确认阶段，但缺少已编译计划。".to_string())?
        .plan
        .sections
        .iter()
        .map(|section| section.id.clone())
        .collect::<Vec<_>>();
    if selected_section_ids.is_empty() {
        return Err("模型生成的提纲没有可执行章节。".to_string());
    }
    println!(
        "提纲已生成：{} 个章节，分析耗时 {:.1} 秒；正在确认全部章节。",
        selected_section_ids.len(),
        analysis_elapsed.as_secs_f64()
    );

    let drafting_started = Instant::now();
    note_pipeline::confirm(
        &app_handle,
        NotePipelineConfirmRequest {
            run_id: started_run.id.clone(),
            selected_section_ids,
        },
        channel,
    )
    .await?;
    let completed = wait_for_phase(
        &app_handle,
        &started_run.id,
        options.drafting_timeout,
        &[NotePipelinePhase::Done],
        "章节生成与持久化",
    )
    .await?;
    let drafting_elapsed = drafting_started.elapsed();

    let state = app_handle.state::<AppState>();
    let detail = note_pipeline::get_detail(&state, &started_run.id)?;
    let note_id = completed
        .note_id
        .clone()
        .ok_or_else(|| "任务已完成，但没有关联持久化笔记 ID。".to_string())?;
    let note = state.library_repository.get_note(&note_id)?;
    let sources = state.library_repository.list_note_sources(&note_id)?;
    let sqlite_path = data_dir.join("library").join("library.sqlite3");
    if !sqlite_path.is_file() {
        return Err(format!("隔离 SQLite 不存在：{}", sqlite_path.display()));
    }
    if note.content.trim().is_empty() {
        return Err("持久化笔记正文为空。".to_string());
    }
    if sources.is_empty() {
        return Err("持久化笔记没有章节来源记录。".to_string());
    }
    if detail.source_chunks.is_empty() || detail.evidence.is_empty() {
        return Err("完整链路没有返回持久化 Source Chunk 或 Evidence。".to_string());
    }
    if detail.nodes.iter().any(|node| {
        node.section_id.is_some()
            && matches!(
                node.node_type,
                crate::chat::note_pipeline::DeepNoteNodeType::ExtractEvidence
                    | crate::chat::note_pipeline::DeepNoteNodeType::DraftSection
                    | crate::chat::note_pipeline::DeepNoteNodeType::ValidateSection
            )
            && node.evidence_ids.is_empty()
    }) {
        return Err(
            "至少一个 Evidence/Draft/Validate DAG 节点没有持久化 Evidence ID。".to_string(),
        );
    }
    let sidecar = serde_json::from_str::<serde_json::Value>(&detail.sidecar_json)
        .map_err(|error| format!("深度笔记 Sidecar 不是有效 JSON：{error}"))?;
    let sidecar_sections = sidecar
        .get("sections")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "深度笔记 Sidecar 缺少 sections。".to_string())?;
    if sidecar_sections.iter().any(|section| {
        section
            .get("evidenceIds")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty)
    }) {
        return Err("至少一个 Sidecar 章节没有记录 Evidence ID。".to_string());
    }

    let mut event_counts = BTreeMap::new();
    for event in &detail.events {
        *event_counts.entry(event.event_type.clone()).or_insert(0) += 1;
    }
    for required in [
        "contextCoverageCompleted",
        "modelCallCompleted",
        "outlineReady",
        "skillProfileLoaded",
        "skillApplied",
        "sourceChunkCreated",
        "evidenceCreated",
        "dagNodeCompleted",
        "runCompleted",
    ] {
        if event_counts.get(required).copied().unwrap_or_default() == 0 {
            return Err(format!("完整链路缺少持久化事件：{required}"));
        }
    }
    if !detail.context_budget.coverage_complete {
        return Err("输入覆盖没有完成。".to_string());
    }
    if !detail.context_budget.omitted_message_ids.is_empty() {
        return Err(format!(
            "输入覆盖遗漏了 {} 条消息。",
            detail.context_budget.omitted_message_ids.len()
        ));
    }
    if detail
        .sections
        .iter()
        .any(|section| section.status != NotePipelineSectionStatus::Completed)
    {
        return Err("至少一个已选章节未进入 completed 状态。".to_string());
    }

    let retries = detail
        .sections
        .iter()
        .map(|section| u32::from(section.attempt_count.saturating_sub(1)))
        .sum();
    let revisions = detail
        .sections
        .iter()
        .map(|section| u32::from(section.revision_count))
        .sum();
    let channel_event_count = channel_events
        .lock()
        .map(|events| events.len())
        .unwrap_or(0);
    let completed_sections = detail
        .sections
        .iter()
        .filter(|section| section.status == NotePipelineSectionStatus::Completed)
        .count();
    let failed_sections = detail
        .sections
        .iter()
        .filter(|section| section.status == NotePipelineSectionStatus::Failed)
        .count();
    let outline_sections = detail
        .plan_version
        .as_ref()
        .map(|plan| plan.plan.sections.len())
        .unwrap_or_default();

    Ok(TestResult {
        mock_model: env::var("MNEMORA_DEEP_NOTE_MOCK").ok().as_deref() == Some("1"),
        markdown_path: options.input,
        isolation_dir: options.output_dir,
        conversation_id,
        message_count,
        source_bytes,
        run_id: completed.id,
        provider_id: completed.provider_id,
        model_id: completed.model_id,
        note_id,
        note_title: note.title,
        note_chars: note.content.chars().count(),
        source_count: sources.len(),
        outline_sections,
        completed_sections,
        failed_sections,
        source_chunk_count: detail.source_chunk_count,
        estimated_input_tokens: detail.context_budget.estimated_input_tokens,
        processed_chunks: detail.context_budget.processed_chunk_count,
        processed_messages: detail.context_budget.processed_message_count,
        total_messages: detail.context_budget.total_message_count,
        coverage_complete: detail.context_budget.coverage_complete,
        omitted_messages: detail.context_budget.omitted_message_ids.len(),
        semantic_calls_used: detail.budget.semantic_calls_used,
        semantic_call_limit: detail.budget.semantic_call_limit,
        retries,
        revisions,
        analysis_elapsed,
        drafting_elapsed,
        total_elapsed: total_started.elapsed(),
        event_counts,
        channel_event_count,
        warnings: completed.warnings,
    })
}

async fn wait_for_phase<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    run_id: &str,
    timeout: Duration,
    expected: &[NotePipelinePhase],
    label: &str,
) -> Result<crate::library::types::NotePipelineRun, String> {
    let started = Instant::now();
    let mut last_phase = None;
    loop {
        let run = {
            let state = app.state::<AppState>();
            state.library_repository.get_note_pipeline_run(run_id)?
        };
        if last_phase != Some(run.phase) {
            println!("{label}阶段：{}", run.phase.as_str());
            last_phase = Some(run.phase);
        }
        if expected.contains(&run.phase) {
            return Ok(run);
        }
        if matches!(
            run.phase,
            NotePipelinePhase::Error
                | NotePipelinePhase::Blocked
                | NotePipelinePhase::Cancelled
                | NotePipelinePhase::Cancelling
                | NotePipelinePhase::Paused
        ) {
            return Err(format!(
                "{label}在阶段 {} 终止：{}",
                run.phase.as_str(),
                run.error_message.as_deref().unwrap_or("没有错误详情")
            ));
        }
        if started.elapsed() >= timeout {
            let _ = note_pipeline::cancel(app, run_id).await;
            return Err(format!(
                "{label}超过 {:.0} 分钟仍未完成，最后阶段为 {}。",
                timeout.as_secs_f64() / 60.0,
                run.phase.as_str()
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn parse_conversation(
    markdown: &str,
    path: &Path,
    max_messages: Option<usize>,
) -> Result<StoredConversation, String> {
    let title = markdown
        .lines()
        .find_map(|line| line.trim().strip_prefix("# "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            path.file_stem()
                .map(|value| value.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "深度笔记 E2E 对话".to_string());
    let conversation_id = format!("e2e-{}", Uuid::new_v4());
    let now = now_millis();
    let mut parsed = Vec::<(ModelRole, String)>::new();
    let mut role: Option<ModelRole> = None;
    let mut buffer = Vec::<&str>::new();
    for line in markdown.lines() {
        let next_role = match line.trim() {
            "## 用户" => Some(ModelRole::User),
            "## 助手" => Some(ModelRole::Assistant),
            _ => None,
        };
        if let Some(next_role) = next_role {
            if let Some(current_role) = role.take() {
                let content = buffer.join("\n").trim().to_string();
                if !content.is_empty() {
                    parsed.push((current_role, content));
                }
            }
            role = Some(next_role);
            buffer.clear();
        } else if role.is_some() {
            buffer.push(line);
        }
    }
    if let Some(current_role) = role {
        let content = buffer.join("\n").trim().to_string();
        if !content.is_empty() {
            parsed.push((current_role, content));
        }
    }
    if let Some(limit) = max_messages {
        parsed.truncate(limit.max(1));
    }
    if parsed.is_empty() {
        return Err("Markdown 中没有找到精确的 `## 用户` / `## 助手` 对话段。".to_string());
    }
    let messages = parsed
        .into_iter()
        .enumerate()
        .map(|(index, (role, content))| StoredChatMessage {
            id: format!("message-{}-{}", index + 1, Uuid::new_v4()),
            conversation_id: conversation_id.clone(),
            role,
            content,
            attachments: Vec::new(),
            literature_references: Vec::new(),
            note_references: Vec::new(),
            reasoning: None,
            status: MessageStatus::Completed,
            created_at: now.saturating_add(index as u64),
            updated_at: now.saturating_add(index as u64),
            model_id: None,
            model_snapshot: None,
            usage: None,
            activated_skills: Vec::new(),
            tool_traces: Vec::new(),
            agent_events: Some(Vec::new()),
            agent_run_id: None,
            workflow_summary: None,
            error_message: None,
        })
        .collect::<Vec<_>>();
    let conversation = StoredConversation {
        id: conversation_id,
        title,
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
        created_at: now,
        updated_at: now.saturating_add(1),
    };
    conversation.validate()?;
    Ok(conversation)
}

fn parse_options() -> Result<Options, String> {
    let mut args = env::args().skip(1);
    let input = args.next().map(PathBuf::from).ok_or_else(|| {
        "用法：deep-note-e2e <对话.md> [--config-source DIR] [--resource-dir DIR] [--output-dir DIR] [--report FILE] [--analysis-timeout-minutes N] [--drafting-timeout-minutes N]".to_string()
    })?;
    let app_data = env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "没有找到 Windows APPDATA 目录。".to_string())?;
    let unique = format!(
        "{}-{}",
        now_millis(),
        Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>()
    );
    let mut options = Options {
        input,
        config_source: app_data.join("com.mnemora.app"),
        resource_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"),
        output_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("deep-note-e2e")
            .join(unique),
        report_path: None,
        provider_id: None,
        model_id: None,
        max_messages: None,
        analysis_timeout: DEFAULT_ANALYSIS_TIMEOUT,
        drafting_timeout: DEFAULT_DRAFTING_TIMEOUT,
    };
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("参数 {flag} 缺少值。"))?;
        match flag.as_str() {
            "--config-source" => options.config_source = PathBuf::from(value),
            "--resource-dir" => options.resource_dir = PathBuf::from(value),
            "--output-dir" => options.output_dir = PathBuf::from(value),
            "--report" => options.report_path = Some(PathBuf::from(value)),
            "--provider-id" => options.provider_id = Some(value),
            "--model-id" => options.model_id = Some(value),
            "--max-messages" => {
                options.max_messages = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| "--max-messages 必须是正整数。".to_string())?,
                )
            }
            "--analysis-timeout-minutes" => {
                options.analysis_timeout = parse_minutes(&flag, &value)?
            }
            "--drafting-timeout-minutes" => {
                options.drafting_timeout = parse_minutes(&flag, &value)?
            }
            _ => return Err(format!("未知参数：{flag}")),
        }
    }
    Ok(options)
}

fn parse_minutes(flag: &str, value: &str) -> Result<Duration, String> {
    let minutes = value
        .parse::<u64>()
        .map_err(|_| format!("参数 {flag} 必须是正整数分钟数。"))?;
    if minutes == 0 || minutes > 24 * 60 {
        return Err(format!("参数 {flag} 必须在 1 到 1440 之间。"));
    }
    Ok(Duration::from_secs(minutes * 60))
}

fn validate_options(options: &Options) -> Result<(), String> {
    if !options.input.is_file() {
        return Err(format!("测试 Markdown 不存在：{}", options.input.display()));
    }
    for name in ["app-settings.json", "model-settings.json"] {
        let path = options.config_source.join(name);
        if !path.is_file() {
            return Err(format!("模型配置文件不存在：{}", path.display()));
        }
    }
    if !options.resource_dir.is_dir() {
        return Err(format!(
            "资源目录不存在：{}",
            options.resource_dir.display()
        ));
    }
    if options.output_dir.exists() {
        return Err(format!(
            "隔离输出目录已经存在，为避免覆盖数据已停止：{}",
            options.output_dir.display()
        ));
    }
    if options.provider_id.is_some() != options.model_id.is_some() {
        return Err("--provider-id 与 --model-id 必须同时提供。".to_string());
    }
    Ok(())
}

fn prepare_isolation(options: &Options) -> Result<(), String> {
    let config_dir = options.output_dir.join("config");
    fs::create_dir_all(&config_dir).map_err(|error| format!("创建隔离配置目录失败：{error}"))?;
    fs::create_dir_all(options.output_dir.join("data"))
        .map_err(|error| format!("创建隔离数据目录失败：{error}"))?;
    fs::create_dir_all(options.output_dir.join("logs"))
        .map_err(|error| format!("创建隔离日志目录失败：{error}"))?;
    for name in ["app-settings.json", "model-settings.json"] {
        fs::copy(options.config_source.join(name), config_dir.join(name))
            .map_err(|error| format!("复制隔离配置 {name} 失败：{error}"))?;
    }
    if let (Some(provider_id), Some(model_id)) = (&options.provider_id, &options.model_id) {
        let settings_path = config_dir.join("model-settings.json");
        let raw = fs::read_to_string(&settings_path)
            .map_err(|error| format!("读取隔离模型配置失败：{error}"))?;
        let mut settings = serde_json::from_str::<Value>(&raw)
            .map_err(|error| format!("解析隔离模型配置失败：{error}"))?;
        let root = settings
            .as_object_mut()
            .ok_or_else(|| "隔离模型配置的根节点不是对象。".to_string())?;
        root.insert(
            "noteProviderId".to_string(),
            Value::String(provider_id.clone()),
        );
        root.insert("noteModelId".to_string(), Value::String(model_id.clone()));
        let serialized = serde_json::to_string_pretty(&settings)
            .map_err(|error| format!("序列化隔离模型配置失败：{error}"))?;
        fs::write(&settings_path, serialized)
            .map_err(|error| format!("写入隔离模型配置失败：{error}"))?;
    }
    Ok(())
}

fn print_progress(value: &Value) {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let message = value.get("message").and_then(Value::as_str);
    let current = value.get("current").and_then(Value::as_u64);
    let total = value.get("total").and_then(Value::as_u64);
    match (message, current, total) {
        (Some(message), Some(current), Some(total)) => {
            println!("[{event_type}] {message} ({current}/{total})")
        }
        (Some(message), _, _) => println!("[{event_type}] {message}"),
        _ => println!("[{event_type}]"),
    }
}

fn render_report(result: &TestResult) -> String {
    let event_lines = result
        .event_counts
        .iter()
        .map(|(event, count)| format!("| `{event}` | {count} |"))
        .collect::<Vec<_>>()
        .join("\n");
    let warnings = if result.warnings.is_empty() {
        "- 无".to_string()
    } else {
        result
            .warnings
            .iter()
            .map(|warning| format!("- {}", escape_markdown(warning)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "# 深度笔记真实长对话全链路测试报告\n\n\
         ## 结论\n\n\
         - 结果：通过\n\
         - 输入：`{}`\n\
         - 隔离目录：`{}`\n\
         - 会话 ID：`{}`\n\
         - Run ID：`{}`\n\
         - Note ID：`{}`\n\n\
         ## 输入与模型\n\n\
         | 项目 | 结果 |\n| --- | ---: |\n\
         | Markdown 字节数 | {} |\n\
         | 解析消息数 | {} |\n\
         | Provider ID | `{}` |\n\
         | Model ID | `{}` |\n\
         | 模型执行模式 | {} |\n\
         | 估算输入 Token | {} |\n\n\
         ## 上下文覆盖\n\n\
         | 项目 | 结果 |\n| --- | ---: |\n\
         | 来源分块数 | {} |\n\
         | 已处理分块数 | {} |\n\
         | 已处理消息 / 总消息 | {} / {} |\n\
         | 覆盖完成 | {} |\n\
         | 遗漏消息数 | {} |\n\n\
         ## 执行与持久化\n\n\
         | 项目 | 结果 |\n| --- | ---: |\n\
         | 提纲章节数 | {} |\n\
         | 完成章节数 | {} |\n\
         | 失败章节数 | {} |\n\
         | 节点额外重试数 | {} |\n\
         | 语义修订数 | {} |\n\
         | 语义调用 | {} / {} |\n\
         | 笔记标题 | {} |\n\
         | 笔记正文字符数 | {} |\n\
         | 来源记录数 | {} |\n\
         | Channel 事件数 | {} |\n\n\
         ## 耗时\n\n\
         | 阶段 | 秒 |\n| --- | ---: |\n\
         | 分析与提纲 | {:.1} |\n\
         | 章节生成与持久化 | {:.1} |\n\
         | 全链路 | {:.1} |\n\n\
         ## 持久化事件\n\n\
         | 事件 | 次数 |\n| --- | ---: |\n{}\n\n\
         ## 警告或降级\n\n{}\n",
        result.markdown_path.display(),
        result.isolation_dir.display(),
        result.conversation_id,
        result.run_id,
        result.note_id,
        result.source_bytes,
        result.message_count,
        result.provider_id,
        result.model_id,
        if result.mock_model {
            "隔离模拟模型"
        } else {
            "真实 Provider"
        },
        result.estimated_input_tokens,
        result.source_chunk_count,
        result.processed_chunks,
        result.processed_messages,
        result.total_messages,
        if result.coverage_complete {
            "是"
        } else {
            "否"
        },
        result.omitted_messages,
        result.outline_sections,
        result.completed_sections,
        result.failed_sections,
        result.retries,
        result.revisions,
        result.semantic_calls_used,
        result.semantic_call_limit,
        escape_markdown(&result.note_title),
        result.note_chars,
        result.source_count,
        result.channel_event_count,
        result.analysis_elapsed.as_secs_f64(),
        result.drafting_elapsed.as_secs_f64(),
        result.total_elapsed.as_secs_f64(),
        event_lines,
        warnings,
    )
}

fn write_report(path: &Path, report: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建报告目录失败：{error}"))?;
    }
    fs::write(path, report).map_err(|error| format!("写入测试报告失败：{error}"))
}

fn escape_markdown(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
