import {
  Eye,
  EyeOff,
  PlugZap,
  Plus,
  RefreshCw,
  Server,
  Star,
  Trash2,
} from "lucide-react";
import type {
  ApiProtocol,
  AuthScheme,
  ModelCapabilities,
  ProviderConfig,
  ProviderKind,
  ProviderModelConfig,
} from "../../../types/modelSettings";
import {
  matchModelDefaults,
  resolveSupportsFunctionCalling,
  resolveSupportsReasoning,
  resolveSupportsVision,
} from "../../../data/modelMatching";

const CAPABILITY_BADGES = [
  ["webSearch", "联网"],
  ["imageGeneration", "生图"],
  ["embedding", "嵌入"],
] as const;

type CapabilityOverrideKey = "vision" | "functionCalling" | "reasoning";

function capabilityPatch(
  model: ProviderModelConfig,
  key: CapabilityOverrideKey,
  value: string,
): Pick<ProviderModelConfig, "capabilities"> {
  const capabilities: ModelCapabilities = { ...(model.capabilities ?? {}) };
  if (value === "auto") delete capabilities[key];
  else capabilities[key] = value === "on";
  return {
    capabilities: Object.values(capabilities).every((item) => item === undefined)
      ? undefined
      : capabilities,
  };
}

const PROVIDER_KIND_LABELS: Record<ProviderKind, string> = {
  openai: "OpenAI 官方",
  anthropic: "Anthropic 官方",
  gemini: "Gemini 官方",
  custom: "自定义中转站",
};

const PROTOCOL_LABELS: Record<ApiProtocol, string> = {
  openAiChatCompletions: "OpenAI Chat Completions",
  openAiResponses: "OpenAI Responses",
  anthropicMessages: "Anthropic Messages",
  geminiGenerateContent: "Gemini GenerateContent",
};

const PROTOCOL_SHORT_LABELS: Record<ApiProtocol, string> = {
  openAiChatCompletions: "OpenAI Chat",
  openAiResponses: "OpenAI Responses",
  anthropicMessages: "Anthropic",
  geminiGenerateContent: "Gemini",
};

const AUTH_SCHEME_LABELS: Record<AuthScheme, string> = {
  protocolDefault: "跟随协议",
  bearer: "Bearer Token",
  xApiKey: "x-api-key",
  xGoogApiKey: "x-goog-api-key",
};

export type ProviderField = "name" | "baseUrl";
export type ProviderErrors = Partial<Record<ProviderField, string>>;

type ActionFeedback = { kind: "success" | "error"; message: string } | undefined;

type ProviderDetailProps = {
  provider: ProviderConfig | null;
  defaultProviderId: string | null;
  defaultModelId: string | null;
  providerErrors: ProviderErrors;
  modelErrors: Record<string, string>;
  secretDraft: string;
  pendingSecretDelete: boolean;
  apiKeyVisible: boolean;
  actionFeedback: ActionFeedback;
  availableModels: string[];
  newApiModel: string;
  newDisplayName: string;
  saving: boolean;
  testing: boolean;
  fetching: boolean;
  credentialStatus: string;
  hasEffectiveApiKey: boolean;
  onUpdateProvider: (patch: Partial<ProviderConfig>) => void;
  onClearProviderError: (field: ProviderField) => void;
  onDeleteProvider: () => void;
  onApiKeyChange: (value: string) => void;
  onDeleteApiKey: () => void;
  onToggleApiKeyVisibility: () => void;
  onFetchModels: () => void;
  onTestConnection: () => void;
  onSelectAvailableModel: (model: string) => void;
  onNewApiModelChange: (value: string) => void;
  onNewDisplayNameChange: (value: string) => void;
  onAddModel: () => void;
  onUpdateModel: (modelId: string, patch: Partial<ProviderModelConfig>) => void;
  onSetDefaultModel: (modelId: string) => void;
  onDeleteModel: (modelId: string) => void;
};

export function ProviderDetail(props: ProviderDetailProps) {
  const {
    provider,
    defaultProviderId,
    defaultModelId,
    providerErrors,
    modelErrors,
    secretDraft,
    pendingSecretDelete,
    apiKeyVisible,
    actionFeedback,
    availableModels,
    newApiModel,
    newDisplayName,
    saving,
    testing,
    fetching,
    credentialStatus,
    hasEffectiveApiKey,
    onUpdateProvider,
    onClearProviderError,
    onDeleteProvider,
    onApiKeyChange,
    onDeleteApiKey,
    onToggleApiKeyVisibility,
    onFetchModels,
    onTestConnection,
    onSelectAvailableModel,
    onNewApiModelChange,
    onNewDisplayNameChange,
    onAddModel,
    onUpdateModel,
    onSetDefaultModel,
    onDeleteModel,
  } = props;

  if (!provider) {
    return (
      <div className="provider-detail">
        <div className="provider-empty-state">
          <Server size={24} />
          <span>添加一个供应商</span>
        </div>
      </div>
    );
  }

  const networkBusy = saving || testing || fetching;

  return (
    <div className="provider-detail">
      <section className="provider-section provider-section-header">
        <div className="provider-title-block">
          <Server size={18} />
          <div>
            <h3>{provider.name || "未命名供应商"}</h3>
            <span>{PROTOCOL_SHORT_LABELS[provider.protocol]}</span>
          </div>
        </div>
        <div className="provider-header-actions">
          <label className="settings-switch-label">
            <input
              type="checkbox"
              checked={provider.enabled}
              onChange={(event) => onUpdateProvider({ enabled: event.target.checked })}
            />
            <span>启用</span>
          </label>
          <button
            className="settings-icon-danger"
            type="button"
            title="删除供应商"
            aria-label="删除供应商"
            onClick={onDeleteProvider}
          >
            <Trash2 size={16} />
          </button>
        </div>
      </section>

      <section className="provider-section">
        <h4>供应商</h4>
        <div className="settings-field-grid">
          <div className="settings-field">
            <label htmlFor={`provider-name-${provider.id}`}>显示名称</label>
            <input
              id={`provider-name-${provider.id}`}
              className={providerErrors.name ? "settings-input settings-input-error" : "settings-input"}
              value={provider.name}
              onChange={(event) => {
                onUpdateProvider({ name: event.target.value });
                onClearProviderError("name");
              }}
            />
            {providerErrors.name ? <span className="settings-field-error">{providerErrors.name}</span> : null}
          </div>
          <div className="settings-field">
            <label htmlFor={`provider-kind-${provider.id}`}>供应商类型</label>
            <select
              id={`provider-kind-${provider.id}`}
              className="settings-input settings-select"
              value={provider.kind}
              onChange={(event) => onUpdateProvider({ kind: event.target.value as ProviderKind })}
            >
              {Object.entries(PROVIDER_KIND_LABELS).map(([value, label]) => (
                <option value={value} key={value}>{label}</option>
              ))}
            </select>
          </div>
        </div>
      </section>

      <section className="provider-section">
        <h4>接口</h4>
        <div className="settings-field">
          <label htmlFor={`base-url-${provider.id}`}>API Base URL</label>
          <input
            id={`base-url-${provider.id}`}
            className={providerErrors.baseUrl ? "settings-input settings-input-error" : "settings-input"}
            type="url"
            inputMode="url"
            placeholder="https://api.example.com/v1"
            value={provider.baseUrl}
            spellCheck={false}
            onChange={(event) => {
              onUpdateProvider({ baseUrl: event.target.value });
              onClearProviderError("baseUrl");
            }}
          />
          {providerErrors.baseUrl ? <span className="settings-field-error">{providerErrors.baseUrl}</span> : null}
        </div>

        <div className="settings-field-grid">
          <div className="settings-field">
            <label htmlFor={`protocol-${provider.id}`}>API 协议</label>
            <select
              id={`protocol-${provider.id}`}
              className="settings-input settings-select"
              value={provider.protocol}
              onChange={(event) => onUpdateProvider({ protocol: event.target.value as ApiProtocol })}
            >
              {Object.entries(PROTOCOL_LABELS).map(([value, label]) => (
                <option value={value} key={value}>{label}</option>
              ))}
            </select>
          </div>
          <div className="settings-field">
            <label htmlFor={`auth-scheme-${provider.id}`}>认证方式</label>
            <select
              id={`auth-scheme-${provider.id}`}
              className="settings-input settings-select"
              value={provider.authScheme}
              onChange={(event) => onUpdateProvider({ authScheme: event.target.value as AuthScheme })}
            >
              {Object.entries(AUTH_SCHEME_LABELS).map(([value, label]) => (
                <option value={value} key={value}>{label}</option>
              ))}
            </select>
          </div>
        </div>

        <div className="settings-field">
          <div className="settings-label-row">
            <label htmlFor={`api-key-${provider.id}`}>API Key</label>
            <span>{credentialStatus}</span>
          </div>
          <div className="settings-secret-row">
            <div className="settings-secret-input">
              <input
                id={`api-key-${provider.id}`}
                className="settings-input"
                type={apiKeyVisible ? "text" : "password"}
                placeholder={pendingSecretDelete
                  ? "保存后删除"
                  : provider.hasApiKey
                    ? "已安全保存；输入新值可替换"
                    : "输入 API Key"}
                value={secretDraft}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => onApiKeyChange(event.target.value)}
              />
              <button
                className="settings-secret-toggle"
                type="button"
                title={apiKeyVisible ? "隐藏 API Key" : "显示 API Key"}
                aria-label={apiKeyVisible ? "隐藏 API Key" : "显示 API Key"}
                onClick={onToggleApiKeyVisibility}
              >
                {apiKeyVisible ? <EyeOff size={17} /> : <Eye size={17} />}
              </button>
            </div>
            {hasEffectiveApiKey ? (
              <button
                className="settings-icon-danger"
                type="button"
                title="删除 API Key"
                aria-label="删除 API Key"
                onClick={onDeleteApiKey}
              >
                <Trash2 size={15} />
              </button>
            ) : null}
          </div>
        </div>

        <div className="provider-network-actions">
          <div className="provider-action-status" aria-live="polite">
            {actionFeedback ? (
              <span className={`provider-action-${actionFeedback.kind}`}>{actionFeedback.message}</span>
            ) : null}
          </div>
          <button
            className="settings-button settings-button-secondary"
            type="button"
            disabled={networkBusy}
            onClick={onFetchModels}
          >
            <RefreshCw size={15} className={fetching ? "settings-spin" : ""} />
            <span>{fetching ? "获取中" : "获取模型"}</span>
          </button>
          <button
            className="settings-button settings-button-secondary"
            type="button"
            disabled={networkBusy}
            onClick={onTestConnection}
          >
            <PlugZap size={15} />
            <span>{testing ? "测试中" : "测试连接"}</span>
          </button>
        </div>
      </section>

      <section className="provider-section provider-model-section">
        <div className="provider-section-title-row">
          <h4>模型</h4>
          <span>{provider.models.length}</span>
        </div>

        {availableModels.length > 0 ? (
          <div className="settings-field discovered-model-field">
            <div className="settings-label-row">
              <label htmlFor={`available-model-${provider.id}`}>获取到的模型</label>
              <span>{availableModels.length}</span>
            </div>
            <select
              id={`available-model-${provider.id}`}
              className="settings-input settings-select"
              value=""
              onChange={(event) => onSelectAvailableModel(event.target.value)}
            >
              <option value="">选择模型</option>
              {availableModels.map((model) => <option value={model} key={model}>{model}</option>)}
            </select>
          </div>
        ) : null}

        <div className="model-add-row">
          <div className="settings-field">
            <label htmlFor={`new-api-model-${provider.id}`}>API Model</label>
            <input
              id={`new-api-model-${provider.id}`}
              className="settings-input"
              placeholder="deepseek-v4"
              value={newApiModel}
              spellCheck={false}
              onChange={(event) => onNewApiModelChange(event.target.value)}
            />
          </div>
          <div className="settings-field">
            <label htmlFor={`new-display-name-${provider.id}`}>Display Name</label>
            <input
              id={`new-display-name-${provider.id}`}
              className="settings-input"
              placeholder={newApiModel || "自定义显示名称"}
              value={newDisplayName}
              onChange={(event) => onNewDisplayNameChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key !== "Enter") return;
                event.preventDefault();
                onAddModel();
              }}
            />
          </div>
          <button className="model-add-button" type="button" title="添加模型" onClick={onAddModel}>
            <Plus size={17} />
          </button>
        </div>

        <div className="model-list" aria-label="模型映射">
          {provider.models.length === 0 ? (
            <div className="model-list-empty">尚未添加模型</div>
          ) : provider.models.map((model) => {
            const isDefault = defaultProviderId === provider.id && defaultModelId === model.id;
            return (
              <div className="model-row" key={model.id}>
                <label className="model-enabled-toggle" title={model.enabled ? "停用模型" : "启用模型"}>
                  <input
                    type="checkbox"
                    checked={model.enabled}
                    onChange={(event) => onUpdateModel(model.id, { enabled: event.target.checked })}
                  />
                </label>
                <div className="settings-field">
                  <label htmlFor={`api-model-${model.id}`}>API Model</label>
                  <input
                    id={`api-model-${model.id}`}
                    className={modelErrors[model.id] ? "settings-input settings-input-error" : "settings-input"}
                    value={model.apiModel}
                    spellCheck={false}
                    onChange={(event) => onUpdateModel(model.id, { apiModel: event.target.value })}
                  />
                  {modelErrors[model.id] ? (
                    <span className="settings-field-error">{modelErrors[model.id]}</span>
                  ) : null}
                </div>
                <div className="settings-field">
                  <label htmlFor={`display-name-${model.id}`}>Display Name</label>
                  <input
                    id={`display-name-${model.id}`}
                    className="settings-input"
                    value={model.displayName}
                    onChange={(event) => onUpdateModel(model.id, { displayName: event.target.value })}
                  />
                </div>
                <div className="settings-field">
                  <label htmlFor={`context-window-${model.id}`}>上下文窗口</label>
                  <input
                    id={`context-window-${model.id}`}
                    className="settings-input"
                    type="number"
                    min={1_024}
                    max={10_000_000}
                    step={1_024}
                    value={model.contextWindowTokens ?? ""}
                    placeholder="128000"
                    onChange={(event) => onUpdateModel(model.id, {
                      contextWindowTokens: event.target.value ? Number(event.target.value) : null,
                    })}
                  />
                </div>
                {(() => {
                  const capabilities = matchModelDefaults(model.apiModel)?.capabilities;
                  const vision = resolveSupportsVision(model.apiModel, model.capabilities?.vision);
                  const tools = resolveSupportsFunctionCalling(
                    model.apiModel,
                    model.capabilities?.functionCalling,
                  );
                  const reasoning = resolveSupportsReasoning(
                    model.apiModel,
                    model.capabilities?.reasoning,
                  );
                  const badges: Array<{ key: string; label: string; tone: "on" | "off" }> = [];
                  if (vision === true) badges.push({ key: "vision", label: "视觉", tone: "on" });
                  if (vision === false) badges.push({ key: "vision", label: "不支持图片", tone: "off" });
                  badges.push({
                    key: "functionCalling",
                    label: tools ? "工具" : "无工具",
                    tone: tools ? "on" : "off",
                  });
                  if (reasoning !== undefined) badges.push({
                    key: "reasoning",
                    label: reasoning ? "推理" : "无推理",
                    tone: reasoning ? "on" : "off",
                  });
                  for (const [key, label] of CAPABILITY_BADGES) {
                    if (capabilities?.[key]) badges.push({ key, label, tone: "on" });
                  }
                  return (
                    <div className="model-meta-row">
                      <div className="model-badges" aria-label="模型能力">
                        {badges.length > 0 ? (
                          badges.map((badge) => (
                            <span
                              className={badge.tone === "off"
                                ? "model-badge model-badge-off"
                                : "model-badge"}
                              key={badge.key}
                            >
                              {badge.label}
                            </span>
                          ))
                        ) : (
                          <span className="model-badge model-badge-unknown">能力未收录</span>
                        )}
                      </div>
                      <div className="model-capability-overrides" aria-label="模型能力覆盖">
                        {([
                          {
                            key: "vision",
                            label: "图片",
                            automatic: resolveSupportsVision(model.apiModel),
                          },
                          {
                            key: "functionCalling",
                            label: "工具",
                            automatic: resolveSupportsFunctionCalling(model.apiModel),
                          },
                          {
                            key: "reasoning",
                            label: "推理",
                            automatic: resolveSupportsReasoning(model.apiModel),
                          },
                        ] as const).map((capability) => (
                          <label
                            className="model-capability-override"
                            htmlFor={`${capability.key}-${model.id}`}
                            key={capability.key}
                          >
                            <span>{capability.label}</span>
                            <select
                              id={`${capability.key}-${model.id}`}
                              className="settings-input settings-select"
                              value={model.capabilities?.[capability.key] === undefined
                                ? "auto"
                                : model.capabilities[capability.key]
                                  ? "on"
                                  : "off"}
                              onChange={(event) => onUpdateModel(
                                model.id,
                                capabilityPatch(model, capability.key, event.target.value),
                              )}
                            >
                              <option value="auto">
                                {capability.automatic === true
                                  ? "自动：支持"
                                  : capability.automatic === false
                                    ? "自动：不支持"
                                    : "自动：未收录"}
                              </option>
                              <option value="on">支持</option>
                              <option value="off">不支持</option>
                            </select>
                          </label>
                        ))}
                      </div>
                    </div>
                  );
                })()}
                <details className="model-pricing">
                  <summary>用量价格（USD / 百万 Token）</summary>
                  <div className="model-pricing-grid">
                    {([
                      ["inputPerMillion", "普通输入"],
                      ["outputPerMillion", "输出"],
                      ["cacheReadPerMillion", "缓存读取"],
                      ["cacheWritePerMillion", "缓存创建"],
                    ] as const).map(([key, label]) => {
                      // 占位显示数据库默认价：留空即采用该默认（与后端定价回退一致），
                      // 中转站倍率不同的用户在此覆盖。
                      const defaultPrice = matchModelDefaults(model.apiModel)?.pricing?.[key];
                      return (
                        <label key={key}>
                          <span>{label}</span>
                          <input
                            className="settings-input"
                            type="number"
                            min={0}
                            step="0.0001"
                            value={model.pricing?.[key] ?? ""}
                            placeholder={defaultPrice !== undefined ? `默认 ${defaultPrice}` : "未设置"}
                            onChange={(event) => onUpdateModel(model.id, {
                              pricing: {
                                ...model.pricing,
                                currency: "USD",
                                [key]: event.target.value ? Number(event.target.value) : undefined,
                              },
                            })}
                          />
                        </label>
                      );
                    })}
                  </div>
                </details>
                <button
                  className={`model-row-action model-default-button${isDefault ? " model-default-button-active" : ""}`}
                  type="button"
                  title={isDefault ? "当前默认模型" : "设为默认模型"}
                  disabled={!provider.enabled || !model.enabled}
                  onClick={() => onSetDefaultModel(model.id)}
                >
                  <Star size={15} fill={isDefault ? "currentColor" : "none"} />
                </button>
                <button
                  className="model-row-action model-delete-button"
                  type="button"
                  title="删除模型"
                  onClick={() => onDeleteModel(model.id)}
                >
                  <Trash2 size={15} />
                </button>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
