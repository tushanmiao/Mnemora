/**
 * 模型设置数据结构
 *
 * 层次关系：
 * - `ModelSettings`：模型设置根对象，保存供应商列表和全局默认模型。
 * - `ProviderConfig`：一个独立供应商实例，可以是官方服务或任意中转站。
 * - `ProviderModelConfig`：供应商下的一条模型映射，区分 API Model 与 Display Name。
 * - `ApiProtocol`：实际发送请求时采用的网络协议，与供应商身份相互独立。
 *
 * 用户选择仍使用 `providerId + modelId`；运行时容量状态还会加入 endpoint、协议、
 * 凭据代际与传输方式，避免中转站配置变化后错误继承旧包线。
 */

/** 用户对供应商来源的分类，不决定实际网络协议。 */
export type ProviderKind = "openai" | "anthropic" | "gemini" | "custom";

/** 第一版支持的四种模型 API 协议。 */
export type ApiProtocol =
  | "openAiChatCompletions"
  | "openAiResponses"
  | "anthropicMessages"
  | "geminiGenerateContent";

/** API Key 的传递方式；`protocolDefault` 表示使用协议默认认证方式。 */
export type AuthScheme =
  | "protocolDefault"
  | "bearer"
  | "xApiKey"
  | "xGoogApiKey";

/** 本地成本计算使用的每百万 Token 价格。当前只支持 USD。 */
export interface ModelPricing {
  inputPerMillion?: number;
  outputPerMillion?: number;
  cacheReadPerMillion?: number;
  cacheWritePerMillion?: number;
  currency: "USD";
}

/**
 * 用户对模型能力的显式覆盖；字段缺省表示"跟随内置模型数据库的默认判断"。
 * 主要服务于中转站上改了名、数据库匹配不到的模型。
 */
export interface ModelCapabilities {
  /** 是否支持图片输入（视觉）。false 时发送图片会在请求前被拦截。 */
  vision?: boolean;
  /** 是否支持结构化函数/工具调用。未知时按不支持处理，避免试探性执行。 */
  functionCalling?: boolean;
  /** 是否支持独立 reasoning/thinking 输出。 */
  reasoning?: boolean;
  /** 是否支持文本 embedding；缺省时跟随内置模型数据库。 */
  embedding?: boolean;
  /** 是否支持流式响应；保留为模型能力快照。 */
  streaming?: boolean;
  /** 当前模型与供应商组合是否支持原生 Tool Search。 */
  nativeToolSearch?: boolean;
  /** 当前模型与供应商组合是否支持原生上下文压缩。 */
  nativeCompaction?: boolean;
}

/** 一个供应商下的模型映射。 */
export interface ProviderModelConfig {
  /** Mnemora 内部使用的稳定模型 ID。 */
  id: string;
  /** 实际发送给远程 API 的模型名称，例如 `deepseek-v4`。 */
  apiModel: string;
  /** 用户在界面中看到的名称，可以自由修改。 */
  displayName: string;
  /** 模型上下文窗口大小，用于计算当前对话的上下文占用。 */
  contextWindowTokens: number | null;
  /** 可选价格；每条用量记录会保存当时的快照，后续修改不会重算历史。 */
  pricing?: ModelPricing;
  /** 能力覆盖；缺省时跟随内置模型数据库。 */
  capabilities?: ModelCapabilities;
  /** 关闭后不出现在可选模型中，但配置仍然保留。 */
  enabled: boolean;
}

/** 一个可独立配置的官方服务或中转站。 */
export interface ProviderConfig {
  /** Mnemora 内部使用的稳定供应商 ID。 */
  id: string;
  /** 用户自定义的供应商显示名称。 */
  name: string;
  /** 供应商来源分类。 */
  kind: ProviderKind;
  /** 该供应商实际使用的 API 协议。 */
  protocol: ApiProtocol;
  /** API Key 的认证方式。 */
  authScheme: AuthScheme;
  /** API 服务基础地址，由 Rust adapter 追加对应协议路径。 */
  baseUrl: string;
  /** 非敏感凭据代际；由 Rust 在 API Key 写入或删除时递增。 */
  credentialRevision: number;
  /** 是否已经配置 API Key；普通配置读取不应返回完整 Key。 */
  hasApiKey: boolean;
  /** 关闭后该供应商及其模型不会出现在模型选择器中。 */
  enabled: boolean;
  /** 该供应商下已经添加的模型映射。 */
  models: ProviderModelConfig[];
}

/** 模型设置根对象。 */
export interface ModelSettings {
  /** 配置结构版本，由 Rust 负责迁移和校验。 */
  version: number;
  /** 所有官方服务和中转站实例。 */
  providers: ProviderConfig[];
  /** 全局默认供应商 ID；为空表示尚未选择默认模型。 */
  defaultProviderId: string | null;
  /** 全局默认模型 ID；必须属于 `defaultProviderId` 对应供应商。 */
  defaultModelId: string | null;
  /** 深度笔记专用模型；任一字段为空时跟随当前 Chat 模型。 */
  noteProviderId: string | null;
  noteModelId: string | null;
}

/** 设置页提交给 Rust 的单向密钥变更；普通读取不会返回完整 API Key。 */
export type ProviderApiKeyUpdate =
  | { providerId: string; action: "set"; apiKey: string }
  | { providerId: string; action: "delete" };

export const CURRENT_MODEL_SETTINGS_VERSION = 7;

/** 创建首次启动时的三家官方供应商配置。 */
export function createInitialModelSettings(): ModelSettings {
  return {
    version: CURRENT_MODEL_SETTINGS_VERSION,
    providers: [
      {
        id: "official-openai",
        name: "OpenAI",
        kind: "openai",
        protocol: "openAiResponses",
        authScheme: "protocolDefault",
        baseUrl: "https://api.openai.com/v1",
        credentialRevision: 0,
        hasApiKey: false,
        enabled: true,
        models: [],
      },
      {
        id: "official-anthropic",
        name: "Anthropic",
        kind: "anthropic",
        protocol: "anthropicMessages",
        authScheme: "protocolDefault",
        baseUrl: "https://api.anthropic.com/v1",
        credentialRevision: 0,
        hasApiKey: false,
        enabled: true,
        models: [],
      },
      {
        id: "official-gemini",
        name: "Gemini",
        kind: "gemini",
        protocol: "geminiGenerateContent",
        authScheme: "protocolDefault",
        baseUrl: "https://generativelanguage.googleapis.com/v1beta",
        credentialRevision: 0,
        hasApiKey: false,
        enabled: true,
        models: [],
      },
    ],
    defaultProviderId: null,
    defaultModelId: null,
    noteProviderId: null,
    noteModelId: null,
  };
}

export function resolveNoteModel(
  settings: ModelSettings,
  conversationProviderId: string | null,
  conversationModelId: string | null,
) {
  if (settings.noteProviderId && settings.noteModelId) {
    const provider = settings.providers.find(
      (item) => item.enabled && item.id === settings.noteProviderId,
    );
    const model = provider?.models.find(
      (item) => item.enabled && item.id === settings.noteModelId,
    );
    if (provider && model) return { provider, model };
  }
  return resolveConversationModel(settings, conversationProviderId, conversationModelId);
}

/** 查找当前全局默认模型，并确保供应商和模型仍然存在。 */
export function resolveDefaultModel(settings: ModelSettings) {
  const provider = settings.providers.find(
    (item) => item.id === settings.defaultProviderId,
  );
  const model = provider?.models.find(
    (item) => item.id === settings.defaultModelId,
  );

  return provider && model ? { provider, model } : null;
}

/**
 * 按对话记录的供应商/模型 ID 解析当前可用模型。
 * 回退顺序与聊天一致：精确匹配 → 仅按模型 ID 跨供应商查找 → 全局默认模型。
 */
export function resolveConversationModel(
  settings: ModelSettings,
  providerId: string | null,
  modelId: string | null,
) {
  if (providerId && modelId) {
    const provider = settings.providers.find(
      (item) => item.enabled && item.id === providerId,
    );
    const model = provider?.models.find(
      (item) => item.enabled && item.id === modelId,
    );
    if (provider && model) return { provider, model };
  } else if (modelId) {
    for (const provider of settings.providers) {
      if (!provider.enabled) continue;
      const model = provider.models.find(
        (item) => item.enabled && item.id === modelId,
      );
      if (model) return { provider, model };
    }
  }
  return resolveDefaultModel(settings);
}
