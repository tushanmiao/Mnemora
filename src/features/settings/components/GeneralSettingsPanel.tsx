import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import {
  AlertCircle,
  CheckCircle2,
  Download,
  FolderOpen,
  ImageUp,
  RefreshCw,
  Save,
  Trash2,
  Upload,
} from "lucide-react";
import {
  chooseWorkingDirectory,
  exportSettingsBundle,
  importSettingsBundle,
} from "../api/appSettings";
import type {
  AppSettings,
  SettingsBundle,
  ThemeColor,
  ThemeMode,
  ThemePreset,
  FontPreset,
  ChineseFontFamily,
  LatinFontFamily,
} from "../../../types/appSettings";
import type { ModelSettings } from "../../../types/modelSettings";
import {
  DEFAULT_SURFACE_OPACITY,
  MAX_SURFACE_OPACITY,
  MIN_SURFACE_OPACITY,
  validateThemeBackgroundCss,
} from "../utils/themeBackground";
import "../styles/general-settings.css";
import { FONT_PRESET_VALUES } from "../utils/fontSettings";
import { useI18n } from "../../../i18n/I18nProvider";

type Feedback = { kind: "success" | "error"; message: string } | null;

type GeneralSettingsPanelProps = {
  settings: AppSettings;
  modelSettings: ModelSettings;
  initialError: string | null;
  onPreview: (settings: AppSettings) => void;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
  onImported: (bundle: SettingsBundle) => void;
  onDefaultModelChange: (providerId: string, modelId: string) => Promise<void>;
};

const TOKEN_OPTIONS = [4_096, 8_192, 16_384, 32_768, 65_536, 131_072];
const MAX_AVATAR_BYTES = 2 * 1024 * 1024;
const ACCEPTED_AVATAR_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
const THEME_PRESETS: ThemePreset[] = ["mnemora", "forest", "ocean", "rose", "paper", "graphite", "highContrast"];
const THEME_COLORS: ThemeColor[] = ["neutral", "warm", "cool", "rose", "amber", "violet"];

export function GeneralSettingsPanel({
  settings,
  modelSettings,
  initialError,
  onPreview,
  onSave,
  onImported,
  onDefaultModelChange,
}: GeneralSettingsPanelProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState(false);
  const [backupBusy, setBackupBusy] = useState(false);
  const [includeMemoryInBackup, setIncludeMemoryInBackup] = useState(false);
  const avatarInputRef = useRef<HTMLInputElement>(null);
  const [feedback, setFeedback] = useState<Feedback>(
    initialError ? { kind: "error", message: initialError } : null,
  );

  useEffect(() => setDraft(settings), [settings]);
  useEffect(() => {
    if (initialError) setFeedback({ kind: "error", message: initialError });
  }, [initialError]);

  const modelOptions = useMemo(() => modelSettings.providers
    .filter((provider) => provider.enabled)
    .flatMap((provider) => provider.models
      .filter((model) => model.enabled)
      .map((model) => ({
        providerId: provider.id,
        providerName: provider.name,
        modelId: model.id,
        value: JSON.stringify([provider.id, model.id]),
        label: `${provider.name} · ${model.displayName}`,
      }))), [modelSettings.providers]);

  const defaultModelValue = modelSettings.defaultProviderId && modelSettings.defaultModelId
    ? JSON.stringify([modelSettings.defaultProviderId, modelSettings.defaultModelId])
    : "";

  const updateDraft = <Key extends keyof AppSettings>(key: Key, value: AppSettings[Key]) => {
    setDraft((current) => {
      const next = { ...current, [key]: value };
      if (
        key === "theme"
        || key === "interfaceLanguage"
        || key === "themePreset"
        || key === "themeColor"
        || key === "fontSize"
        || key === "letterSpacing"
        || key === "fontPreset"
        || key === "chineseFontFamily"
        || key === "latinFontFamily"
      ) onPreview(next);
      return next;
    });
    setFeedback(null);
  };

  const updateThemeBackground = (patch: Partial<AppSettings["themeBackground"]>) => {
    setDraft((current) => {
      const next = {
        ...current,
        themeBackground: { ...current.themeBackground, ...patch },
      };
      onPreview(next);
      return next;
    });
    setFeedback(null);
  };

  const backgroundError = draft.themeBackground.enabled
    ? draft.themeBackground.css.trim()
      ? validateThemeBackgroundCss(draft.themeBackground.css)
      : t("general.backgroundRequired")
    : null;

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (backgroundError) {
      setFeedback({ kind: "error", message: backgroundError });
      return;
    }
    setSaving(true);
    setFeedback(null);
    try {
      const saved = await onSave(draft);
      setDraft(saved);
      setFeedback({ kind: "success", message: t("general.saved") });
    } catch (error) {
      setFeedback({
        kind: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  };

  const handleExport = async () => {
    const memoryNotice = includeMemoryInBackup ? t("general.exportMemoryNotice") : "";
    if (!window.confirm(t("general.exportConfirm", { memory: memoryNotice }))) return;
    setBackupBusy(true);
    setFeedback(null);
    try {
      const exported = await exportSettingsBundle(includeMemoryInBackup);
      if (exported) setFeedback({ kind: "success", message: t("general.exported") });
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setBackupBusy(false);
    }
  };

  const handleImport = async () => {
    setBackupBusy(true);
    setFeedback(null);
    try {
      const bundle = await importSettingsBundle();
      if (bundle) {
        onImported(bundle);
        setDraft(bundle.appSettings);
        setFeedback({
          kind: "success",
          message: bundle.memoryImported
            ? t("general.importedWithMemory")
            : t("general.importedWithoutMemory"),
        });
      }
    } catch (error) {
      setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
    } finally {
      setBackupBusy(false);
    }
  };

  const handleAvatarFile = (file: File | undefined) => {
    if (!file) return;
    if (!ACCEPTED_AVATAR_TYPES.has(file.type)) {
      setFeedback({ kind: "error", message: t("general.avatarTypeError") });
      return;
    }
    if (file.size > MAX_AVATAR_BYTES) {
      setFeedback({ kind: "error", message: t("general.avatarSizeError") });
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") updateDraft("userAvatar", reader.result);
    };
    reader.onerror = () => setFeedback({ kind: "error", message: t("general.avatarReadError") });
    reader.readAsDataURL(file);
  };

  return (
    <form className="settings-content general-settings-content" onSubmit={handleSubmit} noValidate>
      <div className="settings-content-heading">
        <div>
          <h2>{t("settings.general")}</h2>
          <span>{t("general.subtitle")}</span>
        </div>
        <button className="settings-button settings-button-primary" type="submit" disabled={saving}>
          <Save size={16} />
          <span>{saving ? t("common.saving") : t("common.save")}</span>
        </button>
      </div>

      <div className="general-settings-scroll">
        <section className="general-settings-section">
          <h3>{t("general.appearance")}</h3>
          <SettingRow label={t("general.interfaceLanguage")}>
            <select
              className="settings-input settings-select general-control"
              value={draft.interfaceLanguage}
              onChange={(event) => updateDraft("interfaceLanguage", event.target.value as AppSettings["interfaceLanguage"])}
            >
              <option value="zh">中文</option>
              <option value="en">English</option>
            </select>
          </SettingRow>
          <SettingRow label={t("general.theme")}>
            <SegmentedControl
              value={draft.theme}
              options={[
                { value: "system", label: t("general.system") },
                { value: "light", label: t("general.light") },
                { value: "dark", label: t("general.dark") },
              ]}
              onChange={(value) => updateDraft("theme", value as ThemeMode)}
            />
          </SettingRow>
          <SettingRow label={t("general.themePreset")} stack>
            <div className="theme-preset-options" role="radiogroup" aria-label={t("general.themePreset")}>
              {THEME_PRESETS.map((value) => (
                <button
                  className={`theme-preset-option${draft.themePreset === value ? " theme-preset-option-active" : ""}`}
                  data-theme-preset-preview={value}
                  type="button"
                  role="radio"
                  aria-checked={draft.themePreset === value}
                  key={value}
                  onClick={() => updateDraft("themePreset", value)}
                >
                  <span className="theme-preset-swatch" aria-hidden="true">
                    <i /><i /><i />
                  </span>
                  <span>{themePresetLabel(value, t)}</span>
                </button>
              ))}
            </div>
          </SettingRow>
          <SettingRow label={t("general.accent")}>
            <div className="theme-color-options" role="radiogroup" aria-label={t("general.themeColors")}>
              {THEME_COLORS.map((value) => (
                <button
                  className={`theme-color-option theme-color-${value}${draft.themeColor === value ? " theme-color-option-active" : ""}`}
                  type="button"
                  role="radio"
                  aria-checked={draft.themeColor === value}
                  key={value}
                  onClick={() => updateDraft("themeColor", value)}
                >
                  <span className="theme-color-swatch" aria-hidden="true" />
                  <span>{themeColorLabel(value, t)}</span>
                </button>
              ))}
            </div>
          </SettingRow>
          <SettingRow label={t("general.customBackground")} description={t("general.backgroundDescription")}>
            <Toggle
              checked={draft.themeBackground.enabled}
              onChange={(enabled) => updateThemeBackground({ enabled })}
            />
          </SettingRow>
          {draft.themeBackground.enabled ? (
            <>
              <SettingRow label={t("general.backgroundCss")} stack>
                <div className="theme-background-editor">
                  <textarea
                    className={`theme-background-input${backgroundError ? " theme-background-input-error" : ""}`}
                    value={draft.themeBackground.css}
                    rows={3}
                    spellCheck={false}
                    aria-invalid={Boolean(backgroundError)}
                    placeholder="linear-gradient(135deg, #f7f8f6, #dfeae3)"
                    onChange={(event) => updateThemeBackground({ css: event.target.value })}
                  />
                  <div
                    className="theme-background-preview"
                    style={{ background: backgroundError || !draft.themeBackground.css.trim()
                      ? "var(--color-app)"
                      : draft.themeBackground.css.trim() }}
                    aria-label={t("general.backgroundPreview")}
                  />
                  {backgroundError ? <span className="theme-background-error">{backgroundError}</span> : null}
                </div>
              </SettingRow>
              <SettingRow label={t("general.surfaceOpacity")}>
                <div className="font-size-control theme-opacity-control">
                  <input
                    type="range"
                    min={MIN_SURFACE_OPACITY}
                    max={MAX_SURFACE_OPACITY}
                    step={1}
                    value={draft.themeBackground.surfaceOpacity}
                    aria-label={t("general.surfaceOpacity")}
                    onChange={(event) => updateThemeBackground({ surfaceOpacity: Number(event.target.value) })}
                  />
                  <output>{draft.themeBackground.surfaceOpacity}%</output>
                </div>
              </SettingRow>
            </>
          ) : null}
          <SettingRow label={t("general.fontSize")} description={t("general.fontSizeDescription")}>
            <div className="font-size-control">
              <input
                type="range"
                min={12}
                max={28}
                step={1}
                value={draft.fontSize}
                aria-label={t("general.fontSize")}
                onChange={(event) => updateDraft("fontSize", Number(event.target.value))}
              />
              <output>{draft.fontSize} px</output>
            </div>
          </SettingRow>
          <SettingRow label={t("general.fontPreset")} description={t("general.fontPresetDescription")}>
            <SegmentedControl
              value={draft.fontPreset}
              options={[
                { value: "system", label: t("general.systemUi") },
                { value: "academic", label: t("general.academic") },
                { value: "custom", label: t("general.custom") },
              ]}
              onChange={(value) => {
                const preset = value as FontPreset;
                if (preset === "custom") {
                  updateDraft("fontPreset", preset);
                  return;
                }
                setDraft((current) => {
                  const next = { ...current, fontPreset: preset, ...FONT_PRESET_VALUES[preset] };
                  onPreview(next);
                  return next;
                });
                setFeedback(null);
              }}
            />
          </SettingRow>
          <SettingRow label={t("general.letterSpacing")} description={t("general.letterSpacingDescription")}>
            <div className="font-size-control">
              <input
                type="range"
                min={0}
                max={1.5}
                step={0.1}
                value={draft.letterSpacing}
                aria-label={t("general.letterSpacing")}
                onChange={(event) => updateDraft("letterSpacing", Number(event.target.value))}
              />
              <output>{draft.letterSpacing.toFixed(1)} px</output>
            </div>
          </SettingRow>
          {draft.fontPreset === "custom" ? (
            <>
              <SettingRow label={t("general.chineseFont")}>
                <select
                  className="settings-input settings-select general-control"
                  value={draft.chineseFontFamily}
                  onChange={(event) => updateDraft("chineseFontFamily", event.target.value as ChineseFontFamily)}
                >
                  <option value="system">{t("general.systemChinese")}</option>
                  <option value="microsoftYaHei">微软雅黑</option>
                  <option value="simsun">宋体</option>
                  <option value="notoSansCjk">Noto Sans CJK</option>
                  <option value="notoSerifCjk">Noto Serif CJK</option>
                </select>
              </SettingRow>
              <SettingRow label={t("general.latinFont")}>
                <select
                  className="settings-input settings-select general-control"
                  value={draft.latinFontFamily}
                  onChange={(event) => updateDraft("latinFontFamily", event.target.value as LatinFontFamily)}
                >
                  <option value="system">{t("general.systemLatin")}</option>
                  <option value="segoeUi">Segoe UI</option>
                  <option value="inter">Inter</option>
                  <option value="timesNewRoman">Times New Roman</option>
                  <option value="georgia">Georgia</option>
                </select>
              </SettingRow>
            </>
          ) : null}
          <div className="appearance-reset-row">
            <button
              className="settings-button settings-button-secondary"
              type="button"
              onClick={() => {
                const next = {
                  ...draft,
                  theme: "system" as const,
                  themePreset: "mnemora" as const,
                  themeColor: "neutral" as const,
                  themeBackground: {
                    enabled: false,
                    css: "",
                    surfaceOpacity: DEFAULT_SURFACE_OPACITY,
                  },
                  fontSize: 14,
                  letterSpacing: 0,
                  fontPreset: "system" as const,
                  chineseFontFamily: "system" as const,
                  latinFontFamily: "system" as const,
                };
                setDraft(next);
                onPreview(next);
                setFeedback(null);
              }}
            >
              <RefreshCw size={15} />
              <span>{t("general.resetAppearance")}</span>
            </button>
          </div>
        </section>

        <section className="general-settings-section">
          <h3>{t("general.behavior")}</h3>
          <SettingRow label={t("general.launchStartup")}>
            <Toggle checked={draft.launchAtStartup} onChange={(value) => updateDraft("launchAtStartup", value)} />
          </SettingRow>
          <SettingRow label={t("general.retry")} description={t("general.retryDescription")}>
            <Toggle checked={draft.retryEnabled} onChange={(value) => updateDraft("retryEnabled", value)} />
          </SettingRow>
          {draft.retryEnabled ? (
            <SettingRow label={t("general.maxRetries")}>
              <input
                className="settings-input general-number-input"
                type="number"
                min={1}
                max={5}
                value={draft.retryAttempts}
                onChange={(event) => updateDraft("retryAttempts", Number(event.target.value))}
              />
            </SettingRow>
          ) : null}
        </section>

        <section className="general-settings-section">
          <h3>{t("general.profile")}</h3>
          <SettingRow label={t("general.username")}>
            <input
              className="settings-input general-control"
              value={draft.userDisplayName}
              placeholder={t("common.optional")}
              onChange={(event) => updateDraft("userDisplayName", event.target.value)}
            />
          </SettingRow>
          <SettingRow label={t("general.avatar")} description={t("general.avatarDescription")} stack>
            <div className="profile-avatar-row">
              <div className="profile-avatar-preview" aria-hidden="true">
                {draft.userAvatar ? <img src={draft.userAvatar} alt="" /> : (draft.userDisplayName.trim()[0] ?? "M").toUpperCase()}
              </div>
              <div className="profile-avatar-actions">
                <button className="settings-button settings-button-secondary" type="button" onClick={() => avatarInputRef.current?.click()}>
                  <ImageUp size={15} /><span>{t("general.chooseImage")}</span>
                </button>
                {draft.userAvatar ? (
                  <button className="settings-button settings-button-secondary" type="button" onClick={() => updateDraft("userAvatar", "")}>
                    <Trash2 size={15} /><span>{t("general.remove")}</span>
                  </button>
                ) : null}
                <input
                  ref={avatarInputRef}
                  className="profile-avatar-input"
                  type="file"
                  accept="image/png,image/jpeg,image/webp,image/gif"
                  onChange={(event) => {
                    handleAvatarFile(event.target.files?.[0]);
                    event.target.value = "";
                  }}
                />
              </div>
            </div>
          </SettingRow>
        </section>

        <section className="general-settings-section">
          <h3>{t("general.chatDefaults")}</h3>
          <SettingRow label={t("general.defaultModel")}>
            <select
              className="settings-input settings-select general-control"
              value={defaultModelValue}
              onChange={(event) => {
                const option = modelOptions.find((item) => item.value === event.target.value);
                if (!option) return;
                void onDefaultModelChange(option.providerId, option.modelId).catch((error) => {
                  setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
                });
              }}
            >
              <option value="">{t("general.notSet")}</option>
              {modelOptions.map((option) => (
                <option value={option.value} key={`${option.providerId}:${option.modelId}`}>
                  {option.label}
                </option>
              ))}
            </select>
          </SettingRow>
          <SettingRow label={t("general.workingDirectory")} description={t("general.workingDirectoryDescription")} stack>
            <div className="working-directory-row">
              <input
                className="settings-input"
                value={draft.workingDirectory}
                placeholder={t("general.workingDirectoryPlaceholder")}
                onChange={(event) => updateDraft("workingDirectory", event.target.value)}
              />
              <button
                className="settings-button settings-button-secondary"
                type="button"
                onClick={() => void chooseWorkingDirectory().then((path) => {
                  if (path) updateDraft("workingDirectory", path);
                })}
              >
                <FolderOpen size={15} />
                <span>{t("general.choose")}</span>
              </button>
              <button
                className="settings-button settings-button-secondary"
                type="button"
                onClick={() => updateDraft("workingDirectory", "")}
              >
                <RefreshCw size={15} />
                <span>{t("general.restoreDefault")}</span>
              </button>
            </div>
          </SettingRow>
          <SettingRow label={t("general.systemPrompt")} description={t("general.systemPromptDescription")} stack>
            <textarea
              className="settings-textarea"
              rows={5}
              value={draft.systemPrompt}
              onChange={(event) => updateDraft("systemPrompt", event.target.value)}
            />
          </SettingRow>
        </section>

        <section className="general-settings-section">
          <h3>{t("general.response")}</h3>
          <SettingRow label={t("general.streaming")}>
            <Toggle checked={draft.streamEnabled} onChange={(value) => updateDraft("streamEnabled", value)} />
          </SettingRow>
          <SettingRow label={t("general.thinking")} description={t("general.thinkingDescription")}>
            <Toggle checked={draft.thinkingEnabled} onChange={(value) => updateDraft("thinkingEnabled", value)} />
          </SettingRow>
          <SettingRow label={t("general.maxTokens")}>
            <select
              className="settings-input settings-select general-control"
              value={draft.maxOutputTokens}
              onChange={(event) => updateDraft("maxOutputTokens", Number(event.target.value))}
            >
              {TOKEN_OPTIONS.map((tokens) => <option value={tokens} key={tokens}>{tokens.toLocaleString()} tokens</option>)}
            </select>
          </SettingRow>
          <SettingRow label={t("general.responseLanguage")}>
            <select
              className="settings-input settings-select general-control"
              value={draft.responseLanguage}
              onChange={(event) => updateDraft("responseLanguage", event.target.value as AppSettings["responseLanguage"])}
            >
              <option value="followInput">{t("general.followInput")}</option>
              <option value="zh">中文</option>
              <option value="zhHant">繁体中文</option>
              <option value="en">English</option>
            </select>
          </SettingRow>
        </section>

        <section className="general-settings-section">
          <h3>{t("general.backup")}</h3>
          <label className="backup-memory-option">
            <input
              type="checkbox"
              checked={includeMemoryInBackup}
              disabled={backupBusy}
              onChange={(event) => setIncludeMemoryInBackup(event.target.checked)}
            />
            <span>
              <strong>{t("general.includeMemory")}</strong>
              <small>{t("general.includeMemoryDescription")}</small>
            </span>
          </label>
          <div className="backup-settings-row">
            <div>
              <strong>{t("general.fullBackup")}</strong>
              <span>{t("general.fullBackupDescription")}</span>
            </div>
            <div>
              <button className="settings-button settings-button-secondary" type="button" disabled={backupBusy} onClick={() => void handleExport()}>
                <Download size={15} /><span>{t("general.exportSettings")}</span>
              </button>
              <button className="settings-button settings-button-secondary" type="button" disabled={backupBusy} onClick={() => void handleImport()}>
                <Upload size={15} /><span>{t("general.importSettings")}</span>
              </button>
            </div>
          </div>
        </section>
      </div>

      {feedback ? (
        <div className={`settings-feedback settings-feedback-${feedback.kind}`} role="status">
          {feedback.kind === "success" ? <CheckCircle2 size={17} /> : <AlertCircle size={17} />}
          <span>{feedback.message}</span>
        </div>
      ) : null}
    </form>
  );
}

function themePresetLabel(value: ThemePreset, t: ReturnType<typeof useI18n>["t"]) {
  if (value === "forest") return t("general.themeForest");
  if (value === "ocean") return t("general.themeOcean");
  if (value === "rose") return t("general.themeRose");
  if (value === "paper") return t("general.themePaper");
  if (value === "graphite") return t("general.themeGraphite");
  if (value === "highContrast") return t("general.themeHighContrast");
  return "Mnemora";
}

function themeColorLabel(value: ThemeColor, t: ReturnType<typeof useI18n>["t"]) {
  if (value === "warm") return t("general.colorWarm");
  if (value === "cool") return t("general.colorCool");
  if (value === "rose") return t("general.colorRose");
  if (value === "amber") return t("general.colorAmber");
  if (value === "violet") return t("general.colorViolet");
  return t("general.colorNeutral");
}

function SettingRow({
  label,
  description,
  stack = false,
  children,
}: {
  label: string;
  description?: string;
  stack?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className={`general-setting-row${stack ? " general-setting-row-stack" : ""}`}>
      <div className="general-setting-copy">
        <strong>{label}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      <div className="general-setting-control">{children}</div>
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <button
      className={`settings-toggle${checked ? " settings-toggle-active" : ""}`}
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span />
    </button>
  );
}

function SegmentedControl({
  value,
  options,
  onChange,
}: {
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <div className="settings-segmented">
      {options.map((option) => (
        <button
          className={value === option.value ? "settings-segmented-active" : ""}
          type="button"
          key={option.value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
