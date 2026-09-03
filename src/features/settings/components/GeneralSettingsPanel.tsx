import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import {
  AlertCircle,
  Archive,
  CheckCircle2,
  Download,
  FolderOpen,
  ImageUp,
  MessagesSquare,
  Palette,
  RefreshCw,
  Save,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  Upload,
  UserRound,
} from "lucide-react";
import {
  chooseWorkingDirectory,
  exportSettingsBundle,
  importSettingsBundle,
} from "../api/appSettings";
import type {
  AppSettings,
  SettingsBundle,
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
import "../styles/theme-preview.generated.css";
import { FONT_PRESET_VALUES } from "../utils/fontSettings";
import { useI18n } from "../../../i18n/I18nProvider";
import { EditableRangeControl } from "./EditableRangeControl";
import { BackgroundImagePicker } from "./BackgroundImagePicker";

type Feedback = { kind: "success" | "error"; message: string } | null;

type GeneralSettingsPanelProps = {
  settings: AppSettings;
  modelSettings: ModelSettings;
  initialError: string | null;
  onPreview: (settings: AppSettings) => void;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
  onImported: (bundle: SettingsBundle) => void;
  onDefaultModelChange: (providerId: string, modelId: string) => Promise<void>;
  onNoteModelChange: (providerId: string | null, modelId: string | null) => Promise<void>;
};

const TOKEN_OPTIONS = [4_096, 8_192, 16_384, 32_768, 65_536, 131_072];

/**
 * 渐变背景预设。
 *
 * 之前只有一个空输入框，用户得自己手打 CSS —— 而 `linear-gradient` 的语法门槛
 * 足够高到没人会去试。这些是可点选的起点，按「天空 / 风景 / 材质」三组意图排列。
 * 三组的差别不是色相，是明度落差：天空组落差大（有方向感），材质组落差极小
 * （安静、不抢内容）。
 */
const BACKGROUND_PRESETS: Array<{ label: string; css: string }> = [
  { label: "晨雾", css: "linear-gradient(170deg, #dbeafe 0%, #eff6ff 45%, #fdf4e7 100%)" },
  { label: "暮色", css: "linear-gradient(190deg, #1e1b4b 0%, #4c1d95 55%, #831843 100%)" },
  {
    label: "极光",
    css: "radial-gradient(at 20% 15%, #134e4a 0%, transparent 55%), radial-gradient(at 80% 70%, #1e1b4b 0%, transparent 60%), #0f172a",
  },
  { label: "远山", css: "linear-gradient(to bottom, #e0f2fe 0%, #f0f9ff 40%, #ecfdf5 70%, #f7fee7 100%)" },
  { label: "深林", css: "linear-gradient(165deg, #052e16 0%, #14532d 50%, #1c1917 100%)" },
  { label: "沙丘", css: "linear-gradient(155deg, #fef3c7 0%, #fed7aa 50%, #fecaca 100%)" },
  { label: "宣纸", css: "radial-gradient(at 30% 20%, #fffbeb 0%, transparent 50%), #fefce8" },
  { label: "石墨", css: "conic-gradient(from 210deg at 70% 30%, #27272a 0%, #18181b 40%, #09090b 100%)" },
];

const MAX_AVATAR_BYTES = 2 * 1024 * 1024;
const ACCEPTED_AVATAR_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif"]);
type TranslationKey = Parameters<ReturnType<typeof useI18n>["t"]>[0];
// 主题按家族分组：同一家族共享材质性格（边界、圆角、阴影），
// 家族之间才换整套调色，所以分组本身就是有信息量的结构。
const THEME_PRESET_GROUPS: { labelKey: TranslationKey; presets: ThemePreset[] }[] = [
  { labelKey: "general.themeGroupWorkshop", presets: ["graphite", "dawn"] },
  { labelKey: "general.themeGroupPaper", presets: ["xuan", "cyanotype", "paper"] },
  { labelKey: "general.themeGroupCard", presets: ["mnemora", "ocean", "lamp"] },
  { labelKey: "general.themeGroupPlain", presets: ["forest", "rose"] },
  { labelKey: "general.themeGroupAccess", presets: ["highContrast"] },
];
const THEME_PRESET_LABEL: Record<ThemePreset, { name: TranslationKey; hint: TranslationKey }> = {
  dawn: { name: "general.themeDawn", hint: "general.themeDawnHint" },
  lamp: { name: "general.themeLamp", hint: "general.themeLampHint" },
  graphite: { name: "general.themeGraphite", hint: "general.themeGraphiteHint" },
  xuan: { name: "general.themeXuan", hint: "general.themeXuanHint" },
  cyanotype: { name: "general.themeCyanotype", hint: "general.themeCyanotypeHint" },
  paper: { name: "general.themePaper", hint: "general.themePaperHint" },
  mnemora: { name: "general.themeMnemora", hint: "general.themeMnemoraHint" },
  forest: { name: "general.themeForest", hint: "general.themeForestHint" },
  ocean: { name: "general.themeOcean", hint: "general.themeOceanHint" },
  rose: { name: "general.themeRose", hint: "general.themeRoseHint" },
  highContrast: { name: "general.themeHighContrast", hint: "general.themeHighContrastHint" },
};

export function GeneralSettingsPanel({
  settings,
  modelSettings,
  initialError,
  onPreview,
  onSave,
  onImported,
  onDefaultModelChange,
  onNoteModelChange,
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
  const noteModelValue = modelSettings.noteProviderId && modelSettings.noteModelId
    ? JSON.stringify([modelSettings.noteProviderId, modelSettings.noteModelId])
    : "";

  const updateDraft = <Key extends keyof AppSettings>(key: Key, value: AppSettings[Key]) => {
    setDraft((current) => {
      const next = { ...current, [key]: value };
      if (
        key === "theme"
        || key === "interfaceLanguage"
        || key === "themePreset"
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

      <div className="settings-scroll settings-scroll-measure">
        <section className="settings-section">
          <div className="settings-section-head">
            <UserRound size={16} />
            <h3>{t("general.profile")}</h3>
          </div>
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

        <section className="settings-section">
          <div className="settings-section-head">
            <Palette size={16} />
            <h3>{t("general.appearance")}</h3>
          </div>
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
            <div role="radiogroup" aria-label={t("general.themePreset")}>
              {THEME_PRESET_GROUPS.map((group) => (
                <div className="theme-preset-group" key={group.labelKey}>
                  <span className="theme-preset-group-label">{t(group.labelKey)}</span>
                  <div className="theme-preset-options">
                    {group.presets.map((value) => (
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
                          <span className="theme-preset-rail"><i /><i /><i /></span>
                          <span className="theme-preset-side" />
                          <span className="theme-preset-main">
                            <span className="theme-preset-card"><u /><u /></span>
                            <span className="theme-preset-palette">
                              <b /><b /><b /><b /><b /><b />
                            </span>
                          </span>
                        </span>
                        <span className="theme-preset-name">
                          <span>{themePresetLabel(value, t)}</span>
                          <em>{themePresetHint(value, t)}</em>
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
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
              <SettingRow label={t("general.backgroundImage")} description={t("general.backgroundImageDescription")} stack>
                <BackgroundImagePicker
                  currentCss={draft.themeBackground.css}
                  onSelect={(css) => updateThemeBackground({ css })}
                />
              </SettingRow>
              <SettingRow label={t("general.backgroundPresets")} stack>
                <div className="background-preset-grid">
                  {BACKGROUND_PRESETS.map((preset) => (
                    <button
                      key={preset.label}
                      type="button"
                      className="background-preset"
                      data-active={draft.themeBackground.css.trim() === preset.css ? "true" : undefined}
                      aria-pressed={draft.themeBackground.css.trim() === preset.css}
                      onClick={() => updateThemeBackground({ css: preset.css })}
                    >
                      <span className="background-preset-swatch" style={{ background: preset.css }} />
                      <span>{preset.label}</span>
                    </button>
                  ))}
                </div>
              </SettingRow>
              <SettingRow label={t("general.surfaceOpacity")}>
                <EditableRangeControl
                  value={draft.themeBackground.surfaceOpacity}
                  min={MIN_SURFACE_OPACITY}
                  max={MAX_SURFACE_OPACITY}
                  step={1}
                  suffix="%"
                  ariaLabel={t("general.surfaceOpacity")}
                  onChange={(surfaceOpacity) => updateThemeBackground({ surfaceOpacity })}
                />
              </SettingRow>
            </>
          ) : null}
          <SettingRow label={t("general.fontSize")} description={t("general.fontSizeDescription")}>
            <EditableRangeControl
              value={draft.fontSize}
              min={12}
              max={28}
              step={1}
              suffix="px"
              ariaLabel={t("general.fontSize")}
              onChange={(value) => updateDraft("fontSize", value)}
            />
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
            <EditableRangeControl
              value={draft.letterSpacing}
              min={0}
              max={1.5}
              step={0.1}
              suffix="px"
              fractionDigits={1}
              ariaLabel={t("general.letterSpacing")}
              onChange={(value) => updateDraft("letterSpacing", value)}
            />
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
          <div className="settings-row-actions">
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

        <section className="settings-section">
          <div className="settings-section-head">
            <MessagesSquare size={16} />
            <h3>{t("general.chatDefaults")}</h3>
          </div>
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
          <SettingRow label={t("general.noteModel")} description={t("general.noteModelDescription")}>
            <select
              className="settings-input settings-select general-control"
              value={noteModelValue}
              onChange={(event) => {
                if (!event.target.value) {
                  void onNoteModelChange(null, null).catch((error) => {
                    setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
                  });
                  return;
                }
                const option = modelOptions.find((item) => item.value === event.target.value);
                if (!option) return;
                void onNoteModelChange(option.providerId, option.modelId).catch((error) => {
                  setFeedback({ kind: "error", message: error instanceof Error ? error.message : String(error) });
                });
              }}
            >
              <option value="">{t("general.followChatModel")}</option>
              {modelOptions.map((option) => (
                <option value={option.value} key={`note:${option.providerId}:${option.modelId}`}>
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

        <section className="settings-section">
          <div className="settings-section-head">
            <Sparkles size={16} />
            <h3>{t("general.response")}</h3>
          </div>
          <SettingRow label={t("general.streaming")}>
            <Toggle checked={draft.streamEnabled} onChange={(value) => updateDraft("streamEnabled", value)} />
          </SettingRow>
          <SettingRow
            label={t("general.deepNoteStreamKeepalive")}
            description={t("general.deepNoteStreamKeepaliveDescription")}
          >
            <Toggle
              checked={draft.deepNoteStreamKeepalive}
              onChange={(value) => updateDraft("deepNoteStreamKeepalive", value)}
            />
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

        <section className="settings-section">
          <div className="settings-section-head">
            <Archive size={16} />
            <h3>{t("general.backup")}</h3>
          </div>
          <SettingRow label={t("general.fullBackup")} description={t("general.fullBackupDescription")}>
            <button className="settings-button settings-button-secondary" type="button" disabled={backupBusy} onClick={() => void handleExport()}>
              <Download size={15} /><span>{t("general.exportSettings")}</span>
            </button>
            <button className="settings-button settings-button-secondary" type="button" disabled={backupBusy} onClick={() => void handleImport()}>
              <Upload size={15} /><span>{t("general.importSettings")}</span>
            </button>
          </SettingRow>
          <div className="settings-row settings-row-stack">
            <label className="settings-check">
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
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-section-head">
            <SlidersHorizontal size={16} />
            <h3>{t("general.behavior")}</h3>
          </div>
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
          <SettingRow label={t("general.agentMaxRounds")} description={t("general.agentMaxRoundsDescription")}>
            <select
              className="settings-input settings-select general-control"
              value={draft.agentMaxRounds}
              onChange={(event) => updateDraft(
                "agentMaxRounds",
                Number(event.target.value) as AppSettings["agentMaxRounds"],
              )}
            >
              {[5, 10, 20, 50, 100].map((rounds) => (
                <option value={rounds} key={rounds}>{t("general.agentRoundsOption", { count: rounds })}</option>
              ))}
            </select>
          </SettingRow>
          <SettingRow label={t("general.showChatTaskProgress")} description={t("general.showChatTaskProgressDescription")}>
            <Toggle checked={draft.showChatTaskProgress} onChange={(value) => updateDraft("showChatTaskProgress", value)} />
          </SettingRow>
          <SettingRow label={t("general.launchStartup")}>
            <Toggle checked={draft.launchAtStartup} onChange={(value) => updateDraft("launchAtStartup", value)} />
          </SettingRow>
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
  return t(THEME_PRESET_LABEL[value].name);
}

function themePresetHint(value: ThemePreset, t: ReturnType<typeof useI18n>["t"]) {
  return t(THEME_PRESET_LABEL[value].hint);
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
    <div className={`settings-row${stack ? " settings-row-stack" : ""}`}>
      <div className="settings-row-copy">
        <strong>{label}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      <div className="settings-row-control">{children}</div>
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
