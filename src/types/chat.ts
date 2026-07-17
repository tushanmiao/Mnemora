/**
 * Chat 领域类型总览
 * =================
 *
 * 本文件集中定义 Chat 功能在“界面展示、数据存储、提示词组装、模型请求”几个层次中
 * 共同使用的基础类型。这里只描述数据结构，不负责请求模型、保存数据库或渲染界面。
 *
 * 一、基础枚举与状态类型
 * ----------------------
 * - `MessageRole`：聊天界面允许显示的消息角色，仅包含用户和助手。
 * - `ModelMessageRole`：模型运行层的角色，在界面角色基础上增加 `system` 和 `tool`。
 * - `MessageStatus`：一条聊天消息从等待、生成到完成或失败的状态。
 * - `AiPermissionMode`：AI 使用工具或执行敏感操作时的权限策略。
 * - `SystemPromptSource`：System Prompt 片段的来源类型。
 *
 * 二、聊天界面与持久化实体
 * ------------------------
 * - `ModelSnapshot`：保存助手回复生成时的模型身份，防止历史显示受模型配置变化影响。
 * - `ModelUsage`：记录一次助手回复的 Token、成本和耗时。
 * - `ChatMessage`：用户在时间线中看到的单条消息，可关联模型快照和用量。
 * - `Conversation`：完整对话实体，包含 `ChatMessage[]` 和本对话的运行配置。
 * - `ConversationListItem`：侧边栏使用的轻量摘要，不包含完整消息列表。
 *
 * 三、可复用配置与对话组织
 * ------------------------
 * - `AssistantProfile`：可复用的助手身份、提示词、默认模型和权限配置。
 * - `PromptTemplate`：插入用户输入框的普通提示词模板，不属于 System Prompt。
 * - `ChatProject`：组织对话和资料的项目，可提供项目级 System Prompt。
 * - `ConversationCollection`：仅用于整理对话的集合，不向模型注入内容。
 *
 * 四、System Prompt 组装层
 * -----------------------
 * - `SystemPromptSource`：标记提示词来自内置规则、全局设置、助手、项目或对话等位置。
 * - `SystemPromptSection`：一段带来源信息的提示词，便于按层组合、调试和审计。
 *
 * 五、模型运行层
 * --------------
 * - `ModelMessage`：发送到模型适配器前的供应商无关消息。
 * - `ModelRequest`：供应商无关的完整模型请求，包含最终 System Prompt 和消息历史。
 *
 * 六、导出的运行时常量
 * --------------------
 * - `AI_PERMISSION_LABELS`：把 `AiPermissionMode` 转换为界面显示的中文名称。
 *
 * 主要关系
 * --------
 *
 * `AssistantProfile`、`ChatProject`、`Conversation`
 *                + 全局设置、记忆、文档上下文、工具说明
 *                                  |
 *                                  v
 *                    `SystemPromptSection[]`
 *                                  |
 *                                  v
 *                       最终 `systemPrompt`
 *                                  |
 * `Conversation.messages` --------+--------> `ModelRequest`
 *        |                                      |
 *        v                                      v
 *  `ChatMessage[]`                        模型供应商适配器
 *
 * 需要特别区分：
 * - `ChatMessage` 服务于界面展示和本地持久化；`ModelMessage` 服务于模型请求。
 * - `Conversation` 是完整详情；`ConversationListItem` 是侧边栏摘要。
 * - `PromptTemplate` 生成普通用户输入；`SystemPromptSection` 参与系统提示词组装。
 * - `ConversationCollection` 只负责分类；`ChatProject` 还可以向模型提供项目上下文。
 */

/** 显示在聊天界面中的普通消息角色。 */
export type MessageRole = "user" | "assistant";

/**
 * 模型运行层使用的消息角色。
 * `system` 和 `tool` 属于内部上下文，不需要渲染成普通聊天气泡。
 */
export type ModelMessageRole = MessageRole | "system" | "tool";

/** 一条消息从创建到结束可能经历的状态。 */
export type MessageStatus =
  | "pending"
  | "streaming"
  | "completed"
  | "stopped"
  | "error";

/** AI 执行工具或敏感操作时采用的权限模式。 */
export type AiPermissionMode =
  | "askEveryTime"
  | "askSensitive"
  | "fullAccess";

/** 助手消息创建时使用的模型身份快照，避免模型改名或删除后历史信息丢失。 */
export interface ModelSnapshot {
  /** 模型在 Mnemora 中的唯一标识。 */
  id: string;
  /** 用户可见的模型名称。 */
  name: string;
  /** 模型所属供应商的唯一标识。 */
  providerId: string;
}

/**
 * 一次助手回复的供应商无关用量数据。
 *
 * 字段均为可选，因为不同供应商提供的 Token、缓存、成本和耗时信息并不完全一致。
 * 成本应在回复完成时计算并保存，避免模型价格变化后影响历史记录。
 */
export interface ModelUsage {
  /** 本次模型请求使用的输入 Token 数。 */
  inputTokens?: number;
  /** 本次模型回复产生的输出 Token 数。 */
  outputTokens?: number;
  /** 输入和输出 Token 总数。 */
  totalTokens?: number;
  /** 模型在推理或思考阶段使用的 Token 数。 */
  reasoningTokens?: number;
  /** 从 Prompt Cache 中读取的 Token 数。 */
  cacheReadTokens?: number;
  /** 本次请求新写入 Prompt Cache 的 Token 数。 */
  cacheWriteTokens?: number;
  /** 按请求发生时的模型价格计算出的成本。 */
  cost?: number;
  /** 从请求开始到收到第一个 Token 的耗时，单位为毫秒。 */
  timeToFirstTokenMs?: number;
  /** 从请求开始到回复完全结束的总耗时，单位为毫秒。 */
  totalDurationMs?: number;
}

/** 用户在聊天时间线中看到的一条消息。 */
export interface ChatMessage {
  /** 消息唯一标识，建议使用 `crypto.randomUUID()` 生成。 */
  id: string;
  /** 消息所属对话的唯一标识。 */
  conversationId: string;
  /** 消息发送者，只允许用户或助手。 */
  role: MessageRole;
  /** 消息的主要文本内容。 */
  content: string;
  /** 消息当前的生成或完成状态。 */
  status: MessageStatus;
  /** 消息创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 消息最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
  /** 生成该消息时实际使用的模型 ID，仅助手消息需要。 */
  modelId?: string;
  /** 生成该消息时的模型身份快照，仅助手消息需要。 */
  modelSnapshot?: ModelSnapshot;
  /** 生成该助手消息产生的 Token、成本和耗时信息。 */
  usage?: ModelUsage;
  /** 消息生成失败时提供给界面的错误说明。 */
  errorMessage?: string;
}

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
  /** 当前对话使用的模型 ID；为空表示使用全局默认模型。 */
  modelId: string | null;
  /**
   * 当前对话独有的自定义指令。
   * 这不是最终发送给模型的完整 System Prompt，后续还会与其他提示词来源组合。
   */
  systemPrompt: string;
  /** 当前对话执行工具时采用的权限模式。 */
  permissionMode: AiPermissionMode;
  /** 对话所属项目 ID；为空表示未加入项目。 */
  projectId: string | null;
  /** 对话所属集合 ID；为空表示未加入集合。 */
  collectionId: string | null;
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
  /** 对话当前使用的模型 ID。 */
  modelId: string | null;
  /** 对话所属项目 ID。 */
  projectId: string | null;
  /** 对话所属集合 ID。 */
  collectionId: string | null;
  /** 对话是否置顶。 */
  pinned: boolean;
  /** 对话创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 对话最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
}

/** 可在多个对话中重复使用的助手配置。 */
export interface AssistantProfile {
  /** 助手唯一标识。 */
  id: string;
  /** 助手名称。 */
  name: string;
  /** 助手用途和能力的说明。 */
  description: string;
  /** 控制助手身份、行为和回答方式的长期系统提示词。 */
  systemPrompt: string;
  /** 助手默认使用的模型 ID。 */
  modelId: string | null;
  /** 助手默认采用的权限模式。 */
  permissionMode: AiPermissionMode;
  /** 助手创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 助手最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
}

/** 可以快速插入用户输入框的常用提示词，不属于 System Prompt。 */
export interface PromptTemplate {
  /** 提示词模板唯一标识。 */
  id: string;
  /** 提示词模板名称。 */
  title: string;
  /** 插入用户输入框的完整文本。 */
  content: string;
  /** 模板创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 模板最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
}

/** 用于组织相关对话、文献和工作上下文的项目。 */
export interface ChatProject {
  /** 项目唯一标识。 */
  id: string;
  /** 项目名称。 */
  name: string;
  /** 项目目标或内容说明。 */
  description: string;
  /** 项目在界面中的标识颜色。 */
  color: string | null;
  /** 组装模型上下文时加入的项目级指令。 */
  systemPrompt: string;
  /** 项目创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 项目最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
}

/** 只用于整理对话的轻量集合，本身不向模型注入指令。 */
export interface ConversationCollection {
  /** 集合唯一标识。 */
  id: string;
  /** 集合名称。 */
  name: string;
  /** 集合在界面中的标识颜色。 */
  color: string | null;
  /** 集合创建时间，使用毫秒时间戳。 */
  createdAt: number;
  /** 集合最后更新时间，使用毫秒时间戳。 */
  updatedAt: number;
}

/** 最终 System Prompt 中每一段内容的来源。 */
export type SystemPromptSource =
  | "builtin"
  | "global"
  | "assistant"
  | "project"
  | "conversation"
  | "memory"
  | "documentContext"
  | "tools";

/** 用于组装最终 System Prompt 的一段可追踪内容。 */
export interface SystemPromptSection {
  /** 提示词片段唯一标识。 */
  id: string;
  /** 提示词片段的来源类型。 */
  source: SystemPromptSource;
  /** 提示词片段的可读标题，便于调试和审计。 */
  title: string;
  /** 提示词片段的实际文本。 */
  content: string;
}

/** 发送给模型运行层的一条标准化消息。 */
export interface ModelMessage {
  /** 模型消息角色，可以包含内部的 system 和 tool。 */
  role: ModelMessageRole;
  /** 当前模型消息的文本内容。 */
  content: string;
}

/** 转换成 OpenAI、Anthropic 或 Gemini 请求前的供应商无关请求。 */
export interface ModelRequest {
  /** 本次请求使用的模型 ID。 */
  modelId: string;
  /** 将所有提示词片段组合后得到的最终 System Prompt。 */
  systemPrompt: string;
  /** 参与组装最终 System Prompt 的原始片段，便于调试和审计。 */
  systemPromptSections: SystemPromptSection[];
  /** 发送给模型的标准化消息历史。 */
  messages: ModelMessage[];
  /** 是否要求供应商使用流式输出。 */
  stream: boolean;
}

/** AI 权限模式与中文显示名称的对应关系。 */
export const AI_PERMISSION_LABELS: Record<AiPermissionMode, string> = {
  askEveryTime: "每次确认",
  askSensitive: "敏感确认",
  fullAccess: "完全访问",
};
