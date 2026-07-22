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

const MAX_AGENT_ROUNDS: u32 = 8;
const MAX_TOOL_CALLS_PER_ROUND: usize = 8;
const MAX_PARALLEL_SAFE_TOOLS: usize = 4;
const TOOL_APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

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
                content: error.message.clone(),
                preview: error.message,
                is_error: true,
                activated_skill_id: None,
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
        request.tools.retain(|tool| tool.name == "skill");
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

    for round_index in 0..MAX_AGENT_ROUNDS {
        if round_index == MAX_AGENT_ROUNDS - 1 && !request.tools.is_empty() {
            request.tools.clear();
            request
                .system_prompt
                .get_or_insert_with(String::new)
                .push_str("\n\n工具轮数已用尽，请直接根据已有结果给出最终回答。");
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
        if response.tool_calls.len() > MAX_TOOL_CALLS_PER_ROUND {
            return Err(ModelError::invalid_configuration(format!(
                "单轮工具调用不能超过 {MAX_TOOL_CALLS_PER_ROUND} 个。"
            )));
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
                        content: error.message.clone(),
                        preview: error.message,
                        is_error: true,
                        activated_skill_id: None,
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
            });
            if let Some(skill_id) = execution.activated_skill_id.as_ref() {
                if !activated_skill_ids.contains(skill_id) {
                    activated_skill_ids.push(skill_id.clone());
                }
            }
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
    Err(ModelError::provider(
        "Agent 达到最大轮数，未能生成最终回答。",
    ))
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
) -> Result<ModelStreamOutcome, ModelError> {
    let mut run_usage = ModelUsage {
        call_count: 0,
        ..ModelUsage::default()
    };
    let mut skill_cache = SkillRunCache::default();
    let mut activated_skill_ids = Vec::<String>::new();

    for round_index in 0..MAX_AGENT_ROUNDS {
        if round_index == MAX_AGENT_ROUNDS - 1 && !request.tools.is_empty() {
            request.tools.clear();
            let prompt = request.system_prompt.get_or_insert_with(String::new);
            prompt.push_str(
                "\n\n本次 Agent 已达到工具轮数上限。不要再请求工具，请根据已有结果给出最终回答，并明确说明仍缺少的信息。",
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
        if summary.tool_calls.len() > MAX_TOOL_CALLS_PER_ROUND {
            return Err(ModelError::invalid_configuration(format!(
                "单轮工具调用不能超过 {MAX_TOOL_CALLS_PER_ROUND} 个。"
            )));
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
    Err(ModelError::provider(
        "Agent 达到最大轮数，未能生成最终回答。",
    ))
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
        content: message.to_string(),
        preview: message.to_string(),
        is_error: true,
        activated_skill_id: None,
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

async fn prepare_call(
    state: &AppState,
    request: ChatCompletionRequest,
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
    let use_agent_tools = request.operation.as_deref() != Some("contextCompression");
    let memory_settings = state
        .app_settings
        .read()
        .map_err(|_| ModelError::provider("应用设置暂时不可用，请重新启动应用后再试。"))?
        .memory;
    let tool_context =
        agent::build_runtime_context(&request, &state.skill_repository, memory_settings)?;
    let repository = state.conversation_repository.clone();
    let skill_repository = state.skill_repository.clone();
    let mut model_request = tauri::async_runtime::spawn_blocking(move || {
        request.into_model_request(api_model, &repository, &skill_repository)
    })
    .await
    .map_err(|error| ModelError::provider(format!("读取聊天附件任务失败：{error}")))??;
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
        pricing: model.pricing.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::ai::{
        error::{ModelError, ModelErrorKind},
        types::{ModelUsage, UsageSource},
    };
    use crate::settings::types::{ModelSettings, ProviderKind, ProviderModelConfig};
    use tokio::sync::oneshot;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn resolves_model_only_within_requested_provider() {
        let mut settings = ModelSettings::default();
        settings.providers[0].models.push(ProviderModelConfig {
            id: "model-1".to_string(),
            api_model: "gpt-test".to_string(),
            display_name: "GPT Test".to_string(),
            context_window_tokens: Some(128_000),
            pricing: None,
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
