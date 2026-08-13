//! Chat 请求服务。
//!
//! 非流式和流式调用共享目标解析与系统凭据读取。流式调用额外负责运行注册、Channel 事件
//! 和真实网络取消；设置锁不会跨网络请求持有，活动运行结束后一定从注册表移除。

use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::{
    ai::{
        dispatcher,
        error::{ModelError, ModelErrorKind},
        stream,
        types::{
            ModelMessage, ModelRequest, ModelResponse, ModelRole, ModelStreamChunk,
            ModelStreamOutcome, ModelToolCall, ModelToolResult, ModelUsage, ProviderRequestContext,
        },
    },
    chat::agent::{self, SkillRunCache, ToolRuntimeContext, ToolTraceSnapshot, ToolTraceStatus},
    request_debug::{self, RequestDebugRecordInput, RequestDebugRequest, RequestDebugResponse},
    settings::types::{ApiProtocol, AuthScheme, ModelPricing, ModelSettings, ProviderKind},
    state::AppState,
    usage::{self, UsageRecordInput},
};

use super::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatStreamRequest, ModelStreamEvent,
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

pub async fn complete(
    state: &AppState,
    request: ChatCompletionRequest,
) -> Result<ChatCompletionResponse, ModelError> {
    request.validate()?;
    let conversation_id = request.conversation_id.clone();
    let message_id = request.message_id.clone();
    let operation = request
        .operation
        .clone()
        .unwrap_or_else(|| "chatComplete".to_string());
    let prepared = prepare_call(state, request).await?;
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
    let prepared = prepare_call(state, request.completion).await?;
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

    {
        let mut active_runs = state.active_chat_runs.lock().await;
        if active_runs.contains_key(&run_id) {
            return Err(ModelError::invalid_configuration(
                "相同 Run ID 的流式请求已经存在。",
            ));
        }
        active_runs.insert(run_id.clone(), cancellation.clone());
    }

    if let Err(error) = on_event.send(ModelStreamEvent::Started {
        run_id: run_id.clone(),
        conversation_id: conversation_id.clone(),
        message_id: message_id.clone(),
    }) {
        state.active_chat_runs.lock().await.remove(&run_id);
        return Err(ModelError::provider(format!(
            "无法发送流式开始事件：{error}"
        )));
    }

    let created_at_ms = usage::now_ms();
    let started_at = Instant::now();
    let mut response_preview = String::new();
    let mut reasoning_preview = String::new();
    let result = run_agent_stream(
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
    state.active_chat_runs.lock().await.remove(&run_id);
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
    cancellation: &CancellationToken,
    calls: &[ModelToolCall],
) -> Vec<Option<ParallelToolExecution>> {
    let mut pending = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| {
            agent::parallel_safe(&call.name)
                && !agent::requires_approval(context.permission_mode, call)
        })
        .map(|(index, call)| (index, call.clone()));
    let mut results = vec![None; calls.len()];
    let mut tasks = tokio::task::JoinSet::new();
    let spawn = |tasks: &mut tokio::task::JoinSet<_>, index: usize, call: ModelToolCall| {
        let context = context.clone();
        let conversations = state.conversation_repository.clone();
        let skills = state.skill_repository.clone();
        let memory = state.memory_repository.clone();
        let cancellation = cancellation.clone();
        tasks.spawn(async move {
            let started = Instant::now();
            let mut skill_cache = SkillRunCache::default();
            let result = agent::execute_tool(
                &call,
                &context,
                &conversations,
                &skills,
                &memory,
                &mut skill_cache,
                &cancellation,
            )
            .await;
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
        if let Ok((index, result, duration_ms)) = joined {
            let execution = result.unwrap_or_else(|error| agent::ToolExecution {
                output_chars: error.message.chars().count(),
                content: error.message.clone(),
                preview: error.message,
                is_error: true,
                activated_skill_id: None,
                output_truncated: false,
            });
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
) -> Result<AgentCompleteResult, ModelError> {
    if tool_context.permission_mode
        == crate::chat::conversation_types::AiPermissionMode::AskEveryTime
    {
        request.tools.retain(|tool| {
            matches!(
                tool.name.as_str(),
                "skill" | "search_tools" | "search_skills"
            )
        });
    }
    let cancellation = CancellationToken::new();
    let run_id = uuid::Uuid::new_v4().to_string();
    let mut run_usage = ModelUsage {
        call_count: 0,
        ..ModelUsage::default()
    };
    let mut skill_cache = SkillRunCache::default();
    let mut activated_skill_ids = Vec::new();
    let mut tool_traces = Vec::new();
    let max_agent_rounds = agent_round_limit(state);
    let mut tool_call_total = 0usize;
    let mut force_final_answer = false;

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
        let created_at_ms = usage::now_ms();
        let started_at = Instant::now();
        let retry_policy = retry_policy(state);
        let mut retry_index = 0;
        let result = loop {
            match dispatcher::complete(&state.http, context, &request).await {
                Ok(response) => break Ok(response),
                Err(error) if retry_index < retry_policy.max_retries && should_retry(&error) => {
                    tokio::time::sleep(retry_delay(&error, retry_index)).await;
                    retry_index += 1;
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
        let mut parallel_results =
            execute_parallel_safe_tools(state, tool_context, &cancellation, &tool_calls).await;
        for (index, call) in tool_calls.into_iter().enumerate() {
            let result = if let Some(result) = parallel_results[index].take() {
                result
            } else {
                let started = Instant::now();
                let execution = if agent::requires_approval(tool_context.permission_mode, &call) {
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
                risk: agent::tool_risk(&call),
                argument_summary: agent::argument_summary(&call),
                preview: Some(execution.preview.clone()),
                duration_ms: Some(result.duration_ms),
                input_chars: Some(call.arguments.to_string().chars().count()),
                output_chars: Some(execution.output_chars),
                output_truncated: Some(execution.output_truncated),
                error_kind: execution.is_error.then_some("toolExecution".to_string()),
            });
            if let Some(skill_id) = execution.activated_skill_id.as_ref() {
                if !activated_skill_ids.contains(skill_id) {
                    activated_skill_ids.push(skill_id.clone());
                }
            }
            agent::apply_tool_disclosures(&mut request, &call, &execution);
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
                "skill" | "search_tools" | "search_skills"
            )
        });
    }
    let mut run_usage = ModelUsage {
        call_count: 0,
        ..ModelUsage::default()
    };
    let mut skill_cache = SkillRunCache::default();
    let mut activated_skill_ids = Vec::<String>::new();
    let max_agent_rounds = agent_round_limit(state);
    let mut tool_call_total = 0usize;
    let mut force_final_answer = false;

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
        let call_started_at_ms = usage::now_ms();
        let call_started = Instant::now();
        let mut round_text = String::new();
        let outcome = stream_inner(
            state,
            context,
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
            summary.usage = (run_usage.call_count > 0).then_some(run_usage);
            return Ok(ModelStreamOutcome::Completed(summary));
        }
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

        let tool_calls = summary.tool_calls;
        request.messages.push(ModelMessage {
            role: ModelRole::Assistant,
            content: round_text,
            images: Vec::new(),
            tool_calls: tool_calls.clone(),
            tool_result: None,
        });
        for call in tool_calls.iter().filter(|call| {
            agent::parallel_safe(&call.name)
                && !agent::requires_approval(tool_context.permission_mode, call)
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
                    risk: agent::tool_risk(call),
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
        let mut parallel_results =
            execute_parallel_safe_tools(state, tool_context, cancellation, &tool_calls).await;
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
                        risk: agent::tool_risk(&call),
                        argument_summary: agent::argument_summary(&call),
                        preview: Some(result.execution.preview.clone()),
                        duration_ms: Some(result.duration_ms),
                        input_chars: Some(call.arguments.to_string().chars().count()),
                        output_chars: Some(result.execution.output_chars),
                        output_truncated: Some(result.execution.output_truncated),
                        error_kind: result
                            .execution
                            .is_error
                            .then_some("toolExecution".to_string()),
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
            agent::apply_tool_disclosures(&mut request, &call, &result);
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
    let risk = agent::tool_risk(call);
    let argument_summary = agent::argument_summary(call);
    if agent::requires_approval(context.permission_mode, call) {
        let approval_id = uuid::Uuid::new_v4().to_string();
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
        let (sender, receiver) = oneshot::channel();
        state
            .pending_tool_approvals
            .lock()
            .await
            .insert(approval_id.clone(), sender);
        let sent = on_event.send(ModelStreamEvent::ToolApprovalRequested {
            run_id: run_id.to_string(),
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
            approval_id: approval_id.clone(),
            trace,
        });
        if sent.is_err() {
            state
                .pending_tool_approvals
                .lock()
                .await
                .remove(&approval_id);
            return rejected_tool("无法向界面发送工具审批请求。", call);
        }
        // 审批对象不能因前端失联永久留在 Rust 状态中。五分钟未响应按拒绝处理。
        let approved = wait_for_tool_approval(receiver, cancellation, TOOL_APPROVAL_TIMEOUT).await;
        state
            .pending_tool_approvals
            .lock()
            .await
            .remove(&approval_id);
        if !approved {
            let _ = emit_tool_trace(
                on_event,
                run_id,
                conversation_id,
                message_id,
                ToolTraceSnapshot {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    status: ToolTraceStatus::Rejected,
                    risk,
                    argument_summary,
                    preview: Some("用户拒绝了本次工具调用。".to_string()),
                    duration_ms: Some(0),
                    input_chars: Some(call.arguments.to_string().chars().count()),
                    output_chars: Some("用户拒绝了本次工具调用。".chars().count()),
                    output_truncated: Some(false),
                    error_kind: Some("approvalRejected".to_string()),
                },
            );
            return rejected_tool("用户拒绝了本次工具调用。", call);
        }
    }

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
        skill_cache,
        cancellation,
    )
    .await
    {
        Ok(result) => {
            let _ = emit_tool_trace(
                on_event,
                run_id,
                conversation_id,
                message_id,
                ToolTraceSnapshot {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    status: ToolTraceStatus::Completed,
                    risk,
                    argument_summary,
                    preview: Some(result.preview.clone()),
                    duration_ms: Some(elapsed_ms(started)),
                    input_chars: Some(call.arguments.to_string().chars().count()),
                    output_chars: Some(result.output_chars),
                    output_truncated: Some(result.output_truncated),
                    error_kind: result.is_error.then_some("toolExecution".to_string()),
                },
            );
            result
        }
        Err(error) => {
            let message = error.message.clone();
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

async fn wait_for_tool_approval(
    receiver: oneshot::Receiver<bool>,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> bool {
    tokio::select! {
        _ = cancellation.cancelled() => false,
        decision = tokio::time::timeout(timeout, receiver) => {
            decision.ok().and_then(Result::ok).unwrap_or(false)
        },
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
        match stream::stream(&state.http, context, request, cancellation, &mut emit).await {
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

async fn prepare_call(
    state: &AppState,
    mut request: ChatCompletionRequest,
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
    let tool_context = if use_agent_tools {
        agent::build_runtime_context(&request, &state.skill_repository, memory_settings)?
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
        agent::configure_model_request(&mut model_request, &tool_context, l1_memory.as_deref());
        append_agent_runtime_prompt(&mut model_request, agent_round_limit(state));
    }
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
    let active_runs = state.active_chat_runs.lock().await;
    let Some(cancellation) = active_runs.get(run_id.trim()) else {
        return Ok(false);
    };
    cancellation.cancel();
    Ok(true)
}

pub async fn resolve_tool_approval(
    state: &AppState,
    approval_id: &str,
    approved: bool,
) -> Result<bool, ModelError> {
    crate::settings::types::validate_stable_id("Approval ID", approval_id.trim())
        .map_err(ModelError::invalid_configuration)?;
    let sender = state
        .pending_tool_approvals
        .lock()
        .await
        .remove(approval_id.trim());
    Ok(sender.is_some_and(|sender| sender.send(approved).is_ok()))
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
    !matches!(name, "search_tools" | "search_skills")
}

fn should_retry(error: &ModelError) -> bool {
    matches!(
        error.kind,
        ModelErrorKind::RateLimited
            | ModelErrorKind::Timeout
            | ModelErrorKind::Connection
            | ModelErrorKind::Provider
    )
}

fn retry_delay(error: &ModelError, retry_index: u8) -> Duration {
    let exponential_ms = 300u64.saturating_mul(1u64 << retry_index.min(4));
    Duration::from_millis(
        error
            .retry_after_ms
            .unwrap_or(exponential_ms)
            .clamp(100, 5_000),
    )
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
        types::{ModelOptions, ModelRole, ModelToolCall, ModelUsage, UsageSource},
    };
    use crate::chat::{
        conversation_types::{AiPermissionMode, StoredChatAttachment},
        types::{ChatCompletionRequest, ChatModelMessage, ChatWorkspaceMode},
    };
    use crate::settings::types::{
        ModelCapabilities, ModelSettings, ProviderKind, ProviderModelConfig,
    };
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
        sender.send(false).unwrap();
        assert!(
            !super::wait_for_tool_approval(
                receiver,
                &CancellationToken::new(),
                Duration::from_secs(1),
            )
            .await
        );

        let (_sender, receiver) = oneshot::channel();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(
            !super::wait_for_tool_approval(receiver, &cancellation, Duration::from_secs(1)).await
        );
    }

    #[tokio::test]
    async fn tool_approval_timeout_rejects_and_releases_receiver() {
        let (_sender, receiver) = oneshot::channel();
        assert!(
            !super::wait_for_tool_approval(
                receiver,
                &CancellationToken::new(),
                Duration::from_millis(1),
            )
            .await
        );
    }
}
