//! Chat 请求服务。
//!
//! 非流式和流式调用共享目标解析与系统凭据读取。流式调用额外负责运行注册、Channel 事件
//! 和真实网络取消；设置锁不会跨网络请求持有，活动运行结束后一定从注册表移除。

use std::{
    future::Future,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::{
    ai::{
        concurrency::ProviderRequestClass,
        dispatcher,
        error::{ModelError, ModelErrorKind},
        stream,
        types::{
            ModelMessage, ModelRequest, ModelResponse, ModelRole, ModelStreamChunk,
            ModelStreamOutcome, ModelToolCall, ModelToolResult, ModelUsage, ProviderRequestContext,
        },
    },
    chat::agent::{
        self,
        run_machine::{AgentRunEvent, ToolCallEvent},
        SkillRunCache, ToolInterruptKind, ToolQuestion, ToolQuestionAnswer, ToolRisk,
        ToolRuntimeContext, ToolTraceSnapshot, ToolTraceStatus,
    },
    request_debug::{self, RequestDebugRecordInput, RequestDebugRequest, RequestDebugResponse},
    settings::types::{ApiProtocol, AuthScheme, ModelPricing, ModelSettings, ProviderKind},
    state::{AppState, PendingToolApproval, ToolInterruptResponse},
    usage::{self, UsageRecordInput},
};

use super::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatStreamRequest, ChatWorkspaceContext,
    ModelStreamEvent,
};

struct ResolvedTarget {
    provider_id: String,
    provider_name: String,
    model_id: String,
    provider_kind: ProviderKind,
    protocol: ApiProtocol,
    auth_scheme: AuthScheme,
    base_url: String,
    api_model: String,
    display_name: String,
    context_window_tokens: Option<u64>,
    pricing: Option<ModelPricing>,
    /// 是否支持图片输入：用户覆盖优先，其次内置模型数据库；`None` 表示未知（放行）。
    supports_vision: Option<bool>,
    /// 是否支持结构化 Tool Calling；未知采用保守 false。
    supports_tools: bool,
    /// 是否支持独立 reasoning/thinking；未知不强制关闭普通生成。
    supports_reasoning: Option<bool>,
}

struct PreparedCall {
    target: ResolvedTarget,
    request: ModelRequest,
    api_key: Zeroizing<String>,
    tool_context: ToolRuntimeContext,
}

struct AgentCompleteResult {
    response: ModelResponse,
    activated_skill_ids: Vec<String>,
    tool_traces: Vec<ToolTraceSnapshot>,
}

#[derive(Clone)]
struct ParallelToolExecution {
    execution: agent::ToolExecution,
    duration_ms: u64,
}

#[derive(Default)]
struct UsageCallMetadata {
    run_id: Option<String>,
    round_index: Option<u32>,
    call_index: Option<u32>,
    parent_operation: Option<String>,
    activated_skill_ids: Vec<String>,
    tool_definition_count: u32,
    tool_call_count: u32,
}

#[derive(Clone, Copy)]
struct RetryPolicy {
    max_retries: u8,
}

#[derive(Debug, Clone)]
pub enum CompletionProgress {
    AttemptStarted {
        retry_index: u8,
        max_retries: u8,
    },
    RetryScheduled {
        retry_index: u8,
        max_retries: u8,
        delay_ms: u64,
        error: ModelError,
    },
    /// 流式保活尝试失败，本次调用已回落非流式。携带触发回落的错误。
    StreamKeepaliveFellBack {
        error: ModelError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionTransport {
    Streaming,
    NonStreaming,
}

impl CompletionTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::NonStreaming => "nonStreaming",
        }
    }
}

/// 一次真正要发给 provider 的物理请求。
///
/// `retry_index` 统计同载荷重试，`request_index` 则贯穿整个逻辑调用。流式失败后回落
/// 非流式时，两者的 retry_index 相同、request_index 不同；预算必须按后者扣减。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpstreamRequestAttempt {
    pub retry_index: u8,
    pub max_retries: u8,
    pub request_index: u32,
    pub transport: CompletionTransport,
    pub request_bytes: usize,
}

pub struct CompleteExecution<'a> {
    pub cancellation: &'a CancellationToken,
    pub max_retries: Option<u8>,
    pub attempt_timeout: Option<Duration>,
    pub retry_predicate: Option<&'a (dyn Fn(&ModelError) -> bool + Send + Sync)>,
    pub on_progress: Option<&'a (dyn Fn(CompletionProgress) + Send + Sync)>,
    /// 物理 HTTP 请求发出前的同步闸门。返回错误会阻止该请求发出。
    ///
    /// DeepNote 用它在 SQLite 的 IMMEDIATE 事务里原子扣减上游请求预算；普通重试和
    /// 流式回落都会经过这里，所以不会再把一次逻辑调用误记成一次上游请求。
    pub before_upstream_request:
        Option<&'a (dyn Fn(UpstreamRequestAttempt) -> Result<(), ModelError> + Send + Sync)>,
    /// 是否用流式请求换取非流式结果，目的是保活（见 `dispatcher::complete_via_stream`）。
    ///
    /// 只影响传输方式，不影响返回给调用方的形态。首次尝试若因流式相关原因失败，
    /// 本次调用会自动回落非流式，并通过 `on_progress` 报一次
    /// `CompletionProgress::StreamKeepaliveFellBack`。
    pub prefer_streaming: bool,
    /// 请求体字节数硬闸。`None` 表示不设闸。
    ///
    /// 中转站限制的是 body 字节数而非 token，且**不可协商** —— 超限的请求发出去只会
    /// 被网关拒绝或截断，白等一个 attempt 超时。所以这里在发出之前就拦下来，并以
    /// `ContextLengthExceeded` 上报，让调用方走既有的缩小载荷通路
    /// （深度笔记的 `should_fallback_to_chunked_planner` 已经认这个 kind）。
    pub max_request_bytes: Option<usize>,
}

impl CompleteExecution<'_> {
    /// 非流式、不重试、不限时的默认执行策略。
    pub fn plain(cancellation: &CancellationToken) -> CompleteExecution<'_> {
        CompleteExecution {
            cancellation,
            max_retries: None,
            attempt_timeout: None,
            retry_predicate: None,
            on_progress: None,
            before_upstream_request: None,
            prefer_streaming: false,
            max_request_bytes: None,
        }
    }
}

pub async fn complete(
    state: &AppState,
    request: ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ModelError> {
    let cancellation = CancellationToken::new();
    let execution = CompleteExecution::plain(&cancellation);
    complete_with_execution(state, request, &execution).await
}

pub async fn complete_with_execution(
    state: &AppState,
    request: ChatCompletionRequest,
    execution: &CompleteExecution<'_>,
) -> Result<ChatCompletionResponse, ModelError> {
    request.validate()?;
    let conversation_id = request.conversation_id.clone();
    let message_id = request.message_id.clone();
    let operation = request
        .operation
        .clone()
        .unwrap_or_else(|| "chatComplete".to_string());
    let prepared = prepare_call(state, request, false).await?;
    let context = ProviderRequestContext {
        protocol: prepared.target.protocol,
        auth_scheme: prepared.target.auth_scheme,
        base_url: &prepared.target.base_url,
        api_key: prepared.api_key.trim(),
    };
    let debug_request = request_debug::is_enabled(state)
        .then(|| request_debug::build_request(&context, &prepared.request, false))
        .and_then(Result::ok);
    let created_at_ms = usage::now_ms();
    let started_at = Instant::now();
    let result = run_agent_complete(
        state,
        &context,
        prepared.request.clone(),
        &prepared.target,
        &prepared.tool_context,
        conversation_id.as_deref(),
        message_id.as_deref(),
        &operation,
        execution,
    )
    .await;
    let duration_ms = elapsed_ms(started_at);

    match result {
        Ok(result) => {
            if let Some(debug_request) = debug_request {
                record_debug(
                    state,
                    &prepared.target,
                    created_at_ms,
                    duration_ms,
                    "success",
                    conversation_id,
                    message_id,
                    debug_request,
                    request_debug::success_response(
                        Some(200),
                        &result.response.text,
                        result.response.reasoning.as_deref(),
                        result.response.finish_reason.as_deref(),
                        result.response.usage.as_ref(),
                    ),
                    result.response.usage.clone(),
                );
            }
            Ok(ChatCompletionResponse {
                response: result.response,
                activated_skill_ids: result.activated_skill_ids,
                tool_traces: result.tool_traces,
            })
        }
        Err(error) => {
            if let Some(debug_request) = debug_request {
                record_debug(
                    state,
                    &prepared.target,
                    created_at_ms,
                    duration_ms,
                    "error",
                    conversation_id,
                    message_id,
                    debug_request,
                    request_debug::error_response(&error, ""),
                    None,
                );
            }
            Err(error)
        }
    }
}

pub async fn stream(
    state: &AppState,
    request: ChatStreamRequest,
    on_event: Channel<ModelStreamEvent>,
) -> Result<(), ModelError> {
    request.validate()?;
    let run_id = request.run_id.trim().to_string();
    let conversation_id = request.conversation_id.trim().to_string();
    let message_id = request.message_id.trim().to_string();
    let prepared = prepare_call(state, request.completion, true).await?;
    let context = ProviderRequestContext {
        protocol: prepared.target.protocol,
        auth_scheme: prepared.target.auth_scheme,
        base_url: &prepared.target.base_url,
        api_key: prepared.api_key.trim(),
    };
    let debug_request = request_debug::is_enabled(state)
        .then(|| request_debug::build_request(&context, &prepared.request, true))
        .and_then(Result::ok);
    let cancellation = CancellationToken::new();
    let runtime_instance_id = uuid::Uuid::new_v4().to_string();

    {
        let mut active_runs = state.active_chat_runs.lock().await;
        if active_runs.contains_key(&run_id) {
            return Err(ModelError::invalid_configuration(
                "相同 Run ID 的流式请求已经存在。",
            ));
        }
        active_runs.insert(run_id.clone(), cancellation.clone());
    }

    if let Err(error) = state.library_repository.create_agent_run(
        &run_id,
        &conversation_id,
        &message_id,
        &runtime_instance_id,
        &prepared.target.model_id,
    ) {
        state.active_chat_runs.lock().await.remove(&run_id);
        return Err(ModelError::provider(format!(
            "无法建立 Agent 状态机运行记录：{error}"
        )));
    }

    if let Err(error) = on_event.send(ModelStreamEvent::Started {
        run_id: run_id.clone(),
        conversation_id: conversation_id.clone(),
        message_id: message_id.clone(),
    }) {
        state.active_chat_runs.lock().await.remove(&run_id);
        let _ = state.library_repository.transition_agent_run(
            &run_id,
            AgentRunEvent::CancelRequested,
            Some(&format!("agent-start-channel-failed:{run_id}")),
            r#"{"reason":"startChannelClosed"}"#,
            None,
        );
        let _ = state.library_repository.transition_agent_run(
            &run_id,
            AgentRunEvent::WorkerStopped,
            None,
            r#"{"reason":"workerNotStarted"}"#,
            None,
        );
        return Err(ModelError::provider(format!(
            "无法发送流式开始事件：{error}"
        )));
    }

    let created_at_ms = usage::now_ms();
    let started_at = Instant::now();
    let mut response_preview = String::new();
    let mut reasoning_preview = String::new();
    let mut result = run_agent_stream(
        state,
        &context,
        prepared.request.clone(),
        &prepared.target,
        &prepared.tool_context,
        &cancellation,
        &on_event,
        &run_id,
        &conversation_id,
        &message_id,
        &mut response_preview,
        &mut reasoning_preview,
    )
    .await;
    if cancellation.is_cancelled() && result.is_err() {
        result = Ok(ModelStreamOutcome::Cancelled);
    }
    let persisted_terminal = match &result {
        Ok(ModelStreamOutcome::Completed(_)) => state.library_repository.transition_agent_run(
            &run_id,
            AgentRunEvent::FinalizationCompleted,
            None,
            "{}",
            None,
        ),
        Ok(ModelStreamOutcome::Cancelled) => {
            let _ = state.library_repository.transition_agent_run(
                &run_id,
                AgentRunEvent::CancelRequested,
                Some(&format!("agent-cancel:{run_id}")),
                r#"{"reason":"workerObservedCancellation"}"#,
                None,
            );
            state.library_repository.transition_agent_run(
                &run_id,
                AgentRunEvent::WorkerStopped,
                None,
                r#"{"reason":"cooperativeWorkerExit"}"#,
                None,
            )
        }
        Err(error) => state.library_repository.transition_agent_run(
            &run_id,
            AgentRunEvent::PanicDetected,
            None,
            &serde_json::json!({ "kind": format!("{:?}", error.kind) }).to_string(),
            Some(&error.message),
        ),
    };
    state.active_chat_runs.lock().await.remove(&run_id);
    state.close_tool_approvals_for_run(&run_id).await;
    if let Err(error) = persisted_terminal {
        result = Err(ModelError::provider(format!(
            "Agent 已结束，但持久化终态失败：{error}"
        )));
    }
    let duration_ms = elapsed_ms(started_at);

    let (status, _status_code, usage_value, error_kind, debug_response) = match &result {
        Ok(ModelStreamOutcome::Completed(summary)) => (
            "success",
            Some(200),
            summary.usage.clone(),
            None,
            request_debug::success_response(
                Some(200),
                &response_preview,
                (!reasoning_preview.is_empty()).then_some(reasoning_preview.as_str()),
                summary.finish_reason.as_deref(),
                summary.usage.as_ref(),
            ),
        ),
        Ok(ModelStreamOutcome::Cancelled) => (
            "stopped",
            None,
            None,
            None,
            request_debug::stopped_response(&response_preview),
        ),
        Err(error) => (
            "error",
            error.status_code,
            None,
            Some(format!("{:?}", error.kind)),
            request_debug::error_response(error, &response_preview),
        ),
    };
    let _ = error_kind;
    if let Some(debug_request) = debug_request {
        record_debug(
            state,
            &prepared.target,
            created_at_ms,
            duration_ms,
            status,
            Some(conversation_id.clone()),
            Some(message_id.clone()),
            debug_request,
            debug_response,
            usage_value,
        );
    }

    let terminal_event = match result {
        Ok(ModelStreamOutcome::Completed(summary)) => ModelStreamEvent::Completed {
            run_id,
            conversation_id,
            message_id,
            finish_reason: summary.finish_reason,
            usage: summary.usage,
        },
        Ok(ModelStreamOutcome::Cancelled) => ModelStreamEvent::Stopped {
            run_id,
            conversation_id,
            message_id,
        },
        Err(error) => ModelStreamEvent::Error {
            run_id,
            conversation_id,
            message_id,
            error,
        },
    };
    on_event
        .send(terminal_event)
        .map_err(|error| ModelError::provider(format!("无法发送流式结束事件：{error}")))
}

const DEFAULT_MAX_AGENT_ROUNDS: u16 = 20;
const MAX_TOOL_CALLS_PER_ROUND: usize = 12;
const MAX_TOOL_CALLS_PER_RUN: usize = 100;
const MAX_PARALLEL_SAFE_TOOLS: usize = 12;
const TOOL_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolBatchBudget {
    Execute { next_business_total: usize },
    Finalize,
}

fn is_final_agent_call(call_index: u16, max_agent_rounds: u16) -> bool {
    call_index == max_agent_rounds
}

/// 一次 Agent 运行最多占用的调用槽位数：`max_agent_rounds` 轮业务调用外，再留一
/// 个收尾调用的位置。
///
/// `#[allow(dead_code)]`：只被单测使用。保留是因为它锁定的是 `is_final_agent_call`
/// 的边界语义 —— 「第 max_agent_rounds 次是最后一次业务调用」意味着总槽位是
/// max_agent_rounds + 1，那个 +1 靠这个函数写明，而不是散落在调用点的算术里。
#[allow(dead_code)]
fn agent_call_slots(max_agent_rounds: u16) -> usize {
    usize::from(max_agent_rounds).saturating_add(1)
}

fn evaluate_tool_batch(
    calls: &[ModelToolCall],
    current_business_total: usize,
) -> Result<ToolBatchBudget, ModelError> {
    if calls.len() > MAX_TOOL_CALLS_PER_ROUND {
        return Err(ModelError::invalid_configuration(format!(
            "单轮工具调用不能超过 {MAX_TOOL_CALLS_PER_ROUND} 个。"
        )));
    }
    let business_calls = calls
        .iter()
        .filter(|call| is_business_tool(&call.name))
        .count();
    let next_business_total = current_business_total.saturating_add(business_calls);
    if next_business_total > MAX_TOOL_CALLS_PER_RUN {
        Ok(ToolBatchBudget::Finalize)
    } else {
        Ok(ToolBatchBudget::Execute {
            next_business_total,
        })
    }
}

fn append_agent_runtime_prompt(request: &mut ModelRequest, max_agent_rounds: u16) {
    let prompt = request.system_prompt.get_or_insert_with(String::new);
    prompt.push_str(&format!(
        "\n\n<mnemora_agent_runtime>\n你可以在任务确实需要时使用已披露的 Tool 和 Skill。复杂任务按“规划 -> 执行 -> 观察结果 -> 反思 -> 必要时重规划”的循环推进；每一步都必须以真实工具结果为依据，不要假装已经执行。信息足够时应尽早结束并给出正文，不要为了用工具而用工具。本次最多有 {max_agent_rounds} 个可执行工具的 Agent 轮次，单轮最多 {MAX_TOOL_CALLS_PER_ROUND} 个工具，整次运行最多 {MAX_TOOL_CALLS_PER_RUN} 个业务工具；运行层还会保留一次不含工具的最终汇总调用。\n</mnemora_agent_runtime>"
    ));
}

async fn execute_parallel_safe_tools(
    state: &AppState,
    context: &ToolRuntimeContext,
    run_cache: &SkillRunCache,
    cancellation: &CancellationToken,
    calls: &[ModelToolCall],
    persisted_run_id: Option<&str>,
) -> Vec<Option<ParallelToolExecution>> {
    let mut pending = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            agent::parallel_safe(context, &call.name) && !agent::requires_approval(context, call)
        })
        .map(|(index, call)| (index, call.clone()));
    let mut results = vec![None; calls.len()];
    let mut tasks = tokio::task::JoinSet::new();
    let spawn = |tasks: &mut tokio::task::JoinSet<_>, index: usize, call: ModelToolCall| {
        let context = context.clone();
        let conversations = state.conversation_repository.clone();
        let skills = state.skill_repository.clone();
        let memory = state.memory_repository.clone();
        let library = state.library_repository.clone();
        let library_operations = state.library_operations.clone();
        let mcp = state.mcp_manager.clone();
        let mut skill_cache = run_cache.clone();
        let cancellation = cancellation.clone();
        let persisted_run_id = persisted_run_id.map(str::to_string);
        tasks.spawn(async move {
            let started = Instant::now();
            let persisted_versions = persisted_run_id.as_deref().and_then(|run_id| {
                let arguments_hash = format!(
                    "{:x}",
                    Sha256::digest(call.arguments.to_string().as_bytes())
                );
                let (source_json, catalog_revision) = agent::tool_provenance(&context, &call.name);
                match library.create_agent_tool_call(
                    run_id,
                    &call.id,
                    &call.name,
                    &format!("{:?}", agent::tool_risk(&context, &call)),
                    &arguments_hash,
                    &source_json,
                    &catalog_revision,
                    None,
                    None,
                ) {
                    Ok((_, execution_version, state_version)) => match library
                        .transition_agent_tool_call(
                            run_id,
                            &call.id,
                            ToolCallEvent::Started,
                            execution_version,
                            Some(state_version),
                            None,
                            None,
                        ) {
                        Ok((_, execution_version, state_version)) => {
                            Some((execution_version, state_version))
                        }
                        Err(_) => {
                            let _ = library.transition_agent_tool_call(
                                run_id,
                                &call.id,
                                ToolCallEvent::Cancelled,
                                execution_version,
                                Some(state_version),
                                Some("Tool Call 启动失败。"),
                                Some("stateTransitionFailed"),
                            );
                            None
                        }
                    },
                    Err(_) => None,
                }
            });
            let result = if persisted_run_id.is_some() && persisted_versions.is_none() {
                agent::ToolExecution {
                    content: "Tool Call 状态记录失败，已阻止并行工具执行。".to_string(),
                    preview: "Tool Call 状态记录失败，已阻止并行工具执行。".to_string(),
                    is_error: true,
                    activated_skill_id: None,
                    output_chars: "Tool Call 状态记录失败，已阻止并行工具执行。"
                        .chars()
                        .count(),
                    output_truncated: false,
                }
            } else {
                agent::execute_tool(
                    &call,
                    &context,
                    &conversations,
                    &skills,
                    &memory,
                    &library,
                    &library_operations,
                    &mcp,
                    &mut skill_cache,
                    &cancellation,
                )
                .await
                .unwrap_or_else(|error| agent::ToolExecution {
                    output_chars: error.message.chars().count(),
                    content: error.message.clone(),
                    preview: error.message,
                    is_error: true,
                    activated_skill_id: None,
                    output_truncated: false,
                })
            };
            let result = if let (Some(run_id), Some((execution_version, state_version))) =
                (persisted_run_id.as_deref(), persisted_versions)
            {
                let event = if result.is_error {
                    ToolCallEvent::Failed
                } else {
                    ToolCallEvent::Succeeded
                };
                let error_kind = result.error_kind();
                match library.transition_agent_tool_call(
                    run_id,
                    &call.id,
                    event,
                    execution_version,
                    Some(state_version),
                    Some(&result.preview),
                    error_kind.as_deref(),
                ) {
                    Ok(_) => result,
                    Err(error) => {
                        rejected_tool(&format!("并行 Tool 终态因版本冲突被拒绝：{error}"), &call)
                    }
                }
            } else {
                result
            };
            (index, result, elapsed_ms(started))
        });
    };

    for _ in 0..MAX_PARALLEL_SAFE_TOOLS {
        let Some((index, call)) = pending.next() else {
            break;
        };
        spawn(&mut tasks, index, call);
    }
    while let Some(joined) = tasks.join_next().await {
        if let Ok((index, execution, duration_ms)) = joined {
            results[index] = Some(ParallelToolExecution {
                execution,
                duration_ms,
            });
        }
        if let Some((index, call)) = pending.next() {
            spawn(&mut tasks, index, call);
        }
    }
    results
}

#[allow(clippy::too_many_arguments)]
async fn run_agent_complete(
    state: &AppState,
    context: &ProviderRequestContext<'_>,
    mut request: ModelRequest,
    target: &ResolvedTarget,
    tool_context: &ToolRuntimeContext,
    conversation_id: Option<&str>,
    message_id: Option<&str>,
    parent_operation: &str,
    execution: &CompleteExecution<'_>,
) -> Result<AgentCompleteResult, ModelError> {
    if tool_context.permission_mode
        == crate::chat::conversation_types::AiPermissionMode::AskEveryTime
    {
        request.tools.retain(|tool| {
            matches!(
                tool.name.as_str(),
                "activate_skill"
                    | "inspect_skill"
                    | "search_tools"
                    | "inspect_tool"
                    | "search_skills"
                    | "read_skill_resource"
            )
        });
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    let mut run_usage = ModelUsage {
        call_count: 0,
        ..ModelUsage::default()
    };
    let mut skill_cache = SkillRunCache::with_activated(&tool_context.manual_skill_ids);
    let mut activated_skill_ids = Vec::new();
    let mut tool_traces = Vec::new();
    let max_agent_rounds = agent_round_limit(state);
    let mut tool_call_total = 0usize;
    let mut force_final_answer = false;
    let mut upstream_request_index = 0u32;
    let request_class = if parent_operation.starts_with("deepNote") {
        ProviderRequestClass::Background
    } else {
        ProviderRequestClass::Interactive
    };

    // `max_agent_rounds` counts rounds that may execute tools. The inclusive final slot is
    // deliberately tool-free so a budget boundary still yields a useful answer.
    for call_index in 0..=max_agent_rounds {
        let round_index = u32::from(call_index);
        let final_call = force_final_answer || is_final_agent_call(call_index, max_agent_rounds);
        if final_call && !request.tools.is_empty() {
            request.tools.clear();
            request
                .system_prompt
                .get_or_insert_with(String::new)
                .push_str("\n\nAgent 运行预算已用尽。不要再请求工具，请直接根据已有结果给出最终回答，并明确说明仍缺少的信息。");
        }
        // 逐轮称重：一轮内重试的载荷不变，工具结果进入下一轮后才需要重算。流式与
        // 非流式 body 分别缓存，因为流式回落会在同一个 retry_index 内发出第二个物理
        // 请求，两者都必须使用自己真正会发出的字节数。
        let needs_request_metadata =
            execution.max_request_bytes.is_some() || execution.before_upstream_request.is_some();
        let non_streaming_request_bytes = needs_request_metadata
            .then(|| dispatcher::request_body_bytes_for_transport(context, &request, false))
            .transpose()?;
        let streaming_request_bytes = (needs_request_metadata && execution.prefer_streaming)
            .then(|| dispatcher::request_body_bytes_for_transport(context, &request, true))
            .transpose()?;
        if let Some(limit) = execution.max_request_bytes {
            if let Some(bytes) = non_streaming_request_bytes {
                enforce_request_byte_limit(bytes, limit)?;
            }
            if let Some(bytes) = streaming_request_bytes {
                enforce_request_byte_limit(bytes, limit)?;
            }
        }
        let created_at_ms = usage::now_ms();
        let started_at = Instant::now();
        let retry_policy = RetryPolicy {
            max_retries: execution
                .max_retries
                .unwrap_or_else(|| retry_policy(state).max_retries),
        };
        let mut retry_index = 0;
        // 流式保活是**每次调用**内的一次性降级，不占用重试预算：一旦回落，本次调用
        // 后续的全部尝试都走非流式，避免对同一个不支持流式的上游反复试探。
        let mut streaming = execution.prefer_streaming;
        let result = loop {
            if let Some(callback) = execution.on_progress {
                callback(CompletionProgress::AttemptStarted {
                    retry_index,
                    max_retries: retry_policy.max_retries,
                });
            }
            let attempt = if streaming {
                let request_index = upstream_request_index.saturating_add(1);
                let permit = state
                    .provider_concurrency
                    .acquire(&target.provider_id, request_class, execution.cancellation)
                    .await?;
                if let Some(callback) = execution.before_upstream_request {
                    callback(UpstreamRequestAttempt {
                        retry_index,
                        max_retries: retry_policy.max_retries,
                        request_index,
                        transport: CompletionTransport::Streaming,
                        request_bytes: streaming_request_bytes.unwrap_or(0),
                    })?;
                }
                upstream_request_index = request_index;
                let streamed = completion_attempt(
                    execution.cancellation,
                    execution.attempt_timeout,
                    dispatcher::complete_via_stream(
                        &state.http,
                        context,
                        &request,
                        execution.cancellation,
                    ),
                )
                .await;
                drop(permit);
                match streamed {
                    Err(error) if should_fall_back_from_streaming(&error) => {
                        streaming = false;
                        if let Some(callback) = execution.on_progress {
                            callback(CompletionProgress::StreamKeepaliveFellBack {
                                error: error.clone(),
                            });
                        }
                        let request_index = upstream_request_index.saturating_add(1);
                        let permit = state
                            .provider_concurrency
                            .acquire(&target.provider_id, request_class, execution.cancellation)
                            .await?;
                        if let Some(callback) = execution.before_upstream_request {
                            callback(UpstreamRequestAttempt {
                                retry_index,
                                max_retries: retry_policy.max_retries,
                                request_index,
                                transport: CompletionTransport::NonStreaming,
                                request_bytes: non_streaming_request_bytes.unwrap_or(0),
                            })?;
                        }
                        upstream_request_index = request_index;
                        let result = completion_attempt(
                            execution.cancellation,
                            execution.attempt_timeout,
                            dispatcher::complete(&state.http, context, &request),
                        )
                        .await;
                        drop(permit);
                        result
                    }
                    other => other,
                }
            } else {
                let request_index = upstream_request_index.saturating_add(1);
                let permit = state
                    .provider_concurrency
                    .acquire(&target.provider_id, request_class, execution.cancellation)
                    .await?;
                if let Some(callback) = execution.before_upstream_request {
                    callback(UpstreamRequestAttempt {
                        retry_index,
                        max_retries: retry_policy.max_retries,
                        request_index,
                        transport: CompletionTransport::NonStreaming,
                        request_bytes: non_streaming_request_bytes.unwrap_or(0),
                    })?;
                }
                upstream_request_index = request_index;
                let result = completion_attempt(
                    execution.cancellation,
                    execution.attempt_timeout,
                    dispatcher::complete(&state.http, context, &request),
                )
                .await;
                drop(permit);
                result
            };
            match attempt {
                Ok(response) => break Ok(response),
                Err(error)
                    if retry_index < retry_policy.max_retries
                        && should_retry(&error)
                        && execution
                            .retry_predicate
                            .map_or(true, |predicate| predicate(&error)) =>
                {
                    let delay = retry_delay(&error, retry_index);
                    if let Some(callback) = execution.on_progress {
                        callback(CompletionProgress::RetryScheduled {
                            retry_index: retry_index + 1,
                            max_retries: retry_policy.max_retries,
                            delay_ms: delay.as_millis().min(u128::from(u64::MAX)) as u64,
                            error: error.clone(),
                        });
                    }
                    retry_index += 1;
                    tokio::select! {
                        _ = execution.cancellation.cancelled() => break Err(ModelError::cancelled()),
                        _ = tokio::time::sleep(delay) => {}
                    }
                }
                Err(error) => break Err(error),
            }
        };
        let duration_ms = elapsed_ms(started_at);
        let (status, status_code, mut usage_value, tool_call_count, error_kind) = match &result {
            Ok(response) => (
                "success",
                Some(200),
                response.usage.clone(),
                response.tool_calls.len() as u32,
                None,
            ),
            Err(error) => (
                "error",
                error.status_code,
                None,
                0,
                Some(format!("{:?}", error.kind)),
            ),
        };
        set_overall_duration(&mut usage_value, duration_ms);
        apply_usage_origin(&mut usage_value, target.provider_kind);
        apply_usage_pricing(&mut usage_value, target, created_at_ms);
        record_usage(
            state,
            target,
            created_at_ms,
            duration_ms,
            "agentModelCall",
            status,
            status_code,
            usage_value.clone(),
            conversation_id.map(str::to_string),
            message_id.map(str::to_string),
            error_kind,
            UsageCallMetadata {
                run_id: Some(run_id.clone()),
                round_index: Some(round_index),
                call_index: Some(round_index),
                parent_operation: Some(parent_operation.to_string()),
                activated_skill_ids: activated_skill_ids.clone(),
                tool_definition_count: request.tools.len() as u32,
                tool_call_count,
            },
        )
        .await;

        let mut response = result?;
        if let Some(call_usage) = usage_value.as_ref() {
            crate::usage::normalize::merge_run_usage(&mut run_usage, call_usage);
        }
        if response.tool_calls.is_empty() {
            response.usage = (run_usage.call_count > 0).then_some(run_usage);
            return Ok(AgentCompleteResult {
                response,
                activated_skill_ids,
                tool_traces,
            });
        }
        agent::validate_disclosed_tool_calls(&request, &response.tool_calls)?;
        if final_call {
            return Err(ModelError::invalid_response(
                "最终汇总调用仍返回了工具请求，无法安全继续执行。",
            ));
        }
        match evaluate_tool_batch(&response.tool_calls, tool_call_total)? {
            ToolBatchBudget::Execute {
                next_business_total,
            } => tool_call_total = next_business_total,
            ToolBatchBudget::Finalize => {
                force_final_answer = true;
                continue;
            }
        }
        let tool_calls = response.tool_calls;
        request.messages.push(ModelMessage {
            role: ModelRole::Assistant,
            content: response.text,
            images: Vec::new(),
            tool_calls: tool_calls.clone(),
            tool_result: None,
        });
        let mut parallel_results = execute_parallel_safe_tools(
            state,
            tool_context,
            &skill_cache,
            execution.cancellation,
            &tool_calls,
            None,
        )
        .await;
        for (index, call) in tool_calls.into_iter().enumerate() {
            let result = if let Some(result) = parallel_results[index].take() {
                result
            } else {
                let started = Instant::now();
                let execution = if agent::requires_approval(tool_context, &call) {
                    agent::ToolExecution {
                        content: "当前为非流式请求，无法显示工具审批；本次敏感工具调用已拒绝。"
                            .to_string(),
                        preview: "敏感工具需要在流式模式中由用户确认。".to_string(),
                        is_error: true,
                        activated_skill_id: None,
                        output_chars:
                            "当前为非流式请求，无法显示工具审批；本次敏感工具调用已拒绝。"
                                .chars()
                                .count(),
                        output_truncated: false,
                    }
                } else {
                    agent::execute_tool(
                        &call,
                        tool_context,
                        &state.conversation_repository,
                        &state.skill_repository,
                        &state.memory_repository,
                        &state.library_repository,
                        &state.library_operations,
                        &state.mcp_manager,
                        &mut skill_cache,
                        execution.cancellation,
                    )
                    .await
                    .unwrap_or_else(|error| agent::ToolExecution {
                        output_chars: error.message.chars().count(),
                        content: error.message.clone(),
                        preview: error.message,
                        is_error: true,
                        activated_skill_id: None,
                        output_truncated: false,
                    })
                };
                ParallelToolExecution {
                    execution,
                    duration_ms: elapsed_ms(started),
                }
            };
            let execution = result.execution;
            tool_traces.push(ToolTraceSnapshot {
                call_id: call.id.clone(),
                name: call.name.clone(),
                status: if execution.is_error {
                    ToolTraceStatus::Failed
                } else {
                    ToolTraceStatus::Completed
                },
                risk: agent::tool_risk(tool_context, &call),
                argument_summary: agent::argument_summary(&call),
                preview: Some(execution.preview.clone()),
                duration_ms: Some(result.duration_ms),
                input_chars: Some(call.arguments.to_string().chars().count()),
                output_chars: Some(execution.output_chars),
                output_truncated: Some(execution.output_truncated),
                error_kind: execution.error_kind(),
            });
            if let Some(skill_id) = execution.activated_skill_id.as_ref() {
                if !activated_skill_ids.contains(skill_id) {
                    activated_skill_ids.push(skill_id.clone());
                }
            }
            agent::apply_tool_disclosures(&mut request, &call, &execution, tool_context);
            request.messages.push(ModelMessage {
                role: ModelRole::Tool,
                content: String::new(),
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_result: Some(ModelToolResult {
                    call_id: call.id,
                    name: call.name,
                    content: execution.content,
                    is_error: execution.is_error,
                }),
            });
        }
    }
    Err(ModelError::provider("Agent 未能在运行预算内生成最终回答。"))
}

async fn completion_attempt<F>(
    cancellation: &CancellationToken,
    attempt_timeout: Option<Duration>,
    dispatch: F,
) -> Result<ModelResponse, ModelError>
where
    F: Future<Output = Result<ModelResponse, ModelError>>,
{
    let dispatch = async {
        if let Some(timeout) = attempt_timeout {
            tokio::time::timeout(timeout, dispatch)
                .await
                .map_err(|_| ModelError::timeout("模型请求超时。"))?
        } else {
            dispatch.await
        }
    };
    tokio::select! {
        _ = cancellation.cancelled() => Err(ModelError::cancelled()),
        result = dispatch => result,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_agent_stream(
    state: &AppState,
    context: &ProviderRequestContext<'_>,
    mut request: ModelRequest,
    target: &ResolvedTarget,
    tool_context: &ToolRuntimeContext,
    cancellation: &CancellationToken,
    on_event: &Channel<ModelStreamEvent>,
    run_id: &str,
    conversation_id: &str,
    message_id: &str,
    response_preview: &mut String,
    reasoning_preview: &mut String,
) -> Result<ModelStreamOutcome, ModelError> {
    if tool_context.permission_mode
        == crate::chat::conversation_types::AiPermissionMode::AskEveryTime
    {
        request.tools.retain(|tool| {
            matches!(
                tool.name.as_str(),
                "activate_skill"
                    | "inspect_skill"
                    | "search_tools"
                    | "inspect_tool"
                    | "search_skills"
                    | "read_skill_resource"
            )
        });
    }
    let mut run_usage = ModelUsage {
        call_count: 0,
        ..ModelUsage::default()
    };
    let mut skill_cache = SkillRunCache::with_activated(&tool_context.manual_skill_ids);
    let mut activated_skill_ids = Vec::<String>::new();
    let max_agent_rounds = agent_round_limit(state);
    let mut tool_call_total = 0usize;
    let mut force_final_answer = false;

    // Slash 显式触发的 Skill 正文已经在 `prepare_call` 中由 Rust 成功读取并注入。
    // 只有走到这里才向前端报告真实激活，避免请求准备失败时界面提前显示“已使用 Skill”。
    for skill_id in &tool_context.manual_skill_ids {
        emit_skill_activated(
            state,
            on_event,
            run_id,
            conversation_id,
            message_id,
            skill_id,
        )?;
    }

    for call_index in 0..=max_agent_rounds {
        let round_index = u32::from(call_index);
        let final_call = force_final_answer || is_final_agent_call(call_index, max_agent_rounds);
        if final_call && !request.tools.is_empty() {
            request.tools.clear();
            let prompt = request.system_prompt.get_or_insert_with(String::new);
            prompt.push_str(
                "\n\n本次 Agent 已达到运行预算。不要再请求工具，请根据已有结果给出最终回答，并明确说明仍缺少的信息。",
            );
        }
        if cancellation.is_cancelled() {
            return Ok(ModelStreamOutcome::Cancelled);
        }
        state
            .library_repository
            .transition_agent_run(
                run_id,
                AgentRunEvent::ModelCallStarted,
                None,
                &serde_json::json!({ "roundIndex": round_index }).to_string(),
                None,
            )
            .map_err(|error| {
                ModelError::provider(format!("Agent 模型调用状态提交失败：{error}"))
            })?;
        let call_started_at_ms = usage::now_ms();
        let call_started = Instant::now();
        let mut round_text = String::new();
        let outcome = stream_inner(
            state,
            context,
            &target.provider_id,
            &request,
            cancellation,
            on_event,
            run_id,
            conversation_id,
            message_id,
            response_preview,
            reasoning_preview,
            &mut round_text,
        )
        .await;
        let call_duration_ms = elapsed_ms(call_started);
        let (status, status_code, usage_value, tool_call_count, error_kind) = match &outcome {
            Ok(ModelStreamOutcome::Completed(summary)) => (
                "success",
                Some(200),
                summary.usage.clone(),
                summary.tool_calls.len() as u32,
                None,
            ),
            Ok(ModelStreamOutcome::Cancelled) => ("stopped", None, None, 0, None),
            Err(error) => (
                "error",
                error.status_code,
                None,
                0,
                Some(format!("{:?}", error.kind)),
            ),
        };
        let mut usage_value = usage_value;
        set_overall_duration(&mut usage_value, call_duration_ms);
        apply_usage_origin(&mut usage_value, target.provider_kind);
        apply_usage_pricing(&mut usage_value, target, call_started_at_ms);
        record_usage(
            state,
            target,
            call_started_at_ms,
            call_duration_ms,
            "agentModelCall",
            status,
            status_code,
            usage_value.clone(),
            Some(conversation_id.to_string()),
            Some(message_id.to_string()),
            error_kind,
            UsageCallMetadata {
                run_id: Some(run_id.to_string()),
                round_index: Some(round_index),
                call_index: Some(round_index),
                parent_operation: Some("chatStream".to_string()),
                activated_skill_ids: activated_skill_ids.clone(),
                tool_definition_count: request.tools.len() as u32,
                tool_call_count,
            },
        )
        .await;

        let ModelStreamOutcome::Completed(mut summary) = outcome? else {
            return Ok(ModelStreamOutcome::Cancelled);
        };
        if let Some(call_usage) = usage_value.as_ref() {
            crate::usage::normalize::merge_run_usage(&mut run_usage, call_usage);
        }
        if summary.tool_calls.is_empty() {
            state
                .library_repository
                .transition_agent_run(
                    run_id,
                    AgentRunEvent::FinalizationStarted,
                    None,
                    &serde_json::json!({ "roundIndex": round_index }).to_string(),
                    None,
                )
                .map_err(|error| {
                    ModelError::provider(format!("Agent 最终整理状态提交失败：{error}"))
                })?;
            summary.usage = (run_usage.call_count > 0).then_some(run_usage);
            return Ok(ModelStreamOutcome::Completed(summary));
        }
        agent::validate_disclosed_tool_calls(&request, &summary.tool_calls)?;
        if final_call {
            return Err(ModelError::invalid_response(
                "最终汇总调用仍返回了工具请求，无法安全继续执行。",
            ));
        }
        match evaluate_tool_batch(&summary.tool_calls, tool_call_total)? {
            ToolBatchBudget::Execute {
                next_business_total,
            } => tool_call_total = next_business_total,
            ToolBatchBudget::Finalize => {
                force_final_answer = true;
                continue;
            }
        }

        state
            .library_repository
            .transition_agent_run(
                run_id,
                AgentRunEvent::ToolBatchStarted,
                None,
                &serde_json::json!({
                    "roundIndex": round_index,
                    "toolCallCount": summary.tool_calls.len(),
                })
                .to_string(),
                None,
            )
            .map_err(|error| {
                ModelError::provider(format!("Agent 工具批次启动状态提交失败：{error}"))
            })?;

        let tool_calls = summary.tool_calls;
        request.messages.push(ModelMessage {
            role: ModelRole::Assistant,
            content: round_text,
            images: Vec::new(),
            tool_calls: tool_calls.clone(),
            tool_result: None,
        });
        for call in tool_calls.iter().filter(|call| {
            agent::parallel_safe(tool_context, &call.name)
                && !agent::requires_approval(tool_context, call)
        }) {
            emit_tool_trace(
                on_event,
                run_id,
                conversation_id,
                message_id,
                ToolTraceSnapshot {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    status: ToolTraceStatus::Running,
                    risk: agent::tool_risk(tool_context, call),
                    argument_summary: agent::argument_summary(call),
                    preview: None,
                    duration_ms: None,
                    input_chars: Some(call.arguments.to_string().chars().count()),
                    output_chars: None,
                    output_truncated: None,
                    error_kind: None,
                },
            )?;
        }
        let mut parallel_results = execute_parallel_safe_tools(
            state,
            tool_context,
            &skill_cache,
            cancellation,
            &tool_calls,
            Some(run_id),
        )
        .await;
        for (index, call) in tool_calls.into_iter().enumerate() {
            let result = if let Some(result) = parallel_results[index].take() {
                emit_tool_trace(
                    on_event,
                    run_id,
                    conversation_id,
                    message_id,
                    ToolTraceSnapshot {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        status: if result.execution.is_error {
                            ToolTraceStatus::Failed
                        } else {
                            ToolTraceStatus::Completed
                        },
                        risk: agent::tool_risk(tool_context, &call),
                        argument_summary: agent::argument_summary(&call),
                        preview: Some(result.execution.preview.clone()),
                        duration_ms: Some(result.duration_ms),
                        input_chars: Some(call.arguments.to_string().chars().count()),
                        output_chars: Some(result.execution.output_chars),
                        output_truncated: Some(result.execution.output_truncated),
                        error_kind: result.execution.error_kind(),
                    },
                )?;
                result.execution
            } else {
                execute_agent_tool(
                    state,
                    tool_context,
                    &mut skill_cache,
                    cancellation,
                    on_event,
                    run_id,
                    conversation_id,
                    message_id,
                    &call,
                )
                .await
            };
            if let Some(skill_id) = result.activated_skill_id.as_ref() {
                if !activated_skill_ids.contains(skill_id) {
                    activated_skill_ids.push(skill_id.clone());
                    emit_skill_activated(
                        state,
                        on_event,
                        run_id,
                        conversation_id,
                        message_id,
                        skill_id,
                    )?;
                }
            }
            agent::apply_tool_disclosures(&mut request, &call, &result, tool_context);
            request.messages.push(ModelMessage {
                role: ModelRole::Tool,
                content: String::new(),
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_result: Some(ModelToolResult {
                    call_id: call.id,
                    name: call.name,
                    content: result.content,
                    is_error: result.is_error,
                }),
            });
        }
        if cancellation.is_cancelled() {
            return Ok(ModelStreamOutcome::Cancelled);
        }
        state
            .library_repository
            .transition_agent_run(
                run_id,
                AgentRunEvent::ToolBatchCompleted,
                None,
                &serde_json::json!({ "roundIndex": round_index }).to_string(),
                None,
            )
            .map_err(|error| {
                ModelError::provider(format!("Agent 工具批次状态提交失败：{error}"))
            })?;
    }
    Err(ModelError::provider("Agent 未能在运行预算内生成最终回答。"))
}

#[allow(clippy::too_many_arguments)]
async fn execute_agent_tool(
    state: &AppState,
    context: &ToolRuntimeContext,
    skill_cache: &mut SkillRunCache,
    cancellation: &CancellationToken,
    on_event: &Channel<ModelStreamEvent>,
    run_id: &str,
    conversation_id: &str,
    message_id: &str,
    call: &ModelToolCall,
) -> agent::ToolExecution {
    let risk = agent::tool_risk(context, call);
    let argument_summary = agent::argument_summary(call);
    let approval_required = agent::requires_approval(context, call);
    let approval_id = approval_required.then(|| uuid::Uuid::new_v4().to_string());
    let expires_at_ms = approval_required
        .then(|| usage::now_ms().saturating_add(TOOL_APPROVAL_TIMEOUT.as_millis() as u64));
    let arguments_hash = format!(
        "{:x}",
        Sha256::digest(call.arguments.to_string().as_bytes())
    );
    let (source_json, catalog_revision) = agent::tool_provenance(context, &call.name);
    let (_, execution_version, mut tool_state_version) =
        match state.library_repository.create_agent_tool_call(
            run_id,
            &call.id,
            &call.name,
            &format!("{risk:?}"),
            &arguments_hash,
            &source_json,
            &catalog_revision,
            approval_id.as_deref(),
            expires_at_ms,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return rejected_tool(
                    &format!("Tool Call 状态记录创建失败，已阻止执行：{error}"),
                    call,
                );
            }
        };

    if approval_required {
        if let Err(error) = state.library_repository.transition_agent_run(
            run_id,
            AgentRunEvent::ApprovalRequired,
            None,
            &serde_json::json!({ "callId": call.id }).to_string(),
            None,
        ) {
            let _ = state.library_repository.transition_agent_tool_call(
                run_id,
                &call.id,
                ToolCallEvent::Cancelled,
                execution_version,
                Some(tool_state_version),
                Some("Agent 无法进入审批状态。"),
                Some("agentStateConflict"),
            );
            return rejected_tool(&format!("Agent 无法进入工具审批状态：{error}"), call);
        }
        let approval_id = approval_id.expect("approval id exists when approval is required");
        let trace = ToolTraceSnapshot {
            call_id: call.id.clone(),
            name: call.name.clone(),
            status: ToolTraceStatus::AwaitingApproval,
            risk,
            argument_summary: argument_summary.clone(),
            preview: None,
            duration_ms: None,
            input_chars: Some(call.arguments.to_string().chars().count()),
            output_chars: None,
            output_truncated: None,
            error_kind: None,
        };
        // 提问工具的入参本身就是要展示的问题；解析失败按普通审批处理，界面至少还能用。
        let interrupt = match parse_tool_questions(&call.name, &call.arguments) {
            Some(questions) => ToolInterruptKind::Question { questions },
            None => ToolInterruptKind::Approval,
        };
        let (sender, receiver) = oneshot::channel();
        state.pending_tool_approvals.lock().await.insert(
            approval_id.clone(),
            PendingToolApproval {
                sender,
                run_id: run_id.to_string(),
                call_id: call.id.clone(),
                execution_version,
                state_version: tool_state_version,
                expires_at_ms: expires_at_ms.unwrap_or_default(),
                interrupt: interrupt.clone(),
            },
        );
        let sent = on_event.send(ModelStreamEvent::ToolApprovalRequested {
            run_id: run_id.to_string(),
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
            approval_id: approval_id.clone(),
            trace,
            interrupt,
        });
        if sent.is_err() {
            state
                .pending_tool_approvals
                .lock()
                .await
                .remove(&approval_id);
            let _ = state.library_repository.transition_agent_tool_call(
                run_id,
                &call.id,
                ToolCallEvent::Rejected,
                execution_version,
                Some(tool_state_version),
                Some("无法向界面发送工具审批请求。"),
                Some("approvalChannelClosed"),
            );
            let _ = state.library_repository.transition_agent_run(
                run_id,
                AgentRunEvent::ApprovalsResolved,
                None,
                &serde_json::json!({ "callId": call.id, "approved": false }).to_string(),
                None,
            );
            return rejected_tool("无法向界面发送工具审批请求。", call);
        }
        // 审批对象不能因前端失联永久留在 Rust 状态中。五分钟未响应按拒绝处理。
        let (approval_outcome, answers) =
            wait_for_tool_approval(receiver, cancellation, TOOL_APPROVAL_TIMEOUT).await;
        state
            .pending_tool_approvals
            .lock()
            .await
            .remove(&approval_id);
        // 提问工具没有「执行」这一步：用户的选择本身就是结果，直接回给模型。
        if approval_outcome == ToolApprovalOutcome::Answered {
            return answered_tool(
                state,
                run_id,
                conversation_id,
                message_id,
                on_event,
                AnsweredTool {
                    call,
                    answers,
                    risk,
                    argument_summary: argument_summary.clone(),
                    execution_version,
                    tool_state_version,
                },
            );
        }
        if approval_outcome != ToolApprovalOutcome::Approved {
            let (event, status, message, error_kind) = match approval_outcome {
                ToolApprovalOutcome::Rejected => (
                    None,
                    ToolTraceStatus::Rejected,
                    "用户拒绝了本次工具调用。",
                    "approvalRejected",
                ),
                ToolApprovalOutcome::TimedOut => (
                    Some(ToolCallEvent::TimedOut),
                    ToolTraceStatus::TimedOut,
                    "工具审批已超时。",
                    "approvalTimedOut",
                ),
                ToolApprovalOutcome::Cancelled => (
                    Some(ToolCallEvent::Cancelled),
                    ToolTraceStatus::Cancelled,
                    "Agent 已取消本次工具调用。",
                    "agentCancelled",
                ),
                ToolApprovalOutcome::ChannelClosed => (
                    Some(ToolCallEvent::Rejected),
                    ToolTraceStatus::Rejected,
                    "工具审批通道已关闭。",
                    "approvalChannelClosed",
                ),
                ToolApprovalOutcome::Approved | ToolApprovalOutcome::Answered => unreachable!(),
            };
            if let Some(event) = event {
                let _ = state.library_repository.transition_agent_tool_call(
                    run_id,
                    &call.id,
                    event,
                    execution_version,
                    Some(tool_state_version),
                    Some(message),
                    Some(error_kind),
                );
            }
            if approval_outcome != ToolApprovalOutcome::Cancelled {
                let _ = state.library_repository.transition_agent_run(
                    run_id,
                    AgentRunEvent::ApprovalsResolved,
                    None,
                    &serde_json::json!({ "callId": call.id, "approved": false }).to_string(),
                    None,
                );
            }
            let _ = emit_tool_trace(
                on_event,
                run_id,
                conversation_id,
                message_id,
                ToolTraceSnapshot {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    status,
                    risk,
                    argument_summary,
                    preview: Some(message.to_string()),
                    duration_ms: Some(0),
                    input_chars: Some(call.arguments.to_string().chars().count()),
                    output_chars: Some(message.chars().count()),
                    output_truncated: Some(false),
                    error_kind: Some(error_kind.to_string()),
                },
            );
            return rejected_tool(message, call);
        }
        // resolve_tool_approval 已先以 CAS 持久化 AwaitingApproval -> Approved。
        tool_state_version = tool_state_version.saturating_add(1);
        if let Err(error) = state.library_repository.transition_agent_run(
            run_id,
            AgentRunEvent::ApprovalsResolved,
            None,
            &serde_json::json!({ "callId": call.id, "approved": true }).to_string(),
            None,
        ) {
            let _ = state.library_repository.transition_agent_tool_call(
                run_id,
                &call.id,
                ToolCallEvent::Cancelled,
                execution_version,
                Some(tool_state_version),
                Some("审批后 Agent 状态已变化。"),
                Some("agentStateConflict"),
            );
            return rejected_tool(&format!("审批后 Agent 状态已变化：{error}"), call);
        }
        tool_state_version = match state.library_repository.transition_agent_tool_call(
            run_id,
            &call.id,
            ToolCallEvent::Enqueued,
            execution_version,
            Some(tool_state_version),
            None,
            None,
        ) {
            Ok((_, _, version)) => version,
            Err(error) => {
                let _ = state.library_repository.transition_agent_tool_call(
                    run_id,
                    &call.id,
                    ToolCallEvent::Cancelled,
                    execution_version,
                    Some(tool_state_version),
                    Some("Tool Call 入队失败。"),
                    Some("stateTransitionFailed"),
                );
                return rejected_tool(&format!("Tool Call 入队失败：{error}"), call);
            }
        };
    }

    tool_state_version = match state.library_repository.transition_agent_tool_call(
        run_id,
        &call.id,
        ToolCallEvent::Started,
        execution_version,
        Some(tool_state_version),
        None,
        None,
    ) {
        Ok((_, _, version)) => version,
        Err(error) => {
            let _ = state.library_repository.transition_agent_tool_call(
                run_id,
                &call.id,
                ToolCallEvent::Cancelled,
                execution_version,
                Some(tool_state_version),
                Some("Tool Call 启动失败。"),
                Some("stateTransitionFailed"),
            );
            return rejected_tool(&format!("Tool Call 启动被状态机拒绝：{error}"), call);
        }
    };

    let started = Instant::now();
    let _ = emit_tool_trace(
        on_event,
        run_id,
        conversation_id,
        message_id,
        ToolTraceSnapshot {
            call_id: call.id.clone(),
            name: call.name.clone(),
            status: ToolTraceStatus::Running,
            risk,
            argument_summary: argument_summary.clone(),
            preview: None,
            duration_ms: None,
            input_chars: Some(call.arguments.to_string().chars().count()),
            output_chars: None,
            output_truncated: None,
            error_kind: None,
        },
    );
    match agent::execute_tool(
        call,
        context,
        &state.conversation_repository,
        &state.skill_repository,
        &state.memory_repository,
        &state.library_repository,
        &state.library_operations,
        &state.mcp_manager,
        skill_cache,
        cancellation,
    )
    .await
    {
        Ok(result) => {
            let terminal_event = if result.is_error {
                ToolCallEvent::Failed
            } else {
                ToolCallEvent::Succeeded
            };
            let error_kind = result.error_kind();
            if let Err(error) = state.library_repository.transition_agent_tool_call(
                run_id,
                &call.id,
                terminal_event,
                execution_version,
                Some(tool_state_version),
                Some(&result.preview),
                error_kind.as_deref(),
            ) {
                return rejected_tool(
                    &format!("Tool 已返回，但终态因版本冲突被拒绝：{error}"),
                    call,
                );
            }
            let _ = emit_tool_trace(
                on_event,
                run_id,
                conversation_id,
                message_id,
                ToolTraceSnapshot {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    status: if result.is_error {
                        ToolTraceStatus::Failed
                    } else {
                        ToolTraceStatus::Completed
                    },
                    risk,
                    argument_summary,
                    preview: Some(result.preview.clone()),
                    duration_ms: Some(elapsed_ms(started)),
                    input_chars: Some(call.arguments.to_string().chars().count()),
                    output_chars: Some(result.output_chars),
                    output_truncated: Some(result.output_truncated),
                    error_kind,
                },
            );
            result
        }
        Err(error) => {
            let message = error.message.clone();
            let _ = state.library_repository.transition_agent_tool_call(
                run_id,
                &call.id,
                ToolCallEvent::Failed,
                execution_version,
                Some(tool_state_version),
                Some(&message),
                Some(&format!("{:?}", error.kind)),
            );
            let _ = emit_tool_trace(
                on_event,
                run_id,
                conversation_id,
                message_id,
                ToolTraceSnapshot {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    status: ToolTraceStatus::Failed,
                    risk,
                    argument_summary,
                    preview: Some(message.clone()),
                    duration_ms: Some(elapsed_ms(started)),
                    input_chars: Some(call.arguments.to_string().chars().count()),
                    output_chars: Some(message.chars().count()),
                    output_truncated: Some(false),
                    error_kind: Some(format!("{:?}", error.kind)),
                },
            );
            rejected_tool(&message, call)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolApprovalOutcome {
    Approved,
    Answered,
    Rejected,
    Cancelled,
    TimedOut,
    ChannelClosed,
}

/// 答案单独回传，让 `ToolApprovalOutcome` 保持 `Copy`——它在下游被 `!=` 反复比较。
async fn wait_for_tool_approval(
    receiver: oneshot::Receiver<ToolInterruptResponse>,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> (ToolApprovalOutcome, Vec<ToolQuestionAnswer>) {
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => (ToolApprovalOutcome::Cancelled, Vec::new()),
        decision = tokio::time::timeout(timeout, receiver) => {
            match decision {
                Err(_) => (ToolApprovalOutcome::TimedOut, Vec::new()),
                Ok(Ok(ToolInterruptResponse::Approval(true))) => {
                    (ToolApprovalOutcome::Approved, Vec::new())
                }
                Ok(Ok(ToolInterruptResponse::Approval(false))) => {
                    (ToolApprovalOutcome::Rejected, Vec::new())
                }
                Ok(Ok(ToolInterruptResponse::Answers(answers))) => {
                    (ToolApprovalOutcome::Answered, answers)
                }
                Ok(Err(_)) => (ToolApprovalOutcome::ChannelClosed, Vec::new()),
            }
        },
    }
}

/// 从提问工具的入参里取出要展示的问题。
///
/// 返回 `None` 表示「这不是提问工具，或入参不可用」，调用方退回普通审批流程 ——
/// 界面至少还能让用户放行或拒绝，不会卡死在一个渲染不出来的弹窗上。
fn parse_tool_questions(name: &str, arguments: &serde_json::Value) -> Option<Vec<ToolQuestion>> {
    if name != crate::chat::agent::catalog::ASK_USER_TOOL_NAME {
        return None;
    }
    let questions = arguments.get("questions")?.as_array()?;
    if questions.is_empty() || questions.len() > crate::chat::agent::catalog::ASK_USER_MAX_QUESTIONS
    {
        return None;
    }
    let parsed = questions
        .iter()
        .map(|value| serde_json::from_value::<ToolQuestion>(value.clone()).ok())
        .collect::<Option<Vec<_>>>()?;
    if parsed.iter().any(|question| !question.is_renderable()) {
        return None;
    }
    // header 是回答的主键：重了前端就只剩一个控件，却要交回两条同名答案，模型
    // 无从分辨哪条对应哪题。宁可退回普通审批，也不要渲染一个答不对题的弹窗。
    let mut headers = parsed
        .iter()
        .map(|question| question.header.trim())
        .collect::<Vec<_>>();
    headers.sort_unstable();
    headers.dedup();
    if headers.len() != parsed.len() {
        return None;
    }
    Some(parsed)
}

/// 提问工具收尾所需的上下文。字段多到值得单独成结构，免得参数表变成十个位置参数。
struct AnsweredTool<'a> {
    call: &'a ModelToolCall,
    answers: Vec<ToolQuestionAnswer>,
    risk: ToolRisk,
    argument_summary: String,
    execution_version: u32,
    tool_state_version: u32,
}

/// 把用户的选择落库并回给模型。
///
/// 不进执行队列：提问工具没有副作用，用户的选择就是它的返回值。
fn answered_tool(
    state: &AppState,
    run_id: &str,
    conversation_id: &str,
    message_id: &str,
    on_event: &Channel<ModelStreamEvent>,
    context: AnsweredTool<'_>,
) -> agent::ToolExecution {
    let AnsweredTool {
        call,
        answers,
        risk,
        argument_summary,
        execution_version,
        tool_state_version,
    } = context;
    // 序列化失败不该让整轮挂掉：退回纯文本，模型仍能读懂用户选了什么。
    let content = serde_json::to_string(&answers).unwrap_or_else(|_| {
        answers
            .iter()
            .map(|answer| format!("{}：{}", answer.header, answer.values.join("、")))
            .collect::<Vec<_>>()
            .join("\n")
    });
    let preview = answers
        .iter()
        .map(|answer| format!("{}：{}", answer.header, answer.values.join("、")))
        .collect::<Vec<_>>()
        .join("；");
    let _ = state.library_repository.transition_agent_tool_call(
        run_id,
        &call.id,
        ToolCallEvent::Succeeded,
        execution_version,
        Some(tool_state_version.saturating_add(1)),
        None,
        None,
    );
    let _ = state.library_repository.transition_agent_run(
        run_id,
        AgentRunEvent::ApprovalsResolved,
        None,
        &serde_json::json!({ "callId": call.id, "answered": true }).to_string(),
        None,
    );
    let _ = emit_tool_trace(
        on_event,
        run_id,
        conversation_id,
        message_id,
        ToolTraceSnapshot {
            call_id: call.id.clone(),
            name: call.name.clone(),
            status: ToolTraceStatus::Answered,
            risk,
            argument_summary,
            preview: Some(preview.clone()),
            duration_ms: Some(0),
            input_chars: Some(call.arguments.to_string().chars().count()),
            output_chars: Some(content.chars().count()),
            output_truncated: Some(false),
            error_kind: None,
        },
    );
    agent::ToolExecution {
        output_chars: content.chars().count(),
        content,
        preview,
        is_error: false,
        activated_skill_id: None,
        output_truncated: false,
    }
}

fn rejected_tool(message: &str, _call: &ModelToolCall) -> agent::ToolExecution {
    agent::ToolExecution {
        output_chars: message.chars().count(),
        content: message.to_string(),
        preview: message.to_string(),
        is_error: true,
        activated_skill_id: None,
        output_truncated: false,
    }
}

fn emit_tool_trace(
    on_event: &Channel<ModelStreamEvent>,
    run_id: &str,
    conversation_id: &str,
    message_id: &str,
    trace: ToolTraceSnapshot,
) -> Result<(), ModelError> {
    on_event
        .send(ModelStreamEvent::ToolTrace {
            run_id: run_id.to_string(),
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
            trace,
        })
        .map_err(|error| ModelError::provider(format!("无法发送工具轨迹：{error}")))
}

fn emit_skill_activated(
    state: &AppState,
    on_event: &Channel<ModelStreamEvent>,
    run_id: &str,
    conversation_id: &str,
    message_id: &str,
    skill_id: &str,
) -> Result<(), ModelError> {
    let summary = state
        .skill_repository
        .list()
        .map_err(ModelError::invalid_configuration)?
        .skills
        .into_iter()
        .find(|skill| skill.id == skill_id)
        .ok_or_else(|| ModelError::invalid_configuration("模型激活的技能已不存在。"))?;
    on_event
        .send(ModelStreamEvent::SkillActivated {
            run_id: run_id.to_string(),
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
            skill_id: summary.id,
            name: summary.name,
            version: summary.version,
            content_hash: summary.content_hash,
        })
        .map_err(|error| ModelError::provider(format!("无法发送技能激活事件：{error}")))
}

#[allow(clippy::too_many_arguments)]
async fn stream_inner(
    state: &AppState,
    context: &ProviderRequestContext<'_>,
    provider_id: &str,
    request: &ModelRequest,
    cancellation: &CancellationToken,
    on_event: &Channel<ModelStreamEvent>,
    run_id: &str,
    conversation_id: &str,
    message_id: &str,
    response_preview: &mut String,
    reasoning_preview: &mut String,
    round_text: &mut String,
) -> Result<ModelStreamOutcome, ModelError> {
    let retry_policy = retry_policy(state);
    let mut retry_index = 0;
    let mut emitted_output = false;
    loop {
        let permit = state
            .provider_concurrency
            .acquire(
                provider_id,
                ProviderRequestClass::Interactive,
                cancellation,
            )
            .await?;
        let mut emit = |chunk: ModelStreamChunk| match chunk {
            ModelStreamChunk::TextDelta(delta) => {
                request_debug::append_preview(response_preview, &delta);
                round_text.push_str(&delta);
                on_event
                    .send(ModelStreamEvent::TextDelta {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.to_string(),
                        message_id: message_id.to_string(),
                        delta,
                    })
                    .map_err(|error| ModelError::provider(format!("无法发送文本增量：{error}")))?;
                emitted_output = true;
                Ok(())
            }
            ModelStreamChunk::ReasoningDelta(delta) => {
                request_debug::append_preview(reasoning_preview, &delta);
                on_event
                    .send(ModelStreamEvent::ReasoningDelta {
                        run_id: run_id.to_string(),
                        conversation_id: conversation_id.to_string(),
                        message_id: message_id.to_string(),
                        delta,
                    })
                    .map_err(|error| ModelError::provider(format!("无法发送思考增量：{error}")))?;
                emitted_output = true;
                Ok(())
            }
            ModelStreamChunk::ToolCallDelta { .. } => {
                emitted_output = true;
                Ok(())
            }
        };
        let outcome = stream::stream(&state.http, context, request, cancellation, &mut emit).await;
        drop(permit);
        match outcome {
            Ok(outcome) => return Ok(outcome),
            Err(error)
                if !emitted_output
                    && retry_index < retry_policy.max_retries
                    && should_retry(&error) =>
            {
                let delay = retry_delay(&error, retry_index);
                tokio::select! {
                    _ = cancellation.cancelled() => return Ok(ModelStreamOutcome::Cancelled),
                    _ = tokio::time::sleep(delay) => {}
                }
                retry_index += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

fn unanswered_tail_attachment_flags(request: &ChatCompletionRequest) -> (bool, bool) {
    let last_assistant_index = request
        .messages
        .iter()
        .rposition(|message| message.role == ModelRole::Assistant);
    request
        .messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            message.role == ModelRole::User
                && last_assistant_index.is_none_or(|assistant_index| *index > assistant_index)
        })
        .flat_map(|(_, message)| message.attachments.iter())
        .fold((false, false), |(has_image, has_document), attachment| {
            (
                has_image || attachment.kind == "image",
                has_document || attachment.kind != "image",
            )
        })
}

fn validate_attachment_capabilities(
    request: &ChatCompletionRequest,
    supports_vision: Option<bool>,
    supports_tools: bool,
    display_name: &str,
) -> Result<(), ModelError> {
    let (has_image, has_document) = unanswered_tail_attachment_flags(request);
    if supports_vision == Some(false) && has_image {
        return Err(ModelError::invalid_configuration(format!(
            "当前模型 {display_name} 不支持图片输入，不能接收本轮图片附件。请移除图片、切换到支持视觉的模型，或在确认中转商确实支持后于模型能力设置中开启视觉能力。"
        )));
    }
    if !supports_tools && has_document {
        return Err(ModelError::invalid_configuration(format!(
            "当前模型 {display_name} 不支持工具调用，不能接收或读取本轮文档附件。请移除文档、切换到支持工具的模型，或在确认中转商确实支持后于模型能力设置中开启 Function Calling。"
        )));
    }
    Ok(())
}

fn estimate_model_text_tokens(value: &str) -> u64 {
    let ascii = value
        .chars()
        .filter(|character| character.is_ascii())
        .count() as u64;
    let non_ascii = value
        .chars()
        .filter(|character| !character.is_ascii())
        .count() as u64;
    (ascii + 3) / 4 + non_ascii
}

fn estimate_model_request_tokens(request: &ModelRequest) -> u64 {
    let system = request
        .system_prompt
        .as_deref()
        .map(estimate_model_text_tokens)
        .unwrap_or_default();
    let messages = request.messages.iter().fold(0u64, |total, message| {
        total
            + estimate_model_text_tokens(&message.content)
            + (message.images.len() as u64).saturating_mul(1_200)
            + message
                .tool_result
                .as_ref()
                .map(|result| estimate_model_text_tokens(&result.content))
                .unwrap_or_default()
            + message
                .tool_calls
                .iter()
                .map(|call| estimate_model_text_tokens(&call.arguments.to_string()))
                .sum::<u64>()
            + 8
    });
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            estimate_model_text_tokens(&tool.name)
                + estimate_model_text_tokens(&tool.description)
                + estimate_model_text_tokens(&tool.input_schema.to_string())
        })
        .sum::<u64>();
    system + messages + tools
}

fn validate_context_budget(
    request: &ModelRequest,
    target: &ResolvedTarget,
) -> Result<(), ModelError> {
    let Some(context_window_tokens) = target.context_window_tokens else {
        return Ok(());
    };
    let input_tokens = estimate_model_request_tokens(request);
    let output_reserve = u64::from(request.options.max_output_tokens.unwrap_or(4_096));
    let safety_margin = (context_window_tokens / 12).max(4_096);
    let required = input_tokens
        .saturating_add(output_reserve)
        .saturating_add(safety_margin);
    if required > context_window_tokens {
        return Err(ModelError::context_length(
            format!(
                "当前请求预计需要约 {input_tokens} Token，模型上下文上限为 {context_window_tokens} Token，已预留 {output_reserve} Token 输出和 {safety_margin} Token 安全余量。请先压缩或分块处理，不会自动切换模型。"
            ),
        ));
    }
    Ok(())
}

async fn prepare_call(
    state: &AppState,
    mut request: ChatCompletionRequest,
    // 传给工具披露：只有流式请求能弹出提问界面。
    streaming: bool,
) -> Result<PreparedCall, ModelError> {
    let provider_id = request.provider_id.trim().to_string();
    let model_id = request.model_id.trim().to_string();
    let target = {
        let settings = state
            .model_settings
            .read()
            .map_err(|_| ModelError::provider("模型设置暂时不可用，请重新启动应用后再试。"))?;
        resolve_target(&settings, &provider_id, &model_id)?
    };
    validate_attachment_capabilities(
        &request,
        target.supports_vision,
        target.supports_tools,
        &target.display_name,
    )?;

    let secrets = state.secrets;
    let provider_id_for_store = provider_id.clone();
    let api_key =
        tauri::async_runtime::spawn_blocking(move || secrets.get_api_key(&provider_id_for_store))
            .await
            .map_err(|_| ModelError::provider("读取系统凭据的后台任务失败。"))?
            .map_err(|_| ModelError::provider("无法从系统凭据读取 API Key。"))?
            .ok_or_else(ModelError::missing_api_key)?;
    let api_key = Zeroizing::new(api_key);
    if api_key.trim().is_empty() {
        return Err(ModelError::missing_api_key());
    }

    let api_model = target.api_model.clone();
    let is_auxiliary_operation = request.is_auxiliary_operation();
    let requested_skill =
        !request.activated_skill_ids.is_empty() || request.slash_skill_id.is_some();
    let use_agent_tools = !is_auxiliary_operation && target.supports_tools;
    if use_agent_tools {
        if let Some(ChatWorkspaceContext::Note { note_snapshot, .. }) =
            request.workspace_context.as_mut()
        {
            *note_snapshot = None;
        }
    }
    if !use_agent_tools && requested_skill {
        request.system_prompt.push_str(
            "\n\n当前模型配置不支持结构化工具调用，因此本轮没有加载或执行用户指定的 Skill。请直接回答可以仅凭对话上下文回答的部分，并明确说明无法执行该 Skill。",
        );
        request.activated_skill_ids.clear();
        request.slash_skill_id = None;
    }
    let memory_settings = state
        .app_settings
        .read()
        .map_err(|_| ModelError::provider("应用设置暂时不可用，请重新启动应用后再试。"))?
        .memory;
    let proxy_settings = state
        .app_settings
        .read()
        .map_err(|_| ModelError::provider("应用设置暂时不可用，请重新启动应用后再试。"))?
        .update_proxy
        .clone();
    let tool_context = if use_agent_tools {
        let working_directory = state
            .app_settings
            .read()
            .map_err(|_| ModelError::provider("应用设置暂时不可用，请重新启动应用后再试。"))?
            .working_directory
            .clone();
        agent::build_runtime_context(
            &request,
            &state.skill_repository,
            &state.mcp_manager,
            memory_settings,
            &working_directory,
            proxy_settings,
        )?
    } else {
        ToolRuntimeContext::disabled(request.permission_mode)
    };
    let repository = state.conversation_repository.clone();
    let skill_repository = state.skill_repository.clone();
    let mut model_request = tauri::async_runtime::spawn_blocking(move || {
        request.into_model_request(api_model, &repository, &skill_repository)
    })
    .await
    .map_err(|error| ModelError::provider(format!("读取聊天附件任务失败：{error}")))??;
    if target.supports_reasoning == Some(false) {
        model_request.options.thinking_enabled = false;
    }
    if use_agent_tools {
        let l1_memory = if memory_settings.enabled
            && memory_settings.inject_l1
            && memory_settings.allow_model_read
        {
            let repository = state.memory_repository.clone();
            Some(
                tauri::async_runtime::spawn_blocking(move || {
                    repository.read(crate::memory::MemoryLayer::L1)
                })
                .await
                .map_err(|error| ModelError::provider(format!("读取 L1 记忆任务失败：{error}")))?
                .map_err(ModelError::invalid_configuration)?,
            )
        } else {
            None
        };
        agent::configure_model_request(
            &mut model_request,
            &tool_context,
            l1_memory.as_deref(),
            streaming,
        );
        append_agent_runtime_prompt(&mut model_request, agent_round_limit(state));
    }
    // Tool schema、Skill 正文和运行时提示都可能显著增大输入，必须在最终请求成形后校验。
    validate_context_budget(&model_request, &target)?;
    Ok(PreparedCall {
        target,
        request: model_request,
        api_key,
        tool_context,
    })
}

#[allow(clippy::too_many_arguments)]
async fn record_usage(
    state: &AppState,
    target: &ResolvedTarget,
    created_at_ms: u64,
    duration_ms: u64,
    operation: &str,
    status: &str,
    status_code: Option<u16>,
    usage_value: Option<ModelUsage>,
    conversation_id: Option<String>,
    message_id: Option<String>,
    error_kind: Option<String>,
    metadata: UsageCallMetadata,
) {
    usage::record_model_call(
        state,
        UsageRecordInput {
            created_at_ms,
            duration_ms,
            source: "chat".to_string(),
            operation: operation.to_string(),
            provider_id: target.provider_id.clone(),
            provider_name: target.provider_name.clone(),
            model_id: target.model_id.clone(),
            api_model: target.api_model.clone(),
            display_name: target.display_name.clone(),
            protocol: protocol_name(target.protocol).to_string(),
            status: status.to_string(),
            status_code,
            usage: usage_value,
            conversation_id,
            message_id,
            run_id: metadata.run_id,
            round_index: metadata.round_index,
            call_index: metadata.call_index,
            parent_operation: metadata.parent_operation,
            activated_skill_ids: metadata.activated_skill_ids,
            tool_definition_count: metadata.tool_definition_count,
            tool_call_count: metadata.tool_call_count,
            error_kind,
        },
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
fn record_debug(
    state: &AppState,
    target: &ResolvedTarget,
    created_at_ms: u64,
    duration_ms: u64,
    status: &str,
    conversation_id: Option<String>,
    message_id: Option<String>,
    request: RequestDebugRequest,
    response: RequestDebugResponse,
    usage_value: Option<ModelUsage>,
) {
    request_debug::record(
        state,
        RequestDebugRecordInput {
            created_at_ms,
            duration_ms,
            provider_id: target.provider_id.clone(),
            provider_name: target.provider_name.clone(),
            model_id: target.model_id.clone(),
            api_model: target.api_model.clone(),
            display_name: target.display_name.clone(),
            protocol: protocol_name(target.protocol).to_string(),
            status: status.to_string(),
            conversation_id,
            message_id,
            request,
            response,
            usage: usage_value,
        },
    );
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn set_overall_duration(usage: &mut Option<ModelUsage>, duration_ms: u64) {
    usage
        .get_or_insert_with(ModelUsage::default)
        .total_duration_ms = Some(duration_ms);
}

fn apply_usage_pricing(
    usage_value: &mut Option<ModelUsage>,
    target: &ResolvedTarget,
    captured_at_ms: u64,
) {
    if let Some(usage_value) = usage_value.as_mut() {
        crate::usage::normalize::apply_pricing(
            usage_value,
            target.pricing.as_ref(),
            captured_at_ms,
        );
    }
}

fn apply_usage_origin(usage_value: &mut Option<ModelUsage>, provider_kind: ProviderKind) {
    let Some(usage) = usage_value.as_mut() else {
        return;
    };
    if provider_kind == ProviderKind::Custom
        && usage.usage_source == crate::ai::types::UsageSource::ProviderReported
    {
        usage.usage_source = crate::ai::types::UsageSource::GatewayNormalized;
    }
}

fn protocol_name(protocol: ApiProtocol) -> &'static str {
    match protocol {
        ApiProtocol::OpenAiChatCompletions => "openAiChatCompletions",
        ApiProtocol::OpenAiResponses => "openAiResponses",
        ApiProtocol::AnthropicMessages => "anthropicMessages",
        ApiProtocol::GeminiGenerateContent => "geminiGenerateContent",
    }
}

pub async fn cancel(state: &AppState, run_id: &str) -> Result<bool, ModelError> {
    crate::settings::types::validate_stable_id("Run ID", run_id.trim())
        .map_err(ModelError::invalid_configuration)?;
    let cancellation = state
        .active_chat_runs
        .lock()
        .await
        .get(run_id.trim())
        .cloned();
    let Some(cancellation) = cancellation else {
        return Ok(false);
    };
    state
        .library_repository
        .transition_agent_run(
            run_id.trim(),
            AgentRunEvent::CancelRequested,
            Some(&format!("agent-cancel:{}", run_id.trim())),
            r#"{"reason":"userRequested"}"#,
            None,
        )
        .map_err(|error| ModelError::provider(format!("Agent 取消状态提交失败：{error}")))?;
    cancellation.cancel();
    state.close_tool_approvals_for_run(run_id.trim()).await;
    Ok(true)
}

pub async fn resolve_tool_approval(
    state: &AppState,
    approval_id: &str,
    approved: bool,
) -> Result<bool, ModelError> {
    let event = if approved {
        ToolCallEvent::Approved
    } else {
        ToolCallEvent::Rejected
    };
    resolve_tool_interrupt(
        state,
        approval_id,
        event,
        ToolInterruptResponse::Approval(approved),
    )
    .await
}

/// 用户对提问工具的作答。
///
/// 落库记 `Answered` 而不是 `Approved`：审计轨迹里「回答了问题」和「批准了危险操作」
/// 不是一回事，混用会让事后追溯失真。
pub async fn resolve_tool_question(
    state: &AppState,
    approval_id: &str,
    answers: Vec<ToolQuestionAnswer>,
) -> Result<bool, ModelError> {
    if answers.is_empty() {
        return Err(ModelError::invalid_configuration(
            "提问工具至少要有一个回答。",
        ));
    }
    resolve_tool_interrupt(
        state,
        approval_id,
        ToolCallEvent::Answered,
        ToolInterruptResponse::Answers(answers),
    )
    .await
}

/// 审批与提问共用的收尾：过期判定、CAS 落库、唤醒 Worker。
async fn resolve_tool_interrupt(
    state: &AppState,
    approval_id: &str,
    event: ToolCallEvent,
    response: ToolInterruptResponse,
) -> Result<bool, ModelError> {
    crate::settings::types::validate_stable_id("Approval ID", approval_id.trim())
        .map_err(ModelError::invalid_configuration)?;
    // 取出前先核对种类：拿「回答」去结掉一个审批，会让模型收到一份它以为执行过的
    // 假结果；反过来用「批准」结掉提问则会跳过用户的选择。两者都保持挂起更安全。
    let pending = {
        let mut pending_approvals = state.pending_tool_approvals.lock().await;
        let Some(existing) = pending_approvals.get(approval_id.trim()) else {
            return Ok(false);
        };
        if !response.matches(&existing.interrupt) {
            return Err(ModelError::invalid_configuration(
                "工具中断的响应类型与请求不符。",
            ));
        }
        pending_approvals
            .remove(approval_id.trim())
            .expect("entry exists under the same lock guard")
    };
    let now = usage::now_ms();
    let decision = if now >= pending.expires_at_ms {
        ToolCallEvent::TimedOut
    } else {
        event
    };
    if state
        .library_repository
        .transition_agent_tool_call(
            &pending.run_id,
            &pending.call_id,
            decision,
            pending.execution_version,
            Some(pending.state_version),
            None,
            None,
        )
        .is_err()
    {
        return Ok(false);
    }
    if decision == ToolCallEvent::TimedOut {
        return Ok(false);
    }
    Ok(pending.sender.send(response).is_ok())
}

fn retry_policy(state: &AppState) -> RetryPolicy {
    state
        .app_settings
        .read()
        .map(|settings| RetryPolicy {
            max_retries: if settings.retry_enabled {
                settings.retry_attempts
            } else {
                0
            },
        })
        .unwrap_or(RetryPolicy { max_retries: 0 })
}

fn agent_round_limit(state: &AppState) -> u16 {
    state
        .app_settings
        .read()
        .map(|settings| settings.agent_max_rounds)
        .unwrap_or(DEFAULT_MAX_AGENT_ROUNDS)
}

fn is_business_tool(name: &str) -> bool {
    !matches!(
        name,
        "search_tools" | "inspect_tool" | "search_skills" | "inspect_skill"
    )
}

fn should_retry(error: &ModelError) -> bool {
    matches!(
        error.kind,
        ModelErrorKind::RateLimited
            | ModelErrorKind::ConcurrencyLimited
            | ModelErrorKind::ClientTimeout
            | ModelErrorKind::UpstreamTimeout
            | ModelErrorKind::Connection
            | ModelErrorKind::Provider
    )
}

/// 流式保活失败是否应当回落到非流式。
///
/// 只认「这个上游/中转站不吃流式」这一类信号：
/// - `InvalidResponse`：SSE 分帧或事件解析失败。不支持流式的网关常见的表现是
///   HTTP 200 却回一个普通 JSON body，解析器会在这里报错。
/// - `InvalidConfiguration`：网关对 `stream: true` 直接回 400/422
///   （`classify_error` 把这两个状态码归到这里）。
///
/// 刻意**不**包含的几类：
/// - 两种超时。流式本来就是为了避免超时，流式都超时了，非流式只会更早死；
///   回落只会把一次失败变成两次，把 P0-6 刚统一好的超时语义又搅乱。
/// - `Connection` / `RateLimited` / `Authentication` / `ContextLengthExceeded`：
///   与传输方式无关，换非流式同样失败。这些交给重试循环或直接上报。
/// - `Cancelled`：用户意图，不能偷偷再发一次请求。
fn should_fall_back_from_streaming(error: &ModelError) -> bool {
    matches!(
        error.kind,
        ModelErrorKind::InvalidResponse | ModelErrorKind::InvalidConfiguration
    )
}

/// 请求体字节硬闸。超限即 `ContextLengthExceeded`，不是 `InvalidConfiguration`。
///
/// kind 的选择是这道闸能起作用的关键：调用方靠 kind 判断「该缩载荷了」
/// （深度笔记的 `should_fallback_to_chunked_planner` 认 `ContextLengthExceeded`，
/// 会把直出规划降级成分块规划）。报成配置错误会让它变成一个死错误，
/// 用户看到的就是任务失败而不是自动缩小重试。
///
/// 边界取 `>`：正好等于上限是合规的。
fn enforce_request_byte_limit(bytes: usize, limit: usize) -> Result<(), ModelError> {
    if bytes > limit {
        return Err(ModelError::context_length(format!(
            "请求体 {bytes} 字节超过上限 {limit} 字节，已在发出前拦下，请缩小本次输入。"
        )));
    }
    Ok(())
}

/// 退避抖动源。项目没有 `rand` 依赖，这里用纳秒时钟做低成本扰动：
/// 目的只是打散并发重试的对齐，不需要密码学强度。
fn jitter_ratio() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.subsec_nanos())
        .unwrap_or(0);
    // 用一次乘法混淆低位，避免调用点密集时纳秒低位近似单调。
    let mixed = (u64::from(nanos)).wrapping_mul(6_364_136_223_846_793_005) >> 33;
    (mixed % 1_000) as f64 / 1_000.0
}

/// 重试退避。基数按错误类型重定，并叠加 ±25% 抖动。
///
/// 修复前 `RateLimited` 落在 `_` 分支上，基数只有 300ms —— 一个不带 `Retry-After`
/// 的 429 会被几乎立刻重投，把限流打成雪崩；而 `ConcurrencyLimited` 是 15s，
/// 两者量级差 50 倍，缺乏统一模型。全程无抖动也意味着并发请求会同步重试。
fn retry_delay(error: &ModelError, retry_index: u8) -> Duration {
    retry_delay_with_jitter(error, retry_index, jitter_ratio())
}

fn retry_delay_with_jitter(error: &ModelError, retry_index: u8, jitter: f64) -> Duration {
    let base_ms: u64 = match error.kind {
        // 限流与并发受限都是"上游让我们慢下来"，基数取同一量级。
        ModelErrorKind::ConcurrencyLimited => 15_000,
        ModelErrorKind::RateLimited => 8_000,
        ModelErrorKind::QuotaExceeded => 15_000,
        // 超时类：中转站压力信号，退避要明显长于普通网络抖动。
        ModelErrorKind::UpstreamTimeout | ModelErrorKind::ClientTimeout => 5_000,
        ModelErrorKind::ProviderUnavailable => 5_000,
        // 连接层瞬时失败，快速重试是合理的。
        ModelErrorKind::Connection => 1_000,
        _ => 1_000,
    };
    let exponential_ms = base_ms.saturating_mul(1u64 << retry_index.min(4));
    // 上游明确给了 Retry-After 就尊重它，只补一点抖动避免同刻齐发。
    let target_ms = error.retry_after_ms.unwrap_or(exponential_ms);
    let jitter_span = target_ms / 2;
    let jittered = target_ms
        .saturating_sub(jitter_span / 2)
        .saturating_add((jitter_span as f64 * jitter.clamp(0.0, 1.0)) as u64);
    Duration::from_millis(jittered.clamp(100, 120_000))
}

fn resolve_target(
    settings: &ModelSettings,
    provider_id: &str,
    model_id: &str,
) -> Result<ResolvedTarget, ModelError> {
    let provider = settings
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| ModelError::invalid_configuration("没有找到指定的模型供应商。"))?;
    if !provider.enabled {
        return Err(ModelError::invalid_configuration(
            "当前模型供应商已经停用。",
        ));
    }

    let model = provider
        .models
        .iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| ModelError::invalid_configuration("指定模型不属于当前供应商。"))?;
    if !model.enabled {
        return Err(ModelError::invalid_configuration("当前模型已经停用。"));
    }

    // 定价：用户显式配置优先，缺省回退到内置模型数据库的默认价（Kivio 同款优先级）。
    let pricing = model
        .pricing
        .clone()
        .or_else(|| crate::ai::model::database_pricing(&model.api_model));
    // 视觉能力：用户覆盖 → 内置模型数据库 → 名称家族启发式；均未知时返回 None，由调用方放行。
    let supports_vision = model
        .capabilities
        .and_then(|capabilities| capabilities.vision)
        .or_else(|| crate::ai::model::resolve_supports_vision(&model.api_model));
    // Tool 能力：用户覆盖 → 数据库；未知模型保守关闭，由用户在中转商配置中显式开启。
    let supports_tools = model
        .capabilities
        .and_then(|capabilities| capabilities.function_calling)
        .or_else(|| crate::ai::model::database_supports_function_calling(&model.api_model))
        .unwrap_or(false);
    let supports_reasoning = model
        .capabilities
        .and_then(|capabilities| capabilities.reasoning)
        .or_else(|| crate::ai::model::database_supports_reasoning(&model.api_model));
    let context_window_tokens = model
        .context_window_tokens
        .or_else(|| crate::ai::model::database_context_window_tokens(&model.api_model));

    Ok(ResolvedTarget {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        model_id: model.id.clone(),
        provider_kind: provider.kind,
        protocol: provider.protocol,
        auth_scheme: provider.auth_scheme,
        base_url: provider.base_url.clone(),
        api_model: model.api_model.clone(),
        display_name: model.display_name.clone(),
        context_window_tokens,
        pricing,
        supports_vision,
        supports_tools,
        supports_reasoning,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::ai::{
        error::{ModelError, ModelErrorKind},
        types::{
            ModelImage, ModelMessage, ModelOptions, ModelRequest, ModelResponse, ModelRole,
            ModelTool, ModelToolCall, ModelUsage, UsageSource,
        },
    };
    use crate::chat::{
        agent::types::ToolQuestionAnswer,
        conversation_types::{AiPermissionMode, StoredChatAttachment},
        types::{ChatCompletionRequest, ChatModelMessage, ChatWorkspaceMode},
    };
    use crate::settings::types::{
        ModelCapabilities, ModelSettings, ProviderKind, ProviderModelConfig,
    };
    use crate::state::ToolInterruptResponse;
    use tokio::sync::oneshot;
    use tokio_util::sync::CancellationToken;

    fn push_model(settings: &mut ModelSettings, api_model: &str) {
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-1".to_string(),
            api_model: api_model.to_string(),
            display_name: api_model.to_string(),
            context_window_tokens: None,
            pricing: None,
            capabilities: None,
            enabled: true,
        });
    }

    fn tool_calls(name: &str, count: usize) -> Vec<ModelToolCall> {
        (0..count)
            .map(|index| ModelToolCall {
                id: format!("call-{index}"),
                name: name.to_string(),
                arguments: serde_json::json!({}),
                provider_signature: None,
            })
            .collect()
    }

    fn attachment(id: &str, kind: &str, name: &str, mime_type: &str) -> StoredChatAttachment {
        StoredChatAttachment {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            mime_type: mime_type.to_string(),
            size_bytes: 128,
            path: format!("{id}_{name}"),
            preview_path: None,
            width: (kind == "image").then_some(8),
            height: (kind == "image").then_some(6),
        }
    }

    fn attachment_request(messages: Vec<ChatModelMessage>) -> ChatCompletionRequest {
        ChatCompletionRequest {
            provider_id: "provider-1".to_string(),
            model_id: "model-1".to_string(),
            conversation_id: Some("conversation-1".to_string()),
            message_id: Some("message-1".to_string()),
            operation: Some("chatComplete".to_string()),
            system_prompt: String::new(),
            activated_skill_ids: Vec::new(),
            slash_skill_id: None,
            permission_mode: AiPermissionMode::AskSensitive,
            workspace_mode: ChatWorkspaceMode::Chat,
            workspace_context: None,
            messages,
            options: ModelOptions::default(),
        }
    }

    #[test]
    fn attachment_capability_gate_ignores_answered_history() {
        let request = attachment_request(vec![
            ChatModelMessage {
                role: ModelRole::User,
                content: "读取旧文档".to_string(),
                attachments: vec![attachment(
                    "old-document",
                    "file",
                    "old.pdf",
                    "application/pdf",
                )],
            },
            ChatModelMessage {
                role: ModelRole::Assistant,
                content: "旧回答".to_string(),
                attachments: Vec::new(),
            },
            ChatModelMessage {
                role: ModelRole::User,
                content: "新的纯文本问题".to_string(),
                attachments: Vec::new(),
            },
        ]);

        assert_eq!(
            super::unanswered_tail_attachment_flags(&request),
            (false, false)
        );
        super::validate_attachment_capabilities(&request, Some(false), false, "Text Model")
            .expect("answered historical attachments must not block a new text turn");
    }

    #[test]
    fn attachment_capability_gate_rejects_current_unsupported_inputs() {
        let document_request = attachment_request(vec![ChatModelMessage {
            role: ModelRole::User,
            content: "读取文档".to_string(),
            attachments: vec![attachment(
                "new-document",
                "file",
                "current.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            )],
        }]);
        let error = super::validate_attachment_capabilities(
            &document_request,
            Some(true),
            false,
            "Vision Only",
        )
        .expect_err("a model without tools must reject current document attachments");
        assert_eq!(error.kind, ModelErrorKind::InvalidConfiguration);
        assert!(error.message.contains("文档附件"));

        let image_request = attachment_request(vec![ChatModelMessage {
            role: ModelRole::User,
            content: "查看图片".to_string(),
            attachments: vec![attachment("new-image", "image", "current.png", "image/png")],
        }]);
        let error = super::validate_attachment_capabilities(
            &image_request,
            Some(false),
            true,
            "Document Only",
        )
        .expect_err("a model without vision must reject current image attachments");
        assert_eq!(error.kind, ModelErrorKind::InvalidConfiguration);
        assert!(error.message.contains("图片附件"));

        super::validate_attachment_capabilities(&image_request, None, false, "Unknown Vision")
            .expect("unknown vision follows the existing pass-through policy");
    }

    #[test]
    fn agent_round_budget_reserves_one_tool_free_final_call() {
        assert_eq!(super::agent_call_slots(20), 21);
        assert!((0..20).all(|index| !super::is_final_agent_call(index, 20)));
        assert!(super::is_final_agent_call(20, 20));
    }

    #[test]
    fn thirteenth_tool_in_one_round_is_rejected() {
        let error = super::evaluate_tool_batch(&tool_calls("memory_read", 13), 0)
            .expect_err("the thirteenth call must be rejected");
        assert!(error.message.contains("12"));
    }

    #[test]
    fn hundred_and_first_business_tool_forces_finalization() {
        let decision = super::evaluate_tool_batch(&tool_calls("memory_read", 1), 100).unwrap();
        assert_eq!(decision, super::ToolBatchBudget::Finalize);
    }

    #[test]
    fn tool_search_does_not_consume_the_business_tool_budget() {
        let decision = super::evaluate_tool_batch(&tool_calls("search_tools", 12), 100).unwrap();
        assert_eq!(
            decision,
            super::ToolBatchBudget::Execute {
                next_business_total: 100,
            }
        );
    }

    #[test]
    fn vision_resolves_from_database_when_no_override() {
        let mut settings = ModelSettings::default();
        push_model(&mut settings, "gpt-5.5");
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert_eq!(target.supports_vision, Some(true));
    }

    #[test]
    fn vision_override_wins_over_database() {
        let mut settings = ModelSettings::default();
        push_model(&mut settings, "gpt-5.5");
        settings.providers[0].models[0].capabilities = Some(ModelCapabilities {
            vision: Some(false),
            ..ModelCapabilities::default()
        });
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert_eq!(target.supports_vision, Some(false));
    }

    #[test]
    fn vision_unknown_model_stays_none() {
        let mut settings = ModelSettings::default();
        push_model(&mut settings, "totally-unknown-model-xyz");
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert_eq!(target.supports_vision, None);
    }

    #[test]
    fn tool_and_reasoning_capabilities_resolve_without_switching_models() {
        let mut settings = ModelSettings::default();
        push_model(&mut settings, "gpt-5.5");
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert!(target.supports_tools);
        assert_eq!(target.supports_reasoning, Some(true));

        settings.providers[0].models[0].capabilities = Some(ModelCapabilities {
            function_calling: Some(false),
            reasoning: Some(false),
            ..ModelCapabilities::default()
        });
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert!(!target.supports_tools);
        assert_eq!(target.supports_reasoning, Some(false));
        assert_eq!(target.api_model, "gpt-5.5");
    }

    #[test]
    fn unknown_models_require_explicit_tool_override() {
        let mut settings = ModelSettings::default();
        push_model(&mut settings, "unknown-relay-model");
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert!(!target.supports_tools);

        settings.providers[0].models[0].capabilities = Some(ModelCapabilities {
            function_calling: Some(true),
            ..ModelCapabilities::default()
        });
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert!(target.supports_tools);
    }

    #[test]
    fn pricing_falls_back_to_database_defaults() {
        let mut settings = ModelSettings::default();
        push_model(&mut settings, "gpt-5.5");
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        let pricing = target.pricing.expect("database pricing should apply");
        assert!(pricing.input_per_million.is_some());

        // 用户显式配置优先于数据库默认。
        settings.providers[0].models[0].pricing = Some(crate::settings::types::ModelPricing {
            input_per_million: Some(1.23),
            output_per_million: None,
            cache_read_per_million: None,
            cache_write_per_million: None,
            currency: "USD".to_string(),
        });
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert_eq!(
            target.pricing.and_then(|pricing| pricing.input_per_million),
            Some(1.23)
        );
    }

    #[test]
    fn resolves_model_only_within_requested_provider() {
        let mut settings = ModelSettings::default();
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-1".to_string(),
            api_model: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            context_window_tokens: Some(128_000),
            pricing: None,
            capabilities: None,
            enabled: true,
        });

        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        assert_eq!(target.api_model, "gpt-test");
        assert!(super::resolve_target(&settings, "official-anthropic", "model-1").is_err());
    }

    #[test]
    fn rejects_disabled_provider_or_model() {
        let mut settings = ModelSettings::default();
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-1".to_string(),
            api_model: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            context_window_tokens: Some(128_000),
            pricing: None,
            capabilities: None,
            enabled: false,
        });
        assert!(super::resolve_target(&settings, "official-openai", "model-1").is_err());

        settings.providers[0].models[0].enabled = true;
        settings.providers[0].enabled = false;
        assert!(super::resolve_target(&settings, "official-openai", "model-1").is_err());
    }

    fn model_error(kind: ModelErrorKind, retry_after_ms: Option<u64>) -> ModelError {
        ModelError {
            kind,
            message: "test".to_string(),
            status_code: None,
            provider_code: None,
            retry_after_ms,
        }
    }

    #[test]
    fn retry_delay_uses_a_meaningful_base_for_rate_limiting() {
        // 修复前 RateLimited 落在 `_` 分支上，基数只有 300ms，等于把限流打成雪崩。
        let delay =
            super::retry_delay_with_jitter(&model_error(ModelErrorKind::RateLimited, None), 0, 0.0);
        assert!(
            delay >= Duration::from_millis(4_000),
            "限流退避不应短于 4s，实际 {delay:?}"
        );
        let quota = super::retry_delay_with_jitter(
            &model_error(ModelErrorKind::QuotaExceeded, None),
            0,
            0.0,
        );
        assert!(quota >= Duration::from_millis(7_000));
    }

    #[test]
    fn retry_delay_treats_both_timeout_kinds_the_same() {
        // ClientTimeout 与 UpstreamTimeout 是同一物理原因，退避量级必须一致。
        let client = super::retry_delay_with_jitter(
            &model_error(ModelErrorKind::ClientTimeout, None),
            0,
            0.5,
        );
        let upstream = super::retry_delay_with_jitter(
            &model_error(ModelErrorKind::UpstreamTimeout, None),
            0,
            0.5,
        );
        assert_eq!(client, upstream);
    }

    #[test]
    fn retry_delay_grows_exponentially_and_stays_clamped() {
        let error = model_error(ModelErrorKind::Connection, None);
        let first = super::retry_delay_with_jitter(&error, 0, 0.5);
        let later = super::retry_delay_with_jitter(&error, 3, 0.5);
        assert!(
            later > first,
            "退避应随重试次数增长：{first:?} -> {later:?}"
        );
        // 指数增长在 retry_index=4 处封顶，再叠加抖动也不得越过 120s 上限。
        let saturated = super::retry_delay_with_jitter(
            &model_error(ModelErrorKind::ConcurrencyLimited, None),
            9,
            1.0,
        );
        assert_eq!(saturated, Duration::from_millis(120_000));
        let floored = super::retry_delay_with_jitter(
            &model_error(ModelErrorKind::Connection, Some(10)),
            0,
            0.0,
        );
        assert_eq!(floored, Duration::from_millis(100));
    }

    #[test]
    fn retry_delay_applies_jitter_around_retry_after() {
        // 上游给了 Retry-After 就尊重它的量级，但仍要抖动，避免并发请求同刻齐发。
        let error = model_error(ModelErrorKind::RateLimited, Some(4_000));
        let low = super::retry_delay_with_jitter(&error, 0, 0.0);
        let mid = super::retry_delay_with_jitter(&error, 0, 0.5);
        let high = super::retry_delay_with_jitter(&error, 0, 1.0);
        assert_eq!(low, Duration::from_millis(3_000));
        assert_eq!(mid, Duration::from_millis(4_000));
        assert_eq!(high, Duration::from_millis(5_000));
        assert!(low < mid && mid < high, "抖动应当单调映射到时延区间");
    }

    #[test]
    fn jitter_ratio_stays_within_the_unit_interval() {
        for _ in 0..256 {
            let ratio = super::jitter_ratio();
            assert!((0.0..1.0).contains(&ratio), "抖动比例越界：{ratio}");
        }
    }

    #[test]
    fn retries_only_transient_model_errors() {
        let transient = ModelError {
            kind: ModelErrorKind::RateLimited,
            message: "retry".to_string(),
            status_code: Some(429),
            provider_code: None,
            retry_after_ms: None,
        };
        let permanent = ModelError {
            kind: ModelErrorKind::Authentication,
            message: "stop".to_string(),
            status_code: Some(401),
            provider_code: None,
            retry_after_ms: None,
        };
        assert!(super::should_retry(&transient));
        assert!(!super::should_retry(&permanent));
    }

    #[test]
    fn streaming_falls_back_only_on_stream_shaped_failures() {
        // 网关对 `stream: true` 回 400/422 → InvalidConfiguration；
        // 回 200 但 body 不是 SSE → 分帧/解析失败 → InvalidResponse。
        for kind in [
            ModelErrorKind::InvalidResponse,
            ModelErrorKind::InvalidConfiguration,
        ] {
            assert!(super::should_fall_back_from_streaming(&model_error(
                kind, None
            )));
        }
    }

    #[test]
    fn streaming_does_not_fall_back_on_timeouts() {
        // 流式本来就是为了避免超时。流式都超时了，非流式只会更早死；回落只会把
        // 一次失败变成两次，并且与 P0-6 统一好的超时语义冲突。
        for kind in [
            ModelErrorKind::ClientTimeout,
            ModelErrorKind::UpstreamTimeout,
        ] {
            assert!(!super::should_fall_back_from_streaming(&model_error(
                kind, None
            )));
        }
    }

    #[test]
    fn streaming_does_not_fall_back_on_transport_independent_failures() {
        for kind in [
            ModelErrorKind::Connection,
            ModelErrorKind::RateLimited,
            ModelErrorKind::ConcurrencyLimited,
            ModelErrorKind::Authentication,
            ModelErrorKind::QuotaExceeded,
            ModelErrorKind::ContextLengthExceeded,
            ModelErrorKind::ContentFiltered,
            ModelErrorKind::ProviderUnavailable,
            ModelErrorKind::ModelNotFound,
            ModelErrorKind::Provider,
            // 取消是用户意图，绝不能偷偷再发一次非流式请求。
            ModelErrorKind::Cancelled,
        ] {
            assert!(
                !super::should_fall_back_from_streaming(&model_error(kind, None)),
                "{kind:?} 不应触发流式回落"
            );
        }
    }

    #[test]
    fn oversized_requests_are_intercepted_before_sending() {
        let error = super::enforce_request_byte_limit(2_048, 1_024)
            .expect_err("超限请求必须在发出前被拦下");
        // kind 必须是 ContextLengthExceeded：调用方靠它触发缩小载荷，
        // 换成别的 kind 这道闸就从「自动降级」退化成「任务失败」。
        assert_eq!(error.kind, ModelErrorKind::ContextLengthExceeded);
        assert!(
            error.message.contains("2048") && error.message.contains("1024"),
            "错误信息要同时给出实际值和上限，否则无法判断该缩多少：{}",
            error.message
        );
    }

    #[test]
    fn requests_at_or_below_the_limit_pass_the_gate() {
        assert!(super::enforce_request_byte_limit(1_024, 1_024).is_ok());
        assert!(super::enforce_request_byte_limit(0, 1_024).is_ok());
        assert!(super::enforce_request_byte_limit(1_023, 1_024).is_ok());
    }

    #[test]
    fn plain_execution_sets_no_byte_gate() {
        let cancellation = CancellationToken::new();
        let execution = super::CompleteExecution::plain(&cancellation);
        // 只有深度笔记设闸。普通聊天不设：聊天的载荷由用户直接可见地控制，
        // 在这里拦一刀只会变成一个无法自愈的发送失败。
        assert!(execution.max_request_bytes.is_none());
        assert!(!execution.prefer_streaming);
    }

    #[test]
    fn context_budget_counts_tool_contracts_and_preserves_error_kind() {
        let mut settings = ModelSettings::default();
        push_model(&mut settings, "unknown-budget-model");
        settings.providers[0].models[0].context_window_tokens = Some(5_000);
        let target = super::resolve_target(&settings, "official-openai", "model-1").unwrap();
        let mut request = ModelRequest {
            model: "unknown-budget-model".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: "ok".to_string(),
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions {
                max_output_tokens: Some(512),
                ..ModelOptions::default()
            },
            tools: Vec::new(),
        };
        super::validate_context_budget(&request, &target)
            .expect("the small request should fit before tools are disclosed");

        request.tools.push(ModelTool {
            name: "large_tool".to_string(),
            description: "x".repeat(2_000),
            input_schema: serde_json::json!({"type": "object"}),
        });
        let error = super::validate_context_budget(&request, &target)
            .expect_err("the disclosed tool contract must count toward the hard budget");
        assert_eq!(error.kind, ModelErrorKind::ContextLengthExceeded);
        assert!(error.message.contains("不会自动切换模型"));
    }

    #[test]
    fn token_estimator_counts_ascii_and_non_ascii_conservatively() {
        assert_eq!(super::estimate_model_text_tokens("abcdefgh中文"), 4);

        let mut request = ModelRequest {
            model: "test".to_string(),
            system_prompt: None,
            messages: vec![ModelMessage {
                role: ModelRole::User,
                content: String::new(),
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_result: None,
            }],
            options: ModelOptions::default(),
            tools: Vec::new(),
        };
        let without_image = super::estimate_model_request_tokens(&request);
        request.messages[0].images.push(ModelImage {
            name: "image.png".to_string(),
            media_type: "image/png".to_string(),
            data_base64: "AA==".to_string(),
        });
        assert_eq!(
            super::estimate_model_request_tokens(&request),
            without_image + 1_200
        );
    }

    #[test]
    fn custom_provider_usage_is_marked_as_gateway_normalized() {
        let mut usage = Some(ModelUsage {
            input_tokens: Some(10),
            usage_source: UsageSource::ProviderReported,
            ..ModelUsage::default()
        });
        super::apply_usage_origin(&mut usage, ProviderKind::Custom);
        assert_eq!(usage.unwrap().usage_source, UsageSource::GatewayNormalized);
    }

    #[tokio::test]
    async fn tool_approval_rejection_and_cancellation_never_approve() {
        let (sender, receiver) = oneshot::channel();
        sender.send(ToolInterruptResponse::Approval(false)).unwrap();
        assert_eq!(
            super::wait_for_tool_approval(
                receiver,
                &CancellationToken::new(),
                Duration::from_secs(1),
            )
            .await
            .0,
            super::ToolApprovalOutcome::Rejected,
        );

        let (_sender, receiver) = oneshot::channel();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            super::wait_for_tool_approval(receiver, &cancellation, Duration::from_secs(1))
                .await
                .0,
            super::ToolApprovalOutcome::Cancelled,
        );
    }

    #[tokio::test]
    async fn tool_approval_timeout_rejects_and_releases_receiver() {
        let (_sender, receiver) = oneshot::channel();
        assert_eq!(
            super::wait_for_tool_approval(
                receiver,
                &CancellationToken::new(),
                Duration::from_millis(1),
            )
            .await
            .0,
            super::ToolApprovalOutcome::TimedOut,
        );
    }

    /// 不合规的提问载荷要退回普通工具路径，不能挂起等一个渲染不出来的弹窗。
    #[test]
    fn unrenderable_question_payloads_never_open_a_dialog() {
        let name = crate::chat::agent::catalog::ASK_USER_TOOL_NAME;
        let one_option = serde_json::json!({
            "questions": [{
                "question": "存到哪里？",
                "header": "存储位置",
                "multiSelect": false,
                "options": [{ "label": "默认目录", "description": "跟随系统" }],
            }],
        });
        assert!(super::parse_tool_questions(name, &one_option).is_none());

        let blank_header = serde_json::json!({
            "questions": [{
                "question": "存到哪里？",
                "header": "   ",
                "multiSelect": false,
                "options": [
                    { "label": "默认目录", "description": "跟随系统" },
                    { "label": "自定义目录", "description": "自己选" },
                ],
            }],
        });
        assert!(super::parse_tool_questions(name, &blank_header).is_none());

        assert!(
            super::parse_tool_questions(name, &serde_json::json!({ "questions": [] })).is_none()
        );
    }

    /// 换个工具名调同样的载荷，不该被当成提问。
    #[test]
    fn only_the_ask_user_tool_opens_a_question_dialog() {
        let payload = serde_json::json!({
            "questions": [{
                "question": "存到哪里？",
                "header": "存储位置",
                "multiSelect": false,
                "options": [
                    { "label": "默认目录", "description": "跟随系统" },
                    { "label": "自定义目录", "description": "自己选" },
                ],
            }],
        });
        assert!(super::parse_tool_questions("workspace_read", &payload).is_none());
        assert_eq!(
            super::parse_tool_questions(crate::chat::agent::catalog::ASK_USER_TOOL_NAME, &payload)
                .map(|questions| questions.len()),
            Some(1),
        );
    }

    /// 回答要原样送到执行层：丢了答案，模型就只知道「用户答了」而不知道答了什么。
    #[tokio::test]
    async fn tool_question_answers_reach_the_caller_intact() {
        let (sender, receiver) = oneshot::channel();
        sender
            .send(ToolInterruptResponse::Answers(vec![ToolQuestionAnswer {
                header: "存储位置".to_string(),
                values: vec!["自定义目录".to_string()],
            }]))
            .unwrap();
        let (outcome, answers) = super::wait_for_tool_approval(
            receiver,
            &CancellationToken::new(),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(outcome, super::ToolApprovalOutcome::Answered);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].values, vec!["自定义目录".to_string()]);
    }

    #[tokio::test]
    async fn completion_attempt_stops_immediately_after_cancellation() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result = super::completion_attempt(
            &cancellation,
            None,
            std::future::pending::<Result<ModelResponse, ModelError>>(),
        )
        .await;
        assert_eq!(result.unwrap_err().kind, ModelErrorKind::Cancelled);
    }

    #[tokio::test]
    async fn completion_attempt_enforces_the_per_request_timeout() {
        let result = super::completion_attempt(
            &CancellationToken::new(),
            Some(Duration::from_millis(1)),
            std::future::pending::<Result<ModelResponse, ModelError>>(),
        )
        .await;
        assert_eq!(result.unwrap_err().kind, ModelErrorKind::ClientTimeout);
    }
}
