import type { AiPermissionMode, ChatMessage } from "./chat";
import type { ReasoningEffort } from "../data/modelMatching";

/**
 * 会话领域类型。
 *
 * `ChatMessage` 是时间线和本地存储共用的消息实体；`Conversation` 保存完整消息，
 * `ConversationListItem` 只保存侧栏所需摘要。模型运行合同位于 `chat.ts`，未来的
 * 助手、项目和提示词类型分别位于各自文件。
 */

/** 包含完整消息内容的对话实体。 */
export interface Conversation {
  /** 对话唯一标识。 */
  id: string;
  /** 对话标题。 */
  title: string;
  /** 当前对话中的完整消息列表。 */
  messages: ChatMessage[];
  /** 当前对话绑定的助手 ID；为空表示使用普通 Chat。 */
  assistantId: string | null;
  /** 当前对话使用的供应商 ID，与 `modelId` 共同构成稳定模型身份。 */
  providerId: string | null;
  /** 当前对话使用的模型 ID；为空表示使用全局默认模型。 */
  modelId: string | null;
  /** 浼氳瘽绾у紑鍏筹細null 琛ㄧず璺熼殢鍏ㄥ眬璁剧疆銆?*/
  thinkingEnabled?: boolean | null;
  /** 浼氳瘽绾у己搴︼細null 琛ㄧず浣跨敤 Provider 榛樿銆?*/
  reasoningEffort?: ReasoningEffort | null;
  /**
   * 当前对话独有的自定义指令。
   * 这不是最终发送给模型的完整 System Prompt，后续还会与其他提示词来源组合。
   */
  systemPrompt: string;
  /** 已压缩历史的模型摘要；原消息仍保留用于界面查看。 */
  contextSummary: string;
  /** 该消息及其之前的历史已包含在 `contextSummary` 中。 */
  compressedUntilMessageId: string | null;
  /** 当前对话累计自动压缩次数。 */
  contextCompressionCount: number;
  /** 当前对话默认启用、可在发送时激活的 Skill ID。 */
  enabledSkillIds: string[];
  /** Work Chat 允许引用的文献范围；只保存 ID，不自动注入全文。 */
  linkedLibraryItemIds: string[];
  /** 当前对话执行工具时采用的权限模式。 */
  permissionMode: AiPermissionMode;
  /** 对话所属项目 ID；为空表示未加入项目。 */
  projectId: string | null;
  /** 对话所属集合 ID；为空表示未加入集合。 */
  collectionId: string | null;
  /** 宿主创建的隐藏来源类型；普通 Chat 为 null/undefined。 */
  sourceKind?: "localFiles" | null;
  /** 对话是否置顶。 */
  pinned: boolean;
  /** 对话创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 对话最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
}

/** 侧边栏使用的轻量对话摘要，不加载完整消息内容。 */
export interface ConversationListItem {
  /** 对话唯一标识。 */
  id: string;
  /** 对话标题。 */
  title: string;
  /** 最后一条有效消息的文本预览。 */
  preview: string;
  /** 对话包含的消息数量。 */
  messageCount: number;
  /** 对话绑定的助手 ID。 */
  assistantId: string | null;
  /** 对话当前使用的供应商 ID。 */
  providerId: string | null;
  /** 对话当前使用的模型 ID。 */
  modelId: string | null;
  /** 对话所属项目 ID。 */
  projectId: string | null;
  /** 对话所属集合 ID。 */
  collectionId: string | null;
  /** 隐藏来源会话不会进入普通侧栏分页。 */
  sourceKind?: "localFiles" | null;
  /** 对话是否置顶。 */
  pinned: boolean;
  /** 对话创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 对话最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
}

/** Rust 端对轻量会话索引执行分页后的结果。 */
export interface ConversationListPage {
  /** 当前页的轻量会话摘要。 */
  items: ConversationListItem[];
  /** 当前页在排序后索引中的起始位置。 */
  offset: number;
  /** 本地索引中的会话总数。 */
  total: number;
  /** 是否还可以继续加载下一页。 */
  hasMore: boolean;
}
