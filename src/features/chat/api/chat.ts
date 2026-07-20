import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import type { MessageRole, ModelUsage } from "../../../types/chat";

/** React 交给 Rust 的最小非流式消息，不包含 API Model、Base URL 或 API Key。 */
export type ChatCompletionRequest = {
  providerId: string;
  modelId: string;
  conversationId?: string;
  messageId?: string;
  operation?: "chatComplete" | "contextCompression";
  systemPrompt: string;
  messages: Array<{
    role: MessageRole;
    content: string;
  }>;
  options?: {
    temperature?: number;
    maxOutputTokens?: number;
    thinkingEnabled?: boolean;
  };
};

export type ChatCompletionResponse = {
  text: string;
  reasoning?: string;
  finishReason?: string;
  usage?: ModelUsage;
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
