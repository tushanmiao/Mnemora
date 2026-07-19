import { useEffect, useMemo, useState, type FormEvent } from "react";
import {
  AlertCircle,
  CheckCircle2,
  Save,
  Star,
} from "lucide-react";
import {
  fetchProviderModels,
  testProviderConnection,
} from "../api/providers";
import type {
  ModelSettings,
  ProviderApiKeyUpdate,
  ProviderConfig,
  ProviderModelConfig,
} from "../../../types/modelSettings";
import { ProviderDetail } from "./ProviderDetail";
import { ProviderList } from "./ProviderList";

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

type ModelSettingsPanelProps = {
  settings: ModelSettings;
  initialError: string | null;
  onSave: (
    settings: ModelSettings,
    apiKeyUpdates: ProviderApiKeyUpdate[],
  ) => Promise<ModelSettings>;
};

const EMPTY_ERRORS: ValidationErrors = { providers: {}, models: {} };


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

export function ModelSettingsPanel({
  settings,
  initialError,
  onSave,
}: ModelSettingsPanelProps) {
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
            <ProviderList
              providers={draft.providers}
              selectedProviderId={selectedProviderId}
              isConfigured={hasEffectiveApiKey}
              onAdd={handleAddProvider}
              onSelect={(providerId) => {
                setSelectedProviderId(providerId);
                setNewApiModel("");
                setNewDisplayName("");
                setFeedback(null);
              }}
            />

            <ProviderDetail
              key={selectedProviderId ?? "provider-empty"}
              provider={selectedProvider}
              defaultProviderId={draft.defaultProviderId}
              defaultModelId={draft.defaultModelId}
              providerErrors={selectedProvider ? errors.providers[selectedProvider.id] ?? {} : {}}
              modelErrors={errors.models}
              secretDraft={selectedProvider ? secretDrafts[selectedProvider.id] ?? "" : ""}
              pendingSecretDelete={selectedProvider ? pendingSecretDeletes.has(selectedProvider.id) : false}
              apiKeyVisible={selectedProvider ? visibleApiKeys.has(selectedProvider.id) : false}
              actionFeedback={selectedProvider ? providerActionFeedback[selectedProvider.id] : undefined}
              availableModels={selectedProvider ? availableModels[selectedProvider.id] ?? [] : []}
              newApiModel={newApiModel}
              newDisplayName={newDisplayName}
              saving={saving}
              testing={testingProviderId === selectedProvider?.id}
              fetching={fetchingProviderId === selectedProvider?.id}
              credentialStatus={selectedProvider ? credentialStatus(selectedProvider) : "未配置"}
              hasEffectiveApiKey={selectedProvider ? hasEffectiveApiKey(selectedProvider) : false}
              onUpdateProvider={(patch) => {
                if (selectedProvider) updateProvider(selectedProvider.id, patch);
              }}
              onClearProviderError={(field) => {
                if (selectedProvider) clearProviderError(selectedProvider.id, field);
              }}
              onDeleteProvider={() => {
                if (selectedProvider) handleDeleteProvider(selectedProvider);
              }}
              onApiKeyChange={(value) => {
                if (selectedProvider) handleApiKeyChange(selectedProvider.id, value);
              }}
              onDeleteApiKey={() => {
                if (selectedProvider) handleDeleteApiKey(selectedProvider.id);
              }}
              onToggleApiKeyVisibility={() => {
                if (!selectedProvider) return;
                setVisibleApiKeys((current) => {
                  const next = new Set(current);
                  if (next.has(selectedProvider.id)) next.delete(selectedProvider.id);
                  else next.add(selectedProvider.id);
                  return next;
                });
              }}
              onFetchModels={() => {
                if (selectedProvider) void handleFetchModels(selectedProvider);
              }}
              onTestConnection={() => {
                if (selectedProvider) void handleTestConnection(selectedProvider);
              }}
              onSelectAvailableModel={(model) => {
                setNewApiModel(model);
                setNewDisplayName(model);
              }}
              onNewApiModelChange={(value) => {
                setNewApiModel(value);
                setFeedback(null);
              }}
              onNewDisplayNameChange={setNewDisplayName}
              onAddModel={handleAddModel}
              onUpdateModel={(modelId, patch) => {
                if (selectedProvider) updateModel(selectedProvider.id, modelId, patch);
              }}
              onSetDefaultModel={(modelId) => setDraft((current) => ({
                ...current,
                defaultProviderId: selectedProvider?.id ?? current.defaultProviderId,
                defaultModelId: modelId,
              }))}
              onDeleteModel={(modelId) => {
                if (selectedProvider) handleDeleteModel(selectedProvider.id, modelId);
              }}
            />
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
  );
}

