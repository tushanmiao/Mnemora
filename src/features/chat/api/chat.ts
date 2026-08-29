import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { AiPermissionMode, MessageRole, ModelUsage, ToolTrace } from "../../../types/chat";
import type { ChatAttachment } from "../../../types/attachment";
import type { ReasoningEffort } from "../../../data/modelMatching";

/** React 交给 Rust 的最小非流式消息，不包含 API Model、Base URL 或 API Key。 */
export type ChatCompletionRequest = {
  providerId: string;
  modelId: string;
  conversationId?: string;
  messageId?: string;
  operation?: "chatComplete" | "contextCompression" | "noteSummary" | "deepNote" | "noteEdit";
  systemPrompt: string;
  activatedSkillIds?: string[];
  slashSkillId?: string;
  permissionMode?: AiPermissionMode;
  workspaceMode?: "chat" | "work" | "notes";
  workspaceContext?: {
    kind: "note";
    noteId: string;
    noteTitle: string;
    noteRevisionHash: string;
    noteSnapshot: string;
    sourcePdfId?: string;
    sourcePdfTitle?: string;
    sourcePageIndex?: number;
  };
  messages: Array<{
    role: MessageRole;
    content: string;
    attachments?: ChatAttachment[];
  }>;
  options?: {
    temperature?: number;
    maxOutputTokens?: number;
    thinkingEnabled?: boolean;
    reasoningEffort?: ReasoningEffort;
  };
};

export type ChatCompletionResponse = {
  text: string;
  reasoning?: string;
  finishReason?: string;
  usage?: ModelUsage;
  activatedSkillIds?: string[];
  toolTraces?: ToolTrace[];
};

export type ModelErrorKind =
  | "invalidConfiguration"
  | "missingApiKey"
  | "authentication"
  | "permissionDenied"
  | "rateLimited"
  | "modelNotFound"
  | "contextLengthExceeded"
  | "contentFiltered"
  | "timeout"
  | "connection"
  | "invalidResponse"
  | "provider";

export type ModelError = {
  kind: ModelErrorKind;
  message: string;
  statusCode?: number;
  providerCode?: string;
  retryAfterMs?: number;
};

export type ChatStreamRequest = {
  runId: string;
  conversationId: string;
  messageId: string;
  completion: ChatCompletionRequest;
};

export type AgentRunState =
  | "created"
  | "running"
  | "waiting"
  | "stopping"
  | "completed"
  | "stopped"
  | "failed"
  | "budgetExhausted";

export type AgentToolCallSnapshot = {
  callId: string;
  name: string;
  state: "proposed" | "awaitingApproval" | "approved" | "queued" | "running"
    | "completed" | "rejected" | "failed" | "cancelled" | "timedOut";
  stateVersion: number;
  executionVersion: number;
  approvalId: string | null;
  risk: string;
  source: Record<string, unknown>;
  catalogRevision: string;
  resultPreview: string;
  errorKind: string | null;
  expiresAt: number | null;
  updatedAt: number;
};

export type AgentRunSnapshot = {
  id: string;
  conversationId: string;
  messageId: string;
  state: AgentRunState;
  activity: string;
  stateVersion: number;
  executionVersion: number;
  runtimeInstanceId: string | null;
  modelId: string;
  errorCode: string | null;
  errorMessage: string | null;
  heartbeatAt: number | null;
  createdAt: number;
  updatedAt: number;
  finishedAt: number | null;
  toolCalls: AgentToolCallSnapshot[];
};

export type ModelStreamEvent =
  | {
      type: "started";
      runId: string;
      conversationId: string;
      messageId: string;
    }
  | {
      type: "textDelta";
      runId: string;
      conversationId: string;
      messageId: string;
      delta: string;
    }
  | {
      type: "reasoningDelta";
      runId: string;
      conversationId: string;
      messageId: string;
      delta: string;
    }
  | {
      type: "toolTrace";
      runId: string;
      conversationId: string;
      messageId: string;
      trace: ToolTrace;
    }
  | {
      type: "toolApprovalRequested";
      runId: string;
      conversationId: string;
      messageId: string;
      approvalId: string;
      trace: ToolTrace;
    }
  | {
      type: "skillActivated";
      runId: string;
      conversationId: string;
      messageId: string;
      skillId: string;
      name: string;
      version: string;
      contentHash: string;
    }
  | {
      type: "completed";
      runId: string;
      conversationId: string;
      messageId: string;
      finishReason?: string;
      usage?: ModelUsage;
    }
  | {
      type: "stopped";
      runId: string;
      conversationId: string;
      messageId: string;
    }
  | {
      type: "error";
      runId: string;
      conversationId: string;
      messageId: string;
      error: ModelError;
    };

/** 只在 Tauri 窗口中调用 Rust；普通浏览器预览不会向供应商发送请求。 */
export function completeChat(
  request: ChatCompletionRequest,
): Promise<ChatCompletionResponse> {
  if (!isTauri()) {
    return Promise.reject({
      kind: "invalidConfiguration",
      message: "真实模型请求需要在 Tauri 应用窗口中运行。",
    } satisfies ModelError);
  }
  return invoke<ChatCompletionResponse>("chat_complete", { request });
}

/** 启动真实流式请求；增量和终态只通过当前调用专属的 Channel 返回。 */
export function startChatStream(
  request: ChatStreamRequest,
  onEvent: (event: ModelStreamEvent) => void,
): Promise<void> {
  if (!isTauri()) {
    return Promise.reject({
      kind: "invalidConfiguration",
      message: "真实模型请求需要在 Tauri 应用窗口中运行。",
    } satisfies ModelError);
  }
  const channel = new Channel<ModelStreamEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("chat_stream_start", { request, onEvent: channel });
}

/** 手动停止指定 Run ID；返回 false 表示该运行已经自然结束。 */
export function cancelChatStream(runId: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("chat_stream_cancel", { runId });
}

/** 解析一次待处理工具审批；发送端只会消费第一项决定。 */
export function resolveToolApproval(approvalId: string, approved: boolean): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("chat_tool_approval_resolve", { approvalId, approved });
}

export function getAgentRunSnapshot(runId: string): Promise<AgentRunSnapshot | null> {
  if (!isTauri()) return Promise.resolve(null);
  return invoke<AgentRunSnapshot | null>("chat_agent_run_get", { runId });
}

/** 把 Tauri、JavaScript 或未知错误统一成界面可以安全显示的结构。 */
export function normalizeModelError(error: unknown): ModelError {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<ModelError>;
    if (typeof candidate.message === "string") {
      return {
        kind: candidate.kind ?? "provider",
        message: candidate.message,
        statusCode: candidate.statusCode,
        providerCode: candidate.providerCode,
        retryAfterMs: candidate.retryAfterMs,
      };
    }
  }

  if (error instanceof Error) {
    return { kind: "provider", message: error.message };
  }
  return {
    kind: "provider",
    message: typeof error === "string" ? error : "模型请求失败，请稍后重试。",
  };
}
