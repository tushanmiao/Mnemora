import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  AlertCircle,
  ArrowLeft,
  Bot,
  CheckCircle2,
  Eye,
  EyeOff,
  PlugZap,
  Plus,
  RefreshCw,
  Save,
  Server,
  SlidersHorizontal,
  Star,
  Trash2,
} from "lucide-react";
import { GeneralSettingsPanel } from "./GeneralSettingsPanel";
import {
  fetchProviderModels,
  testProviderConnection,
} from "../api/providers";
import type { AppSettings, SettingsBundle } from "../types/appSettings";
import type {
  ApiProtocol,
  AuthScheme,
  ModelSettings,
  ProviderApiKeyUpdate,
  ProviderConfig,
  ProviderKind,
  ProviderModelConfig,
} from "../types/settings";
import "../styles/settings-page.css";

type ProviderField = "name" | "baseUrl";
type ProviderErrors = Partial<Record<ProviderField, string>>;

type ValidationErrors = {
  providers: Record<string, ProviderErrors>;
  models: Record<string, string>;
};

type FormFeedback = {
  kind: "success" | "error";
  message: string;
} | null;

type ProviderActionFeedback = {
  kind: "success" | "error";
  message: string;
};

type SettingsPageProps = {
  settings: ModelSettings;
  appSettings: AppSettings;
  initialError: string | null;
  appSettingsError: string | null;
  onBack: () => void;
  onSave: (
    settings: ModelSettings,
    apiKeyUpdates: ProviderApiKeyUpdate[],
  ) => Promise<ModelSettings>;
  onPreviewAppSettings: (settings: AppSettings) => void;
  onSaveAppSettings: (settings: AppSettings) => Promise<AppSettings>;
  onSettingsImported: (bundle: SettingsBundle) => void;
  onDefaultModelChange: (providerId: string, modelId: string) => Promise<void>;
};

type SettingsCategory = "general" | "models";

const EMPTY_ERRORS: ValidationErrors = { providers: {}, models: {} };

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

function createId(prefix: string) {
  return `${prefix}-${crypto.randomUUID()}`;
}

function firstSelectableModel(settings: ModelSettings) {
  for (const provider of settings.providers) {
    if (!provider.enabled) continue;
    const model = provider.models.find((item) => item.enabled);
    if (model) return { providerId: provider.id, modelId: model.id };
  }
  return null;
}

function reconcileDefaultModel(settings: ModelSettings): ModelSettings {
  const currentProvider = settings.providers.find(
    (provider) => provider.id === settings.defaultProviderId && provider.enabled,
  );
  const currentModel = currentProvider?.models.find(
    (model) => model.id === settings.defaultModelId && model.enabled,
  );

  if (currentProvider && currentModel) return settings;

  const fallback = firstSelectableModel(settings);
  return {
    ...settings,
    defaultProviderId: fallback?.providerId ?? null,
    defaultModelId: fallback?.modelId ?? null,
  };
}

function normalizeSettings(settings: ModelSettings): ModelSettings {
  const normalized: ModelSettings = {
    ...settings,
    providers: settings.providers.map((provider) => ({
      ...provider,
      name: provider.name.trim(),
      baseUrl: provider.baseUrl.trim().replace(/\/+$/, ""),
      models: provider.models.map((model) => {
        const apiModel = model.apiModel.trim();
        return {
          ...model,
          apiModel,
          displayName: model.displayName.trim() || apiModel,
        };
      }),
    })),
  };

  return reconcileDefaultModel(normalized);
}

function validateSettings(settings: ModelSettings): ValidationErrors {
  const errors: ValidationErrors = { providers: {}, models: {} };

  settings.providers.forEach((provider) => {
    const providerErrors: ProviderErrors = {};

    if (!provider.name) {
      providerErrors.name = "请输入供应商名称。";
    }

    if (!provider.baseUrl) {
      providerErrors.baseUrl = "请输入 API Base URL。";
    } else {
      try {
        const url = new URL(provider.baseUrl);
        if (!(["http:", "https:"] as string[]).includes(url.protocol)) {
          providerErrors.baseUrl = "API Base URL 必须使用 http 或 https。";
        } else if (url.username || url.password) {
          providerErrors.baseUrl = "API Base URL 不能包含用户名或密码。";
        }
      } catch {
        providerErrors.baseUrl = "请输入完整有效的 URL。";
      }
    }

    if (Object.keys(providerErrors).length > 0) {
      errors.providers[provider.id] = providerErrors;
    }

    const seenApiModels = new Set<string>();
    provider.models.forEach((model) => {
      if (!model.apiModel) {
        errors.models[model.id] = "API Model 不能为空。";
        return;
      }
      if (seenApiModels.has(model.apiModel)) {
        errors.models[model.id] = "同一供应商中不能重复添加相同的 API Model。";
        return;
      }
      seenApiModels.add(model.apiModel);
    });
  });

  return errors;
}

function hasValidationErrors(errors: ValidationErrors) {
  return Object.keys(errors.providers).length > 0 || Object.keys(errors.models).length > 0;
}

export function SettingsPage({
  settings,
  appSettings,
  initialError,
  appSettingsError,
  onBack,
  onSave,
  onPreviewAppSettings,
  onSaveAppSettings,
  onSettingsImported,
  onDefaultModelChange,
}: SettingsPageProps) {
  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("general");
  const [draft, setDraft] = useState<ModelSettings>(settings);
  const [secretDrafts, setSecretDrafts] = useState<Record<string, string>>({});
  const [pendingSecretDeletes, setPendingSecretDeletes] = useState<Set<string>>(new Set());
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(
    settings.defaultProviderId ?? settings.providers[0]?.id ?? null,
  );
  const [errors, setErrors] = useState<ValidationErrors>(EMPTY_ERRORS);
  const [visibleApiKeys, setVisibleApiKeys] = useState<Set<string>>(new Set());
  const [newApiModel, setNewApiModel] = useState("");
  const [newDisplayName, setNewDisplayName] = useState("");
  const [feedback, setFeedback] = useState<FormFeedback>(
    initialError ? { kind: "error", message: initialError } : null,
  );
  const [saving, setSaving] = useState(false);
  const [testingProviderId, setTestingProviderId] = useState<string | null>(null);
  const [fetchingProviderId, setFetchingProviderId] = useState<string | null>(null);
  const [providerActionFeedback, setProviderActionFeedback] = useState<
    Record<string, ProviderActionFeedback>
  >({});
  const [availableModels, setAvailableModels] = useState<Record<string, string[]>>({});

  const selectedProvider = useMemo(
    () => draft.providers.find((provider) => provider.id === selectedProviderId) ?? null,
    [draft.providers, selectedProviderId],
  );

  const defaultSelection = useMemo(() => {
    const provider = draft.providers.find((item) => item.id === draft.defaultProviderId);
    const model = provider?.models.find((item) => item.id === draft.defaultModelId);
    return provider && model ? { provider, model } : null;
  }, [draft]);

  useEffect(() => {
    setDraft(settings);
    setSelectedProviderId((current) => (
      current && settings.providers.some((provider) => provider.id === current)
        ? current
        : settings.defaultProviderId ?? settings.providers[0]?.id ?? null
    ));
  }, [settings]);

  useEffect(() => {
    if (initialError) setFeedback({ kind: "error", message: initialError });
  }, [initialError]);

  const hasEffectiveApiKey = (provider: ProviderConfig) => {
    if (pendingSecretDeletes.has(provider.id)) return false;
    return Boolean(secretDrafts[provider.id]?.trim()) || provider.hasApiKey;
  };

  const credentialStatus = (provider: ProviderConfig) => {
    if (pendingSecretDeletes.has(provider.id)) return "待删除";
    if (secretDrafts[provider.id]?.trim()) {
      return provider.hasApiKey ? "待替换" : "待保存";
    }
    return provider.hasApiKey ? "已安全保存" : "未配置";
  };

  const clearProviderError = (providerId: string, field: ProviderField) => {
    setErrors((current) => {
      const providerErrors = { ...current.providers[providerId], [field]: undefined };
      const providers = { ...current.providers };
      if (Object.values(providerErrors).some(Boolean)) providers[providerId] = providerErrors;
      else delete providers[providerId];
      return { ...current, providers };
    });
  };

  const updateProvider = (providerId: string, patch: Partial<ProviderConfig>) => {
    setDraft((current) => reconcileDefaultModel({
      ...current,
      providers: current.providers.map((provider) =>
        provider.id === providerId ? { ...provider, ...patch } : provider,
      ),
    }));
    if ("baseUrl" in patch || "protocol" in patch || "authScheme" in patch) {
      setAvailableModels((current) => {
        const next = { ...current };
        delete next[providerId];
        return next;
      });
      setProviderActionFeedback((current) => {
        const next = { ...current };
        delete next[providerId];
        return next;
      });
    }
    setFeedback(null);
  };

  const updateModel = (
    providerId: string,
    modelId: string,
    patch: Partial<ProviderModelConfig>,
  ) => {
    setDraft((current) => reconcileDefaultModel({
      ...current,
      providers: current.providers.map((provider) =>
        provider.id === providerId
          ? {
              ...provider,
              models: provider.models.map((model) =>
                model.id === modelId ? { ...model, ...patch } : model,
              ),
            }
          : provider,
      ),
    }));
    setErrors((current) => {
      const models = { ...current.models };
      delete models[modelId];
      return { ...current, models };
    });
    setFeedback(null);
  };

  const handleAddProvider = () => {
    const id = createId("provider");
    const provider: ProviderConfig = {
      id,
      name: "自定义中转站",
      kind: "custom",
      protocol: "openAiChatCompletions",
      authScheme: "protocolDefault",
      baseUrl: "",
      hasApiKey: false,
      enabled: true,
      models: [],
    };

    setDraft((current) => ({ ...current, providers: [...current.providers, provider] }));
    setSelectedProviderId(id);
    setNewApiModel("");
    setNewDisplayName("");
    setFeedback(null);
  };

  const handleDeleteProvider = (provider: ProviderConfig) => {
    if (!window.confirm(`删除供应商“${provider.name}”及其模型配置？`)) return;

    setDraft((current) => reconcileDefaultModel({
      ...current,
      providers: current.providers.filter((item) => item.id !== provider.id),
    }));
    setSecretDrafts((current) => {
      const next = { ...current };
      delete next[provider.id];
      return next;
    });
    if (provider.hasApiKey) {
      setPendingSecretDeletes((current) => new Set(current).add(provider.id));
    }
    setSelectedProviderId((currentId) => {
      if (currentId !== provider.id) return currentId;
      return draft.providers.find((item) => item.id !== provider.id)?.id ?? null;
    });
    setFeedback(null);
  };

  const handleApiKeyChange = (providerId: string, value: string) => {
    setSecretDrafts((current) => ({ ...current, [providerId]: value }));
    if (value.trim()) {
      setPendingSecretDeletes((current) => {
        const next = new Set(current);
        next.delete(providerId);
        return next;
      });
    }
    setAvailableModels((current) => {
      const next = { ...current };
      delete next[providerId];
      return next;
    });
    setProviderActionFeedback((current) => {
      const next = { ...current };
      delete next[providerId];
      return next;
    });
    setFeedback(null);
  };

  const handleDeleteApiKey = (providerId: string) => {
    setSecretDrafts((current) => {
      const next = { ...current };
      delete next[providerId];
      return next;
    });
    setPendingSecretDeletes((current) => new Set(current).add(providerId));
    setAvailableModels((current) => {
      const next = { ...current };
      delete next[providerId];
      return next;
    });
    setProviderActionFeedback((current) => {
      const next = { ...current };
      delete next[providerId];
      return next;
    });
    setFeedback(null);
  };

  const validateManualNetworkAction = (provider: ProviderConfig) => {
    const apiKey = secretDrafts[provider.id]?.trim() ?? "";
    if (!provider.baseUrl.trim()) return "请输入 API Base URL。";
    try {
      const url = new URL(provider.baseUrl.trim());
      if (!(["http:", "https:"] as string[]).includes(url.protocol)) {
        return "API Base URL 必须使用 http 或 https。";
      }
    } catch {
      return "请输入完整有效的 API Base URL。";
    }
    if (!apiKey && !hasEffectiveApiKey(provider)) return "请输入 API Key。";
    return null;
  };

  const handleTestConnection = async (provider: ProviderConfig) => {
    if (saving || testingProviderId || fetchingProviderId) return;
    const validationError = validateManualNetworkAction(provider);
    if (validationError) {
      setProviderActionFeedback((current) => ({
        ...current,
        [provider.id]: { kind: "error", message: validationError },
      }));
      return;
    }

    setTestingProviderId(provider.id);
    setProviderActionFeedback((current) => {
      const next = { ...current };
      delete next[provider.id];
      return next;
    });

    try {
      const result = await testProviderConnection(
        provider,
        secretDrafts[provider.id] ?? "",
      );
      setProviderActionFeedback((current) => ({
        ...current,
        [provider.id]: result.success
          ? { kind: "success", message: `本次请求成功 · ${result.latencyMs} ms` }
          : { kind: "error", message: result.error || "连接请求失败。" },
      }));
    } catch (error) {
      setProviderActionFeedback((current) => ({
        ...current,
        [provider.id]: {
          kind: "error",
          message: error instanceof Error ? error.message : String(error),
        },
      }));
    } finally {
      setTestingProviderId(null);
    }
  };

  const handleFetchModels = async (provider: ProviderConfig) => {
    if (saving || testingProviderId || fetchingProviderId) return;
    const validationError = validateManualNetworkAction(provider);
    if (validationError) {
      setProviderActionFeedback((current) => ({
        ...current,
        [provider.id]: { kind: "error", message: validationError },
      }));
      return;
    }

    setFetchingProviderId(provider.id);
    setProviderActionFeedback((current) => {
      const next = { ...current };
      delete next[provider.id];
      return next;
    });

    try {
      const models = await fetchProviderModels(
        provider,
        secretDrafts[provider.id] ?? "",
      );
      setAvailableModels((current) => ({ ...current, [provider.id]: models }));
      setProviderActionFeedback((current) => ({
        ...current,
        [provider.id]: {
          kind: "success",
          message: models.length > 0 ? `已获取 ${models.length} 个模型。` : "模型列表为空。",
        },
      }));
    } catch (error) {
      setProviderActionFeedback((current) => ({
        ...current,
        [provider.id]: {
          kind: "error",
          message: error instanceof Error ? error.message : String(error),
        },
      }));
    } finally {
      setFetchingProviderId(null);
    }
  };

  const handleAddModel = () => {
    if (!selectedProvider) return;
    const apiModel = newApiModel.trim();
    if (!apiModel) {
      setFeedback({ kind: "error", message: "请输入 API Model。" });
      return;
    }
    if (selectedProvider.models.some((model) => model.apiModel === apiModel)) {
      setFeedback({ kind: "error", message: "当前供应商中已经存在这个 API Model。" });
      return;
    }

    const model: ProviderModelConfig = {
      id: createId("model"),
      apiModel,
      displayName: newDisplayName.trim() || apiModel,
      enabled: true,
    };

    setDraft((current) => {
      const next = {
        ...current,
        providers: current.providers.map((provider) =>
          provider.id === selectedProvider.id
            ? { ...provider, models: [...provider.models, model] }
            : provider,
        ),
      };
      return reconcileDefaultModel(next);
    });
    setNewApiModel("");
    setNewDisplayName("");
    setFeedback(null);
  };

  const handleDeleteModel = (providerId: string, modelId: string) => {
    setDraft((current) => reconcileDefaultModel({
      ...current,
      providers: current.providers.map((provider) =>
        provider.id === providerId
          ? { ...provider, models: provider.models.filter((model) => model.id !== modelId) }
          : provider,
      ),
    }));
    setFeedback(null);
  };

  const handleSave = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const normalized = normalizeSettings(draft);
    const nextErrors = validateSettings(normalized);

    setDraft(normalized);
    setErrors(nextErrors);

    if (hasValidationErrors(nextErrors)) {
      const firstInvalidProvider = normalized.providers.find(
        (provider) => nextErrors.providers[provider.id]
          || provider.models.some((model) => nextErrors.models[model.id]),
      );
      if (firstInvalidProvider) setSelectedProviderId(firstInvalidProvider.id);
      setFeedback({ kind: "error", message: "请修正标记的配置后再保存。" });
      return;
    }

    const apiKeyUpdates: ProviderApiKeyUpdate[] = [];
    for (const [providerId, apiKey] of Object.entries(secretDrafts)) {
      const normalizedApiKey = apiKey.trim();
      if (normalizedApiKey) {
        apiKeyUpdates.push({ providerId, action: "set", apiKey: normalizedApiKey });
      }
    }
    for (const providerId of pendingSecretDeletes) {
      if (!secretDrafts[providerId]?.trim()) {
        apiKeyUpdates.push({ providerId, action: "delete" });
      }
    }

    setSaving(true);
    setFeedback(null);
    try {
      const saved = await onSave(normalized, apiKeyUpdates);
      setDraft(saved);
      setSecretDrafts({});
      setPendingSecretDeletes(new Set());
      setFeedback({ kind: "success", message: "模型设置已保存。" });
    } catch (error) {
      setFeedback({
        kind: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="settings-page" aria-label="设置">
      <header className="settings-header">
        <button className="icon-button" type="button" title="返回聊天" onClick={onBack}>
          <ArrowLeft size={19} />
        </button>
        <div>
          <h1>设置</h1>
          <span>{activeCategory === "general" ? "基础" : "模型服务"}</span>
        </div>
      </header>

      <div className="settings-layout">
        <nav className="settings-nav" aria-label="设置分类">
          <button
            className={`settings-nav-item${activeCategory === "general" ? " settings-nav-item-active" : ""}`}
            type="button"
            aria-current={activeCategory === "general" ? "page" : undefined}
            onClick={() => setActiveCategory("general")}
          >
            <SlidersHorizontal size={17} />
            <span>基础</span>
          </button>
          <button
            className={`settings-nav-item${activeCategory === "models" ? " settings-nav-item-active" : ""}`}
            type="button"
            aria-current={activeCategory === "models" ? "page" : undefined}
            onClick={() => setActiveCategory("models")}
          >
            <Bot size={17} />
            <span>模型服务</span>
          </button>
        </nav>

        {activeCategory === "general" ? (
          <GeneralSettingsPanel
            settings={appSettings}
            modelSettings={settings}
            initialError={appSettingsError}
            onPreview={onPreviewAppSettings}
            onSave={onSaveAppSettings}
            onImported={onSettingsImported}
            onDefaultModelChange={onDefaultModelChange}
          />
        ) : (
          <form className="settings-content" onSubmit={handleSave} noValidate>
          <div className="settings-content-heading">
            <div>
              <h2>模型服务</h2>
              <span>{draft.providers.length} 个供应商</span>
            </div>
            <div className="settings-heading-actions">
              {defaultSelection ? (
                <span className="settings-default-summary" title={defaultSelection.model.apiModel}>
                  <Star size={13} fill="currentColor" />
                  {defaultSelection.model.displayName} · {defaultSelection.provider.name}
                </span>
              ) : null}
              <button
                className="settings-button settings-button-primary"
                type="submit"
                disabled={saving}
              >
                <Save size={16} />
                <span>{saving ? "保存中" : "保存"}</span>
              </button>
            </div>
          </div>

          <div className="provider-workspace">
            <aside className="provider-list" aria-label="供应商列表">
              <button
                className="provider-add-button"
                type="button"
                onClick={handleAddProvider}
              >
                <Plus size={15} />
                <span>添加中转站</span>
              </button>

              <div className="provider-list-items">
                {draft.providers.map((provider) => {
                  const selected = provider.id === selectedProviderId;
                  const configured = hasEffectiveApiKey(provider);
                  return (
                    <button
                      className={`provider-list-item${selected ? " provider-list-item-active" : ""}`}
                      type="button"
                      key={provider.id}
                      aria-pressed={selected}
                      onClick={() => {
                        setSelectedProviderId(provider.id);
                        setNewApiModel("");
                        setNewDisplayName("");
                        setFeedback(null);
                      }}
                    >
                      <span
                        className={`provider-list-dot${provider.enabled && configured ? " provider-list-dot-configured" : ""}`}
                        aria-hidden="true"
                      />
                      <span className="provider-list-copy">
                        <strong>{provider.name || "未命名供应商"}</strong>
                        <span>
                          {provider.enabled ? (configured ? "已配置" : "未配置") : "已停用"}
                          {` · ${provider.models.length} 个模型`}
                        </span>
                      </span>
                    </button>
                  );
                })}
              </div>
            </aside>

            <div
              className="provider-detail"
              key={selectedProviderId ?? "provider-empty"}
            >
              {selectedProvider ? (
                <>
                  <section className="provider-section provider-section-header">
                    <div className="provider-title-block">
                      <Server size={18} />
                      <div>
                        <h3>{selectedProvider.name || "未命名供应商"}</h3>
                        <span>{PROTOCOL_SHORT_LABELS[selectedProvider.protocol]}</span>
                      </div>
                    </div>
                    <div className="provider-header-actions">
                      <label className="settings-switch-label">
                        <input
                          type="checkbox"
                          checked={selectedProvider.enabled}
                          onChange={(event) => updateProvider(selectedProvider.id, {
                            enabled: event.target.checked,
                          })}
                        />
                        <span>启用</span>
                      </label>
                      <button
                        className="settings-icon-danger"
                        type="button"
                        title="删除供应商"
                        aria-label="删除供应商"
                        onClick={() => handleDeleteProvider(selectedProvider)}
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  </section>

                  <section className="provider-section">
                    <h4>供应商</h4>
                    <div className="settings-field-grid">
                      <div className="settings-field">
                        <label htmlFor={`provider-name-${selectedProvider.id}`}>显示名称</label>
                        <input
                          id={`provider-name-${selectedProvider.id}`}
                          className={errors.providers[selectedProvider.id]?.name
                            ? "settings-input settings-input-error"
                            : "settings-input"}
                          value={selectedProvider.name}
                          onChange={(event) => {
                            updateProvider(selectedProvider.id, { name: event.target.value });
                            clearProviderError(selectedProvider.id, "name");
                          }}
                        />
                        {errors.providers[selectedProvider.id]?.name ? (
                          <span className="settings-field-error" role="alert">
                            {errors.providers[selectedProvider.id]?.name}
                          </span>
                        ) : null}
                      </div>

                      <div className="settings-field">
                        <label htmlFor={`provider-kind-${selectedProvider.id}`}>供应商类型</label>
                        <select
                          id={`provider-kind-${selectedProvider.id}`}
                          className="settings-input settings-select"
                          value={selectedProvider.kind}
                          onChange={(event) => updateProvider(selectedProvider.id, {
                            kind: event.target.value as ProviderKind,
                          })}
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
                      <label htmlFor={`base-url-${selectedProvider.id}`}>API Base URL</label>
                      <input
                        id={`base-url-${selectedProvider.id}`}
                        className={errors.providers[selectedProvider.id]?.baseUrl
                          ? "settings-input settings-input-error"
                          : "settings-input"}
                        type="url"
                        inputMode="url"
                        placeholder="https://api.example.com/v1"
                        value={selectedProvider.baseUrl}
                        spellCheck={false}
                        onChange={(event) => {
                          updateProvider(selectedProvider.id, { baseUrl: event.target.value });
                          clearProviderError(selectedProvider.id, "baseUrl");
                        }}
                      />
                      {errors.providers[selectedProvider.id]?.baseUrl ? (
                        <span className="settings-field-error" role="alert">
                          {errors.providers[selectedProvider.id]?.baseUrl}
                        </span>
                      ) : null}
                    </div>

                    <div className="settings-field-grid">
                      <div className="settings-field">
                        <label htmlFor={`protocol-${selectedProvider.id}`}>API 协议</label>
                        <select
                          id={`protocol-${selectedProvider.id}`}
                          className="settings-input settings-select"
                          value={selectedProvider.protocol}
                          onChange={(event) => updateProvider(selectedProvider.id, {
                            protocol: event.target.value as ApiProtocol,
                          })}
                        >
                          {Object.entries(PROTOCOL_LABELS).map(([value, label]) => (
                            <option value={value} key={value}>{label}</option>
                          ))}
                        </select>
                      </div>

                      <div className="settings-field">
                        <label htmlFor={`auth-scheme-${selectedProvider.id}`}>认证方式</label>
                        <select
                          id={`auth-scheme-${selectedProvider.id}`}
                          className="settings-input settings-select"
                          value={selectedProvider.authScheme}
                          onChange={(event) => updateProvider(selectedProvider.id, {
                            authScheme: event.target.value as AuthScheme,
                          })}
                        >
                          {Object.entries(AUTH_SCHEME_LABELS).map(([value, label]) => (
                            <option value={value} key={value}>{label}</option>
                          ))}
                        </select>
                      </div>
                    </div>

                    <div className="settings-field">
                      <div className="settings-label-row">
                        <label htmlFor={`api-key-${selectedProvider.id}`}>API Key</label>
                        <span>{credentialStatus(selectedProvider)}</span>
                      </div>
                      <div className="settings-secret-row">
                        <div className="settings-secret-input">
                          <input
                            id={`api-key-${selectedProvider.id}`}
                            className="settings-input"
                            type={visibleApiKeys.has(selectedProvider.id) ? "text" : "password"}
                            placeholder={pendingSecretDeletes.has(selectedProvider.id)
                              ? "保存后删除"
                              : selectedProvider.hasApiKey
                                ? "已安全保存；输入新值可替换"
                                : "输入 API Key"}
                            value={secretDrafts[selectedProvider.id] ?? ""}
                            autoComplete="off"
                            spellCheck={false}
                            onChange={(event) => handleApiKeyChange(
                              selectedProvider.id,
                              event.target.value,
                            )}
                          />
                          <button
                            className="settings-secret-toggle"
                            type="button"
                            title={visibleApiKeys.has(selectedProvider.id) ? "隐藏 API Key" : "显示 API Key"}
                            aria-label={visibleApiKeys.has(selectedProvider.id) ? "隐藏 API Key" : "显示 API Key"}
                            onClick={() => setVisibleApiKeys((current) => {
                              const next = new Set(current);
                              if (next.has(selectedProvider.id)) next.delete(selectedProvider.id);
                              else next.add(selectedProvider.id);
                              return next;
                            })}
                          >
                            {visibleApiKeys.has(selectedProvider.id)
                              ? <EyeOff size={17} />
                              : <Eye size={17} />}
                          </button>
                        </div>
                        {hasEffectiveApiKey(selectedProvider) ? (
                          <button
                            className="settings-icon-danger"
                            type="button"
                            title="删除 API Key"
                            aria-label="删除 API Key"
                            onClick={() => handleDeleteApiKey(selectedProvider.id)}
                          >
                            <Trash2 size={15} />
                          </button>
                        ) : null}
                      </div>
                    </div>

                    <div className="provider-network-actions">
                      <div className="provider-action-status" aria-live="polite">
                        {providerActionFeedback[selectedProvider.id] ? (
                          <span className={`provider-action-${providerActionFeedback[selectedProvider.id].kind}`}>
                            {providerActionFeedback[selectedProvider.id].message}
                          </span>
                        ) : null}
                      </div>
                      <button
                        className="settings-button settings-button-secondary"
                        type="button"
                        disabled={Boolean(saving || testingProviderId || fetchingProviderId)}
                        title="手动获取模型"
                        onClick={() => void handleFetchModels(selectedProvider)}
                      >
                        <RefreshCw
                          size={15}
                          className={fetchingProviderId === selectedProvider.id ? "settings-spin" : ""}
                        />
                        <span>{fetchingProviderId === selectedProvider.id ? "获取中" : "获取模型"}</span>
                      </button>
                      <button
                        className="settings-button settings-button-secondary"
                        type="button"
                        disabled={Boolean(saving || testingProviderId || fetchingProviderId)}
                        title="手动测试连接"
                        onClick={() => void handleTestConnection(selectedProvider)}
                      >
                        <PlugZap size={15} />
                        <span>{testingProviderId === selectedProvider.id ? "测试中" : "测试连接"}</span>
                      </button>
                    </div>
                  </section>

                  <section className="provider-section provider-model-section">
                    <div className="provider-section-title-row">
                      <h4>模型</h4>
                      <span>{selectedProvider.models.length}</span>
                    </div>

                    {(availableModels[selectedProvider.id]?.length ?? 0) > 0 ? (
                      <div className="settings-field discovered-model-field">
                        <div className="settings-label-row">
                          <label htmlFor={`available-model-${selectedProvider.id}`}>获取到的模型</label>
                          <span>{availableModels[selectedProvider.id].length}</span>
                        </div>
                        <select
                          id={`available-model-${selectedProvider.id}`}
                          className="settings-input settings-select"
                          value=""
                          onChange={(event) => {
                            const model = event.target.value;
                            if (!model) return;
                            setNewApiModel(model);
                            setNewDisplayName(model);
                          }}
                        >
                          <option value="">选择模型</option>
                          {availableModels[selectedProvider.id].map((model) => (
                            <option value={model} key={model}>{model}</option>
                          ))}
                        </select>
                      </div>
                    ) : null}

                    <div className="model-add-row">
                      <div className="settings-field">
                        <label htmlFor={`new-api-model-${selectedProvider.id}`}>API Model</label>
                        <input
                          id={`new-api-model-${selectedProvider.id}`}
                          className="settings-input"
                          placeholder="deepseek-v4"
                          value={newApiModel}
                          spellCheck={false}
                          onChange={(event) => {
                            setNewApiModel(event.target.value);
                            setFeedback(null);
                          }}
                        />
                      </div>
                      <div className="settings-field">
                        <label htmlFor={`new-display-name-${selectedProvider.id}`}>Display Name</label>
                        <input
                          id={`new-display-name-${selectedProvider.id}`}
                          className="settings-input"
                          placeholder={newApiModel || "自定义显示名称"}
                          value={newDisplayName}
                          onChange={(event) => setNewDisplayName(event.target.value)}
                          onKeyDown={(event) => {
                            if (event.key !== "Enter") return;
                            event.preventDefault();
                            handleAddModel();
                          }}
                        />
                      </div>
                      <button
                        className="model-add-button"
                        type="button"
                        title="添加模型"
                        aria-label="添加模型"
                        onClick={handleAddModel}
                      >
                        <Plus size={17} />
                      </button>
                    </div>

                    <div className="model-list" aria-label="模型映射">
                      {selectedProvider.models.length === 0 ? (
                        <div className="model-list-empty">尚未添加模型</div>
                      ) : selectedProvider.models.map((model) => {
                        const isDefault = draft.defaultProviderId === selectedProvider.id
                          && draft.defaultModelId === model.id;
                        return (
                          <div className="model-row" key={model.id}>
                            <label className="model-enabled-toggle" title={model.enabled ? "停用模型" : "启用模型"}>
                              <input
                                type="checkbox"
                                checked={model.enabled}
                                onChange={(event) => updateModel(
                                  selectedProvider.id,
                                  model.id,
                                  { enabled: event.target.checked },
                                )}
                              />
                            </label>
                            <div className="settings-field">
                              <label htmlFor={`api-model-${model.id}`}>API Model</label>
                              <input
                                id={`api-model-${model.id}`}
                                className={errors.models[model.id]
                                  ? "settings-input settings-input-error"
                                  : "settings-input"}
                                value={model.apiModel}
                                spellCheck={false}
                                onChange={(event) => updateModel(
                                  selectedProvider.id,
                                  model.id,
                                  { apiModel: event.target.value },
                                )}
                              />
                              {errors.models[model.id] ? (
                                <span className="settings-field-error" role="alert">
                                  {errors.models[model.id]}
                                </span>
                              ) : null}
                            </div>
                            <div className="settings-field">
                              <label htmlFor={`display-name-${model.id}`}>Display Name</label>
                              <input
                                id={`display-name-${model.id}`}
                                className="settings-input"
                                value={model.displayName}
                                onChange={(event) => updateModel(
                                  selectedProvider.id,
                                  model.id,
                                  { displayName: event.target.value },
                                )}
                              />
                            </div>
                            <button
                              className={`model-row-action model-default-button${isDefault ? " model-default-button-active" : ""}`}
                              type="button"
                              title={isDefault ? "当前默认模型" : "设为默认模型"}
                              aria-label={isDefault ? "当前默认模型" : "设为默认模型"}
                              disabled={!selectedProvider.enabled || !model.enabled}
                              onClick={() => setDraft((current) => ({
                                ...current,
                                defaultProviderId: selectedProvider.id,
                                defaultModelId: model.id,
                              }))}
                            >
                              <Star size={15} fill={isDefault ? "currentColor" : "none"} />
                            </button>
                            <button
                              className="model-row-action model-delete-button"
                              type="button"
                              title="删除模型"
                              aria-label="删除模型"
                              onClick={() => handleDeleteModel(selectedProvider.id, model.id)}
                            >
                              <Trash2 size={15} />
                            </button>
                          </div>
                        );
                      })}
                    </div>
                  </section>
                </>
              ) : (
                <div className="provider-empty-state">
                  <Server size={24} />
                  <span>添加一个供应商</span>
                </div>
              )}
            </div>
          </div>

          {feedback ? (
            <div className={`settings-feedback settings-feedback-${feedback.kind}`} role="status">
              {feedback.kind === "success"
                ? <CheckCircle2 size={17} />
                : <AlertCircle size={17} />}
              <span>{feedback.message}</span>
            </div>
          ) : null}
          </form>
        )}
      </div>
    </section>
  );
}
