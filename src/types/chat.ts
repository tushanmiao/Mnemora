import type { SystemPromptSection } from "./prompt";
import type { ChatAttachment } from "./attachment";

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
}

/** 一次助手回复的供应商无关用量数据。 */
export interface ModelUsage {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  reasoningTokens?: number;
  cacheReadTokens?: number;
  cacheWriteTokens?: number;
  cost?: number;
  timeToFirstTokenMs?: number;
  totalDurationMs?: number;
}

/** 用户在聊天时间线中看到的一条消息。 */
export interface ChatMessage {
  id: string;
  conversationId: string;
  role: MessageRole;
  content: string;
  /** 用户随本轮消息添加的图片或本地文件安全副本。 */
  attachments?: ChatAttachment[];
  /** 模型返回的独立思考内容，不与最终回答混在一起。 */
  reasoning?: string;
  status: MessageStatus;
  createdAt: number;
  updatedAt: number;
  modelId?: string;
  modelSnapshot?: ModelSnapshot;
  usage?: ModelUsage;
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
