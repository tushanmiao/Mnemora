import type { SystemPromptSection } from "./prompt";
import type { ChatAttachment } from "./attachment";
import type { AgentWorkflowSummary } from "./workflow";
import type { ApiProtocol } from "./modelSettings";

/** 显示在时间线中的普通消息角色。 */
export type MessageRole = "user" | "assistant";

/** 一条消息从创建到结束可能经历的状态。 */
export type MessageStatus = "pending" | "streaming" | "completed" | "stopped" | "error";

/** AI 执行工具或敏感操作时采用的权限模式。 */
export type AiPermissionMode = "askEveryTime" | "askSensitive" | "fullAccess";

/** 助手消息生成时使用的模型身份快照。 */
export interface ModelSnapshot {
  id: string;
  apiModel: string;
  displayName: string;
  providerId: string;
  providerName: string;
  protocol?: ApiProtocol;
}

/** 一条助手消息实际使用的 Skill 版本快照。 */
export interface ActivatedSkillSnapshot {
  id: string;
  name: string;
  version: string;
  contentHash: string;
  activation: "manual" | "slash" | "model";
}

/** 一次助手回复的供应商无关用量数据。 */
export interface ModelUsage {
  inputTokens?: number;
  nonCachedInputTokens?: number;
  contextInputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  reasoningTokens?: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
  usageSource?: "providerReported" | "gatewayNormalized" | "estimated" | "missing";
  costUsd?: number;
  costSource?: "providerReported" | "localCalculated" | "missing";
  pricingSnapshot?: {
    inputPerMillion?: number;
    outputPerMillion?: number;
    cacheReadPerMillion?: number;
    cacheWritePerMillion?: number;
    currency: string;
    capturedAtMs: number;
    settingsVersion: number;
  };
  timeToFirstTokenMs?: number;
  generationDurationMs?: number;
  outputTokensPerSecond?: number;
  totalDurationMs?: number;
  callCount?: number;
}

export type ToolTraceStatus = "awaitingApproval" | "running" | "completed" | "rejected" | "failed";

/** 助手消息只保存有界工具轨迹，不保存完整工具结果。 */
export interface ToolTrace {
  callId: string;
  name: string;
  status: ToolTraceStatus;
  risk:
    | "builtinRead"
    | "conversationRead"
    | "networkRead"
    | "memoryRead"
    | "memoryWrite"
    | "noteWrite";
  argumentSummary: string;
  preview?: string;
  durationMs?: number;
  inputChars?: number;
  outputChars?: number;
  outputTruncated?: boolean;
  errorKind?: string;
  /** 仅当前运行等待审批时存在，Rust 持久化会忽略该临时字段。 */
  approvalId?: string;
}

/** 用户明确从 Work 文献中加入本轮问题的结构化引用。 */
export interface LiteratureReference {
  /** 引用唯一标识，用于时间线渲染和待发送列表管理。 */
  id: string;
  /** 文献库中的稳定文献 ID。 */
  libraryItemId: string;
  /** 发送引用时的文献标题快照，避免后续改名影响历史记录。 */
  title: string;
  /** PDF 页索引，使用 0-based，与 PDF.js 和文献库批注保持一致。 */
  pageIndex: number;
  /** selection 表示用户选区，page 表示用户手动引用整页文本。 */
  kind: "selection" | "page";
  /** 用户明确加入上下文的有界文本，不包含整篇 PDF。 */
  text: string;
}

/** 用户从助手回答中选中的一条待发送引用。引用只存在于当前输入会话中。 */
export interface ChatQuote {
  id: string;
  text: string;
}

/** 用户从 Markdown 笔记中明确选中的结构化引用。 */
export interface NoteReference {
  id: string;
  noteId: string;
  noteTitle: string;
  /** 引用时的笔记更新时间快照，用于提示内容可能已经变化。 */
  revisionHash: string;
  startLine?: number;
  endLine?: number;
  selectedText: string;
}

/** 用户在聊天时间线中看到的一条消息。 */
export interface ChatMessage {
  id: string;
  conversationId: string;
  role: MessageRole;
  content: string;
  /** 用户随本轮消息添加的图片或本地文件安全副本。 */
  attachments?: ChatAttachment[];
  /** 用户明确加入本轮问题的 PDF 选区或单页引用。 */
  literatureReferences?: LiteratureReference[];
  /** 用户明确加入本轮问题的 Markdown 笔记选区。 */
  noteReferences?: NoteReference[];
  /** 模型返回的独立思考内容，不与最终回答混在一起。 */
  reasoning?: string;
  status: MessageStatus;
  createdAt: number;
  updatedAt: number;
  modelId?: string;
  modelSnapshot?: ModelSnapshot;
  usage?: ModelUsage;
  activatedSkills?: ActivatedSkillSnapshot[];
  toolTraces?: ToolTrace[];
  /** 新 Agent Runtime 的稳定身份；旧会话没有该字段。 */
  agentRunId?: string;
  /** 可随消息快速加载的有界投影，完整流程由兼容字段或事件存储恢复。 */
  workflowSummary?: AgentWorkflowSummary;
  errorMessage?: string;
}

/** 模型运行层允许的消息角色。 */
export type ModelMessageRole = "user" | "assistant" | "system" | "tool";

/** 发送给模型适配层的一条标准化消息。 */
export interface ModelMessage {
  role: ModelMessageRole;
  content: string;
  attachments?: ChatAttachment[];
}

/** 供应商无关的模型请求描述，供未来提示词调试和工具调用扩展使用。 */
export interface ModelRequest {
  providerId: string;
  modelId: string;
  systemPrompt: string;
  systemPromptSections: SystemPromptSection[];
  messages: ModelMessage[];
  stream: boolean;
}

export const AI_PERMISSION_LABELS: Record<AiPermissionMode, string> = {
  askEveryTime: "每次确认",
  askSensitive: "敏感确认",
  fullAccess: "完全访问",
};
