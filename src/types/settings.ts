/**
 * 模型设置数据结构
 *
 * 层次关系：
 * - `ModelSettings`：模型设置根对象，保存供应商列表和全局默认模型。
 * - `ProviderConfig`：一个独立供应商实例，可以是官方服务或任意中转站。
 * - `ProviderModelConfig`：供应商下的一条模型映射，区分 API Model 与 Display Name。
 * - `ApiProtocol`：实际发送请求时采用的网络协议，与供应商身份相互独立。
 *
 * 稳定身份始终使用 `providerId + modelId`。展示名称只用于界面，不能参与请求或数据关联。
 */

/** 用户对供应商来源的分类，不决定实际网络协议。 */
export type ProviderKind = "openai" | "anthropic" | "gemini" | "custom";

/** 第一版支持的三种模型 API 协议。 */
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

/** 一个供应商下的模型映射。 */
export interface ProviderModelConfig {
  /** Mnemora 内部使用的稳定模型 ID。 */
  id: string;
  /** 实际发送给远程 API 的模型名称，例如 `deepseek-v4`。 */
  apiModel: string;
  /** 用户在界面中看到的名称，可以自由修改。 */
  displayName: string;
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
  /** 是否已经配置 API Key；普通配置读取不应返回完整 Key。 */
  hasApiKey: boolean;
  /** 关闭后该供应商及其模型不会出现在模型选择器中。 */
  enabled: boolean;
  /** 该供应商下已经添加的模型映射。 */
  models: ProviderModelConfig[];
}

/** 模型设置根对象。 */
export interface ModelSettings {
  /** 所有官方服务和中转站实例。 */
  providers: ProviderConfig[];
  /** 全局默认供应商 ID；为空表示尚未选择默认模型。 */
  defaultProviderId: string | null;
  /** 全局默认模型 ID；必须属于 `defaultProviderId` 对应供应商。 */
  defaultModelId: string | null;
}

/**
 * 任务 8 的临时密钥草稿。
 *
 * 它与 `ProviderConfig` 分开，后续任务会由 Rust 系统凭据存储替代，普通设置 DTO
 * 始终只保留 `hasApiKey`。
 */
export type ProviderApiKeyDrafts = Record<string, string>;

/** 创建首次启动时的三家官方供应商配置。 */
export function createInitialModelSettings(): ModelSettings {
  return {
    providers: [
      {
        id: "official-openai",
        name: "OpenAI",
        kind: "openai",
        protocol: "openAiResponses",
        authScheme: "protocolDefault",
        baseUrl: "https://api.openai.com/v1",
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
        hasApiKey: false,
        enabled: true,
        models: [],
      },
    ],
    defaultProviderId: null,
    defaultModelId: null,
  };
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
