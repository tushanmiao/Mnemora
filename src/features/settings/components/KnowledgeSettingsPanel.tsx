import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  BookOpenCheck,
  DatabaseZap,
  FileText,
  Image,
  Info,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  RotateCcw,
  Save,
  Search,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import {
  createInitialAppSettings,
  type AppSettings,
  type KnowledgeSettings,
} from "../../../types/appSettings";
import type { ModelSettings } from "../../../types/modelSettings";
import { useI18n } from "../../../i18n/I18nProvider";
import {
  deleteMineruToken,
  getMineruTokenStatus,
  isKnowledgeRuntime,
  setMineruToken,
} from "../../knowledge/api/knowledge";
import "../styles/knowledge-settings.css";

type Props = {
  settings: AppSettings;
  modelSettings: ModelSettings;
  initialError: string | null;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
  onDirtyChange?: (dirty: boolean) => void;
};

const DEFAULT_KNOWLEDGE = createInitialAppSettings().knowledge;

export function KnowledgeSettingsPanel({
  settings,
  modelSettings,
  initialError,
  onSave,
  onDirtyChange,
}: Props) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<KnowledgeSettings>(() => ({
    ...DEFAULT_KNOWLEDGE,
    ...settings.knowledge,
  }));
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error"; text: string } | null>(null);
  const [tokenConfigured, setTokenConfigured] = useState<boolean | null>(null);
  const [tokenValue, setTokenValue] = useState("");
  const [tokenBusy, setTokenBusy] = useState(false);
  const [tokenFeedback, setTokenFeedback] = useState<{ kind: "success" | "error"; text: string } | null>(null);
  const runtimeAvailable = isKnowledgeRuntime();
  const embeddingProviders = useMemo(
    () => modelSettings.providers.filter((provider) => provider.enabled
      && (provider.protocol === "openAiChatCompletions" || provider.protocol === "openAiResponses")),
    [modelSettings.providers],
  );

  useEffect(() => {
    let mounted = true;
    if (!runtimeAvailable) {
      setTokenConfigured(false);
      return () => {
        mounted = false;
      };
    }
    void getMineruTokenStatus()
      .then((status) => {
        if (mounted) setTokenConfigured(status.configured);
      })
      .catch((error) => {
        if (mounted) {
          setTokenConfigured(null);
          setTokenFeedback({
            kind: "error",
            text: error instanceof Error ? error.message : String(error),
          });
        }
      });
    return () => {
      mounted = false;
    };
  }, [runtimeAvailable]);

  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(settings.knowledge),
    [draft, settings.knowledge],
  );

  useEffect(() => {
    if (!dirty) {
      setDraft({ ...DEFAULT_KNOWLEDGE, ...settings.knowledge });
    }
  }, [dirty, settings.knowledge]);

  useEffect(() => {
    onDirtyChange?.(dirty);
    return () => onDirtyChange?.(false);
  }, [dirty, onDirtyChange]);

  const update = <Key extends keyof KnowledgeSettings>(key: Key, value: KnowledgeSettings[Key]) => {
    setFeedback(null);
    setDraft((current) => ({ ...current, [key]: value }));
  };

  const updateToggle = (key: keyof KnowledgeSettings, value: boolean) => {
    update(key, value as never);
  };

  const updateNumber = (key: keyof KnowledgeSettings, value: string) => {
    const parsed = Number(value);
    update(key, (Number.isFinite(parsed) ? parsed : 0) as never);
  };

  const handleRetrievalMode = (mode: KnowledgeSettings["retrievalMode"]) => {
    setFeedback(null);
    setDraft((current) => ({
      ...current,
      retrievalMode: mode,
      embeddingEnabled: mode === "lexical" ? current.embeddingEnabled : true,
      hybridEnabled: mode === "hybrid",
    }));
  };

  const handleEmbeddingToggle = (enabled: boolean) => {
    setFeedback(null);
    setDraft((current) => ({
      ...current,
      embeddingEnabled: enabled,
      retrievalMode: enabled ? current.retrievalMode : "lexical",
      hybridEnabled: enabled && current.hybridEnabled,
    }));
  };

  const reset = () => {
    if (dirty && !window.confirm(t("knowledgeSettings.resetConfirm"))) return;
    setDraft({ ...DEFAULT_KNOWLEDGE });
    setFeedback(null);
  };

  const validate = (): string | null => {
    if (draft.chunkTargetChars < 256 || draft.chunkTargetChars > 8_192) return t("knowledgeSettings.validation");
    if (draft.chunkMaxChars < draft.chunkTargetChars || draft.chunkMaxChars > 16_384) return t("knowledgeSettings.validation");
    if (draft.chunkOverlapChars > Math.floor(draft.chunkTargetChars / 2)) return t("knowledgeSettings.validation");
    if (draft.topK < 1 || draft.topK > 50) return t("knowledgeSettings.validation");
    if (draft.contextMaxBytes < 4 * 1024 || draft.contextMaxBytes > 256 * 1024) return t("knowledgeSettings.validation");
    if (draft.indexConcurrency < 1 || draft.indexConcurrency > 4) return t("knowledgeSettings.validation");
    if (draft.networkTimeoutSeconds < 30 || draft.networkTimeoutSeconds > 600) return t("knowledgeSettings.validation");
    if (draft.remotePageBudgetPerDay < 1 || draft.remoteTaskBudgetPerDay < 1) return t("knowledgeSettings.validation");
    if (draft.retrievalMode !== "lexical" && !draft.embeddingEnabled) return t("knowledgeSettings.validation");
    if (draft.embeddingEnabled) {
      const provider = embeddingProviders.find((candidate) => candidate.id === draft.embeddingProvider);
      if (!provider || !draft.embeddingModel.trim()) return t("knowledgeSettings.embeddingValidation");
    }
    try {
      const endpoint = new URL(draft.mineruEndpoint);
      if (endpoint.protocol !== "https:" || !endpoint.hostname || endpoint.username || endpoint.password || endpoint.search || endpoint.hash) {
        return t("knowledgeSettings.validation");
      }
    } catch {
      return t("knowledgeSettings.validation");
    }
    return null;
  };

  const save = async () => {
    const validationError = validate();
    if (validationError) {
      setFeedback({ kind: "error", text: validationError });
      return;
    }
    setSaving(true);
    setFeedback(null);
    try {
      await onSave({ ...settings, knowledge: draft });
      setFeedback({ kind: "success", text: t("knowledgeSettings.saved") });
    } catch (error) {
      setFeedback({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  const saveToken = async () => {
    if (!tokenValue.trim()) {
      setTokenFeedback({ kind: "error", text: t("knowledgeSettings.tokenRequired") });
      return;
    }
    setTokenBusy(true);
    setTokenFeedback(null);
    try {
      const status = await setMineruToken(tokenValue);
      setTokenConfigured(status.configured);
      // Do not retain or echo the secret after the credential-manager write.
      setTokenValue("");
      setTokenFeedback({ kind: "success", text: t("knowledgeSettings.tokenSaved") });
    } catch (error) {
      setTokenFeedback({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setTokenBusy(false);
    }
  };

  const removeToken = async () => {
    if (!window.confirm(t("knowledgeSettings.tokenDeleteConfirm"))) return;
    setTokenBusy(true);
    setTokenFeedback(null);
    try {
      const status = await deleteMineruToken();
      setTokenConfigured(status.configured);
      setTokenValue("");
      setTokenFeedback({ kind: "success", text: t("knowledgeSettings.tokenDeleted") });
    } catch (error) {
      setTokenFeedback({
        kind: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setTokenBusy(false);
    }
  };

  return (
    <section className="settings-content knowledge-settings-content" aria-label={t("settings.knowledge")}>
      <div className="settings-content-heading">
        <div>
          <h2>{t("settings.knowledge")}</h2>
          <span>{t("knowledgeSettings.subtitle")}</span>
        </div>
        <div className="settings-heading-actions knowledge-settings-actions">
          <button className="settings-button settings-button-secondary" type="button" disabled={saving} onClick={reset}>
            <RotateCcw size={15} />
            <span>{t("knowledgeSettings.reset")}</span>
          </button>
          <button className="settings-button settings-button-primary" type="button" disabled={saving || !dirty} onClick={() => void save()}>
            <Save size={15} />
            <span>{saving ? t("knowledgeSettings.saving") : t("knowledgeSettings.save")}</span>
          </button>
        </div>
      </div>

      {initialError ? (
        <div className="settings-feedback settings-feedback-error" role="alert">
          <AlertTriangle size={16} />
          <span>{initialError}</span>
        </div>
      ) : null}
      {feedback ? (
        <div className={`settings-feedback settings-feedback-${feedback.kind === "success" ? "success" : "error"}`} role={feedback.kind === "error" ? "alert" : "status"}>
          {feedback.kind === "success" ? <ShieldCheck size={16} /> : <AlertTriangle size={16} />}
          <span>{feedback.text}</span>
        </div>
      ) : null}

      <div className="settings-scroll settings-scroll-measure">
        <div className="settings-callout settings-callout-warning knowledge-settings-privacy">
          <LockKeyhole size={17} />
          <div>
            <strong>{t("knowledgeSettings.privacyTitle")}</strong>
            <span>{t("knowledgeSettings.privacyBody")}</span>
          </div>
        </div>

        <section className="settings-section">
          <div className="settings-section-head">
            <BookOpenCheck size={16} />
            <h3>{t("knowledgeSettings.capability")}</h3>
            <p>{t("knowledgeSettings.capabilityDescription")}</p>
          </div>
          <ToggleRow label={t("knowledgeSettings.enabled")} description={t("knowledgeSettings.enabledDescription")} checked={draft.enabled} disabled={saving} onChange={(value) => updateToggle("enabled", value)} />
          <ToggleRow label={t("knowledgeSettings.autoRetrieve")} description={t("knowledgeSettings.autoRetrieveDescription")} checked={draft.autoRetrieve} disabled={saving || !draft.enabled} onChange={(value) => updateToggle("autoRetrieve", value)} />
          <SelectRow label={t("knowledgeSettings.defaultScope")} description={t("knowledgeSettings.defaultScopeDescription")} value={draft.defaultScope} disabled={saving} onChange={(value) => update("defaultScope", value as KnowledgeSettings["defaultScope"])} options={[
            ["library", t("knowledgeSettings.scopeLibrary")],
            ["currentLiterature", t("knowledgeSettings.scopeLiterature")],
            ["currentNote", t("knowledgeSettings.scopeNote")],
          ]} />
          <ToggleRow label={t("knowledgeSettings.groundedWork")} description={t("knowledgeSettings.groundedWorkDescription")} checked={draft.groundedWork} disabled={saving || !draft.enabled} onChange={(value) => updateToggle("groundedWork", value)} />
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <FileText size={16} />
            <h3>{t("knowledgeSettings.pdf")}</h3>
            <p>{t("knowledgeSettings.pdfDescription")}</p>
          </div>
          <ToggleRow label={t("knowledgeSettings.mineruEnabled")} description={t("knowledgeSettings.mineruEnabledDescription")} checked={draft.mineruCloudEnabled} disabled={saving} onChange={(value) => updateToggle("mineruCloudEnabled", value)} />
          <TextRow label={t("knowledgeSettings.mineruEndpoint")} description={t("knowledgeSettings.mineruEndpointDescription")} value={draft.mineruEndpoint} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => update("mineruEndpoint", value)} />
          <SelectRow label={t("knowledgeSettings.mineruModel")} value={draft.mineruModel} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => update("mineruModel", value as KnowledgeSettings["mineruModel"])} options={[
            ["vlm", t("knowledgeSettings.mineruVlm")],
            ["pipeline", t("knowledgeSettings.mineruPipeline")],
          ]} />
          <div className="knowledge-settings-check-grid">
            <CheckRow label={t("knowledgeSettings.mineruOcr")} checked={draft.mineruOcrEnabled} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateToggle("mineruOcrEnabled", value)} />
            <CheckRow label={t("knowledgeSettings.mineruFormula")} checked={draft.mineruFormulaEnabled} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateToggle("mineruFormulaEnabled", value)} />
            <CheckRow label={t("knowledgeSettings.mineruTable")} checked={draft.mineruTableEnabled} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateToggle("mineruTableEnabled", value)} />
            <CheckRow label={t("knowledgeSettings.mineruFigure")} checked={draft.mineruFigureEnabled} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateToggle("mineruFigureEnabled", value)} />
          </div>
          <TextRow label={t("knowledgeSettings.mineruLanguage")} description={t("knowledgeSettings.mineruLanguageDescription")} value={draft.mineruLanguage} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => update("mineruLanguage", value)} />
           <SelectRow label={t("knowledgeSettings.consentMode")} description={t("knowledgeSettings.consentDescription")} value={draft.mineruConsentMode} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => update("mineruConsentMode", value as KnowledgeSettings["mineruConsentMode"])} options={[
            ["ask", t("knowledgeSettings.consentAsk")],
            ["document", t("knowledgeSettings.consentDocument")],
             ["global", t("knowledgeSettings.consentGlobal")],
           ]} />
           <div className="knowledge-token-panel">
             <div className="knowledge-token-heading">
               <span className="knowledge-token-icon" aria-hidden="true"><KeyRound size={16} /></span>
               <div>
                 <strong>{t("knowledgeSettings.tokenTitle")}</strong>
                 <span>{t("knowledgeSettings.tokenDescription")}</span>
               </div>
               <span className={`knowledge-token-status${tokenConfigured ? " is-configured" : ""}`}>
                 {tokenConfigured === null
                   ? t("knowledgeSettings.tokenChecking")
                   : tokenConfigured
                     ? t("knowledgeSettings.tokenConfigured")
                     : t("knowledgeSettings.tokenNotConfigured")}
               </span>
             </div>
             {!runtimeAvailable ? <p className="knowledge-token-runtime-note">{t("knowledgeSettings.tokenDesktopOnly")}</p> : null}
             <div className="knowledge-token-actions">
               <label className="knowledge-token-input-wrap">
                 <span className="knowledge-visually-hidden">{t("knowledgeSettings.tokenInput")}</span>
                 <input
                   type="password"
                   autoComplete="new-password"
                   value={tokenValue}
                   disabled={!runtimeAvailable || tokenBusy}
                   placeholder={t("knowledgeSettings.tokenPlaceholder")}
                   onChange={(event) => setTokenValue(event.target.value)}
                 />
               </label>
               <button className="settings-button settings-button-secondary" type="button" disabled={!runtimeAvailable || tokenBusy || !tokenValue.trim()} onClick={() => void saveToken()}>
                 {tokenBusy ? <LoaderCircle size={14} className="knowledge-spin" /> : <Save size={14} />}
                 <span>{t("knowledgeSettings.tokenSave")}</span>
               </button>
               <button className="settings-button settings-button-secondary" type="button" disabled={!runtimeAvailable || tokenBusy || !tokenConfigured} onClick={() => void removeToken()}>
                 <Trash2 size={14} />
                 <span>{t("knowledgeSettings.tokenDelete")}</span>
               </button>
             </div>
             {tokenFeedback ? <div className={`knowledge-token-feedback is-${tokenFeedback.kind}`} role={tokenFeedback.kind === "error" ? "alert" : "status"}>{tokenFeedback.kind === "error" ? <AlertTriangle size={14} /> : <ShieldCheck size={14} />}<span>{tokenFeedback.text}</span></div> : null}
           </div>
           <ToggleRow label={t("knowledgeSettings.autoParse")} description={t("knowledgeSettings.autoParseDescription")} checked={draft.autoParseImportedPdf} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateToggle("autoParseImportedPdf", value)} />
          <ToggleRow label={t("knowledgeSettings.localFallback")} description={t("knowledgeSettings.localFallbackDescription")} checked={draft.allowLocalTextFallback} disabled={saving} onChange={(value) => updateToggle("allowLocalTextFallback", value)} />
          <div className="knowledge-settings-field-grid">
            <NumberField label={t("knowledgeSettings.pageBudget")} value={draft.remotePageBudgetPerDay} min={1} max={100_000} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateNumber("remotePageBudgetPerDay", value)} />
            <NumberField label={t("knowledgeSettings.taskBudget")} value={draft.remoteTaskBudgetPerDay} min={1} max={1_000} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateNumber("remoteTaskBudgetPerDay", value)} />
            <NumberField label={t("knowledgeSettings.timeout")} value={draft.networkTimeoutSeconds} min={30} max={600} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => updateNumber("networkTimeoutSeconds", value)} />
            <NumberField label={t("knowledgeSettings.concurrency")} value={draft.indexConcurrency} min={1} max={4} disabled={saving} onChange={(value) => updateNumber("indexConcurrency", value)} />
          </div>
          <SelectRow label={t("knowledgeSettings.batchStrategy")} value={draft.batchStrategy} disabled={saving || !draft.mineruCloudEnabled} onChange={(value) => update("batchStrategy", value as KnowledgeSettings["batchStrategy"])} options={[
            ["pageBatches", t("knowledgeSettings.batchPages")],
            ["manualSplit", t("knowledgeSettings.batchManual")],
            ["reject", t("knowledgeSettings.batchReject")],
          ]} />
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <Image size={16} />
            <h3>{t("knowledgeSettings.markdown")}</h3>
            <p>{t("knowledgeSettings.markdownDescription")}</p>
          </div>
          <ToggleRow label={t("knowledgeSettings.markdownAssets")} description={t("knowledgeSettings.markdownAssetsDescription")} checked={draft.markdownAssetsEnabled} disabled={saving} onChange={(value) => updateToggle("markdownAssetsEnabled", value)} />
          <ToggleRow label={t("knowledgeSettings.annotations")} description={t("knowledgeSettings.annotationsDescription")} checked={draft.includeAnnotations} disabled={saving} onChange={(value) => updateToggle("includeAnnotations", value)} />
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <Search size={16} />
            <h3>{t("knowledgeSettings.retrieval")}</h3>
            <p>{t("knowledgeSettings.retrievalDescription")}</p>
          </div>
          <SelectRow label={t("knowledgeSettings.mode")} value={draft.retrievalMode} disabled={saving || !draft.enabled} onChange={(value) => handleRetrievalMode(value as KnowledgeSettings["retrievalMode"])} options={[
            ["lexical", t("knowledgeSettings.modeLexical")],
            ["vector", t("knowledgeSettings.modeVector")],
            ["hybrid", t("knowledgeSettings.modeHybrid")],
          ]} />
          <ToggleRow label={t("knowledgeSettings.embeddingEnabled")} description={t("knowledgeSettings.embeddingEnabledDescription")} checked={draft.embeddingEnabled} disabled={saving || !draft.enabled} onChange={handleEmbeddingToggle} />
          <ToggleRow label={t("knowledgeSettings.hybridEnabled")} description={t("knowledgeSettings.hybridEnabledDescription")} checked={draft.hybridEnabled} disabled={saving || !draft.embeddingEnabled || !draft.enabled} onChange={(value) => {
            updateToggle("hybridEnabled", value);
            if (value) handleRetrievalMode("hybrid");
            else if (draft.retrievalMode === "hybrid") handleRetrievalMode("lexical");
          }} />
          <div className="knowledge-settings-field-grid">
            <SelectRow
              label={t("knowledgeSettings.embeddingProvider")}
              value={draft.embeddingProvider}
              disabled={saving || !draft.embeddingEnabled}
              onChange={(value) => update("embeddingProvider", value)}
              options={[
                ["", t("knowledgeSettings.embeddingProviderNone")],
                ...embeddingProviders.map((provider) => [
                  provider.id,
                  `${provider.name}${provider.hasApiKey ? "" : ` · ${t("knowledgeSettings.embeddingProviderNoKey")}`}`,
                ] as [string, string]),
              ]}
            />
            <TextField label={t("knowledgeSettings.embeddingModel")} value={draft.embeddingModel} disabled={saving || !draft.embeddingEnabled} onChange={(value) => update("embeddingModel", value)} />
            <NumberField label={t("knowledgeSettings.chunkTarget")} value={draft.chunkTargetChars} min={256} max={8_192} disabled={saving} onChange={(value) => updateNumber("chunkTargetChars", value)} />
            <NumberField label={t("knowledgeSettings.chunkMax")} value={draft.chunkMaxChars} min={256} max={16_384} disabled={saving} onChange={(value) => updateNumber("chunkMaxChars", value)} />
            <NumberField label={t("knowledgeSettings.chunkOverlap")} value={draft.chunkOverlapChars} min={0} max={8_192} disabled={saving} onChange={(value) => updateNumber("chunkOverlapChars", value)} />
            <NumberField label={t("knowledgeSettings.topK")} value={draft.topK} min={1} max={50} disabled={saving} onChange={(value) => updateNumber("topK", value)} />
            <NumberField label={t("knowledgeSettings.contextBytes")} value={draft.contextMaxBytes} min={4 * 1024} max={256 * 1024} step={1024} disabled={saving} onChange={(value) => updateNumber("contextMaxBytes", value)} />
          </div>
          <p className="knowledge-settings-range-hint"><Info size={14} />{t("knowledgeSettings.rangeHint")}</p>
          <ToggleRow label={t("knowledgeSettings.embeddingRemote")} description={t("knowledgeSettings.embeddingRemoteDescription")} checked={draft.allowRemoteEmbedding} disabled={saving || !draft.embeddingEnabled} onChange={(value) => updateToggle("allowRemoteEmbedding", value)} />
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <DatabaseZap size={16} />
            <h3>{t("knowledgeSettings.external")}</h3>
            <p>{t("knowledgeSettings.externalDescription")}</p>
          </div>
          <ToggleRow label={t("knowledgeSettings.externalMcp")} description={t("knowledgeSettings.externalMcpDescription")} checked={draft.externalMcpEnabled} disabled={saving} onChange={(value) => updateToggle("externalMcpEnabled", value)} />
          <ToggleRow label={t("knowledgeSettings.debug")} description={t("knowledgeSettings.debugDescription")} checked={draft.debugRetrieval} disabled={saving} onChange={(value) => updateToggle("debugRetrieval", value)} />
          <div className="settings-callout settings-callout-accent knowledge-settings-notice">
            <ShieldCheck size={17} />
            <div>
              <strong>{t("knowledgeSettings.notImplemented")}</strong>
            </div>
          </div>
        </section>
      </div>
    </section>
  );
}

function ToggleRow({
  label,
  description,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <div className="settings-row">
      <div className="settings-row-copy"><strong>{label}</strong><span>{description}</span></div>
      <div className="settings-row-control">
        <button className={`settings-toggle${checked ? " settings-toggle-active" : ""}`} type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={() => onChange(!checked)}><span /></button>
      </div>
    </div>
  );
}

function CheckRow({ label, checked, disabled, onChange }: { label: string; checked: boolean; disabled: boolean; onChange: (value: boolean) => void }) {
  return (
    <label className="knowledge-settings-check">
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(event) => onChange(event.target.checked)} />
      <span>{label}</span>
    </label>
  );
}

function SelectRow({
  label,
  description,
  value,
  disabled,
  options,
  onChange,
}: {
  label: string;
  description?: string;
  value: string;
  disabled: boolean;
  options: Array<[string, string]>;
  onChange: (value: string) => void;
}) {
  return (
    <div className="settings-row">
      <div className="settings-row-copy"><strong>{label}</strong>{description ? <span>{description}</span> : null}</div>
      <div className="settings-row-control">
        <select className="settings-input settings-select knowledge-settings-select" value={value} disabled={disabled} aria-label={label} onChange={(event) => onChange(event.target.value)}>
          {options.map(([optionValue, optionLabel]) => <option value={optionValue} key={optionValue}>{optionLabel}</option>)}
        </select>
      </div>
    </div>
  );
}

function TextRow({ label, description, value, disabled, onChange }: { label: string; description?: string; value: string; disabled: boolean; onChange: (value: string) => void }) {
  return (
    <div className="settings-row">
      <div className="settings-row-copy"><strong>{label}</strong>{description ? <span>{description}</span> : null}</div>
      <div className="settings-row-control knowledge-settings-control-wide"><input className="settings-input" value={value} disabled={disabled} aria-label={label} onChange={(event) => onChange(event.target.value)} /></div>
    </div>
  );
}

function TextField({ label, value, disabled, onChange }: { label: string; value: string; disabled: boolean; onChange: (value: string) => void }) {
  return <label className="knowledge-settings-field"><span>{label}</span><input className="settings-input" value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} /></label>;
}

function NumberField({ label, value, min, max, step = 1, disabled, onChange }: { label: string; value: number; min: number; max: number; step?: number; disabled: boolean; onChange: (value: string) => void }) {
  return <label className="knowledge-settings-field"><span>{label}</span><input className="settings-input" type="number" min={min} max={max} step={step} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} /></label>;
}
